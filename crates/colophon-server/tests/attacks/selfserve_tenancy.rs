//! Self-serve tenant isolation (D-235, SAAS_PLAN S1's exit gate): two accounts that are no
//! administrators each create a project of their own on an installation with open registration
//! (the development stack's). Each sees and changes only its own, through every API family —
//! management, content, assets, MCP, audience administration, previews, destinations, tokens,
//! roles and members — and the project list shows each only its own. Everything outside answers
//! 404 `not_found`, never 403, so nothing confirms another account's project exists.

use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};

use super::{anonymous, as_token, send, token};
use crate::common::{
    ORIGIN, Reply, Server, Session, app, article, call, entries, sign_in, unique_slug,
};

/// One account's corner: its session, its project, a draft in it, and a full-access token.
struct Tenant {
    session: Session,
    project: String,
    entry: String,
    token: String,
}

async fn tenant() -> Tenant {
    let session = sign_in(&format!("iso-{}", unique_slug())).await;
    let made = send(
        &session,
        Method::POST,
        "/api/v1/projects",
        Some(&json!({"slug": unique_slug(), "name": "Isolated"})),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let project = made.body["data"]["id"].as_str().expect("an id").to_owned();
    let draft = send(
        &session,
        Method::POST,
        &entries(&project),
        Some(&article("Only mine", "Private")),
    )
    .await;
    assert_eq!(draft.status, StatusCode::CREATED, "{}", draft.body);
    let entry = draft.body["data"]["id"].as_str().expect("an id").to_owned();
    let token = token(
        &session,
        &project,
        &json!({"name": "Mine", "kind": "full-access", "expires_in_days": 1}),
    )
    .await;
    Tenant {
        session,
        project,
        entry,
        token,
    }
}

/// The project's rows, by API family: what its owner reads with 200 and everyone else gets 404.
fn readable(t: &Tenant) -> Vec<String> {
    let p = format!("/api/v1/projects/{}", t.project);
    let e = format!("{p}/environments/production");
    let entry = format!("{}/{}", entries(&t.project), t.entry);
    vec![
        // Management.
        p.clone(),
        format!("{e}/content-types"),
        entries(&t.project),
        entry.clone(),
        format!("{entry}/revisions"),
        format!("{entry}/comments"),
        format!("{e}/schedules"),
        format!("{e}/changesets"),
        // Assets.
        format!("{e}/assets"),
        // Previews and delivery destinations.
        format!("{e}/previews"),
        format!("{e}/destinations"),
        // Access.
        format!("{p}/members"),
        format!("{p}/invitations"),
        format!("{p}/roles"),
        format!("{p}/tokens"),
        format!("{p}/audit"),
        // Audience administration.
        format!("{p}/audience/users"),
        format!("{p}/audience/roles"),
        format!("{p}/audience/origins"),
    ]
}

/// The paths a token may use at all: its project's `environments/` routes (D-063), previews
/// aside (people open those; a token gets 403 even in its own project).
fn token_paths(t: &Tenant) -> Vec<String> {
    readable(t)
        .into_iter()
        .filter(|path| path.contains("/environments/") && !path.ends_with("/previews"))
        .collect()
}

/// A token's request with extra headers (an `If-Match`, so a save reaches the project check).
async fn token_call(
    secret: &str,
    method: Method,
    path: &str,
    body: Option<&Value>,
    extra: &[(&str, &str)],
) -> Reply {
    let mut request = app()
        .request(method, format!("{ORIGIN}{path}"))
        .header(header::AUTHORIZATION, format!("Bearer {secret}"));
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

fn not_found(reply: &Reply, path: &str, who: &str) {
    assert_eq!(
        (reply.status, reply.body["error"]["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("not_found")),
        "{who} at {path}: {}",
        reply.body
    );
}

/// An MCP `initialize` with a project token on `project`'s endpoint.
async fn mcp_initialize(secret: &str, project: &str) -> Reply {
    let response = app()
        .post(format!(
            "{ORIGIN}/api/v1/projects/{project}/environments/production/mcp"
        ))
        .header(header::AUTHORIZATION, format!("Bearer {secret}"))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-11-25")
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "iso", "version": "0"}}}))
        .send()
        .await
        .expect("request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json().await.unwrap_or(Value::Null);
    Reply {
        status,
        headers,
        body,
    }
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn two_self_serve_accounts_each_read_only_their_own_project_through_every_family() {
    let _server = Server::start_with(&[("COLOPHON_REGISTRATION", "open")]).await;
    let a = tenant().await;
    let b = tenant().await;
    for (mine, theirs, who) in [(&a, &b, "account A"), (&b, &a, "account B")] {
        for path in readable(theirs) {
            // The positive control: the row is there, and its owner reads it.
            let as_owner = send(&theirs.session, Method::GET, &path, None).await;
            assert_eq!(
                as_owner.status,
                StatusCode::OK,
                "owner at {path}: {}",
                as_owner.body
            );
            let as_other = send(&mine.session, Method::GET, &path, None).await;
            not_found(&as_other, &path, who);
            let as_nobody = anonymous(Method::GET, &path, None).await;
            assert_eq!(as_nobody.status, StatusCode::UNAUTHORIZED, "{path}");
        }
        for path in token_paths(theirs) {
            let as_owner_token = as_token(&theirs.token, Method::GET, &path, None).await;
            assert_eq!(
                as_owner_token.status,
                StatusCode::OK,
                "owner's token at {path}: {}",
                as_owner_token.body
            );
            let as_other_token = as_token(&mine.token, Method::GET, &path, None).await;
            not_found(&as_other_token, &path, &format!("{who}'s token"));
        }
        // MCP: their token opens a session on their endpoint; mine doesn't reach it.
        let theirs_opens = mcp_initialize(&theirs.token, &theirs.project).await;
        assert_eq!(theirs_opens.status, StatusCode::OK, "{}", theirs_opens.body);
        let mine_refused = mcp_initialize(&mine.token, &theirs.project).await;
        not_found(&mine_refused, "mcp", &format!("{who}'s token"));
    }
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_self_serve_account_writes_nothing_into_another_account_s_project() {
    let _server = Server::start_with(&[("COLOPHON_REGISTRATION", "open")]).await;
    let a = tenant().await;
    let b = tenant().await;
    let p = format!("/api/v1/projects/{}", b.project);
    let e = format!("{p}/environments/production");
    let writes: Vec<(Method, String, Value)> = vec![
        (Method::POST, entries(&b.project), article("Planted", "x")),
        (
            Method::PUT,
            format!("{}/{}", entries(&b.project), b.entry),
            article("Changed by A", "x"),
        ),
        (
            Method::POST,
            format!("{}/{}/comments", entries(&b.project), b.entry),
            json!({"revision": uuid::Uuid::now_v7(), "body": "planted"}),
        ),
        (
            Method::DELETE,
            format!("{}/{}", entries(&b.project), b.entry),
            Value::Null,
        ),
        (
            Method::POST,
            format!("{p}/tokens"),
            json!({"name": "Planted", "kind": "full-access", "expires_in_days": 1}),
        ),
        (
            Method::POST,
            format!("{p}/invitations"),
            json!({"role": "owner", "label": "planted"}),
        ),
        (
            Method::POST,
            format!("{p}/audience/users"),
            json!({"email": "planted@example.com", "password": "planted-password-123", "role": "member"}),
        ),
        (
            Method::POST,
            format!("{e}/destinations"),
            json!({"name": "planted", "url": "https://example.com/hook"}),
        ),
        (
            Method::POST,
            format!("{e}/previews"),
            json!({"entry": b.entry}),
        ),
        (Method::DELETE, format!("{p}/members/x"), Value::Null),
    ];
    // A save needs a well-formed If-Match before anything else (428 / 400 for any entry, real or
    // not): A gets the real ETag of B's draft, as if leaked, and still reaches only a 404.
    let b_draft = send(
        &b.session,
        Method::GET,
        &format!("{}/{}", entries(&b.project), b.entry),
        None,
    )
    .await;
    let etag = b_draft
        .headers
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .expect("the draft's ETag")
        .to_owned();
    for (n, (method, path, body)) in writes.iter().enumerate() {
        let body = (!body.is_null()).then_some(body);
        let key = format!("isolation-write-{n:04}-{}", unique_slug());
        let extra: &[(&str, &str)] = &[("if-match", etag.as_str()), ("idempotency-key", &key)];
        let by_session = call(&a.session, method.clone(), path, body, extra)
            .await
            .expect("request");
        not_found(&by_session, path, &format!("account A's {method}"));
        if path.contains("/environments/") && !path.ends_with("/previews") {
            let token_key = format!("{key}-token");
            let extra: &[(&str, &str)] =
                &[("if-match", etag.as_str()), ("idempotency-key", &token_key)];
            let by_token = token_call(&a.token, method.clone(), path, body, extra).await;
            not_found(&by_token, path, &format!("account A's token {method}"));
        }
    }
    // Nothing landed: B's project holds its one draft, unchanged, and its own token alone.
    let listed = send(&b.session, Method::GET, &entries(&b.project), None).await;
    let rows = listed.body["data"].as_array().expect("entries");
    assert_eq!(rows.len(), 1, "{}", listed.body);
    let draft = send(
        &b.session,
        Method::GET,
        &format!("{}/{}", entries(&b.project), b.entry),
        None,
    )
    .await;
    assert_eq!(
        draft.body["data"]["content"]["title"], "Only mine",
        "{}",
        draft.body
    );
    let tokens = send(&b.session, Method::GET, &format!("{p}/tokens"), None).await;
    assert_eq!(
        tokens.body["data"].as_array().map(Vec::len),
        Some(1),
        "{}",
        tokens.body
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn the_project_list_and_delivery_show_each_account_only_its_own() {
    let _server = Server::start_with(&[("COLOPHON_REGISTRATION", "open")]).await;
    let a = tenant().await;
    let b = tenant().await;
    for (mine, theirs) in [(&a, &b), (&b, &a)] {
        let listed = send(
            &mine.session,
            Method::GET,
            "/api/v1/projects?limit=100",
            None,
        )
        .await;
        assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
        let ids: Vec<&str> = listed.body["data"]
            .as_array()
            .expect("a page")
            .iter()
            .filter_map(|p| p["id"].as_str())
            .collect();
        assert_eq!(ids, [mine.project.as_str()], "{}", listed.body);
        // Their draft isn't delivered to anyone: not anonymously, not with my token.
        let delivery = format!(
            "/content/v1/{}/production/articles/{}",
            theirs.project, theirs.entry
        );
        let public = anonymous(Method::GET, &delivery, None).await;
        assert_eq!(public.status, StatusCode::NOT_FOUND, "{}", public.body);
        let with_mine = as_token(&mine.token, Method::GET, &delivery, None).await;
        assert_eq!(
            with_mine.status,
            StatusCode::NOT_FOUND,
            "{}",
            with_mine.body
        );
    }
}
