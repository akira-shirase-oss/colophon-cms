//! Single types, end to end (step M.4, D-067): a content type marked single holds at most one
//! entry. A second is refused, however two creations race; a type with two entries can't become
//! single; one with an entry can, and stops being single on request. Needs the stack and
//! migrations.
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

fn settings(single: bool) -> Value {
    json!({"name": "Settings", "title_field": "name", "single": single, "fields": [
        {"id": "name", "label": "Site name", "type": "text", "required": true}
    ]})
}

async fn define(owner: &Session, project: &str, slug: &str, definition: &Value) -> Reply {
    call(
        owner,
        Method::POST,
        &format!("{}/content-types", base(project)),
        Some(&json!({"slug": slug, "definition": definition})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request")
}

async fn change(
    owner: &Session,
    project: &str,
    slug: &str,
    version: u32,
    definition: &Value,
) -> Reply {
    call(
        owner,
        Method::PUT,
        &format!("{}/content-types/{slug}", base(project)),
        Some(definition),
        &[
            ("if-match", &format!("\"v{version}\"")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request")
}

async fn create(owner: &Session, project: &str, slug: &str, name: &str) -> Reply {
    call(
        owner,
        Method::POST,
        &format!("{}/collections/{slug}/entries", base(project)),
        Some(&json!({"name": name})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request")
}

fn reason(reply: &Reply) -> &Value {
    &reply.body["error"]["details"]["reason"]
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_single_type_holds_one_entry_however_creations_race() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;

    // Single from the start: two creations at once, and exactly one wins.
    let made = define(&owner, &project, "site", &settings(true)).await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let (one, two) = tokio::join!(
        create(&owner, &project, "site", "First"),
        create(&owner, &project, "site", "Second")
    );
    let mut statuses = [one.status, two.status];
    statuses.sort();
    assert_eq!(
        statuses,
        [StatusCode::CREATED, StatusCode::CONFLICT],
        "{} {}",
        one.body,
        two.body
    );
    let refused = if one.status == StatusCode::CONFLICT {
        &one
    } else {
        &two
    };
    assert_eq!(reason(refused), "single_exists");
    let again = create(&owner, &project, "site", "Third").await;
    assert_eq!(reason(&again), "single_exists");

    // A collection with two entries can't become single.
    assert_eq!(
        define(&owner, &project, "notes", &settings(false))
            .await
            .status,
        StatusCode::CREATED
    );
    for name in ["A", "B"] {
        assert_eq!(
            create(&owner, &project, "notes", name).await.status,
            StatusCode::CREATED
        );
    }
    let too_many = change(&owner, &project, "notes", 1, &settings(true)).await;
    assert_eq!(
        (
            too_many.status,
            reason(&too_many),
            &too_many.body["error"]["details"]["pointer"]
        ),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            &json!("too_many_entries"),
            &json!("/single")
        ),
        "{}",
        too_many.body
    );

    // One with a single entry can; then a second is refused, until it stops being single.
    assert_eq!(
        define(&owner, &project, "home", &settings(false))
            .await
            .status,
        StatusCode::CREATED
    );
    assert_eq!(
        create(&owner, &project, "home", "Home").await.status,
        StatusCode::CREATED
    );
    let became = change(&owner, &project, "home", 1, &settings(true)).await;
    assert_eq!(became.status, StatusCode::OK, "{}", became.body);
    assert_eq!(
        reason(&create(&owner, &project, "home", "Other").await),
        "single_exists"
    );
    let back = change(&owner, &project, "home", 2, &settings(false)).await;
    assert_eq!(back.status, StatusCode::OK, "{}", back.body);
    assert_eq!(
        create(&owner, &project, "home", "Other").await.status,
        StatusCode::CREATED
    );
}
