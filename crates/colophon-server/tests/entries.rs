//! Entries end to end: the real binary, a real sign-in, and every rule a save must keep, over
//! HTTP. Needs the stack (scripts/stack.sh up) and migrations (scripts/migrate.sh).
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Server, Session, article, call, entries, key, project, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::json;

async fn create(session: &Session, project: &str, title: &str) -> (String, String) {
    let reply = call(
        session,
        Method::POST,
        &entries(project),
        Some(&article(title, "Hello")),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let id = reply.body["data"]["id"].as_str().expect("an id").to_owned();
    (id, reply.etag().expect("an ETag"))
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_draft_is_created_read_saved_and_kept_in_history() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let project = project(&editor).await;

    let body = article("First \"draft\"", "Hello");
    let created = call(
        &editor,
        Method::POST,
        &entries(&project),
        Some(&body),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = created.body["data"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let first = created.etag().expect("an ETag");
    let revision = created.body["data"]["revision"]["id"]
        .as_str()
        .expect("a revision");
    assert_eq!(
        first,
        format!("\"{revision}\""),
        "the ETag names the revision"
    );
    assert_eq!(
        created.body["data"]["content"], body,
        "the content comes back as sent"
    );
    assert!(
        created.body["data"]["revision"]["content_hash"]
            .as_str()
            .is_some_and(|hash| hash.starts_with("sha256:") && hash.len() == 71)
    );

    let read = call(
        &editor,
        Method::GET,
        &format!("{}/{id}", entries(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        (read.status, read.etag()),
        (StatusCode::OK, Some(first.clone()))
    );

    let saved = call(
        &editor,
        Method::PUT,
        &format!("{}/{id}", entries(&project)),
        Some(&article("Second", "Hello again")),
        &[("if-match", &first), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    let second = saved.etag().expect("an ETag");
    assert_ne!(second, first);
    assert_eq!(
        saved.body["data"]["revision"]["base_revision_id"].as_str(),
        Some(revision)
    );

    let history = call(
        &editor,
        Method::GET,
        &format!("{}/{id}/revisions", entries(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    let titles: Vec<&str> = history.body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|revision| revision["title"].as_str())
        .collect();
    assert_eq!(titles, ["Second", "First \"draft\""]);
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_stale_save_is_refused_with_the_current_etag_and_writes_nothing() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let project = project(&editor).await;
    let (id, first) = create(&editor, &project, "First").await;
    let path = format!("{}/{id}", entries(&project));

    let saved = call(
        &editor,
        Method::PUT,
        &path,
        Some(&article("Second", "B")),
        &[("if-match", &first), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    let second = saved.etag().expect("an ETag");

    // Someone still holding the first revision saves on top of it.
    let stale = call(
        &editor,
        Method::PUT,
        &path,
        Some(&article("Late", "C")),
        &[("if-match", &first), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(stale.status, StatusCode::CONFLICT, "{}", stale.body);
    assert_eq!(stale.body["error"]["details"]["reason"], "stale_revision");
    assert_eq!(
        stale.etag(),
        Some(second.clone()),
        "the 409 names the current revision"
    );

    let read = call(&editor, Method::GET, &path, None, &[])
        .await
        .expect("request");
    assert_eq!(read.etag(), Some(second));
    assert_eq!(
        read.body["data"]["content"]["title"], "Second",
        "nothing was written"
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "each refusal, then the replay, in one session"
)]
async fn writes_need_their_preconditions_and_keys_replay_or_refuse() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let project = project(&editor).await;
    let list = entries(&project);

    let no_key = call(&editor, Method::POST, &list, Some(&article("T", "x")), &[])
        .await
        .expect("request");
    assert_eq!(
        (no_key.status, no_key.body["error"]["code"].as_str()),
        (
            StatusCode::PRECONDITION_REQUIRED,
            Some("precondition_required")
        )
    );

    let invalid = call(
        &editor,
        Method::POST,
        &list,
        Some(&json!({"title": " padded", "body": article("T", "x")["body"]})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(invalid.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        (
            invalid.body["error"]["details"]["reason"].as_str(),
            invalid.body["error"]["details"]["pointer"].as_str()
        ),
        (Some("invalid_text"), Some("/title"))
    );

    // The same request with the same key: one entry, and the recorded answer again.
    let once = key();
    let body = article("Once", "x");
    let first = call(
        &editor,
        Method::POST,
        &list,
        Some(&body),
        &[("idempotency-key", &once)],
    )
    .await
    .expect("request");
    let again = call(
        &editor,
        Method::POST,
        &list,
        Some(&body),
        &[("idempotency-key", &once)],
    )
    .await
    .expect("request");
    assert_eq!(first.status, StatusCode::CREATED);
    assert_eq!(again.status, StatusCode::CREATED);
    assert_eq!(again.body, first.body, "a replay is the same answer");
    assert_eq!(again.etag(), first.etag());
    assert!(again.replayed(), "a replay of the recorded answer");
    assert!(!first.replayed());

    // The same key with a different body is refused.
    let reused = call(
        &editor,
        Method::POST,
        &list,
        Some(&article("Different", "y")),
        &[("idempotency-key", &once)],
    )
    .await
    .expect("request");
    assert_eq!(reused.status, StatusCode::CONFLICT);
    assert_eq!(
        reused.body["error"]["details"]["reason"],
        "idempotency_key_reused"
    );

    // Saving without If-Match, or with something that isn't one.
    let id = first.body["data"]["id"].as_str().expect("an id");
    let path = format!("{list}/{id}");
    let no_match = call(
        &editor,
        Method::PUT,
        &path,
        Some(&body),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(no_match.status, StatusCode::PRECONDITION_REQUIRED);
    let star = call(
        &editor,
        Method::PUT,
        &path,
        Some(&body),
        &[("if-match", "*"), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(star.status, StatusCode::BAD_REQUEST);

    let listed = call(&editor, Method::GET, &list, None, &[])
        .await
        .expect("request");
    assert_eq!(
        listed.body["data"].as_array().map(Vec::len),
        Some(1),
        "the replayed create made one entry: {}",
        listed.body
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn lists_page_with_a_cursor_that_only_works_where_it_came_from() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let project = project(&editor).await;
    for title in ["One", "Two", "Three"] {
        create(&editor, &project, title).await;
    }
    let first = call(
        &editor,
        Method::GET,
        &format!("{}?limit=2", entries(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    let titles = |reply: &common::Reply| -> Vec<String> {
        reply.body["data"]
            .as_array()
            .expect("a list")
            .iter()
            .filter_map(|entry| entry["revision"]["title"].as_str().map(str::to_owned))
            .collect()
    };
    assert_eq!(titles(&first), ["Three", "Two"], "newest first");
    let cursor = first.body["next_cursor"].as_str().expect("another page");
    let second = call(
        &editor,
        Method::GET,
        &format!("{}?limit=2&cursor={cursor}", entries(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(titles(&second), ["One"]);
    assert!(second.body["next_cursor"].is_null(), "the last page");

    // The cursor belongs to this project's list: elsewhere it is refused.
    let other = common::project(&editor).await;
    let elsewhere = call(
        &editor,
        Method::GET,
        &format!("{}?cursor={cursor}", entries(&other)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(elsewhere.status, StatusCode::BAD_REQUEST);
    let too_many = call(
        &editor,
        Method::GET,
        &format!("{}?limit=101", entries(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(too_many.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn nobody_outside_the_project_can_read_or_write_its_entries() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let project = project(&editor).await;
    let (id, etag) = create(&editor, &project, "Private").await;
    let path = format!("{}/{id}", entries(&project));

    let stranger = sign_in("dev-editor").await;
    let attempts = [
        (Method::GET, entries(&project), None),
        (Method::GET, path.clone(), None),
        (Method::GET, format!("{path}/revisions"), None),
        (Method::POST, entries(&project), Some(article("Mine", "x"))),
        (Method::PUT, path.clone(), Some(article("Mine", "x"))),
    ];
    for (method, target, body) in attempts {
        let reply = call(
            &stranger,
            method.clone(),
            &target,
            body.as_ref(),
            &[("if-match", &etag), ("idempotency-key", &key())],
        )
        .await
        .expect("request");
        assert_eq!(
            (reply.status, reply.body["error"]["code"].as_str()),
            (StatusCode::NOT_FOUND, Some("not_found")),
            "{method} {target}"
        );
    }

    // Paths that can't name anything are 404 too, never 400 or 500.
    for target in [
        format!(
            "/api/v1/projects/not-a-uuid/environments/production/collections/articles/entries/{id}"
        ),
        format!(
            "/api/v1/projects/{project}/environments/staging/collections/articles/entries/{id}"
        ),
        format!(
            "/api/v1/projects/{project}/environments/production/collections/pages/entries/{id}"
        ),
        format!("{}/not-a-uuid", entries(&project)),
    ] {
        let reply = call(&editor, Method::GET, &target, None, &[])
            .await
            .expect("request");
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{target}");
    }

    // A write without the CSRF token is refused before anything else.
    let response = common::app()
        .put(format!("{}{path}", common::ORIGIN))
        .header("cookie", &editor.cookie)
        .header("origin", common::ORIGIN)
        .header("if-match", &etag)
        .header("idempotency-key", key())
        .json(&article("No token", "x"))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
