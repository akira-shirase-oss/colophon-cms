//! The remote MCP endpoint, end to end (D-121, docs/modules/09-automation.md): `{P}/mcp` over
//! MCP's Streamable HTTP transport, where the agent is a project API token and nothing else.
//! The handshake mints an `Mcp-Session-Id`, `tools/list` and `tools/call` are the same eighteen
//! safe tools stdio serves — with the same JSON-RPC refusals — and the transport's own door
//! checks (a trusted `Origin`, an `Accept` that allows `application/json`, the
//! `MCP-Protocol-Version` header, a session only its own token may use) all hold. Needs the
//! stack, the migrations, and `colophon-mcp` built beside `colophon-server` for the parity
//! half (`cargo test --workspace` builds both).
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use std::{path::PathBuf, process::Stdio, time::Duration};

use common::{ORIGIN, Reply, Server, Session, app, article, call, project, sign_in};
use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader, Lines},
    process::{ChildStdin, ChildStdout},
    time::timeout,
};

/// The protocol version this server speaks — the one the `initialize` body and every later
/// `MCP-Protocol-Version` header names.
const PROTOCOL: &str = "2025-11-25";

/// `{P}/mcp` of `project`'s production environment.
fn mcp_path(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production/mcp")
}

/// `POST {P}/tokens` as `session`; the new token's secret and id (the secret is shown once).
async fn token(session: &Session, project: &str, kind: &str) -> (String, String) {
    let made = call(
        session,
        Method::POST,
        &format!("/api/v1/projects/{project}/tokens"),
        Some(&json!({"name": "MCP agent", "kind": kind, "expires_in_days": 1})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    (
        made.body["data"]["secret"]
            .as_str()
            .expect("once")
            .to_owned(),
        made.body["data"]["id"].as_str().expect("an id").to_owned(),
    )
}

/// A personal access token for `approver`, through the device grant the CLI uses — the
/// credential a remote-MCP request must never be taken for.
async fn personal_token(approver: &Session) -> String {
    let started = app()
        .post(format!("{ORIGIN}/auth/device"))
        .json(&json!({"client_name": "mcp test"}))
        .send()
        .await
        .expect("request");
    assert_eq!(started.status(), StatusCode::CREATED);
    let body: Value = started.json().await.expect("json");
    let (secret, code) = (
        body["data"]["device_secret"]
            .as_str()
            .expect("a secret")
            .to_owned(),
        body["data"]["user_code"]
            .as_str()
            .expect("a code")
            .to_owned(),
    );
    let approval = call(
        approver,
        Method::POST,
        "/auth/device/approve",
        Some(&json!({"user_code": code})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(approval.status, StatusCode::NO_CONTENT, "{}", approval.body);
    let polled = app()
        .post(format!("{ORIGIN}/auth/device/token"))
        .json(&json!({"device_secret": secret}))
        .send()
        .await
        .expect("request");
    let body: Value = polled.json().await.expect("json");
    assert_eq!(body["data"]["status"], "approved", "{body}");
    body["data"]["access_token"]
        .as_str()
        .expect("a personal access token")
        .to_owned()
}

/// `POST {P}/mcp` as the agent `secret`: `Authorization`, the transport's `Accept`, the
/// `MCP-Protocol-Version` header, and `session`'s id when one exists. `drop` names a default
/// header to withhold, `extra` adds or overrides one — the door-check tests bend the defaults
/// this way.
async fn post(
    secret: &str,
    path: &str,
    session: Option<&str>,
    body: &[u8],
    drop: &[&str],
    extra: &[(&str, &str)],
) -> Reply {
    let mut request = app()
        .post(format!("{ORIGIN}{path}"))
        .header(header::AUTHORIZATION, format!("Bearer {secret}"))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::CONTENT_TYPE, "application/json")
        .header("mcp-protocol-version", PROTOCOL)
        .body(body.to_vec());
    if let Some(session) = session {
        request = request.header("mcp-session-id", session);
    }
    let mut built = request.build().expect("a request");
    for name in drop {
        built.headers_mut().remove(*name);
    }
    for (name, value) in extra {
        built.headers_mut().insert(
            header::HeaderName::try_from(*name).expect("a header name"),
            header::HeaderValue::from_str(value).expect("a header value"),
        );
    }
    let response = app().execute(built).await.expect("request");
    Reply {
        status: response.status(),
        headers: response.headers().clone(),
        body: response.json().await.unwrap_or(Value::Null),
    }
}

/// A remote-MCP session on `secret`: the session id `initialize` mints, and request ids.
struct Remote {
    secret: String,
    path: String,
    session: Option<String>,
    next: i64,
}

impl Remote {
    fn new(secret: &str, project: &str) -> Self {
        Self {
            secret: secret.to_owned(),
            path: mcp_path(project),
            session: None,
            next: 0,
        }
    }

    /// The `initialize` handshake: it mints the `Mcp-Session-Id` this client then carries.
    async fn initialize(&mut self) -> Value {
        self.next += 1;
        let reply = post(
            &self.secret,
            &self.path,
            None,
            self.requested("initialize", &init_params()).as_bytes(),
            &[],
            &[],
        )
        .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        self.session = Some(
            reply
                .headers
                .get("mcp-session-id")
                .and_then(|value| value.to_str().ok())
                .expect("initialize mints an Mcp-Session-Id")
                .to_owned(),
        );
        reply.body
    }

    /// The JSON-RPC request line for `method` with the next id.
    fn requested(&self, method: &str, params: &Value) -> String {
        json!({"jsonrpc": "2.0", "id": self.next, "method": method, "params": params}).to_string()
    }

    /// Sends a notification; the reply, which the transport says is a 202 with no body.
    async fn notified(&self, method: &str) -> Reply {
        post(
            &self.secret,
            &self.path,
            self.session.as_deref(),
            json!({"jsonrpc": "2.0", "method": method})
                .to_string()
                .as_bytes(),
            &[],
            &[],
        )
        .await
    }

    /// Sends a request; the JSON-RPC response body (callers check `result` or `error`).
    async fn request(&mut self, method: &str, params: &Value) -> Value {
        self.next += 1;
        let id = self.next;
        let reply = post(
            &self.secret,
            &self.path,
            self.session.as_deref(),
            self.requested(method, params).as_bytes(),
            &[],
            &[],
        )
        .await;
        assert_eq!(reply.status, StatusCode::OK, "{method}: {}", reply.body);
        assert_eq!(reply.body["id"], json!(id), "the answer is to {id}");
        reply.body
    }

    /// A `tools/call` for `name` with `arguments`; the JSON-RPC response.
    async fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", &json!({"name": name, "arguments": arguments}))
            .await
    }

    /// `DELETE {P}/mcp`: end the session early.
    async fn delete(&self) -> Reply {
        let mut request = app()
            .delete(format!("{ORIGIN}{}", self.path))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.secret))
            .header("mcp-protocol-version", PROTOCOL);
        if let Some(session) = &self.session {
            request = request.header("mcp-session-id", session);
        }
        let response = request.send().await.expect("request");
        Reply {
            status: response.status(),
            headers: response.headers().clone(),
            body: response.json().await.unwrap_or(Value::Null),
        }
    }
}

/// The `initialize` params any MCP client sends.
fn init_params() -> Value {
    json!({
        "protocolVersion": PROTOCOL,
        "capabilities": {},
        "clientInfo": {"name": "mcp-test", "version": "1"}
    })
}

/// The tool names a `tools/list` result carries.
fn tool_names(listed: &Value) -> Vec<String> {
    listed["result"]["tools"]
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// The eighteen tools `tools/list` may offer — reads, drafts, proposals; nothing that
/// approves, publishes, resolves or migrates (the stdio boundary, unchanged over HTTP).
const ALL_TOOLS: [&str; 18] = [
    "list_content_types",
    "list_entries",
    "get_entry",
    "query_entries",
    "create_draft",
    "update_draft",
    "create_publish_request",
    "request_review",
    "get_publish_request",
    "list_releases",
    "get_release",
    "list_migrations",
    "get_migration",
    "list_migration_items",
    "list_comments",
    "create_comment",
    "list_tasks",
    "create_task",
];

/// Names an agent might guess at — the privileged verbs MCP must not know, over HTTP either.
const FORBIDDEN_TOOLS: [&str; 7] = [
    "approve",
    "publish",
    "unpublish",
    "resolve_comment",
    "update_task",
    "apply_migration",
    "create_token",
];

/// The whole agent lifecycle over Streamable HTTP: handshake, the session id, the tool
/// table stdio serves, a read and a write through `tools/call`, the refusal boundary, and
/// `DELETE` ending it — after which the id is a stranger's.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: handshake, work, refuse, end"
)]
async fn an_agent_lives_over_streamable_http() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let (agent, _) = token(&owner, &project, "full-access").await;

    let mut remote = Remote::new(&agent, &project);
    let hello = remote.initialize().await;
    assert_eq!(
        hello["result"]["protocolVersion"].as_str(),
        Some(PROTOCOL),
        "{hello}"
    );
    assert_eq!(
        hello["result"]["serverInfo"]["name"].as_str(),
        Some("colophon-mcp"),
        "{hello}"
    );
    // A notification is accepted with nothing to answer: 202, no body.
    let ready = remote.notified("notifications/initialized").await;
    assert_eq!(ready.status, StatusCode::ACCEPTED, "{}", ready.body);
    assert!(ready.body.is_null(), "{}", ready.body);
    // And a stray response is taken the same way.
    let stray = post(
        &agent,
        &mcp_path(&project),
        remote.session.as_deref(),
        json!({"jsonrpc": "2.0", "id": 99, "result": {}})
            .to_string()
            .as_bytes(),
        &[],
        &[],
    )
    .await;
    assert_eq!(stray.status, StatusCode::ACCEPTED, "{}", stray.body);

    // Discovery: the eighteen safe tools, none of the privileged names an agent might guess.
    let listed = remote.request("tools/list", &json!({})).await;
    let tools = tool_names(&listed);
    assert_eq!(tools, ALL_TOOLS, "{listed}");
    for name in FORBIDDEN_TOOLS {
        assert!(!tools.iter().any(|tool| tool == name), "{name} listed");
    }

    // Real work: read the content types, then make the draft and proposal an agent may.
    let types = remote.call("list_content_types", json!({})).await;
    assert_eq!(types["result"]["isError"], json!(false), "{types}");
    let drafted = remote
        .call(
            "create_draft",
            json!({"collection": "articles", "content": article("Over remote MCP", "x")}),
        )
        .await;
    assert_eq!(drafted["result"]["isError"], json!(false), "{drafted}");
    let entry = drafted["result"]["structuredContent"]["entry"]["id"]
        .as_str()
        .expect("a draft id")
        .to_owned();
    let proposed = remote
        .call(
            "create_publish_request",
            json!({"title": "An agent's proposal", "entries": [entry]}),
        )
        .await;
    assert_eq!(proposed["result"]["isError"], json!(false), "{proposed}");

    // The boundary holds exactly as on stdio: guessed names are -32602, bad arguments are
    // -32602 too, an unknown method is -32601, and `ping` answers {}.
    for name in FORBIDDEN_TOOLS {
        let refused = remote.call(name, json!({})).await;
        assert_eq!(
            refused["error"]["code"].as_i64(),
            Some(-32602),
            "{name}: {refused}"
        );
        assert!(refused.get("result").is_none(), "{refused}");
    }
    // A real tool with bad arguments is a typed tool error — `isError` with
    // `structuredContent.error.code` — as the API's refusals arrive, while a malformed
    // `tools/call` envelope is the JSON-RPC `-32602`.
    let invalid = remote
        .call("create_draft", json!({"collection": "no such!"}))
        .await;
    assert_eq!(invalid["result"]["isError"], json!(true), "{invalid}");
    assert_eq!(
        invalid["result"]["structuredContent"]["error"]["code"].as_str(),
        Some("invalid_arguments"),
        "{invalid}"
    );
    let malformed = remote.request("tools/call", &json!({"name": 42})).await;
    assert_eq!(
        malformed["error"]["code"].as_i64(),
        Some(-32602),
        "{malformed}"
    );
    let method = remote.request("resources/list", &json!({})).await;
    assert_eq!(method["error"]["code"].as_i64(), Some(-32601), "{method}");
    let ping = remote.request("ping", &json!({})).await;
    assert_eq!(ping["result"], json!({}), "{ping}");
    let again = remote.request("initialize", &init_params()).await;
    assert_eq!(again["error"]["code"].as_i64(), Some(-32600), "{again}");

    // Nothing the agent did published anything: the owner sees no release.
    let releases = call(
        &owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/environments/production/releases"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        releases.body["data"].as_array().map(Vec::len),
        Some(0),
        "{}",
        releases.body
    );

    // `DELETE` ends the session; its id is then as unknown as one never minted.
    assert_eq!(remote.delete().await.status, StatusCode::OK);
    let after = remote.notified("notifications/initialized").await;
    assert_eq!(after.status, StatusCode::NOT_FOUND, "{}", after.body);
}

/// Only a project API token speaks MCP: a person's browser session and a person's own access
/// token are `api_token_required`, a token for another project is 404, a revoked token is
/// 401, and a session id is no use to a token that didn't mint it.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn only_a_projects_own_token_speaks() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let other = project(&owner).await;
    let project = project(&owner).await;
    let (agent, _) = token(&owner, &project, "full-access").await;
    let (mutiny, mutiny_id) = token(&owner, &project, "read-only").await;
    let (alien, _) = token(&owner, &other, "full-access").await;
    let person = personal_token(&owner).await;
    let path = mcp_path(&project);
    let hello = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": init_params()})
        .to_string();

    // A person, however they authenticated: the endpoint speaks to agents.
    let browser = post(&person, &path, None, hello.as_bytes(), &[], &[]).await;
    assert_eq!(browser.status, StatusCode::FORBIDDEN, "{}", browser.body);
    assert_eq!(
        browser.body["error"]["details"]["reason"].as_str(),
        Some("api_token_required"),
        "{}",
        browser.body
    );
    let cookie = app()
        .post(format!("{ORIGIN}{path}"))
        .header(header::COOKIE, &owner.cookie)
        .header("x-csrf-token", &owner.csrf)
        .header(header::ORIGIN, ORIGIN)
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("mcp-protocol-version", PROTOCOL)
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": init_params()}))
        .send()
        .await
        .expect("request");
    assert_eq!(cookie.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        cookie.json::<Value>().await.expect("json")["error"]["details"]["reason"].as_str(),
        Some("api_token_required"),
        "a signed-in person is no agent either"
    );

    // Another project's token is 404 — as unknown to this project as a person who isn't in it.
    let alien_call = post(&alien, &path, None, hello.as_bytes(), &[], &[]).await;
    assert_eq!(
        alien_call.status,
        StatusCode::NOT_FOUND,
        "{}",
        alien_call.body
    );

    // The agent's own session answers to the agent's token only: the project's other token
    // gets the same 404 an invented id would.
    let mut remote = Remote::new(&agent, &project);
    remote.initialize().await;
    let session = remote.session.as_deref().expect("a session");
    let mutiny_call = post(
        &mutiny,
        &path,
        Some(session),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
            .to_string()
            .as_bytes(),
        &[],
        &[],
    )
    .await;
    assert_eq!(
        mutiny_call.status,
        StatusCode::NOT_FOUND,
        "{}",
        mutiny_call.body
    );
    let invented = post(
        &agent,
        &path,
        Some(&uuid::Uuid::now_v7().to_string()),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
            .to_string()
            .as_bytes(),
        &[],
        &[],
    )
    .await;
    assert_eq!(invented.status, StatusCode::NOT_FOUND, "{}", invented.body);

    // Revoking the token ends everything its sessions let it do — the registry's ownership
    // is no credential.
    let mut doomed = Remote::new(&mutiny, &project);
    doomed.initialize().await;
    let revoked = call(
        &owner,
        Method::DELETE,
        &format!("/api/v1/projects/{project}/tokens/{mutiny_id}"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(revoked.status, StatusCode::OK, "{}", revoked.body);
    let denied = doomed.notified("notifications/initialized").await;
    assert_eq!(denied.status, StatusCode::UNAUTHORIZED, "{}", denied.body);
    let _ended = remote.delete().await;
}

/// The transport's door checks, all of them: a hostile `Origin` is 403, an `Accept` that
/// can't take `application/json` is 400, `MCP-Protocol-Version` is required once a session
/// exists and must name the version spoken, malformed JSON-RPC is a JSON-RPC refusal, a
/// message before `initialize` is `-32002`, and `GET` — no SSE stream exists — is 405.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: each door check in turn"
)]
async fn the_transports_door_checks_hold() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let (agent, _) = token(&owner, &project, "full-access").await;
    let path = mcp_path(&project);
    let hello = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": init_params()})
        .to_string();

    // An Origin the server doesn't trust: the transport's DNS-rebinding check.
    let hostile = post(
        &agent,
        &path,
        None,
        hello.as_bytes(),
        &[],
        &[("origin", "https://evil.example")],
    )
    .await;
    assert_eq!(hostile.status, StatusCode::FORBIDDEN, "{}", hostile.body);
    assert_eq!(
        hostile.body["error"]["details"]["reason"].as_str(),
        Some("origin_not_allowed"),
        "{}",
        hostile.body
    );
    // The server's own origin is fine.
    let home = post(
        &agent,
        &path,
        None,
        hello.as_bytes(),
        &[],
        &[("origin", ORIGIN)],
    )
    .await;
    assert_eq!(home.status, StatusCode::OK, "{}", home.body);
    let session = home
        .headers
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .expect("a minted session")
        .to_owned();

    // An Accept that can't take the endpoint's only answer type is refused before any work.
    for accept in ["text/event-stream", "text/plain"] {
        let refused = post(
            &agent,
            &path,
            None,
            hello.as_bytes(),
            &[],
            &[("accept", accept)],
        )
        .await;
        assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{accept}");
    }

    // Once a session exists the protocol header is required — and it must name the version
    // this server speaks.
    let missing = post(
        &agent,
        &path,
        Some(&session),
        json!({"jsonrpc": "2.0", "id": 2, "method": "ping", "params": {}})
            .to_string()
            .as_bytes(),
        &["mcp-protocol-version"],
        &[],
    )
    .await;
    assert_eq!(missing.status, StatusCode::BAD_REQUEST, "{}", missing.body);
    let wrong = post(
        &agent,
        &path,
        Some(&session),
        json!({"jsonrpc": "2.0", "id": 2, "method": "ping", "params": {}})
            .to_string()
            .as_bytes(),
        &[],
        &[("mcp-protocol-version", "1999-01-01")],
    )
    .await;
    assert_eq!(wrong.status, StatusCode::BAD_REQUEST, "{}", wrong.body);

    // Malformed JSON-RPC is a 400 with the parse refusal; a request before `initialize` is
    // a session error, `-32002`, on the message that asked.
    let garbage = post(&agent, &path, Some(&session), b"this isn't json", &[], &[]).await;
    assert_eq!(garbage.status, StatusCode::BAD_REQUEST, "{}", garbage.body);
    assert_eq!(
        garbage.body["error"]["code"].as_i64(),
        Some(-32700),
        "{}",
        garbage.body
    );
    let early = post(
        &agent,
        &path,
        None,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}})
            .to_string()
            .as_bytes(),
        &[],
        &[],
    )
    .await;
    assert_eq!(early.status, StatusCode::OK, "{}", early.body);
    assert_eq!(
        early.body["error"]["code"].as_i64(),
        Some(-32002),
        "{}",
        early.body
    );

    // `GET` opens no stream — the server never initiates messages — so it's a bare 405.
    let stream = app()
        .get(format!("{ORIGIN}{path}"))
        .header(header::AUTHORIZATION, format!("Bearer {agent}"))
        .header("mcp-session-id", &session)
        .send()
        .await
        .expect("request");
    assert_eq!(stream.status(), StatusCode::METHOD_NOT_ALLOWED);
}

/// The per-token cap, from `COLOPHON_MCP_PER_TOKEN`: when the token's sessions are full the
/// next `initialize` is 429, and ending one makes room again.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn sessions_are_bounded_per_token() {
    let _server = Server::start_with(&[("COLOPHON_MCP_PER_TOKEN", "2")]).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let (agent, _) = token(&owner, &project, "read-only").await;

    let mut first = Remote::new(&agent, &project);
    first.initialize().await;
    let mut second = Remote::new(&agent, &project);
    second.initialize().await;
    let mut third = Remote::new(&agent, &project);
    third.next += 1;
    let refused = post(
        &third.secret,
        &third.path,
        None,
        third.requested("initialize", &init_params()).as_bytes(),
        &[],
        &[],
    )
    .await;
    assert_eq!(
        refused.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        refused.body
    );
    assert!(
        refused.headers.get(header::RETRY_AFTER).is_some(),
        "a 429 says when to ask again"
    );
    assert_eq!(first.delete().await.status, StatusCode::OK);
    let mut fourth = Remote::new(&agent, &project);
    fourth.initialize().await;
    let _ended = (second.delete().await, fourth.delete().await);
}

/// The `colophon-mcp` binary over stdio — the transport these tests compare the HTTP one
/// against. It sits beside `colophon-server` (`cargo test --workspace` builds both).
struct Pipe {
    child: tokio::process::Child,
    stdin: Option<ChildStdin>,
    stdout: Lines<BufReader<ChildStdout>>,
    next: i64,
}

impl Pipe {
    /// Starts the binary on `token` and `project` and runs its `initialize` handshake.
    async fn start(token: &str, project: &str) -> Self {
        let binary =
            PathBuf::from(env!("CARGO_BIN_EXE_colophon-server")).with_file_name("colophon-mcp");
        assert!(
            binary.exists(),
            "{} is missing: run `cargo build -p colophon-mcp` first",
            binary.display()
        );
        let mut child = tokio::process::Command::new(binary)
            .env_clear()
            .env("COLOPHON_SERVER", ORIGIN)
            .env("COLOPHON_TOKEN", token)
            .env("COLOPHON_PROJECT", project)
            .env("COLOPHON_MCP_LOG", "warn")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .expect("the MCP server starts");
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().expect("stdout")).lines();
        let mut mcp = Self {
            child,
            stdin,
            stdout,
            next: 0,
        };
        let init = mcp.request("initialize", init_params()).await;
        assert_eq!(init["result"]["protocolVersion"], PROTOCOL, "{init}");
        mcp.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
        mcp
    }

    async fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().expect("open");
        let mut line = message.to_string();
        line.push('\n');
        stdin.write_all(line.as_bytes()).await.expect("write");
        stdin.flush().await.expect("flush");
    }

    /// Sends a request; its response line.
    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        loop {
            let line = timeout(Duration::from_secs(60), self.stdout.next_line())
                .await
                .expect("an answer within a minute")
                .expect("read")
                .expect("a line before the end");
            let message: Value = serde_json::from_str(&line).expect("each line is JSON");
            if message["id"] == id {
                return message;
            }
        }
    }

    /// Closes stdin, as a host does to end the session; the exit code.
    async fn close(mut self) -> Option<i32> {
        drop(self.stdin.take());
        let status = timeout(Duration::from_secs(10), self.child.wait())
            .await
            .expect("exits once its input ends")
            .expect("wait");
        status.code()
    }
}

/// `result` with each `content[].text` parsed back to JSON: the text and `structuredContent`
/// carry the same answer, and comparing values — not strings — doesn't hinge on which object
/// map the two binaries' `serde_json` was built with (a dev-dependency flips `preserve_order`
/// for a whole build; see crates/colophon-domain/Cargo.toml and D-074).
fn normalized(result: &Value) -> Value {
    let mut result = result.clone();
    if let Some(content) = result["content"].as_array() {
        result["content"] = content
            .iter()
            .map(|block| {
                let mut block = block.clone();
                if let Some(text) = block["text"].as_str()
                    && let Ok(json) = serde_json::from_str::<Value>(text)
                {
                    block["text"] = json;
                }
                block
            })
            .collect::<Vec<_>>()
            .into();
    }
    result
}

/// Both transports serve the same session: identical `tools/list`, identical `tools/call`
/// results and identical refusals — the shared dispatcher makes drift impossible, and this
/// proves it end to end on one token.
#[tokio::test]
#[ignore = "needs the stack, the migrations and the colophon-mcp binary"]
async fn stdio_and_http_are_the_same_server() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let (agent, _) = token(&owner, &project, "full-access").await;

    let mut pipe = Pipe::start(&agent, &project).await;
    let mut wire = Remote::new(&agent, &project);
    wire.initialize().await;

    let over_stdio = pipe.request("tools/list", json!({})).await;
    let over_http = wire.request("tools/list", &json!({})).await;
    assert_eq!(
        over_stdio["result"], over_http["result"],
        "stdio and HTTP must offer the same tools"
    );

    // One call through each: same answer, since it is the same dispatcher.
    let types_stdio = pipe
        .request(
            "tools/call",
            json!({"name": "list_content_types", "arguments": {}}),
        )
        .await;
    let types_http = wire.call("list_content_types", json!({})).await;
    assert_eq!(
        normalized(&types_stdio["result"]),
        normalized(&types_http["result"])
    );

    // And the refusal boundary is the same bytes, not just the same code.
    let refused_stdio = pipe
        .request("tools/call", json!({"name": "publish", "arguments": {}}))
        .await;
    let refused_http = wire.call("publish", json!({})).await;
    assert_eq!(refused_stdio["error"], refused_http["error"]);

    // A read-only token sees the same filtered list on both transports: no write tools.
    let (reader, _) = token(&owner, &project, "read-only").await;
    let mut pipe_ro = Pipe::start(&reader, &project).await;
    let mut wire_ro = Remote::new(&reader, &project);
    wire_ro.initialize().await;
    assert_eq!(
        pipe_ro.request("tools/list", json!({})).await["result"],
        wire_ro.request("tools/list", &json!({})).await["result"],
        "a read-only token's filtered list is one list on both transports"
    );

    assert_eq!(pipe.close().await, Some(0));
    assert_eq!(pipe_ro.close().await, Some(0));
    let _ended = (wire.delete().await, wire_ro.delete().await);
}
