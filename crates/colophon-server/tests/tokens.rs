//! API tokens and the audit log, end to end (step R.4, D-058, D-063): the real binary and real
//! people. A token does exactly what its permissions allow in its own project and nothing
//! elsewhere; nobody makes a token with more power than they hold; a token's secret is shown
//! once; revoking it stops it; what it writes names it; and every change to roles, members,
//! invitations and tokens is in the audit log, which only owners read and nobody edits.
//! Needs the stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{
    ORIGIN, Reply, Server, Session, app, article, call, entries, join, key, project, sign_in,
};
use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};

fn tokens(project: &str) -> String {
    format!("/api/v1/projects/{project}/tokens")
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

/// A request made with an API token's secret, as a script would send it.
async fn as_token(secret: &str, method: Method, path: &str, body: Option<&Value>) -> Reply {
    let mut request = app()
        .request(method, format!("{ORIGIN}{path}"))
        .header(header::AUTHORIZATION, format!("Bearer {secret}"))
        .header("idempotency-key", key());
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request.send().await.expect("request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json().await.unwrap_or(Value::Null);
    Reply {
        status,
        headers,
        body,
    }
}

/// Makes a token; its secret and its view.
async fn make(session: &Session, project: &str, body: &Value) -> (String, Value) {
    let made = send(session, Method::POST, &tokens(project), Some(body), &[]).await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let secret = made.body["data"]["secret"]
        .as_str()
        .expect("the secret, once")
        .to_owned();
    (secret, made.body["data"].clone())
}

fn reason(reply: &Reply) -> &Value {
    &reply.body["error"]["details"]["reason"]
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: make, use, refuse, revoke"
)]
async fn a_token_does_exactly_what_its_permissions_allow_and_stops_when_revoked() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let editor = sign_in("dev-editor").await;
    let other = project(&owner).await;
    let project = project(&owner).await;
    join(&owner, &editor, &project, "editor").await;

    // Read-only: reads articles, writes nothing. The secret appears once, prefixed.
    let (reader, view) = make(
        &owner,
        &project,
        &json!({"name": "Site build", "kind": "read-only", "expires_in_days": 30}),
    )
    .await;
    assert!(
        reader.starts_with("colophon_api_") && reader.len() == 56,
        "{reader}"
    );
    assert_eq!(
        (view["kind"].as_str(), view["state"].as_str()),
        (Some("read-only"), Some("active"))
    );
    let listed = send(&owner, Method::GET, &tokens(&project), None, &[]).await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert!(
        listed.body["data"][0].get("secret").is_none(),
        "a secret is never listed: {}",
        listed.body
    );
    assert_eq!(
        as_token(&reader, Method::GET, &entries(&project), None)
            .await
            .status,
        StatusCode::OK
    );
    let refused = as_token(
        &reader,
        Method::POST,
        &entries(&project),
        Some(&article("No", "x")),
    )
    .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
    assert_eq!(
        (
            reason(&refused),
            &refused.body["error"]["details"]["permission"]
        ),
        (&json!("missing_permission"), &json!("content.create"))
    );

    // Custom: may write articles, and what it writes names the token, not its maker.
    let (writer, writer_view) = make(
        &owner,
        &project,
        &json!({
            "name": "Importer", "kind": "custom", "expires_in_days": 7,
            "permissions": [
                {"action": "content.read", "subject": "articles"},
                {"action": "content.create", "subject": "articles"}
            ]
        }),
    )
    .await;
    let written = as_token(
        &writer,
        Method::POST,
        &entries(&project),
        Some(&article("Imported", "x")),
    )
    .await;
    assert_eq!(written.status, StatusCode::CREATED, "{}", written.body);
    assert_eq!(
        written.body["data"]["created_by"],
        writer_view["identity_id"]
    );

    // Only its own project's content: another project is unknown, people's routes are refused.
    assert_eq!(
        as_token(&writer, Method::GET, &entries(&other), None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    for path in [
        "/me".to_owned(),
        "/api/v1/projects".to_owned(),
        format!("/api/v1/projects/{project}/members"),
        tokens(&project),
        format!("/api/v1/projects/{project}/audit"),
    ] {
        let reply = as_token(&writer, Method::GET, &path, None).await;
        assert_eq!(
            (reply.status, reason(&reply)),
            (StatusCode::FORBIDDEN, &json!("api_token_not_allowed")),
            "{path}: {}",
            reply.body
        );
    }

    // Nobody makes a token that manages access or reads the log, and nobody grants what they
    // don't hold: editors can't make tokens at all.
    let manager = send(
        &owner,
        Method::POST,
        &tokens(&project),
        Some(&json!({
            "name": "Admin", "kind": "custom", "expires_in_days": 1,
            "permissions": [{"action": "members.manage", "subject": "*"}]
        })),
        &[],
    )
    .await;
    assert_eq!(
        (manager.status, reason(&manager)),
        (StatusCode::UNPROCESSABLE_ENTITY, &json!("not_for_tokens")),
        "{}",
        manager.body
    );
    let by_editor = send(
        &editor,
        Method::POST,
        &tokens(&project),
        Some(&json!({"name": "Mine", "kind": "read-only", "expires_in_days": 1})),
        &[],
    )
    .await;
    assert_eq!(
        (
            by_editor.status,
            &by_editor.body["error"]["details"]["permission"]
        ),
        (StatusCode::FORBIDDEN, &json!("tokens.manage"))
    );
    for days in [0, 367] {
        let expiry = send(
            &owner,
            Method::POST,
            &tokens(&project),
            Some(&json!({"name": "Forever", "kind": "read-only", "expires_in_days": days})),
            &[],
        )
        .await;
        assert_eq!(reason(&expiry), "invalid_expiry", "{days}: {}", expiry.body);
    }

    // Revoked: refused from the next request on; revoking again changes nothing.
    let path = format!(
        "{}/{}",
        tokens(&project),
        writer_view["id"].as_str().expect("an id")
    );
    let revoked = send(&owner, Method::DELETE, &path, None, &[]).await;
    assert_eq!(
        (revoked.status, revoked.body["data"]["state"].as_str()),
        (StatusCode::OK, Some("revoked")),
        "{}",
        revoked.body
    );
    assert_eq!(
        as_token(&writer, Method::GET, &entries(&project), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let again = send(&owner, Method::DELETE, &path, None, &[]).await;
    assert_eq!(
        again.body["data"]["revoked_at"],
        revoked.body["data"]["revoked_at"]
    );
    // A made-up secret is just as unknown.
    let forged = format!("colophon_api_{}", "A".repeat(43));
    assert_eq!(
        as_token(&forged, Method::GET, &entries(&project), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "every kind of change, then the log read back"
)]
async fn every_change_to_access_is_in_the_audit_log_which_only_owners_read() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let editor = sign_in("dev-editor").await;
    let project = project(&owner).await;
    let audit = format!("/api/v1/projects/{project}/audit");

    // A role made and changed, an invitation made and revoked, a member's role changed, a token
    // made and revoked.
    let roles = format!("/api/v1/projects/{project}/roles");
    let desk = json!({"key": "desk", "name": "Desk", "permissions": [{"action": "content.read", "subject": "articles"}]});
    assert_eq!(
        send(&owner, Method::POST, &roles, Some(&desk), &[])
            .await
            .status,
        StatusCode::CREATED
    );
    let changed = send(
        &owner,
        Method::PUT,
        &format!("{roles}/desk"),
        Some(&json!({"name": "Desk", "permissions": [{"action": "content.read", "subject": "*"}]})),
        &[("if-match", "\"v1\"")],
    )
    .await;
    assert_eq!(changed.status, StatusCode::OK, "{}", changed.body);
    let invited = send(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/invitations"),
        Some(&json!({"role": "desk", "label": "Spare"})),
        &[],
    )
    .await;
    let invitation = invited.body["data"]["id"]
        .as_str()
        .expect("an invitation")
        .to_owned();
    let link = invited.body["data"]["link"]
        .as_str()
        .expect("a link")
        .to_owned();
    assert_eq!(
        send(
            &owner,
            Method::DELETE,
            &format!("/api/v1/projects/{project}/invitations/{invitation}"),
            None,
            &[]
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    join(&owner, &editor, &project, "editor").await;
    let editor_id = send(&editor, Method::GET, "/me", None, &[]).await.body["data"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    assert_eq!(
        send(
            &owner,
            Method::PUT,
            &format!("/api/v1/projects/{project}/members/{editor_id}"),
            Some(&json!({"role": "desk"})),
            &[]
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let (secret, token) = make(
        &owner,
        &project,
        &json!({"name": "CI", "kind": "read-only", "expires_in_days": 1}),
    )
    .await;
    let token_id = token["id"].as_str().expect("an id");
    assert_eq!(
        send(
            &owner,
            Method::DELETE,
            &format!("{}/{token_id}", tokens(&project)),
            None,
            &[]
        )
        .await
        .status,
        StatusCode::OK
    );

    // Only owners read it.
    let refused = send(&editor, Method::GET, &audit, None, &[]).await;
    assert_eq!(
        (
            refused.status,
            &refused.body["error"]["details"]["permission"]
        ),
        (StatusCode::FORBIDDEN, &json!("audit.read"))
    );

    let read = send(&owner, Method::GET, &audit, None, &[]).await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    let events = read.body["data"].as_array().expect("events");
    let actions: Vec<&str> = events
        .iter()
        .map(|event| event["action"].as_str().expect("an action"))
        .collect();
    assert_eq!(
        actions,
        [
            "token.revoked",
            "token.created",
            "member.role_changed",
            "invitation.accepted",
            "invitation.created",
            "invitation.revoked",
            "invitation.created",
            "role.updated",
            "role.created",
        ],
        "{}",
        read.body
    );
    let role_change = &events[2];
    assert_eq!(role_change["subject"], editor_id.as_str());
    assert_eq!(
        (&role_change["detail"]["from"], &role_change["detail"]["to"]),
        (&json!("editor"), &json!("desk"))
    );
    assert_eq!(role_change["actor"]["kind"], "person");
    assert_eq!(
        events[3]["actor"]["id"],
        editor_id.as_str(),
        "the person who joined accepted"
    );
    assert_eq!(
        events[7]["detail"]["before"]["permissions"][0]["subject"],
        "articles"
    );
    assert_eq!(
        events[7]["detail"]["after"]["permissions"][0]["subject"],
        "*"
    );
    // No secret ever reaches the log: not a token's, not an invitation link's.
    let text = read.body.to_string();
    let invitation_secret = link.split('#').nth(1).expect("a fragment");
    assert!(
        !text.contains(&secret)
            && !text.contains(invitation_secret)
            && !text.contains("colophon_api_")
    );

    // Pages, newest first, and a filter.
    let first = send(&owner, Method::GET, &format!("{audit}?limit=4"), None, &[]).await;
    let cursor = first.body["next_cursor"].as_str().expect("another page");
    let second = send(
        &owner,
        Method::GET,
        &format!("{audit}?limit=4&cursor={cursor}"),
        None,
        &[],
    )
    .await;
    assert_eq!(second.body["data"][0]["action"], "invitation.created");
    let only = send(
        &owner,
        Method::GET,
        &format!("{audit}?action=token.created"),
        None,
        &[],
    )
    .await;
    assert_eq!(only.body["data"].as_array().map(Vec::len), Some(1));
}
