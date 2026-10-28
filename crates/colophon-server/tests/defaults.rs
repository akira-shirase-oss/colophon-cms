//! Field defaults, end to end (step M.4, D-071): a new entry gets the default of each field it
//! leaves out, in canonical form; a value sent is kept; a save gets no defaults; a default the
//! field wouldn't accept is refused where it is defined; and a role limited to some fields can
//! still create an entry whose other fields the defaults fill. Needs the stack and migrations.
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

fn tasks() -> Value {
    json!({
        "name": "Tasks",
        "title_field": "title",
        "fields": [
            {"id": "title", "label": "Title", "type": "text", "required": true},
            {"id": "status", "label": "Status", "type": "enum", "values": ["todo", "done"], "required": true, "default": "todo"},
            {"id": "estimate", "label": "Estimate", "type": "number", "integer": true, "default": 2},
            {"id": "link", "label": "Link", "type": "url", "default": "HTTPS://Example.com"}
        ]
    })
}

async fn create(session: &Session, project: &str, content: &Value) -> Reply {
    send(
        session,
        Method::POST,
        &format!("{}/collections/tasks/entries", base(project)),
        Some(content),
    )
    .await
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_new_entry_gets_the_defaults_of_what_it_leaves_out_and_a_save_gets_none() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let types = format!("{}/content-types", base(&project));

    let mut wrong = tasks();
    wrong["fields"][1]["default"] = json!("later");
    let refused = send(
        &owner,
        Method::POST,
        &types,
        Some(&json!({"slug": "tasks", "definition": wrong})),
    )
    .await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        (
            &refused.body["error"]["details"]["reason"],
            &refused.body["error"]["details"]["pointer"]
        ),
        (
            &json!("invalid_default"),
            &json!("/definition/fields/1/default")
        )
    );
    let made = send(
        &owner,
        Method::POST,
        &types,
        Some(&json!({"slug": "tasks", "definition": tasks()})),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);

    let filled = create(&owner, &project, &json!({"title": "Write docs"})).await;
    assert_eq!(filled.status, StatusCode::CREATED, "{}", filled.body);
    assert_eq!(
        filled.body["data"]["content"],
        json!({"title": "Write docs", "status": "todo", "estimate": 2, "link": "https://example.com/"})
    );
    let chosen = create(
        &owner,
        &project,
        &json!({"title": "Ship", "status": "done", "estimate": 5}),
    )
    .await;
    assert_eq!(
        chosen.body["data"]["content"],
        json!({"title": "Ship", "status": "done", "estimate": 5, "link": "https://example.com/"})
    );

    // A save sends the whole document: what it leaves out is gone, and required is required.
    let entry = filled.body["data"]["id"].as_str().expect("an id");
    let saved = call(
        &owner,
        Method::PUT,
        &format!("{}/collections/tasks/entries/{entry}", base(&project)),
        Some(&json!({"title": "Write docs"})),
        &[
            ("if-match", &filled.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request");
    assert_eq!(
        saved.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        saved.body
    );
    assert_eq!(saved.body["error"]["details"]["reason"], "missing_field");
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn defaults_fill_fields_a_role_may_not_set_and_it_still_may_not_set_them() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let writer = sign_in("dev-editor").await;
    let project = project(&owner).await;
    let made = send(
        &owner,
        Method::POST,
        &format!("{}/content-types", base(&project)),
        Some(&json!({"slug": "tasks", "definition": tasks()})),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let role = send(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/roles"),
        Some(&json!({
            "key": "titles-only",
            "name": "Titles only",
            "permissions": [
                {"action": "content.read", "subject": "tasks"},
                {"action": "content.create", "subject": "tasks", "fields": ["title"]}
            ]
        })),
    )
    .await;
    assert_eq!(role.status, StatusCode::CREATED, "{}", role.body);
    join(&owner, &writer, &project, "titles-only").await;

    let filled = create(&writer, &project, &json!({"title": "Triage"})).await;
    assert_eq!(filled.status, StatusCode::CREATED, "{}", filled.body);
    assert_eq!(filled.body["data"]["content"]["status"], "todo");
    let sent = create(
        &writer,
        &project,
        &json!({"title": "Triage", "status": "done"}),
    )
    .await;
    assert_eq!(
        sent.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        sent.body
    );
    assert_eq!(
        (
            &sent.body["error"]["details"]["reason"],
            &sent.body["error"]["details"]["pointer"]
        ),
        (&json!("field_not_permitted"), &json!("/status"))
    );
}
