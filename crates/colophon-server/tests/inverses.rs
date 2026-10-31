//! Bidirectional references end to end: the real binary, a real sign-in. A reference field may
//! name an `inverse` — a reference field on the target collection — and every save, create,
//! import and delete then mirrors the link into that field, in the same transaction. Needs the
//! stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Reply, Server, Session, call, key, project, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

fn types(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production/content-types")
}

fn entries(project: &str, collection: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production/collections/{collection}/entries")
}

async fn write_type(session: &Session, project: &str, slug: &str, definition: &Value) -> Reply {
    call(
        session,
        Method::POST,
        &types(project),
        Some(&json!({"slug": slug, "definition": definition})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request")
}

async fn put_type(session: &Session, project: &str, slug: &str, definition: &Value) -> Reply {
    call(
        session,
        Method::PUT,
        &format!("{}/{slug}", types(project)),
        Some(definition),
        &[("idempotency-key", &key()), ("if-match", "\"v1\"")],
    )
    .await
    .expect("request")
}

async fn create(session: &Session, path: &str, content: &Value) -> Reply {
    call(
        session,
        Method::POST,
        path,
        Some(content),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request")
}

async fn field_ids(session: &Session, path: &str, entry: &str, field: &str) -> Vec<String> {
    let reply = call(session, Method::GET, &format!("{path}/{entry}"), None, &[])
        .await
        .expect("request");
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    reply.body["data"]["content"][field]
        .as_array()
        .map(|ids| {
            ids.iter()
                .filter_map(|id| id.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn an_inverse_field_mirrors_every_link_change() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let project = project(&editor).await;

    // `posts` first (plain), then `writers` pointing at it, then `posts` gains the mirrored
    // side — the definition order that satisfies every target check.
    let posts_def = json!({
        "name": "Posts", "title_field": "headline",
        "fields": [{"id": "headline", "label": "Headline", "type": "text", "required": true}]
    });
    let writers_def = json!({
        "name": "Writers", "title_field": "name",
        "fields": [
            {"id": "name", "label": "Name", "type": "text", "required": true},
            {"id": "posts", "label": "Posts", "type": "reference", "to": "posts", "many": true}
        ]
    });
    let linked_posts_def = json!({
        "name": "Posts", "title_field": "headline",
        "fields": [
            {"id": "headline", "label": "Headline", "type": "text", "required": true},
            {"id": "writer", "label": "Writer", "type": "reference", "to": "writers", "inverse": "posts"}
        ]
    });
    assert_eq!(
        write_type(&editor, &project, "posts", &posts_def)
            .await
            .status,
        StatusCode::CREATED
    );
    assert_eq!(
        write_type(&editor, &project, "writers", &writers_def)
            .await
            .status,
        StatusCode::CREATED
    );
    // A wrong `inverse` is refused at definition write: no field, or one that isn't a
    // reference back.
    let bad = json!({
        "name": "Posts", "title_field": "headline",
        "fields": [
            {"id": "headline", "label": "Headline", "type": "text", "required": true},
            {"id": "writer", "label": "Writer", "type": "reference", "to": "writers", "inverse": "name"}
        ]
    });
    let refused = put_type(&editor, &project, "posts", &bad).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        put_type(&editor, &project, "posts", &linked_posts_def)
            .await
            .status,
        StatusCode::OK
    );

    let writers = entries(&project, "writers");
    let posts = entries(&project, "posts");
    let ada = create(&editor, &writers, &json!({"name": "Ada"})).await;
    assert_eq!(ada.status, StatusCode::CREATED, "{}", ada.body);
    let ada = ada.body["data"]["id"].as_str().expect("an id").to_owned();
    let grace = create(&editor, &writers, &json!({"name": "Grace"})).await;
    let grace = grace.body["data"]["id"].as_str().expect("an id").to_owned();

    // Linking a post to Ada writes her `posts` for her.
    let post = create(
        &editor,
        &posts,
        &json!({"headline": "Compilers", "writer": ada}),
    )
    .await;
    assert_eq!(post.status, StatusCode::CREATED, "{}", post.body);
    let post_id = post.body["data"]["id"].as_str().expect("an id").to_owned();
    assert_eq!(
        field_ids(&editor, &writers, &ada, "posts").await,
        vec![post_id.clone()]
    );

    // Moving the post to Grace moves the mirror: Ada's list empties, Grace's gains it.
    let etag = post.etag().expect("an ETag");
    let moved = call(
        &editor,
        Method::PUT,
        &format!("{posts}/{post_id}"),
        Some(&json!({"headline": "Compilers", "writer": grace})),
        &[("idempotency-key", &key()), ("if-match", &etag)],
    )
    .await
    .expect("request");
    assert_eq!(moved.status, StatusCode::OK, "{}", moved.body);
    assert_eq!(
        field_ids(&editor, &writers, &ada, "posts").await,
        Vec::<String>::new()
    );
    assert_eq!(
        field_ids(&editor, &writers, &grace, "posts").await,
        vec![post_id.clone()]
    );

    // Deleting the post clears Grace's mirror in the same transaction.
    let deleted = call(
        &editor,
        Method::DELETE,
        &format!("{posts}/{post_id}"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(deleted.status, StatusCode::OK, "{}", deleted.body);
    assert_eq!(
        field_ids(&editor, &writers, &grace, "posts").await,
        Vec::<String>::new()
    );
}
