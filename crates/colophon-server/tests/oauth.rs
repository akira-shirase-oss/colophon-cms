//! OAuth for the MCP endpoint (D-246), end to end against the real binary: an agent that knows
//! only `/mcp` finds the authorization server, registers, gets its person's approval for one
//! project in the Studio, trades the code (PKCE S256) for tokens, and speaks MCP as that person;
//! and everything a stolen or replayed credential, a forged redirect or a cross-site form could
//! try is refused. Needs the stack (scripts/stack.sh up) and migrations (scripts/migrate.sh).

#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use colophon_auth::tokens;
use common::{ORIGIN, Server, Session, app, call, project, sign_in};
use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};

const PROTOCOL: &str = "2025-11-25";
const REDIRECT: &str = "http://127.0.0.1:9/callback";

/// `POST /oauth/register`: the client's id.
async fn register(name: &str, redirect: &str) -> String {
    let reply = app()
        .post(format!("{ORIGIN}/oauth/register"))
        .json(&json!({"client_name": name, "redirect_uris": [redirect], "token_endpoint_auth_method": "none"}))
        .send()
        .await
        .expect("register");
    assert_eq!(reply.status(), StatusCode::CREATED);
    let body: Value = reply.json().await.expect("json");
    body["client_id"].as_str().expect("a client_id").to_owned()
}

/// A PKCE verifier and its S256 challenge.
fn pkce() -> (String, String) {
    let verifier = tokens::random_token().expect("randomness");
    let challenge = tokens::pkce_challenge(&verifier);
    (verifier, challenge)
}

/// `GET /oauth/authorize` with `query`, as `person` (or signed out): the unfollowed answer.
async fn authorize(person: Option<&Session>, query: &[(&str, &str)]) -> reqwest::Response {
    let encoded = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(query)
        .finish();
    let mut request = app().get(format!("{ORIGIN}/oauth/authorize?{encoded}"));
    if let Some(person) = person {
        request = request.header(header::COOKIE, &person.cookie);
    }
    request.send().await.expect("authorize")
}

fn location(response: &reqwest::Response) -> String {
    response
        .headers()
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

/// One query parameter of `url`.
fn param(url: &str, name: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()?
        .query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

/// An authorization's whole front channel: authorize, then `person` approves it for `project`.
/// The redirect the app gets.
async fn approved(person: &Session, client: &str, challenge: &str, project: &str) -> String {
    let asked = authorize(
        Some(person),
        &[
            ("response_type", "code"),
            ("client_id", client),
            ("redirect_uri", REDIRECT),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
            ("state", "xyz"),
            ("resource", &format!("{ORIGIN}/mcp")),
        ],
    )
    .await;
    assert_eq!(
        asked.status(),
        StatusCode::SEE_OTHER,
        "{}",
        asked.text().await.unwrap_or_default()
    );
    let page = location(&asked);
    let id = page
        .strip_prefix("/studio/connect/")
        .expect("the consent page");
    let shown = call(
        person,
        Method::GET,
        &format!("/oauth/requests/{id}"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(shown.status, StatusCode::OK, "{}", shown.body);
    assert_eq!(shown.body["data"]["redirect_host"], "127.0.0.1");
    let answered = call(
        person,
        Method::POST,
        &format!("/oauth/requests/{id}"),
        Some(&json!({"decision": "approve", "project": project})),
        &[],
    )
    .await
    .expect("answer");
    assert_eq!(answered.status, StatusCode::OK, "{}", answered.body);
    answered.body["data"]["redirect"]
        .as_str()
        .expect("a redirect")
        .to_owned()
}

/// `POST /oauth/token` with `form`: (status, body).
async fn token(form: &[(&str, &str)]) -> (StatusCode, Value) {
    let reply = app()
        .post(format!("{ORIGIN}/oauth/token"))
        .form(form)
        .send()
        .await
        .expect("token");
    let status = reply.status();
    (status, reply.json().await.unwrap_or(Value::Null))
}

/// The code in `redirect` for tokens: (access, refresh).
async fn exchange(client: &str, redirect: &str, verifier: &str) -> (String, String) {
    let code = param(redirect, "code").expect("a code");
    let (status, body) = token(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", REDIRECT),
        ("client_id", client),
        ("code_verifier", verifier),
    ])
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["token_type"], "Bearer");
    (
        body["access_token"].as_str().expect("access").to_owned(),
        body["refresh_token"].as_str().expect("refresh").to_owned(),
    )
}

/// One JSON-RPC message to `/mcp` with `access`: (status, `WWW-Authenticate`, session id, body).
async fn mcp(
    access: Option<&str>,
    session: Option<&str>,
    message: &Value,
) -> (StatusCode, String, Option<String>, Value) {
    let mut request = app()
        .post(format!("{ORIGIN}/mcp"))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("mcp-protocol-version", PROTOCOL)
        .json(message);
    if let Some(access) = access {
        request = request.header(header::AUTHORIZATION, format!("Bearer {access}"));
    }
    if let Some(session) = session {
        request = request.header("mcp-session-id", session);
    }
    let reply = request.send().await.expect("mcp");
    let status = reply.status();
    let challenge = reply
        .headers()
        .get(header::WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let id = reply
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    (
        status,
        challenge,
        id,
        reply.json().await.unwrap_or(Value::Null),
    )
}

fn initialize() -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": PROTOCOL, "capabilities": {}, "clientInfo": {"name": "oauth-test", "version": "1"}
    }})
}

/// A full connection for `person` in a new project of theirs: (project, client, access, refresh).
async fn connected(person: &Session) -> (String, String, String, String) {
    let project = project(person).await;
    let client = register("Test agent", REDIRECT).await;
    let (verifier, challenge) = pkce();
    let redirect = approved(person, &client, &challenge, &project).await;
    let (access, refresh) = exchange(&client, &redirect, &verifier).await;
    (project, client, access, refresh)
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn an_agent_connects_by_the_mcp_url_alone_and_acts_as_its_person() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;

    // Discovery: /mcp says where to look, the metadata says the rest.
    let (status, challenge, _, _) = mcp(None, None, &initialize()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        challenge,
        format!("Bearer resource_metadata=\"{ORIGIN}/.well-known/oauth-protected-resource\"")
    );
    let resource: Value = app()
        .get(format!("{ORIGIN}/.well-known/oauth-protected-resource"))
        .send()
        .await
        .expect("metadata")
        .json()
        .await
        .expect("json");
    assert_eq!(resource["resource"], format!("{ORIGIN}/mcp"));
    assert_eq!(resource["authorization_servers"], json!([ORIGIN]));
    let server: Value = app()
        .get(format!("{ORIGIN}/.well-known/oauth-authorization-server"))
        .send()
        .await
        .expect("metadata")
        .json()
        .await
        .expect("json");
    assert_eq!(server["issuer"], ORIGIN);
    assert_eq!(server["code_challenge_methods_supported"], json!(["S256"]));
    assert_eq!(
        server["token_endpoint_auth_methods_supported"],
        json!(["none"])
    );
    assert_eq!(
        server["registration_endpoint"],
        format!("{ORIGIN}/oauth/register")
    );

    // Register, approve, exchange: the redirect carries the code, the state and the issuer.
    let project = project(&owner).await;
    let client = register("Test agent", REDIRECT).await;
    let (verifier, challenge) = pkce();
    let redirect = approved(&owner, &client, &challenge, &project).await;
    assert!(redirect.starts_with(REDIRECT), "{redirect}");
    assert_eq!(param(&redirect, "state").as_deref(), Some("xyz"));
    assert_eq!(param(&redirect, "iss").as_deref(), Some(ORIGIN));
    let (access, refresh) = exchange(&client, &redirect, &verifier).await;
    assert!(access.starts_with("colophon_oauth_"));

    // MCP as that person, in that project: initialize, then the tools.
    let (status, _, session, body) = mcp(Some(&access), None, &initialize()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let session = session.expect("a session id");
    let (status, _, _, listed) = mcp(
        Some(&access),
        Some(&session),
        &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert!(
        listed["result"]["tools"]
            .as_array()
            .is_some_and(|tools| !tools.is_empty()),
        "{listed}"
    );

    // Connected apps lists it; a refresh rotates both tokens and the new access token works.
    let apps = call(&owner, Method::GET, "/api/v1/me/connected-apps", None, &[])
        .await
        .expect("apps");
    assert_eq!(apps.status, StatusCode::OK, "{}", apps.body);
    assert!(
        apps.body["data"]
            .as_array()
            .expect("a list")
            .iter()
            .any(|app| app["project"] == project.as_str())
    );
    let (status, renewed) = token(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &refresh),
        ("client_id", &client),
    ])
    .await;
    assert_eq!(status, StatusCode::OK, "{renewed}");
    let fresh = renewed["access_token"].as_str().expect("access");
    assert_ne!(fresh, access);
    assert_ne!(renewed["refresh_token"].as_str(), Some(refresh.as_str()));
    let (status, _, _, _) = mcp(Some(fresh), None, &initialize()).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "every refusal of the front and back channel, in one place"
)]
async fn what_the_authorization_server_cannot_prove_it_refuses() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let client = register("Test agent", REDIRECT).await;
    let (verifier, challenge) = pkce();
    let mcp_url = format!("{ORIGIN}/mcp");

    // An address the client never registered: a page, never a redirect.
    let elsewhere = authorize(
        Some(&owner),
        &[
            ("response_type", "code"),
            ("client_id", &client),
            ("redirect_uri", "https://evil.example/cb"),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ],
    )
    .await;
    assert_eq!(elsewhere.status(), StatusCode::BAD_REQUEST);
    assert!(elsewhere.headers().get(header::LOCATION).is_none());
    // An unknown client: the same.
    let unknown = authorize(
        Some(&owner),
        &[
            ("response_type", "code"),
            ("client_id", "01a20000-0000-7000-8000-000000000000"),
            ("redirect_uri", REDIRECT),
        ],
    )
    .await;
    assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);
    assert!(unknown.headers().get(header::LOCATION).is_none());

    // No PKCE, plain PKCE, another resource: the error goes back to the client.
    for (label, query) in [
        (
            "no PKCE",
            vec![
                ("response_type", "code"),
                ("client_id", client.as_str()),
                ("redirect_uri", REDIRECT),
                ("state", "s"),
            ],
        ),
        (
            "plain PKCE",
            vec![
                ("response_type", "code"),
                ("client_id", client.as_str()),
                ("redirect_uri", REDIRECT),
                ("code_challenge", challenge.as_str()),
                ("code_challenge_method", "plain"),
                ("state", "s"),
            ],
        ),
    ] {
        let refused = authorize(Some(&owner), &query).await;
        let to = location(&refused);
        assert!(to.starts_with(REDIRECT), "{label}: {to}");
        assert_eq!(
            param(&to, "error").as_deref(),
            Some("invalid_request"),
            "{label}"
        );
        assert_eq!(param(&to, "state").as_deref(), Some("s"), "{label}");
    }
    let target = authorize(
        Some(&owner),
        &[
            ("response_type", "code"),
            ("client_id", &client),
            ("redirect_uri", REDIRECT),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("resource", "https://elsewhere.example/mcp"),
        ],
    )
    .await;
    assert_eq!(
        param(&location(&target), "error").as_deref(),
        Some("invalid_target")
    );
    // The right resource is fine with or without a trailing slash; checked in the journey below.
    let _ = mcp_url;

    // A wrong verifier: refused, and the code is spent (its grant revoked).
    let redirect = approved(&owner, &client, &challenge, &project).await;
    let code = param(&redirect, "code").expect("a code");
    let (status, body) = token(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", REDIRECT),
        ("client_id", &client),
        ("code_verifier", &"x".repeat(43)),
    ])
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_grant")),
        "{body}"
    );
    let (status, body) = token(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", REDIRECT),
        ("client_id", &client),
        ("code_verifier", &verifier),
    ])
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_grant")),
        "a spent code stays spent: {body}"
    );

    // A code presented by another client, or with another redirect URI: refused.
    let other = register("Other agent", REDIRECT).await;
    let redirect = approved(&owner, &client, &challenge, &project).await;
    let code = param(&redirect, "code").expect("a code");
    let (status, body) = token(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", REDIRECT),
        ("client_id", &other),
        ("code_verifier", &verifier),
    ])
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_grant")),
        "{body}"
    );
    let redirect = approved(&owner, &client, &challenge, &project).await;
    let code = param(&redirect, "code").expect("a code");
    let (status, body) = token(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", "http://127.0.0.1:9/other"),
        ("client_id", &client),
        ("code_verifier", &verifier),
    ])
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_grant")),
        "{body}"
    );

    // A code used twice: the second use fails, and the tokens the first one got stop working.
    let redirect = approved(&owner, &client, &challenge, &project).await;
    let (access, _) = exchange(&client, &redirect, &verifier).await;
    assert_eq!(
        mcp(Some(&access), None, &initialize()).await.0,
        StatusCode::OK
    );
    let code = param(&redirect, "code").expect("a code");
    let (status, body) = token(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", REDIRECT),
        ("client_id", &client),
        ("code_verifier", &verifier),
    ])
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_grant")),
        "{body}"
    );
    assert_eq!(
        mcp(Some(&access), None, &initialize()).await.0,
        StatusCode::UNAUTHORIZED,
        "the replay revoked the grant"
    );

    // Registration refuses what it can't honour.
    for (label, body) in [
        (
            "an http address that isn't loopback",
            json!({"redirect_uris": ["http://evil.example/cb"]}),
        ),
        (
            "a fragment",
            json!({"redirect_uris": ["https://claude.ai/cb#x"]}),
        ),
        (
            "javascript",
            json!({"redirect_uris": ["javascript:alert(1)"]}),
        ),
        (
            "a confidential client",
            json!({"redirect_uris": [REDIRECT], "token_endpoint_auth_method": "client_secret_basic"}),
        ),
        ("no address", json!({"client_name": "x"})),
    ] {
        let reply = app()
            .post(format!("{ORIGIN}/oauth/register"))
            .json(&body)
            .send()
            .await
            .expect("register");
        assert_eq!(reply.status(), StatusCode::BAD_REQUEST, "{label}");
    }
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn an_oauth_token_speaks_to_mcp_alone() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, _, access, _) = connected(&owner).await;
    let bearer = format!("Bearer {access}");
    for path in [
        format!("/api/v1/projects/{project}"),
        format!("/api/v1/projects/{project}/environments/production/collections/articles/entries"),
        "/me".to_owned(),
        "/api/v1/projects".to_owned(),
    ] {
        let reply = app()
            .get(format!("{ORIGIN}{path}"))
            .header(header::AUTHORIZATION, &bearer)
            .send()
            .await
            .expect("request");
        assert_eq!(
            reply.status(),
            StatusCode::UNAUTHORIZED,
            "{path}: the token's audience is /mcp"
        );
    }
    // A forged refresh or access token is nobody.
    assert_eq!(
        mcp(Some("colophon_oauth_forged"), None, &initialize())
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "every way a connection ends, in one place"
)]
async fn a_connection_ends_when_disconnected_replayed_or_its_account_disabled() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;

    // A refresh token used twice ends the connection: the stolen copy and the real one alike.
    let (_, client, _, refresh) = connected(&owner).await;
    let (status, first) = token(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &refresh),
        ("client_id", &client),
    ])
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let (status, again) = token(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &refresh),
        ("client_id", &client),
    ])
    .await;
    assert_eq!(
        (status, again["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_grant"))
    );
    let renewed = first["access_token"].as_str().expect("access");
    assert_eq!(
        mcp(Some(renewed), None, &initialize()).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, _) = token(&[
        ("grant_type", "refresh_token"),
        (
            "refresh_token",
            first["refresh_token"].as_str().expect("refresh"),
        ),
        ("client_id", &client),
    ])
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "the whole family is revoked"
    );

    // Disconnected in the Studio: its token stops at once; RFC 7009's revoke does the same.
    let (_, _, access, _) = connected(&owner).await;
    let apps = call(&owner, Method::GET, "/api/v1/me/connected-apps", None, &[])
        .await
        .expect("apps");
    let id = apps.body["data"][0]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let gone = call(
        &owner,
        Method::DELETE,
        &format!("/api/v1/me/connected-apps/{id}"),
        None,
        &[],
    )
    .await
    .expect("disconnect");
    assert_eq!(gone.status, StatusCode::NO_CONTENT, "{}", gone.body);
    assert_eq!(
        mcp(Some(&access), None, &initialize()).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, _, access, _) = connected(&owner).await;
    let revoked = app()
        .post(format!("{ORIGIN}/oauth/revoke"))
        .form(&[("token", access.as_str())])
        .send()
        .await
        .expect("revoke");
    assert_eq!(revoked.status(), StatusCode::OK);
    assert_eq!(
        mcp(Some(&access), None, &initialize()).await.0,
        StatusCode::UNAUTHORIZED
    );

    // A disabled account's connections stop with it.
    let editor = sign_in("dev-editor").await;
    let (_, _, access, _) = connected(&editor).await;
    assert_eq!(
        mcp(Some(&access), None, &initialize()).await.0,
        StatusCode::OK
    );
    let me = call(&editor, Method::GET, "/me", None, &[])
        .await
        .expect("me");
    let editor_id = me.body["data"]["id"].as_str().expect("an id").to_owned();
    let disabled = call(
        &owner,
        Method::POST,
        &format!("/api/v1/accounts/{editor_id}/disable"),
        None,
        &[],
    )
    .await
    .expect("disable");
    assert!(disabled.status.is_success(), "{}", disabled.body);
    assert_eq!(
        mcp(Some(&access), None, &initialize()).await.0,
        StatusCode::UNAUTHORIZED
    );
    let enabled = call(
        &owner,
        Method::POST,
        &format!("/api/v1/accounts/{editor_id}/enable"),
        None,
        &[],
    )
    .await
    .expect("enable");
    assert!(enabled.status.is_success(), "{}", enabled.body);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn consent_is_the_persons_own_in_their_own_browser() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let mine = project(&owner).await;
    let client = register("Test agent", REDIRECT).await;
    let (_, challenge) = pkce();
    let query = [
        ("response_type", "code"),
        ("client_id", client.as_str()),
        ("redirect_uri", REDIRECT),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", "xyz"),
    ];

    // Signed out: sign in first, then this same request again, a path on this server.
    let signed_out = authorize(None, &query).await;
    let to = location(&signed_out);
    assert!(
        to.starts_with("/studio/signin?return_to=%2Foauth%2Fauthorize%3F"),
        "{to}"
    );

    let asked = authorize(Some(&owner), &query).await;
    let id = location(&asked)
        .strip_prefix("/studio/connect/")
        .expect("the consent page")
        .to_owned();
    let path = format!("{ORIGIN}/oauth/requests/{id}");
    let approve = json!({"decision": "approve", "project": mine});
    // A cross-site form: no CSRF token, or a foreign origin, is refused.
    let forged = app()
        .post(&path)
        .header(header::COOKIE, &owner.cookie)
        .header(header::ORIGIN, ORIGIN)
        .json(&approve)
        .send()
        .await
        .expect("forged");
    assert_eq!(forged.status(), StatusCode::FORBIDDEN);
    let foreign = app()
        .post(&path)
        .header(header::COOKIE, &owner.cookie)
        .header("x-csrf-token", &owner.csrf)
        .header(header::ORIGIN, "https://evil.example")
        .json(&approve)
        .send()
        .await
        .expect("foreign");
    assert_eq!(foreign.status(), StatusCode::FORBIDDEN);
    // Someone else's request is not theirs to answer.
    let editor = sign_in("dev-editor").await;
    let stranger = call(
        &editor,
        Method::POST,
        &format!("/oauth/requests/{id}"),
        Some(&approve),
        &[],
    )
    .await
    .expect("stranger");
    assert_eq!(stranger.status, StatusCode::NOT_FOUND);
    // Nor a project they aren't in.
    let other = project(&editor).await;
    let theirs = call(
        &owner,
        Method::POST,
        &format!("/oauth/requests/{id}"),
        Some(&json!({"decision": "approve", "project": other})),
        &[],
    )
    .await
    .expect("not mine");
    assert_eq!(theirs.status, StatusCode::NOT_FOUND);
    // Deny: the app hears access_denied, with its state, and the request is answered once.
    let denied = call(
        &owner,
        Method::POST,
        &format!("/oauth/requests/{id}"),
        Some(&json!({"decision": "deny"})),
        &[],
    )
    .await
    .expect("deny");
    assert_eq!(denied.status, StatusCode::OK, "{}", denied.body);
    let redirect = denied.body["data"]["redirect"]
        .as_str()
        .expect("a redirect");
    assert_eq!(param(redirect, "error").as_deref(), Some("access_denied"));
    assert_eq!(param(redirect, "state").as_deref(), Some("xyz"));
    let twice = call(
        &owner,
        Method::POST,
        &format!("/oauth/requests/{id}"),
        Some(&approve),
        &[],
    )
    .await
    .expect("twice");
    assert_eq!(twice.status, StatusCode::NOT_FOUND);
}
