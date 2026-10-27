//! Roles owners define, end to end (D-058, step R.2): the real binary, two real people. An owner
//! defines a role, the role decides what its holder may do from their next request, a change
//! names the version it edited, a role in use can't be deleted, the owner role can't be changed,
//! permissions that nothing enforces yet are refused, and nobody hands out more than they hold.
//! Needs the stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Reply, Server, Session, article, call, entries, join, key, project, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

fn roles(project: &str) -> String {
    format!("/api/v1/projects/{project}/roles")
}

async fn send(
    session: &Session,
    method: Method,
    path: &str,
    body: Option<&Value>,
    extra: &[(&str, &str)],
) -> Reply {
    call(session, method, path, body, extra)
        .await
        .expect("request")
}

/// The project's owner's and its one other member's identity ids (owners are listed first).
async fn ids(owner: &Session, project: &str) -> (String, String) {
    let listed = send(
        owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/members"),
        None,
        &[],
    )
    .await;
    let id = |index: usize| {
        listed.body["data"][index]["identity_id"]
            .as_str()
            .expect("an id")
            .to_owned()
    };
    (id(0), id(1))
}

async fn write_article(session: &Session, project: &str) -> StatusCode {
    send(
        session,
        Method::POST,
        &entries(project),
        Some(&article("By a reader?", "Body")),
        &[("idempotency-key", &key())],
    )
    .await
    .status
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: define, hold, change, refuse, delete"
)]
async fn a_role_the_owner_defines_decides_what_its_holder_may_do() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let guest = sign_in("dev-editor").await;
    let project = project(&owner).await;

    // A reader of articles, and nothing else.
    let made = send(
        &owner,
        Method::POST,
        &roles(&project),
        Some(&json!({
            "key": "reader",
            "name": "Reader",
            "description": "Reads articles.",
            "permissions": [{"action": "content.read", "subject": "articles"}]
        })),
        &[],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    assert_eq!(made.etag().as_deref(), Some("\"v1\""));
    assert_eq!(made.body["data"]["fixed"], false);
    join(&owner, &guest, &project, "reader").await;

    let listed = send(&guest, Method::GET, &roles(&project), None, &[]).await;
    let keys: Vec<&str> = listed.body["data"]
        .as_array()
        .expect("roles")
        .iter()
        .filter_map(|role| role["key"].as_str())
        .collect();
    assert_eq!(
        keys,
        ["owner", "developer", "editor", "publisher", "reader"]
    );
    assert_eq!(listed.body["data"][4]["members"], 1);
    assert_eq!(listed.body["data"][0]["fixed"], true);

    let read = send(&guest, Method::GET, &entries(&project), None, &[]).await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    assert_eq!(write_article(&guest, &project).await, StatusCode::FORBIDDEN);

    // The owner lets readers write articles; the guest's next request may.
    let reader = format!("{}/reader", roles(&project));
    let writer = json!({
        "name": "Article writer",
        "permissions": [
            {"action": "content.read", "subject": "articles"},
            {"action": "content.create", "subject": "articles"},
            {"action": "content.update", "subject": "articles"}
        ]
    });
    let changed = send(
        &owner,
        Method::PUT,
        &reader,
        Some(&writer),
        &[("if-match", "\"v1\"")],
    )
    .await;
    assert_eq!(changed.status, StatusCode::OK, "{}", changed.body);
    assert_eq!(changed.etag().as_deref(), Some("\"v2\""));
    assert_eq!(write_article(&guest, &project).await, StatusCode::CREATED);

    // An edit of an older version is refused, and says which version is current.
    let stale = send(
        &owner,
        Method::PUT,
        &reader,
        Some(&writer),
        &[("if-match", "\"v1\"")],
    )
    .await;
    assert_eq!(stale.status, StatusCode::CONFLICT, "{}", stale.body);
    assert_eq!(stale.body["error"]["details"]["reason"], "stale_role");
    assert_eq!(stale.etag().as_deref(), Some("\"v2\""));
    let unconditional = send(&owner, Method::PUT, &reader, Some(&writer), &[]).await;
    assert_eq!(unconditional.status, StatusCode::PRECONDITION_REQUIRED);

    // Held, it can't be deleted; the owner role can't be changed at all.
    let held = send(
        &owner,
        Method::DELETE,
        &reader,
        None,
        &[("if-match", "\"v2\"")],
    )
    .await;
    assert_eq!(held.status, StatusCode::CONFLICT, "{}", held.body);
    assert_eq!(held.body["error"]["details"]["reason"], "role_in_use");
    let owner_role = format!("{}/owner", roles(&project));
    let fixed = send(
        &owner,
        Method::PUT,
        &owner_role,
        Some(&writer),
        &[("if-match", "\"v1\"")],
    )
    .await;
    assert_eq!(fixed.status, StatusCode::CONFLICT, "{}", fixed.body);

    // What nothing enforces yet is refused, not ignored.
    for (permission, reason) in [
        (
            json!({"action": "content.everything", "subject": "*"}),
            "unknown_action",
        ),
        (
            json!({"action": "members.manage", "subject": "articles"}),
            "project_wide_action",
        ),
        (
            json!({"action": "content.read", "subject": "articles", "fields": ["title"]}),
            "not_yet_enforced",
        ),
        (
            json!({"action": "content.update", "subject": "*", "condition": "own"}),
            "not_yet_enforced",
        ),
    ] {
        let refused = send(
            &owner,
            Method::POST,
            &roles(&project),
            Some(&json!({"key": "odd", "name": "Odd", "permissions": [permission]})),
            &[],
        )
        .await;
        assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            refused.body["error"]["details"]["reason"],
            reason,
            "{refused:?}",
            refused = refused.body
        );
    }

    // Once nobody holds it, it goes.
    let (_, guest_id) = ids(&owner, &project).await;
    let member = format!("/api/v1/projects/{project}/members/{guest_id}");
    let moved = send(
        &owner,
        Method::PUT,
        &member,
        Some(&json!({"role": "editor"})),
        &[],
    )
    .await;
    assert_eq!(moved.status, StatusCode::NO_CONTENT, "{}", moved.body);
    let deleted = send(
        &owner,
        Method::DELETE,
        &reader,
        None,
        &[("if-match", "\"v2\"")],
    )
    .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    let gone = send(&owner, Method::GET, &reader, None, &[]).await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn nobody_hands_out_more_than_they_hold() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let lead = sign_in("dev-editor").await;
    let project = project(&owner).await;
    let made = send(
        &owner,
        Method::POST,
        &roles(&project),
        Some(&json!({
            "key": "team-lead",
            "name": "Team lead",
            "permissions": [
                {"action": "members.manage", "subject": "*"},
                {"action": "content.read", "subject": "*"},
                {"action": "content.create", "subject": "*"},
                {"action": "content.update", "subject": "*"}
            ]
        })),
        &[],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    join(&owner, &lead, &project, "team-lead").await;

    let invitations = format!("/api/v1/projects/{project}/invitations");
    for role in ["owner", "developer", "publisher"] {
        let refused = send(
            &lead,
            Method::POST,
            &invitations,
            Some(&json!({"role": role, "label": "Someone"})),
            &[],
        )
        .await;
        assert_eq!(refused.status, StatusCode::FORBIDDEN, "inviting as {role}");
    }
    let allowed = send(
        &lead,
        Method::POST,
        &invitations,
        Some(&json!({"role": "editor", "label": "Someone"})),
        &[],
    )
    .await;
    assert_eq!(allowed.status, StatusCode::CREATED, "{}", allowed.body);
    let unknown = send(
        &lead,
        Method::POST,
        &invitations,
        Some(&json!({"role": "nobody", "label": "Someone"})),
        &[],
    )
    .await;
    assert_eq!(unknown.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(unknown.body["error"]["details"]["reason"], "unknown_role");

    let (owner_id, lead_id) = ids(&owner, &project).await;
    let promote = send(
        &lead,
        Method::PUT,
        &format!("/api/v1/projects/{project}/members/{lead_id}"),
        Some(&json!({"role": "owner"})),
        &[],
    )
    .await;
    assert_eq!(promote.status, StatusCode::FORBIDDEN, "{}", promote.body);
    let remove_owner = send(
        &lead,
        Method::DELETE,
        &format!("/api/v1/projects/{project}/members/{owner_id}"),
        None,
        &[],
    )
    .await;
    assert_eq!(remove_owner.status, StatusCode::FORBIDDEN);

    // Managing members isn't managing roles.
    let role = send(
        &lead,
        Method::POST,
        &roles(&project),
        Some(&json!({"key": "mine", "name": "Mine", "permissions": []})),
        &[],
    )
    .await;
    assert_eq!(role.status, StatusCode::FORBIDDEN, "{}", role.body);
}
