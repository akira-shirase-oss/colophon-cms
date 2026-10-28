//! Conditional fields, end to end (step M.4, D-072): a field applies only while another field
//! holds one of some values. While it applies its rules hold, required included; otherwise a
//! value for it is refused where it is, so stored content never keeps what editors can't see;
//! and making a field conditional is refused while stored entries hold it where it wouldn't
//! apply. Needs the stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Reply, Server, Session, call, key, project, sign_in};
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

/// Media whose `duration` applies to some kinds, or to every kind.
fn media(when: Option<Value>) -> Value {
    let mut duration =
        json!({"id": "duration", "label": "Duration", "type": "number", "integer": true});
    if let Some(when) = when {
        duration["when"] = when;
        duration["required"] = json!(true);
    }
    json!({
        "name": "Media",
        "title_field": "title",
        "fields": [
            {"id": "title", "label": "Title", "type": "text", "required": true},
            {"id": "kind", "label": "Kind", "type": "enum", "values": ["article", "video"], "required": true},
            duration
        ]
    })
}

fn reason(reply: &Reply) -> (StatusCode, Value, Value) {
    let details = &reply.body["error"]["details"];
    (
        reply.status,
        details["reason"].clone(),
        details["pointer"].clone(),
    )
}

/// A project with `media` defined as `definition`; the project and its entries' path.
async fn with_media(owner: &Session, definition: Value) -> (String, String) {
    let project = project(owner).await;
    let made = send(
        owner,
        Method::POST,
        &format!("{}/content-types", base(&project)),
        Some(&json!({"slug": "media", "definition": definition})),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let entries = format!("{}/collections/media/entries", base(&project));
    (project, entries)
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_field_applies_only_while_its_condition_holds() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (_, entries) =
        with_media(&owner, media(Some(json!({"field": "kind", "is": "video"})))).await;
    let clip = send(
        &owner,
        Method::POST,
        &entries,
        Some(&json!({"title": "Clip", "kind": "video", "duration": 90})),
    )
    .await;
    assert_eq!(clip.status, StatusCode::CREATED, "{}", clip.body);
    let untimed = send(
        &owner,
        Method::POST,
        &entries,
        Some(&json!({"title": "Clip", "kind": "video"})),
    )
    .await;
    assert_eq!(
        reason(&untimed),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!("missing_field"),
            json!("/duration")
        )
    );
    let essay = send(
        &owner,
        Method::POST,
        &entries,
        Some(&json!({"title": "Essay", "kind": "article"})),
    )
    .await;
    assert_eq!(
        essay.status,
        StatusCode::CREATED,
        "not required where it doesn't apply: {}",
        essay.body
    );
    let timed_essay = send(
        &owner,
        Method::POST,
        &entries,
        Some(&json!({"title": "Essay 2", "kind": "article", "duration": 5})),
    )
    .await;
    assert_eq!(
        reason(&timed_essay),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!("not_applicable"),
            json!("/duration")
        )
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn making_a_field_conditional_waits_for_entries_that_hold_it_where_it_wont_apply() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, entries) = with_media(&owner, media(None)).await;
    let essay = send(
        &owner,
        Method::POST,
        &entries,
        Some(&json!({"title": "Essay", "kind": "article", "duration": 5})),
    )
    .await;
    assert_eq!(essay.status, StatusCode::CREATED, "{}", essay.body);

    let only_video = media(Some(json!({"field": "kind", "is": "video"})));
    let path = format!("{}/content-types/media", base(&project));
    let change = || {
        let (owner, path, definition) = (&owner, path.clone(), only_video.clone());
        async move {
            call(
                owner,
                Method::PUT,
                &path,
                Some(&definition),
                &[("if-match", "\"v1\""), ("idempotency-key", &key())],
            )
            .await
            .expect("request")
        }
    };
    let refused = change().await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"]["reason"],
        "needs_migration"
    );
    assert_eq!(
        refused.body["error"]["details"]["entries"][0]["problem"]["reason"],
        "not_applicable"
    );
    let cleared = call(
        &owner,
        Method::PUT,
        &format!(
            "{entries}/{}",
            essay.body["data"]["id"].as_str().expect("an id")
        ),
        Some(&json!({"title": "Essay", "kind": "article"})),
        &[
            ("if-match", &essay.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request");
    assert_eq!(cleared.status, StatusCode::OK, "{}", cleared.body);
    let accepted = change().await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.body);
    assert_eq!(
        accepted.body["data"]["definition"]["fields"][2]["when"],
        json!({"field": "kind", "is": ["video"]})
    );
}
