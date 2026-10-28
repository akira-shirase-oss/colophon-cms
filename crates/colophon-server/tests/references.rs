//! References between entries, end to end (step M.2, D-065): a story names its author. A
//! reference to no entry, or to an entry of another collection, is refused where it is; readers
//! never get a reference to an entry they can't get, so publishing a story before its author is
//! refused, and so is taking an author down while a published story names them; publishing or
//! taking both down together works. Needs the stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Reply, Server, Session, call, join, key, project, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
}

async fn send(session: &Session, method: Method, path: &str, body: Option<&Value>) -> Reply {
    call(session, method, path, body, &[("idempotency-key", &key())])
        .await
        .expect("request")
}

async fn content_type(owner: &Session, project: &str, slug: &str, definition: &Value) {
    let made = send(
        owner,
        Method::POST,
        &format!("{}/content-types", base(project)),
        Some(&json!({"slug": slug, "definition": definition})),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
}

async fn entry(owner: &Session, project: &str, collection: &str, content: &Value) -> Reply {
    send(
        owner,
        Method::POST,
        &format!("{}/collections/{collection}/entries", base(project)),
        Some(content),
    )
    .await
}

fn id(reply: &Reply) -> String {
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    reply.body["data"]["id"].as_str().expect("an id").to_owned()
}

/// Submits, approves and tries to publish the changeset `body` as `owner`; the publish answer.
async fn try_release(owner: &Session, project: &str, body: &Value) -> Reply {
    let set = send(
        owner,
        Method::POST,
        &format!("{}/changesets", base(project)),
        Some(body),
    )
    .await;
    assert_eq!(set.status, StatusCode::CREATED, "{}", set.body);
    let path = format!(
        "{}/changesets/{}",
        base(project),
        set.body["data"]["id"].as_str().expect("an id")
    );
    let if_match = format!("\"{}\"", set.body["data"]["hash"].as_str().expect("a hash"));
    for (step, body) in [
        ("submit", None),
        ("reviews", Some(json!({"decision": "approve"}))),
    ] {
        let done = call(
            owner,
            Method::POST,
            &format!("{path}/{step}"),
            body.as_ref(),
            &[("if-match", &if_match)],
        )
        .await
        .expect("request");
        assert_eq!(done.status, StatusCode::OK, "{step}: {}", done.body);
    }
    let releases = call(
        owner,
        Method::GET,
        &format!("{}/releases", base(project)),
        None,
        &[],
    )
    .await
    .expect("request");
    let current: i64 = releases
        .headers
        .get("colophon-generation")
        .and_then(|value| value.to_str().ok()?.parse().ok())
        .expect("a generation");
    call(
        owner,
        Method::POST,
        &format!("{path}/publish"),
        Some(&json!({"base_generation": current})),
        &[("if-match", &if_match), ("idempotency-key", &key())],
    )
    .await
    .expect("request")
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: write, refuse, publish, take down"
)]
async fn readers_never_get_a_reference_to_an_entry_they_cannot_get() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    content_type(
        &owner,
        &project,
        "people",
        &json!({"name": "People", "title_field": "name", "fields": [
            {"id": "name", "label": "Name", "type": "text", "required": true}
        ]}),
    )
    .await;
    content_type(
        &owner,
        &project,
        "stories",
        &json!({"name": "Stories", "title_field": "title", "fields": [
            {"id": "title", "label": "Title", "type": "text", "required": true},
            {"id": "author", "label": "Author", "type": "reference", "to": "people"},
            {"id": "related", "label": "Related", "type": "reference", "to": "stories", "many": true, "max": 5}
        ]}),
    )
    .await;
    let ada = id(&entry(&owner, &project, "people", &json!({"name": "Ada"})).await);

    // A reference names an entry of its collection, or the save is refused where it is.
    let nobody = entry(
        &owner,
        &project,
        "stories",
        &json!({"title": "Lost", "author": "01a18f43-a955-755e-ab9c-1ded4406683c"}),
    )
    .await;
    assert_eq!(
        nobody.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        nobody.body
    );
    assert_eq!(
        (
            &nobody.body["error"]["details"]["reason"],
            &nobody.body["error"]["details"]["pointer"]
        ),
        (&json!("reference_not_found"), &json!("/author"))
    );
    let story = id(&entry(
        &owner,
        &project,
        "stories",
        &json!({"title": "First", "author": ada}),
    )
    .await);
    let wrong = entry(
        &owner,
        &project,
        "stories",
        &json!({"title": "Mixed", "author": story, "related": [story]}),
    )
    .await;
    assert_eq!(
        wrong.body["error"]["details"]["pointer"], "/author",
        "a story isn't a person: {}",
        wrong.body
    );
    let in_list = entry(
        &owner,
        &project,
        "stories",
        &json!({"title": "Listed", "related": [story, ada]}),
    )
    .await;
    assert_eq!(
        in_list.body["error"]["details"]["pointer"], "/related/1",
        "{}",
        in_list.body
    );

    // The author's page says who names them, in draft so far.
    let read_ada = |session| {
        let path = format!("{}/collections/people/entries/{ada}", base(&project));
        async move {
            call(session, Method::GET, &path, None, &[])
                .await
                .expect("request")
        }
    };
    assert_eq!(
        read_ada(&owner).await.body["data"]["referenced_by"],
        json!([{"entry_id": story, "collection": "stories", "field": "author", "in_draft": true, "published": false}])
    );

    // Publishing the story alone would show readers an author they can't get.
    let alone = try_release(
        &owner,
        &project,
        &json!({"title": "Story", "entries": [story]}),
    )
    .await;
    assert_eq!(
        (alone.status, &alone.body["error"]["details"]["reason"]),
        (StatusCode::CONFLICT, &json!("references_unpublished")),
        "{}",
        alone.body
    );
    // Together they publish.
    let both = try_release(
        &owner,
        &project,
        &json!({"title": "Both", "entries": [story, ada]}),
    )
    .await;
    assert_eq!(both.status, StatusCode::CREATED, "{}", both.body);
    assert_eq!(
        read_ada(&owner).await.body["data"]["referenced_by"][0]["published"],
        true
    );
    // Someone whose role reads people but not stories isn't told about the story.
    let roles = format!("/api/v1/projects/{project}/roles");
    let reader = json!({"key": "people-reader", "name": "People reader", "permissions": [{"action": "content.read", "subject": "people"}]});
    assert_eq!(
        send(&owner, Method::POST, &roles, Some(&reader))
            .await
            .status,
        StatusCode::CREATED
    );
    let guest = sign_in("dev-editor").await;
    join(&owner, &guest, &project, "people-reader").await;
    let seen = read_ada(&guest).await;
    assert_eq!(seen.status, StatusCode::OK, "{}", seen.body);
    assert_eq!(seen.body["data"]["referenced_by"], json!([]));

    // Taking the author down while the story names them is refused; both together works.
    let author_only = try_release(
        &owner,
        &project,
        &json!({"title": "Down", "unpublish": [ada]}),
    )
    .await;
    assert_eq!(
        (
            author_only.status,
            &author_only.body["error"]["details"]["reason"]
        ),
        (StatusCode::CONFLICT, &json!("still_referenced")),
        "{}",
        author_only.body
    );
    let down = try_release(
        &owner,
        &project,
        &json!({"title": "Down", "unpublish": [ada, story]}),
    )
    .await;
    assert_eq!(down.status, StatusCode::CREATED, "{}", down.body);
}
