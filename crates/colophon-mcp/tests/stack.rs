//! The MCP server against the real server (docs/modules/09-automation.md, "MCP adapter"): a person
//! makes a project and API tokens through the API, then an agent drives the `colophon-mcp` binary
//! over stdio with one of them. A custom token drafts, meets a stale revision, and asks for review;
//! a read-only token is offered no write tools and its writes are refused by the API; and no tool
//! approves or publishes, even for a full-access token that could do both through the API.
//!
//! Needs the stack, migrations, and `colophon-server` built next to `colophon-mcp`
//! (`cargo test --workspace` builds both). Run one at a time, as the server's stack tests are:
//! `cargo test -p colophon-mcp --test stack -- --ignored --test-threads=1`. With
//! `COLOPHON_TEST_DATABASE`, the server uses that database; with `COLOPHON_TEST_ORIGIN` set at
//! compile time, it listens on that origin's port (its callback must be in the dev realm).
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};

use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader, Lines},
    process::{ChildStdin, ChildStdout},
    time::{Instant, sleep, timeout},
};

/// The server's public origin, as the server's own stack tests choose it.
const ORIGIN: &str = match option_env!("COLOPHON_TEST_ORIGIN") {
    Some(origin) => origin,
    None => "http://localhost:58092",
};

/// The eight tools, in the order `tools/list` gives them.
const ALL_TOOLS: [&str; 8] = [
    "list_content_types",
    "list_entries",
    "get_entry",
    "create_draft",
    "update_draft",
    "create_publish_request",
    "request_review",
    "get_publish_request",
];

fn port() -> u16 {
    ORIGIN
        .rsplit(':')
        .next()
        .and_then(|port| port.parse().ok())
        .expect("COLOPHON_TEST_ORIGIN ends with :<port>")
}

/// The real server, killed when dropped.
struct Server(Child);

impl Server {
    async fn start() -> Self {
        let binary =
            PathBuf::from(env!("CARGO_BIN_EXE_colophon-mcp")).with_file_name("colophon-server");
        assert!(
            binary.exists(),
            "{} is missing: run `cargo build -p colophon-server` first",
            binary.display()
        );
        let child = Command::new(binary)
            .env_clear()
            .envs(local_env())
            .env("COLOPHON_LISTEN_ADDR", format!("127.0.0.1:{}", port()))
            .env("COLOPHON_PUBLIC_URL", ORIGIN)
            .env("COLOPHON_LOG", "warn")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the server starts");
        let server = Self(child);
        let deadline = Instant::now() + Duration::from_secs(10);
        while reqwest::get(format!("{ORIGIN}/livez")).await.is_err() {
            assert!(Instant::now() < deadline, "the server never answered");
            sleep(Duration::from_millis(50)).await;
        }
        server
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _already_gone = self.0.kill();
        let _reaped = self.0.wait();
    }
}

/// `deploy/local.env`, with the database named by `COLOPHON_TEST_DATABASE` when it is set.
fn local_env() -> Vec<(String, String)> {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/local.env");
    let database = std::env::var("COLOPHON_TEST_DATABASE").ok();
    std::fs::read_to_string(file)
        .expect("deploy/local.env")
        .lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(key, _)| !key.starts_with('#'))
        .map(|(key, value)| {
            let value = match (&database, value.trim().strip_suffix("/colophon")) {
                (Some(name), Some(server)) if key.ends_with("DATABASE_URL") => {
                    format!("{server}/{name}")
                }
                _ => value.trim().to_owned(),
            };
            (key.trim().to_owned(), value)
        })
        .collect()
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("a client")
}

fn set_cookie(response: &reqwest::Response, name: &str) -> String {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok()?.split(';').next()?.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.to_owned())
        .unwrap_or_default()
}

/// A signed-in person: cookies, and the CSRF token their writes echo.
struct Person {
    cookie: String,
    csrf: String,
}

/// Signs in through the development realm's Keycloak.
async fn sign_in(user: &str) -> Person {
    let start = client()
        .get(format!("{ORIGIN}/auth/login?return_to=/me"))
        .send()
        .await
        .expect("login");
    let binding = set_cookie(&start, "colophon_signin");
    let provider = start.headers()[header::LOCATION]
        .to_str()
        .expect("ascii")
        .to_owned();
    let keycloak = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("a client");
    let page = keycloak
        .get(&provider)
        .send()
        .await
        .expect("page")
        .text()
        .await
        .expect("html");
    let action = page
        .split("action=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("a login form")
        .replace("&amp;", "&");
    let accepted = keycloak
        .post(&action)
        .form(&[("username", user), ("password", "colophon-dev")])
        .send()
        .await
        .expect("credentials");
    let callback = accepted.headers()[header::LOCATION]
        .to_str()
        .expect("ascii")
        .to_owned();
    let done = client()
        .get(&callback)
        .header(header::COOKIE, format!("colophon_signin={binding}"))
        .send()
        .await
        .expect("callback");
    assert_eq!(done.status(), StatusCode::SEE_OTHER, "signed in");
    let (session, csrf) = (
        set_cookie(&done, "colophon_session"),
        set_cookie(&done, "colophon_csrf"),
    );
    Person {
        cookie: format!("colophon_session={session}; colophon_csrf={csrf}"),
        csrf,
    }
}

/// A request as `person`: status and JSON body.
async fn call(
    person: &Person,
    method: Method,
    path: &str,
    body: Option<&Value>,
) -> (StatusCode, Value) {
    let mut request = client()
        .request(method, format!("{ORIGIN}{path}"))
        .header(header::COOKIE, &person.cookie)
        .header("x-csrf-token", &person.csrf)
        .header(header::ORIGIN, ORIGIN)
        .header("idempotency-key", format!("stack-{}", uuid_like()));
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request.send().await.expect("request");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

/// A unique enough suffix for slugs and keys.
fn uuid_like() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after 1970")
        .as_nanos();
    format!("{nanos:x}")
}

/// An article with a one-paragraph body.
fn article(title: &str, text: &str) -> Value {
    json!({
        "title": title,
        "body": {"schema": "colophon/rich-text", "version": 1, "doc": {"type": "doc", "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": text}]}
        ]}}
    })
}

/// The server running, `dev-owner` signed in, and a new project of theirs.
async fn setup() -> (Server, Person, String) {
    let server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let slug = format!("mcp-{}", uuid_like());
    let (status, made) = call(
        &owner,
        Method::POST,
        "/api/v1/projects",
        Some(&json!({"slug": slug, "name": "MCP test"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    let project = made["data"]["id"].as_str().expect("an id").to_owned();
    (server, owner, project)
}

/// Makes an API token of `project` through the API; its secret.
async fn token(owner: &Person, project: &str, body: &Value) -> String {
    let (status, made) = call(
        owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/tokens"),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    made["data"]["secret"]
        .as_str()
        .expect("a secret")
        .to_owned()
}

/// The `colophon-mcp` binary, driven over its stdin and stdout like an agent host does.
struct Mcp {
    child: tokio::process::Child,
    stdin: Option<ChildStdin>,
    stdout: Lines<BufReader<ChildStdout>>,
    next: i64,
}

impl Mcp {
    async fn start(token: &str, project: &str) -> Self {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_colophon-mcp"))
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
        let init = mcp
            .request(
                "initialize",
                json!({"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "stack-test", "version": "1"}}),
            )
            .await;
        assert_eq!(init["result"]["protocolVersion"], "2025-11-25", "{init}");
        assert_eq!(init["result"]["serverInfo"]["name"], "colophon-mcp");
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

    /// Sends a request; its response.
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

    /// The names `tools/list` offers.
    async fn tools(&mut self) -> Vec<String> {
        let listed = self.request("tools/list", json!({})).await;
        listed["result"]["tools"]
            .as_array()
            .unwrap_or_else(|| panic!("a tool list: {listed}"))
            .iter()
            .map(|tool| tool["name"].as_str().expect("a name").to_owned())
            .collect()
    }

    /// Calls a tool; its result.
    async fn call(&mut self, name: &str, arguments: Value) -> Value {
        let response = self
            .request("tools/call", json!({"name": name, "arguments": arguments}))
            .await;
        response
            .get("result")
            .cloned()
            .unwrap_or_else(|| panic!("a result: {response}"))
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

/// A successful tool result's structured content.
fn ok(result: &Value) -> &Value {
    assert_eq!(result["isError"], false, "{result}");
    &result["structuredContent"]
}

/// A failed tool result's error.
fn failed(result: &Value) -> &Value {
    assert_eq!(result["isError"], true, "{result}");
    &result["structuredContent"]["error"]
}

fn text(result: &Value) -> &str {
    result["content"][0]["text"].as_str().expect("text")
}

/// Creates, saves and fails to save over a newer revision; the entry and its current revision.
async fn draft_and_meet_a_stale_revision(mcp: &mut Mcp) -> (String, String) {
    let created = mcp
        .call(
            "create_draft",
            json!({"collection": "articles", "content": article("From an agent", "first")}),
        )
        .await;
    let entry = ok(&created)["entry"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let first = ok(&created)["entry"]["revision"]["id"]
        .as_str()
        .expect("a revision")
        .to_owned();
    assert_eq!(ok(&created)["replayed"], false);
    let saved = mcp
        .call(
            "update_draft",
            json!({"collection": "articles", "id": entry, "revision": first, "content": article("From an agent", "second")}),
        )
        .await;
    let second = ok(&saved)["entry"]["revision"]["id"]
        .as_str()
        .expect("a revision")
        .to_owned();
    assert_ne!(first, second);
    let stale = mcp
        .call(
            "update_draft",
            json!({"collection": "articles", "id": entry, "revision": first, "content": article("From an agent", "overwrite")}),
        )
        .await;
    let refusal = failed(&stale);
    assert_eq!(refusal["status"], 409);
    assert_eq!(refusal["reason"], "stale_revision");
    assert_eq!(refusal["current"], second.as_str());
    assert!(
        text(&stale).contains(&format!("current revision is {second}")),
        "{}",
        text(&stale)
    );
    let read = mcp
        .call("get_entry", json!({"collection": "articles", "id": entry}))
        .await;
    let read = &ok(&read)["entry"];
    assert_eq!(
        read["revision"]["id"],
        second.as_str(),
        "nothing was overwritten"
    );
    assert_eq!(
        read["content"]["body"]["doc"]["content"][0]["content"][0]["text"],
        "second"
    );
    assert_eq!(read["published"], Value::Null, "a draft only");
    assert_eq!(read["referenced_by"], json!([]));
    (entry, second)
}

/// The same key twice creates once.
async fn retry_with_the_same_key(mcp: &mut Mcp) {
    let key = format!("stack-retry-{}", uuid_like());
    let arguments = json!({"collection": "articles", "content": article("Retried", "once"), "idempotency_key": key});
    let first = mcp.call("create_draft", arguments.clone()).await;
    let again = mcp.call("create_draft", arguments).await;
    assert_eq!(
        ok(&again)["entry"]["id"],
        ok(&first)["entry"]["id"],
        "created once"
    );
    assert_eq!(ok(&again)["replayed"], true);
    assert_eq!(ok(&again)["idempotency_key"], key.as_str());
}

/// Makes a publish request for `entry` and asks for its review; the request.
async fn propose(mcp: &mut Mcp, entry: &str) -> Value {
    let made = mcp
        .call(
            "create_publish_request",
            json!({"title": "An agent's post", "entries": [entry]}),
        )
        .await;
    let request = ok(&made)["publish_request"].clone();
    let (id, hash) = (
        request["id"].as_str().expect("an id"),
        request["hash"].as_str().expect("a hash"),
    );
    assert_eq!(request["status"]["state"], "open");
    let wrong = format!("sha256:{}", "0".repeat(64));
    let stale = mcp
        .call("request_review", json!({"id": id, "hash": wrong}))
        .await;
    assert_eq!(failed(&stale)["reason"], "stale_changeset");
    assert_eq!(failed(&stale)["current"], hash);
    let asked = mcp
        .call("request_review", json!({"id": id, "hash": hash}))
        .await;
    assert_eq!(
        ok(&asked)["publish_request"]["status"]["state"],
        "awaiting_review"
    );
    let read = mcp.call("get_publish_request", json!({"id": id})).await;
    ok(&read)["publish_request"].clone()
}

#[tokio::test]
#[ignore = "needs the local stack and colophon-server; see the module docs"]
async fn a_custom_token_drafts_and_asks_for_review_and_nothing_more() {
    let (_server, owner, project) = setup().await;
    let permissions: Vec<Value> = ["content.read", "content.create", "content.update"]
        .iter()
        .map(|action| json!({"action": action, "subject": "*"}))
        .collect();
    let secret = token(
        &owner,
        &project,
        &json!({"name": "agent", "kind": "custom", "permissions": permissions, "expires_in_days": 1}),
    )
    .await;
    let mut mcp = Mcp::start(&secret, &project).await;
    assert_eq!(mcp.tools().await, ALL_TOOLS);
    let (entry, revision) = draft_and_meet_a_stale_revision(&mut mcp).await;
    retry_with_the_same_key(&mut mcp).await;
    let listed = mcp
        .call(
            "list_entries",
            json!({"collection": "articles", "q": "From an agent"}),
        )
        .await;
    let found = &ok(&listed)["entries"];
    assert_eq!(found.as_array().map(Vec::len), Some(1), "{found}");
    let request = propose(&mut mcp, &entry).await;
    assert_eq!(request["status"]["state"], "awaiting_review");
    assert_eq!(
        request["items"][0]["revision_id"],
        revision.as_str(),
        "the exact revision"
    );
    assert_eq!(request["reviews"], json!([]), "nobody approved it");
    // A person sees the same request waiting for them, asked for by the token's own identity.
    let path = format!(
        "/api/v1/projects/{project}/environments/production/changesets/{}",
        request["id"].as_str().expect("an id")
    );
    let (status, seen) = call(&owner, Method::GET, &path, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(seen["data"]["status"]["state"], "awaiting_review");
    assert_eq!(seen["data"]["submitted_by"], seen["data"]["created_by"]);
    assert_eq!(mcp.close().await, Some(0));
}

#[tokio::test]
#[ignore = "needs the local stack and colophon-server; see the module docs"]
async fn a_read_only_token_is_offered_no_writes_and_the_api_refuses_them() {
    let (_server, owner, project) = setup().await;
    let entries =
        format!("/api/v1/projects/{project}/environments/production/collections/articles/entries");
    let (status, made) = call(
        &owner,
        Method::POST,
        &entries,
        Some(&article("By a person", "text")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    let (entry, revision) = (&made["data"]["id"], &made["data"]["revision"]["id"]);
    let secret = token(
        &owner,
        &project,
        &json!({"name": "reader", "kind": "read-only", "expires_in_days": 1}),
    )
    .await;
    let mut mcp = Mcp::start(&secret, &project).await;
    assert_eq!(
        mcp.tools().await,
        [
            "list_content_types",
            "list_entries",
            "get_entry",
            "get_publish_request"
        ]
    );
    let read = mcp
        .call("get_entry", json!({"collection": "articles", "id": entry}))
        .await;
    assert_eq!(ok(&read)["entry"]["revision"]["id"], *revision);
    // Discovery hides the tools; the API, not the MCP server, refuses the writes.
    let created = mcp
        .call(
            "create_draft",
            json!({"collection": "articles", "content": article("Sneaky", "x")}),
        )
        .await;
    let refusal = failed(&created);
    assert_eq!(
        (refusal["status"].clone(), refusal["reason"].clone()),
        (json!(403), json!("missing_permission"))
    );
    assert_eq!(refusal["permission"], "content.create");
    assert!(text(&created).contains("reason missing_permission, permission content.create"));
    let saved = mcp
        .call("update_draft", json!({"collection": "articles", "id": entry, "revision": revision, "content": article("Sneaky", "x")}))
        .await;
    assert_eq!(failed(&saved)["permission"], "content.update");
    let proposed = mcp
        .call(
            "create_publish_request",
            json!({"title": "Sneaky", "entries": [entry]}),
        )
        .await;
    assert_eq!(failed(&proposed)["status"], 403);
    let (_, listed) = call(&owner, Method::GET, &entries, None).await;
    assert_eq!(
        listed["data"].as_array().map(Vec::len),
        Some(1),
        "nothing was written: {listed}"
    );
    let (_, requests) = call(
        &owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/environments/production/changesets"),
        None,
    )
    .await;
    assert_eq!(requests["data"], json!([]));
    assert_eq!(mcp.close().await, Some(0));
}

#[tokio::test]
#[ignore = "needs the local stack and colophon-server; see the module docs"]
async fn no_tool_approves_or_publishes_even_for_a_full_access_token() {
    let (_server, owner, project) = setup().await;
    let secret = token(
        &owner,
        &project,
        &json!({"name": "full", "kind": "full-access", "expires_in_days": 1}),
    )
    .await;
    let mut mcp = Mcp::start(&secret, &project).await;
    assert_eq!(
        mcp.tools().await,
        ALL_TOOLS,
        "the same eight tools, whatever the token may do"
    );
    for name in [
        "approve",
        "approve_publish_request",
        "review",
        "review_publish_request",
        "publish",
        "publish_request",
        "unpublish",
        "schedule",
        "schedule_publish",
        "create_release",
        "manage_roles",
        "create_token",
        "add_member",
    ] {
        let response = mcp
            .request("tools/call", json!({"name": name, "arguments": {}}))
            .await;
        assert_eq!(response["error"]["code"], -32602, "{name}: {response}");
    }
    let created = mcp
        .call(
            "create_draft",
            json!({"collection": "articles", "content": article("Proposed", "x")}),
        )
        .await;
    let entry = ok(&created)["entry"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let request = propose(&mut mcp, &entry).await;
    // An approval can't be smuggled in through a tool's arguments either.
    let id = request["id"].as_str().expect("an id");
    let hash = request["hash"].as_str().expect("a hash");
    let smuggled = mcp
        .call(
            "request_review",
            json!({"id": id, "hash": hash, "decision": "approve"}),
        )
        .await;
    assert_eq!(failed(&smuggled)["code"], "invalid_arguments");
    let read = mcp.call("get_publish_request", json!({"id": id})).await;
    assert_eq!(
        ok(&read)["publish_request"]["status"]["state"],
        "awaiting_review"
    );
    assert_eq!(ok(&read)["publish_request"]["reviews"], json!([]));
    let (status, releases) = call(
        &owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/environments/production/releases"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(releases["data"], json!([]), "nothing was published");
    assert_eq!(mcp.close().await, Some(0));
}
