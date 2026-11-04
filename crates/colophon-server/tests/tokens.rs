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
    as_token_with(secret, method, path, body, &[]).await
}

/// [`as_token`] with extra headers, such as the `If-Match` a review or publish asks for.
async fn as_token_with(
    secret: &str,
    method: Method,
    path: &str,
    body: Option<&Value>,
    extra: &[(&str, &str)],
) -> Reply {
    let mut request = app()
        .request(method, format!("{ORIGIN}{path}"))
        .header(header::AUTHORIZATION, format!("Bearer {secret}"))
        .header("idempotency-key", key());
    for (name, value) in extra {
        request = request.header(*name, *value);
    }
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

/// `POST {changeset}/{action}` as `session`, `If-Match` naming the hash they saw.
async fn act(session: &Session, path: &str, hash: &str, body: Option<&Value>) -> Reply {
    call(
        session,
        Method::POST,
        path,
        body,
        &[("if-match", &format!("\"{hash}\""))],
    )
    .await
    .expect("request")
}

/// [`act`] as the API token `secret`.
async fn act_as(secret: &str, path: &str, hash: &str, body: Option<&Value>) -> Reply {
    as_token_with(
        secret,
        Method::POST,
        path,
        body,
        &[("if-match", &format!("\"{hash}\""))],
    )
    .await
}

fn state(reply: &Reply) -> &str {
    reply.body["data"]["status"]["state"]
        .as_str()
        .unwrap_or("?")
}

fn hash(reply: &Reply) -> String {
    reply.body["data"]["hash"]
        .as_str()
        .expect("a hash")
        .to_owned()
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
    // It can ask what it may do, as can the owner.
    let own = format!("/api/v1/projects/{project}/environments/production/permissions");
    let mine = as_token(&writer, Method::GET, &own, None).await;
    assert_eq!(mine.status, StatusCode::OK, "{}", mine.body);
    assert_eq!(
        (
            &mine.body["data"]["kind"],
            &mine.body["data"]["role"],
            mine.body["data"]["permissions"].as_array().map(Vec::len)
        ),
        (&json!("token"), &Value::Null, Some(2))
    );
    let owner_view = send(&owner, Method::GET, &own, None, &[]).await;
    assert_eq!(
        (
            &owner_view.body["data"]["kind"],
            &owner_view.body["data"]["role"]
        ),
        (&json!("person"), &json!("owner"))
    );
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

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "the whole rule, in order: same token, another token, the person, a stage"
)]
async fn a_token_can_never_approve_a_token_submitted_changeset() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let base = format!("/api/v1/projects/{project}/environments/production");
    let sets = format!("{base}/changesets");

    // Two tokens that may draft, review and publish: as powerful as a token can be (D-063).
    let (agent, _) = make(
        &owner,
        &project,
        &json!({"name": "Writer", "kind": "full-access", "expires_in_days": 1}),
    )
    .await;
    let (second, _) = make(
        &owner,
        &project,
        &json!({"name": "Approver", "kind": "full-access", "expires_in_days": 1}),
    )
    .await;

    // The token drafts an entry, pins it in a changeset and submits it: `submitted_by` is the
    // token's own identity.
    let drafted = as_token(
        &agent,
        Method::POST,
        &entries(&project),
        Some(&article("Written by the agent", "x")),
    )
    .await;
    assert_eq!(drafted.status, StatusCode::CREATED, "{}", drafted.body);
    let entry = drafted.body["data"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let created = as_token(
        &agent,
        Method::POST,
        &sets,
        Some(&json!({"title": "The agent's proposal", "entries": [entry]})),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let path = format!(
        "{sets}/{}",
        created.body["data"]["id"].as_str().expect("an id")
    );
    let pinned = hash(&created);
    let submitted = act_as(&agent, &format!("{path}/submit"), &pinned, None).await;
    assert_eq!(state(&submitted), "awaiting_review", "{}", submitted.body);

    // Approving it would be submit → approve → publish by one credential with no person in the
    // loop: the refusal is typed, and it is a 403 — the permission exists, the rule forbids it.
    let own = act_as(
        &agent,
        &format!("{path}/reviews"),
        &pinned,
        Some(&json!({"decision": "approve"})),
    )
    .await;
    assert_eq!(
        (own.status, reason(&own)),
        (StatusCode::FORBIDDEN, &json!("agent_self_review")),
        "{}",
        own.body
    );

    // A second token is refused exactly the same way: minting another credential is free, so
    // the check is token-ness on both sides, not identity equality (D-111).
    let other = act_as(
        &second,
        &format!("{path}/reviews"),
        &pinned,
        Some(&json!({"decision": "approve"})),
    )
    .await;
    assert_eq!(
        (other.status, reason(&other)),
        (StatusCode::FORBIDDEN, &json!("agent_self_review")),
        "{}",
        other.body
    );

    // What a token may still do: ask for changes — a review that keeps it a proposal.
    let asked = act_as(
        &second,
        &format!("{path}/reviews"),
        &pinned,
        Some(&json!({"decision": "request_changes", "comment": "Needs a person's eyes"})),
    )
    .await;
    assert_eq!(state(&asked), "changes_requested", "{}", asked.body);

    // A person approves the token's proposal — the loop has a person in it — and then the token
    // may publish it: autonomous publishing is the explicit grant it already holds.
    let approved = act(
        &owner,
        &format!("{path}/reviews"),
        &pinned,
        Some(&json!({"decision": "approve"})),
    )
    .await;
    assert_eq!(state(&approved), "approved", "{}", approved.body);
    let releases = send(&owner, Method::GET, &format!("{base}/releases"), None, &[]).await;
    let current: i64 = releases
        .headers
        .get("colophon-generation")
        .and_then(|value| value.to_str().ok()?.parse().ok())
        .expect("a generation");
    let published = as_token_with(
        &agent,
        Method::POST,
        &format!("{path}/publish"),
        Some(&json!({"base_generation": current})),
        &[("if-match", &format!("\"{pinned}\""))],
    )
    .await;
    assert_eq!(
        published.status,
        StatusCode::CREATED,
        "a person approved it, so the token's publish grant applies: {}",
        published.body
    );

    // The rule is one-sided: a token may approve a person's changeset under the same
    // `content.review` grant, because the person is already in the loop.
    let theirs = common::create_article(&owner, &project, "The owner's piece").await;
    let set = call(
        &owner,
        Method::POST,
        &sets,
        Some(&json!({"title": "The owner's proposal", "entries": [theirs]})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(set.status, StatusCode::CREATED, "{}", set.body);
    let own_path = format!("{sets}/{}", set.body["data"]["id"].as_str().expect("an id"));
    let own_hash = hash(&set);
    act(&owner, &format!("{own_path}/submit"), &own_hash, None).await;
    let by_token = act_as(
        &second,
        &format!("{own_path}/reviews"),
        &own_hash,
        Some(&json!({"decision": "approve"})),
    )
    .await;
    assert_eq!(
        state(&by_token),
        "approved",
        "a token may approve what a person submitted: {}",
        by_token.body
    );

    // With stages configured the rule is the same, stage by stage: the token's named approval
    // on a token's changeset is refused, the person's lands.
    let shown = send(
        &owner,
        Method::GET,
        &format!("{base}/review-stages"),
        None,
        &[],
    )
    .await;
    let staged = send(
        &owner,
        Method::PUT,
        &format!("{base}/review-stages"),
        Some(&json!({"stages": [{"key": "qa", "name": "QA"}]})),
        &[
            ("if-match", &shown.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await;
    assert_eq!(staged.status, StatusCode::OK, "{}", staged.body);
    let more = as_token(
        &agent,
        Method::POST,
        &entries(&project),
        Some(&article("Staged work", "x")),
    )
    .await;
    let staged_set = as_token(
        &agent,
        Method::POST,
        &sets,
        Some(&json!({
            "title": "Staged proposal",
            "entries": [more.body["data"]["id"].as_str().expect("an id")]
        })),
    )
    .await;
    assert_eq!(
        staged_set.status,
        StatusCode::CREATED,
        "{}",
        staged_set.body
    );
    let stage_path = format!(
        "{sets}/{}",
        staged_set.body["data"]["id"].as_str().expect("an id")
    );
    let stage_hash = hash(&staged_set);
    act_as(&agent, &format!("{stage_path}/submit"), &stage_hash, None).await;
    let at_stage = act_as(
        &agent,
        &format!("{stage_path}/reviews"),
        &stage_hash,
        Some(&json!({"decision": "approve", "stage": "qa"})),
    )
    .await;
    assert_eq!(
        (at_stage.status, reason(&at_stage)),
        (StatusCode::FORBIDDEN, &json!("agent_self_review")),
        "a named stage changes nothing: {}",
        at_stage.body
    );
    let by_person = act(
        &owner,
        &format!("{stage_path}/reviews"),
        &stage_hash,
        Some(&json!({"decision": "approve", "stage": "qa"})),
    )
    .await;
    assert_eq!(state(&by_person), "approved", "{}", by_person.body);
}
