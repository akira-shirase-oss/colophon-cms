//! Shared components, end to end (step M.4, D-073): a content type that uses a component holds
//! its fields, whatever copy it was sent; changing the component gives every type that uses it a
//! new version, checked against stored content, or changes nothing and names the type in the way;
//! and only a role that may change every content type may change a component. Needs the stack
//! and migrations.
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

async fn put(session: &Session, path: &str, version: u32, body: &Value) -> Reply {
    call(
        session,
        Method::PUT,
        path,
        Some(body),
        &[
            ("if-match", &format!("\"v{version}\"")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request")
}

fn seo(required: bool) -> Value {
    json!({"name": "SEO", "fields": [
        {"id": "meta_title", "label": "Meta title", "type": "text", "max_chars": 70, "required": required}
    ]})
}

fn pages() -> Value {
    json!({"name": "Pages", "title_field": "title", "fields": [
        {"id": "title", "label": "Title", "type": "text", "required": true},
        {"id": "seo", "label": "SEO", "type": "component", "component": "seo"}
    ]})
}

/// A project with the `seo` component and `pages` using it.
async fn set_up(owner: &Session) -> String {
    let project = project(owner).await;
    let made = send(
        owner,
        Method::POST,
        &format!("{}/components", base(&project)),
        Some(&json!({"slug": "seo", "definition": seo(false)})),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let typed = send(
        owner,
        Method::POST,
        &format!("{}/content-types", base(&project)),
        Some(&json!({"slug": "pages", "definition": pages()})),
    )
    .await;
    assert_eq!(typed.status, StatusCode::CREATED, "{}", typed.body);
    assert_eq!(
        typed.body["data"]["definition"]["fields"][1],
        json!({"id": "seo", "label": "SEO", "type": "object", "required": false, "component": "seo",
               "fields": [{"id": "meta_title", "label": "Meta title", "type": "text", "required": false, "max_chars": 70, "multiline": false}]})
    );
    project
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_component_change_reaches_every_type_that_uses_it_or_nothing_changes() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = set_up(&owner).await;
    let entries = format!("{}/collections/pages/entries", base(&project));
    let component = format!("{}/components/seo", base(&project));
    let home = send(
        &owner,
        Method::POST,
        &entries,
        Some(&json!({"title": "Home", "seo": {}})),
    )
    .await;
    assert_eq!(home.status, StatusCode::CREATED, "{}", home.body);
    let titled = send(
        &owner,
        Method::POST,
        &entries,
        Some(&json!({"title": "About", "seo": {"meta_title": "About us"}})),
    )
    .await;
    assert_eq!(titled.status, StatusCode::CREATED, "{}", titled.body);

    // Home has no meta title: requiring one is refused, named, and nothing changes.
    let refused = put(&owner, &component, 1, &seo(true)).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    let details = &refused.body["error"]["details"];
    assert_eq!(details["reason"], "needs_migration");
    assert_eq!(details["entries"][0]["title"], "Home");
    assert!(
        refused.body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("`pages` uses this component")),
        "{}",
        refused.body
    );
    let still = call(&owner, Method::GET, &component, None, &[])
        .await
        .expect("request");
    assert_eq!(still.etag().as_deref(), Some("\"v1\""));
    let pages_now = call(
        &owner,
        Method::GET,
        &format!("{}/content-types/pages", base(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(pages_now.etag().as_deref(), Some("\"v1\""));

    // A wider change goes through, and pages has a new version holding it.
    let mut wider = seo(false);
    wider["fields"]
        .as_array_mut()
        .expect("a list")
        .push(json!({"id": "noindex", "label": "Hide from search", "type": "boolean"}));
    let changed = put(&owner, &component, 1, &wider).await;
    assert_eq!(changed.status, StatusCode::OK, "{}", changed.body);
    assert_eq!(
        changed.body["data"]["content_types"],
        json!([{"slug": "pages", "version": 2}])
    );
    let hidden = send(
        &owner,
        Method::POST,
        &entries,
        Some(&json!({"title": "Drafts", "seo": {"noindex": true}})),
    )
    .await;
    assert_eq!(hidden.status, StatusCode::CREATED, "{}", hidden.body);
    let stale = put(&owner, &component, 1, &wider).await;
    assert_eq!(stale.status, StatusCode::CONFLICT, "{}", stale.body);
    assert_eq!(stale.body["error"]["details"]["reason"], "stale_version");
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_type_holds_the_components_current_fields_whatever_it_was_sent() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = set_up(&owner).await;
    let types = format!("{}/content-types", base(&project));

    let mut unknown = pages();
    unknown["fields"][1]["component"] = json!("nothing");
    let refused = send(
        &owner,
        Method::POST,
        &types,
        Some(&json!({"slug": "posts", "definition": unknown})),
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
            &json!("unknown_component"),
            &json!("/definition/fields/1/component")
        )
    );

    // A copy of the component's fields that says something else is replaced by the real ones.
    let mut forged = pages();
    forged["fields"][1] = json!({"id": "seo", "label": "SEO", "type": "object", "component": "seo",
                                 "fields": [{"id": "anything", "label": "Anything", "type": "json"}]});
    forged["fields"]
        .as_array_mut()
        .expect("a list")
        .push(json!({"id": "summary", "label": "Summary", "type": "text"}));
    let saved = put(&owner, &format!("{types}/pages"), 1, &forged).await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    assert_eq!(
        saved.body["data"]["definition"]["fields"][1]["fields"][0]["id"],
        "meta_title"
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn only_a_role_that_may_change_every_type_may_change_a_component() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let designer = sign_in("dev-editor").await;
    let project = set_up(&owner).await;
    let role = send(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/roles"),
        Some(&json!({
            "key": "pages-designer",
            "name": "Pages designer",
            "permissions": [
                {"action": "content.read", "subject": "*"},
                {"action": "schema.change", "subject": "pages"}
            ]
        })),
    )
    .await;
    assert_eq!(role.status, StatusCode::CREATED, "{}", role.body);
    join(&owner, &designer, &project, "pages-designer").await;

    let components = format!("{}/components", base(&project));
    let listed = call(&designer, Method::GET, &components, None, &[])
        .await
        .expect("request");
    assert_eq!(listed.status, StatusCode::OK, "any member reads components");
    let refused = put(&designer, &format!("{components}/seo"), 1, &seo(false)).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
    // Using one is a change to the type, which this role may make.
    let used = put(
        &designer,
        &format!("{}/content-types/pages", base(&project)),
        1,
        &pages(),
    )
    .await;
    assert_eq!(used.status, StatusCode::OK, "{}", used.body);
}
