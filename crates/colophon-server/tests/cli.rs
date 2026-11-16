//! The command line against the real server (step 1.6): `colophon login` approved in a browser
//! session, then `whoami`, `content create` and `content list` creating and reading the same
//! records the Studio does, every exit code of the CLI contract, and `logout` revoking the token.
//! The D-119 machine contract runs here too: the `data`/`error` envelope `--json` and `--jsonl`
//! print, and `colophon mcp` serving the standalone MCP server over its own stdio.
//! Needs the stack, migrations, and the `colophon` binary next to the server's
//! (`cargo test --workspace` builds both).
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use std::{
    io::{BufRead as _, BufReader},
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::Duration,
};

use common::{ORIGIN, Server, Session, call, create_article, join, project, sign_in, unique_slug};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader as LinesBuf, Lines},
    process::{ChildStdin, ChildStdout},
    time::timeout,
};

/// The CLI binary, built alongside the server's.
fn colophon_binary() -> PathBuf {
    let server = PathBuf::from(env!("CARGO_BIN_EXE_colophon-server"));
    let cli = server.with_file_name("colophon");
    assert!(
        cli.exists(),
        "{} is missing: run `cargo build -p colophon-cli` first",
        cli.display()
    );
    cli
}

fn colophon(config: &Path, args: &[&str]) -> Output {
    Command::new(colophon_binary())
        .args(args)
        .env_clear()
        .env("COLOPHON_CONFIG_DIR", config)
        .output()
        .expect("the CLI runs")
}

fn json_out(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "stdout is one JSON value: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Runs `colophon login`, approves it as `approver`, and waits for it to finish; its output, with
/// everything it wrote to stderr.
async fn login(config: &Path, approver: &Session) -> Output {
    let mut child = Command::new(colophon_binary())
        .args([
            "login",
            "--server",
            ORIGIN,
            "--name",
            "test terminal",
            "--json",
        ])
        .env_clear()
        .env("COLOPHON_CONFIG_DIR", config)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the CLI starts");
    let stderr = child.stderr.take().expect("stderr");
    // Keep reading stderr to the end: a reader that stopped at the code would close the pipe,
    // and the CLI's next line ("Waiting for approval…") would then fail to print and panic.
    let (found, code) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut lines = Vec::new();
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Some(code) = line.split("shows the code ").nth(1) {
                let _ignored = found.send(code.trim_end_matches('.').to_owned());
            }
            lines.push(line);
        }
        lines.join("\n")
    });
    let code = tokio::task::spawn_blocking(move || code.recv())
        .await
        .expect("task")
        .expect("the CLI prints the code");

    let shown = call(
        approver,
        Method::GET,
        &format!("/auth/device/{code}"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        shown.body["data"]["client_name"], "test terminal",
        "the approver sees what is asking"
    );
    let approval = call(
        approver,
        Method::POST,
        "/auth/device/approve",
        Some(&json!({ "user_code": code })),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(approval.status, StatusCode::NO_CONTENT, "{}", approval.body);
    let mut output =
        tokio::task::spawn_blocking(move || child.wait_with_output().expect("the CLI exits"))
            .await
            .expect("task");
    output.stderr = reader.join().expect("the reader finishes").into_bytes();
    output
}

fn article(title: &str) -> String {
    json!({
        "title": title,
        "body": {"schema": "colophon/rich-text", "version": 1, "doc": {"type": "doc", "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": "Written in a terminal."}]}
        ]}}
    })
    .to_string()
}

#[tokio::test]
#[ignore = "needs the stack, migrations and the colophon binary"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: sign in, write, refuse, sign out"
)]
async fn the_cli_signs_in_through_the_browser_and_creates_the_same_records() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let slug = unique_slug();
    let created = call(
        &editor,
        Method::POST,
        "/api/v1/projects",
        Some(&json!({"slug": slug, "name": "CLI project"})),
        &[],
    )
    .await
    .expect("request");
    let project = created.body["data"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let config = std::env::temp_dir().join(format!("colophon-cli-{}", uuid::Uuid::now_v7()));

    let output = login(&config, &editor).await;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(json_out(&output)["data"]["status"], "signed_in");
    let credentials = config.join("credentials.json");
    let mode = std::fs::metadata(&credentials)
        .expect("written")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "only the owner can read the token");

    let whoami = colophon(&config, &["whoami", "--json"]);
    assert_eq!(json_out(&whoami)["data"]["me"]["display_name"], "Dev Owner");

    // Create an article by slug, then read the very same record through the API.
    let create = colophon(
        &config,
        &[
            "content",
            "create",
            "articles",
            "--project",
            &slug,
            "--data",
            &article("From the CLI"),
            "--json",
        ],
    );
    assert_eq!(
        create.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&create.stderr)
    );
    let outcome = json_out(&create);
    assert_eq!(outcome["data"]["status"], "created");
    let id = outcome["data"]["entry"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let key = outcome["data"]["idempotency_key"]
        .as_str()
        .expect("a key")
        .to_owned();
    let read = call(
        &editor,
        Method::GET,
        &format!(
            "/api/v1/projects/{project}/environments/production/collections/articles/entries/{id}"
        ),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        read.body["data"]["content"]["title"], "From the CLI",
        "the Studio sees what the CLI made"
    );

    // The same key again is the same entry, not a second one.
    let again = colophon(
        &config,
        &[
            "content",
            "create",
            "articles",
            "--project",
            &project,
            "--data",
            &article("From the CLI"),
            "--idempotency-key",
            &key,
            "--json",
        ],
    );
    let replay = json_out(&again);
    assert_eq!(
        (
            replay["data"]["status"].as_str(),
            replay["data"]["entry"]["id"].as_str()
        ),
        (Some("replayed"), Some(id.as_str()))
    );
    let list = colophon(
        &config,
        &["content", "list", "articles", "--project", &slug, "--json"],
    );
    let entries = json_out(&list)["data"]["entries"]
        .as_array()
        .cloned()
        .expect("a list");
    assert_eq!(
        entries.len(),
        1,
        "one entry, however many times it was sent"
    );
    assert_eq!(entries[0]["id"], id.as_str());

    // The CLI contract's exit codes.
    let bad = colophon(
        &config,
        &[
            "content",
            "create",
            "articles",
            "--project",
            &slug,
            "--data",
            r#"{"title":"No body"}"#,
        ],
    );
    assert_eq!(bad.status.code(), Some(2), "invalid content");
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("/body"),
        "{}",
        String::from_utf8_lossy(&bad.stderr)
    );
    assert!(bad.stdout.is_empty(), "errors never go to stdout");
    let stranger = colophon(
        &config,
        &[
            "content",
            "list",
            "articles",
            "--project",
            "not-a-project-of-mine",
        ],
    );
    assert_eq!(stranger.status.code(), Some(3), "not a member");
    let reused = colophon(
        &config,
        &[
            "content",
            "create",
            "articles",
            "--project",
            &slug,
            "--data",
            &article("Different"),
            "--idempotency-key",
            &key,
        ],
    );
    assert_eq!(
        reused.status.code(),
        Some(4),
        "a key reused for different content is a conflict"
    );

    // A token can't approve another device; logging out revokes it.
    let file: Value =
        serde_json::from_slice(&std::fs::read(&credentials).expect("read")).expect("JSON");
    let token = file["profiles"]["default"]["token"]
        .as_str()
        .expect("a token")
        .to_owned();
    let by_token = common::app()
        .post(format!("{ORIGIN}/auth/device/approve"))
        .bearer_auth(&token)
        .json(&json!({"user_code": "BCDF-GHJK"}))
        .send()
        .await
        .expect("request");
    assert_eq!(by_token.status(), StatusCode::FORBIDDEN);
    let logout = colophon(&config, &["logout"]);
    assert_eq!(
        logout.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&logout.stderr)
    );
    let after = common::app()
        .get(format!("{ORIGIN}/me"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("request");
    assert_eq!(
        after.status(),
        StatusCode::UNAUTHORIZED,
        "the token is revoked on the server"
    );
    assert_eq!(
        colophon(&config, &["whoami"]).status.code(),
        Some(3),
        "and forgotten here"
    );

    let unknown = common::app()
        .post(format!("{ORIGIN}/auth/device/token"))
        .json(&json!({"device_secret": "x".repeat(43)}))
        .send()
        .await
        .expect("request");
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    let _cleanup = std::fs::remove_dir_all(&config);
}

/// Every new command family has help text (the CLI parity slice, docs/modules/08-cli.md).
#[tokio::test]
#[ignore = "needs the colophon binary"]
async fn the_new_commands_all_have_help_text() {
    let config = std::env::temp_dir().join(format!("colophon-cli-{}", uuid::Uuid::now_v7()));
    for command in [
        ["content", "query"],
        ["content", "import"],
        ["content", "export"],
        ["release", "list"],
        ["release", "show"],
        ["migration", "list"],
        ["migration", "show"],
        ["migration", "items"],
        ["migration", "plan"],
        ["migration", "approve"],
        ["migration", "apply"],
        ["migration", "cancel"],
        ["migration", "resolve"],
        ["comment", "list"],
        ["comment", "create"],
        ["task", "list"],
        ["task", "show"],
        ["task", "create"],
        ["task", "update"],
    ] {
        let mut args = command.to_vec();
        args.push("--help");
        let help = colophon(&config, &args);
        assert_eq!(
            help.status.code(),
            Some(0),
            "{args:?}: {}",
            String::from_utf8_lossy(&help.stderr)
        );
        let text = String::from_utf8_lossy(&help.stdout);
        assert!(text.contains("Usage:"), "{args:?}: {text}");
    }
    // A top-level command too — it takes no family prefix.
    let help = colophon(&config, &["mcp", "--help"]);
    assert_eq!(
        help.status.code(),
        Some(0),
        "mcp: {}",
        String::from_utf8_lossy(&help.stderr)
    );
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(text.contains("Usage:"), "mcp: {text}");
}

/// A real `content query` round trip: posts created through the CLI are found by a filtered,
/// sorted query document the same CLI posts, and `next_cursor` pages like the API's own
/// (docs/modules/08-cli.md).
#[tokio::test]
#[ignore = "needs the stack, migrations and the colophon binary"]
#[expect(
    clippy::too_many_lines,
    reason = "one session, told in order: type, three posts, two pages, the refusals"
)]
async fn content_query_runs_a_document_against_the_cli_s_own_entries() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let slug = unique_slug();
    let created = call(
        &editor,
        Method::POST,
        "/api/v1/projects",
        Some(&json!({"slug": slug, "name": "Query project"})),
        &[],
    )
    .await
    .expect("request");
    let project = created.body["data"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    // A collection with an indexed field to filter and sort on, made through the API.
    let posts = call(
        &editor,
        Method::POST,
        &format!("/api/v1/projects/{project}/environments/production/content-types"),
        Some(&json!({
            "slug": "posts",
            "definition": {
                "name": "Posts",
                "title_field": "headline",
                "fields": [
                    {"id": "headline", "label": "Headline", "type": "text", "required": true, "index": true},
                    {"id": "rating", "label": "Rating", "type": "number", "integer": true, "min": 0, "max": 5, "index": true}
                ]
            }
        })),
        &[("idempotency-key", "cli-query-type-1")],
    )
    .await
    .expect("request");
    assert_eq!(posts.status, StatusCode::CREATED, "{}", posts.body);

    let config = std::env::temp_dir().join(format!("colophon-cli-{}", uuid::Uuid::now_v7()));
    let output = login(&config, &editor).await;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    for (headline, rating) in [("Alpha news", 5), ("Beta guide", 3), ("Gamma news", 4)] {
        let made = colophon(
            &config,
            &[
                "content",
                "create",
                "posts",
                "--project",
                &slug,
                "--data",
                &json!({"headline": headline, "rating": rating}).to_string(),
                "--json",
            ],
        );
        assert_eq!(
            made.status.code(),
            Some(0),
            "{headline}: {}",
            String::from_utf8_lossy(&made.stderr)
        );
    }

    // A filtered, sorted document: rating at least 4, highest first, one per page.
    let document = json!({
        "filters": [{"field": "rating", "op": "gte", "value": 4}],
        "sort": [{"field": "rating", "dir": "desc"}],
        "limit": 1
    });
    let first = colophon(
        &config,
        &[
            "content",
            "query",
            "posts",
            "--project",
            &slug,
            "--data",
            &document.to_string(),
            "--json",
        ],
    );
    assert_eq!(
        first.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let page = json_out(&first);
    assert_eq!(page["command"], "content query");
    assert_eq!(page["data"]["collection"], "posts");
    let entries = page["data"]["entries"].as_array().expect("a page");
    assert_eq!(entries.len(), 1, "{page}");
    assert_eq!(
        entries[0]["revision"]["title"].as_str(),
        Some("Alpha news"),
        "the 5 first, descending"
    );
    assert!(
        page["data"]["applied"]["query_hash"]
            .as_str()
            .is_some_and(|hash| hash.starts_with("sha256:")),
        "the canonical query is echoed with its hash: {page}"
    );
    let cursor = page["data"]["next_cursor"]
        .as_str()
        .expect("a page boundary")
        .to_owned();

    // The next page, through --cursor: the query hash binds it to the same document.
    let second = colophon(
        &config,
        &[
            "content",
            "query",
            "posts",
            "--project",
            &slug,
            "--data",
            &document.to_string(),
            "--cursor",
            &cursor,
            "--json",
        ],
    );
    assert_eq!(
        second.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let page = json_out(&second);
    let entries = page["data"]["entries"].as_array().expect("a page");
    assert_eq!(entries.len(), 1, "{page}");
    assert_eq!(entries[0]["revision"]["title"].as_str(), Some("Gamma news"));

    // A cursor of one query refuses against another — the API's own check, exit 2.
    let mismatched = colophon(
        &config,
        &[
            "content",
            "query",
            "posts",
            "--project",
            &slug,
            "--data",
            r#"{"limit": 1}"#,
            "--cursor",
            &cursor,
        ],
    );
    assert_eq!(
        mismatched.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&mismatched.stderr)
    );

    // --limit is sugar for the document's; it refuses a document that sets one already.
    let doubled = colophon(
        &config,
        &[
            "content",
            "query",
            "posts",
            "--project",
            &slug,
            "--data",
            &document.to_string(),
            "--limit",
            "2",
        ],
    );
    assert_eq!(doubled.status.code(), Some(2), "limit is said once");

    let _cleanup = std::fs::remove_dir_all(&config);
}

/// The output contract end to end (D-119): `--json` wraps its result in the versioned
/// `data` envelope, `--jsonl` prints one envelope per entry across every page of the
/// list, and a refusal reaches stdout as the API's own `error` object — `code`,
/// `details` and `request_id` whole — while stderr still says `error: …` for a person.
/// Nothing a machine reads carries an ANSI escape.
#[tokio::test]
#[ignore = "needs the stack, migrations and the colophon binary"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: a reader, three notes, the envelope and the refusal"
)]
async fn the_machine_contract_envelopes_results_and_streams_items() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let guest = sign_in("dev-editor").await;
    let project = project(&owner).await;

    // A reader of articles, and nothing else.
    let made = call(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/roles"),
        Some(&json!({
            "key": "reader",
            "name": "Reader",
            "permissions": [{"action": "content.read", "subject": "articles"}]
        })),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    join(&owner, &guest, &project, "reader").await;

    let mut ids = Vec::new();
    for title in ["First note", "Second note", "Third note"] {
        ids.push(create_article(&owner, &project, title).await);
    }

    let config = std::env::temp_dir().join(format!("colophon-cli-{}", uuid::Uuid::now_v7()));
    let output = login(&config, &guest).await;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // --json: exactly one object; the page is `data`.
    let list = colophon(
        &config,
        &[
            "content",
            "list",
            "articles",
            "--project",
            &project,
            "--json",
        ],
    );
    assert_eq!(
        list.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&list.stderr)
    );
    let page = json_out(&list);
    assert_eq!(page["version"], 2);
    assert_eq!(page["command"], "content list");
    assert_eq!(page["data"]["collection"], "articles");
    assert_eq!(
        page["data"]["entries"].as_array().expect("entries").len(),
        3
    );
    assert!(page["data"]["next_cursor"].is_null(), "{page}");
    assert!(
        !String::from_utf8_lossy(&list.stdout).contains('\u{1b}'),
        "machine output never carries an ANSI escape"
    );

    // --jsonl: one object per entry, pages of two followed to the end.
    let stream = colophon(
        &config,
        &[
            "content",
            "list",
            "articles",
            "--project",
            &project,
            "--jsonl",
            "--limit",
            "2",
        ],
    );
    assert_eq!(
        stream.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&stream.stderr)
    );
    let stdout = String::from_utf8_lossy(&stream.stdout);
    assert!(!stdout.contains('\u{1b}'), "no ANSI in the stream");
    let mut seen = Vec::new();
    for line in stdout.lines() {
        let envelope: Value = serde_json::from_str(line).expect("each line is one JSON object");
        assert_eq!(envelope["version"], 2, "{envelope}");
        assert_eq!(envelope["command"], "content list", "{envelope}");
        seen.push(envelope["data"]["id"].as_str().expect("an id").to_owned());
    }
    ids.sort();
    seen.sort();
    assert_eq!(seen, ids, "every entry streamed exactly once");

    // A refusal is the API's own error object on stdout, exit 3; stderr carries the
    // same failure in words, as a text run's would.
    let denied = colophon(
        &config,
        &[
            "content",
            "create",
            "articles",
            "--project",
            &project,
            "--data",
            &article("Not mine to write"),
            "--json",
        ],
    );
    assert_eq!(
        denied.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&denied.stderr)
    );
    let envelope = json_out(&denied);
    assert_eq!(envelope["version"], 2);
    assert_eq!(envelope["command"], "content create");
    assert_eq!(envelope["error"]["code"], "forbidden", "{envelope}");
    assert_eq!(
        envelope["error"]["details"]["reason"], "missing_permission",
        "{envelope}"
    );
    assert_eq!(
        envelope["error"]["details"]["permission"], "content.create",
        "{envelope}"
    );
    assert!(
        envelope["error"]["request_id"].is_string(),
        "the API's request id travels verbatim: {envelope}"
    );
    assert!(
        String::from_utf8_lossy(&denied.stderr).contains("error:"),
        "stderr still says it in words: {}",
        String::from_utf8_lossy(&denied.stderr)
    );

    // --json and --jsonl pick one format; together they refuse before anything runs.
    let both = colophon(
        &config,
        &[
            "content",
            "list",
            "articles",
            "--project",
            &project,
            "--json",
            "--jsonl",
        ],
    );
    assert_eq!(both.status.code(), Some(2), "one format, not two");
    assert!(both.stdout.is_empty(), "a usage refusal stays off stdout");

    let _cleanup = std::fs::remove_dir_all(&config);
}

/// `content import` and `content export` end to end (D-123): a JSONL file plans, lands and
/// replays under its key, a bad file is a usage refusal that never reached the server, the
/// export streams the same documents back — raw on text, enveloped on `--jsonl`, collected
/// on `--json` — and the file imports into another project. A read-only member's import
/// carries the API's refusal whole.
#[tokio::test]
#[ignore = "needs the stack, migrations and the colophon binary"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: plan, import, replay, export, round-trip, refusals"
)]
async fn content_import_and_export_round_trip_through_the_cli() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let guest = sign_in("dev-editor").await;
    let project = project(&owner).await;

    // A reader of articles: whole-field reads can export, but only `content.create` imports.
    let made = call(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/roles"),
        Some(&json!({
            "key": "reader",
            "name": "Reader",
            "permissions": [{"action": "content.read", "subject": "articles"}]
        })),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    join(&owner, &guest, &project, "reader").await;

    let config = std::env::temp_dir().join(format!("colophon-cli-{}", uuid::Uuid::now_v7()));
    let output = login(&config, &owner).await;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let files = std::env::temp_dir().join(format!("colophon-io-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&files).expect("a directory");

    // A file whose third item misses a required field: a dry run reports it, an applied
    // import refuses it — and lands nothing, the one-transaction rule.
    let broken = files.join("broken.jsonl");
    std::fs::write(
        &broken,
        format!(
            "{}\n{}\n{}\n",
            article("First post"),
            article("Second post"),
            r#"{"title":"No body"}"#
        ),
    )
    .expect("written");
    let plan = colophon(
        &config,
        &[
            "content",
            "import",
            "articles",
            "--project",
            &project,
            "--file",
            broken.to_str().expect("utf-8"),
            "--dry-run",
            "--json",
        ],
    );
    assert_eq!(
        plan.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let plan = json_out(&plan);
    assert_eq!(plan["version"], 2);
    assert_eq!(plan["command"], "content import");
    assert_eq!(plan["data"]["status"], "planned", "{plan}");
    assert_eq!(plan["data"]["dry_run"], true, "{plan}");
    assert!(plan["data"]["idempotency_key"].is_null(), "{plan}");
    let outcomes = plan["data"]["outcomes"].as_array().expect("outcomes");
    assert_eq!(outcomes.len(), 3, "{plan}");
    assert_eq!(
        outcomes[0]["status"], "would_create",
        "a good item plans to create: {plan}"
    );
    assert_eq!(
        outcomes[2]["status"], "invalid",
        "the item missing `body` plans invalid: {plan}"
    );
    assert_eq!(
        outcomes[2]["refusal"]["code"], "missing_field",
        "the refusal names what it would fail with: {plan}"
    );

    // Applying it refuses at the item's pointer — and lands nothing.
    let refused = colophon(
        &config,
        &[
            "content",
            "import",
            "articles",
            "--project",
            &project,
            "--file",
            broken.to_str().expect("utf-8"),
            "--json",
        ],
    );
    assert_eq!(
        refused.status.code(),
        Some(2),
        "a 422 is the usage class: {}",
        String::from_utf8_lossy(&refused.stderr)
    );
    let envelope = json_out(&refused);
    assert_eq!(envelope["error"]["code"], "validation_failed", "{envelope}");
    assert_eq!(
        envelope["error"]["details"]["reason"], "missing_field",
        "{envelope}"
    );
    assert_eq!(
        envelope["error"]["details"]["pointer"], "/line/3/body",
        "the refusal names the line and field: {envelope}"
    );
    let list = colophon(
        &config,
        &[
            "content",
            "list",
            "articles",
            "--project",
            &project,
            "--json",
        ],
    );
    assert_eq!(
        json_out(&list)["data"]["entries"]
            .as_array()
            .expect("entries")
            .len(),
        0,
        "one bad item rolls the whole file back"
    );

    // The same three items, all valid: they land in one transaction.
    let good = files.join("good.jsonl");
    std::fs::write(
        &good,
        format!(
            "{}\n{}\n{}\n",
            article("First post"),
            article("Second post"),
            article("Third post"),
        ),
    )
    .expect("written");
    let import = colophon(
        &config,
        &[
            "content",
            "import",
            "articles",
            "--project",
            &project,
            "--file",
            good.to_str().expect("utf-8"),
            "--json",
        ],
    );
    assert_eq!(
        import.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&import.stderr)
    );
    let imported = json_out(&import);
    assert_eq!(imported["command"], "content import");
    assert_eq!(imported["data"]["status"], "imported", "{imported}");
    assert_eq!(imported["data"]["collection"], "articles", "{imported}");
    assert_eq!(imported["data"]["imported"], 3, "{imported}");
    let key = imported["data"]["idempotency_key"]
        .as_str()
        .expect("a generated key")
        .to_owned();
    let outcomes = imported["data"]["outcomes"].as_array().expect("outcomes");
    assert_eq!(outcomes.len(), 3, "{imported}");
    assert_eq!(outcomes[0]["item"], "/line/1", "{imported}");
    assert_eq!(outcomes[0]["status"], "created", "{imported}");

    // The same file under the same key is the recorded answer, not a second import.
    let again = colophon(
        &config,
        &[
            "content",
            "import",
            "articles",
            "--project",
            &project,
            "--file",
            good.to_str().expect("utf-8"),
            "--idempotency-key",
            &key,
            "--json",
        ],
    );
    let replay = json_out(&again);
    assert_eq!(replay["data"]["status"], "replayed", "{replay}");
    assert_eq!(replay["data"]["imported"], 3, "{replay}");

    // A JSON array through --data imports too — a fourth entry.
    let array = colophon(
        &config,
        &[
            "content",
            "import",
            "articles",
            "--project",
            &project,
            "--data",
            &format!("[{}]", article("Fourth post")),
            "--json",
        ],
    );
    let array = json_out(&array);
    assert_eq!(array["data"]["imported"], 1, "{array}");
    let list = colophon(
        &config,
        &[
            "content",
            "list",
            "articles",
            "--project",
            &project,
            "--json",
        ],
    );
    assert_eq!(
        json_out(&list)["data"]["entries"]
            .as_array()
            .expect("entries")
            .len(),
        4,
        "the replay added nothing, the array added one"
    );

    // Text export is the file itself: four documents, one a line, newest first.
    let export = colophon(
        &config,
        &["content", "export", "articles", "--project", &project],
    );
    assert_eq!(
        export.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&export.stderr)
    );
    let text = String::from_utf8_lossy(&export.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4, "{text}");
    for (index, line) in lines.iter().enumerate() {
        let document: Value = serde_json::from_str(line).expect("each line is a document");
        assert!(document["title"].is_string(), "line {index}: {line}");
    }
    assert!(lines[0].contains("Fourth post"), "newest first: {text}");
    assert!(
        String::from_utf8_lossy(&export.stderr).contains("Exported 4 entries"),
        "the count is the diagnostic on stderr: {}",
        String::from_utf8_lossy(&export.stderr)
    );

    // --jsonl: the same documents, each in the versioned envelope, as they arrive.
    let stream = colophon(
        &config,
        &[
            "content",
            "export",
            "articles",
            "--project",
            &project,
            "--jsonl",
        ],
    );
    assert_eq!(
        stream.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&stream.stderr)
    );
    let stdout = String::from_utf8_lossy(&stream.stdout);
    assert!(!stdout.contains('\u{1b}'), "no ANSI in the stream");
    let mut titles = Vec::new();
    for line in stdout.lines() {
        let envelope: Value = serde_json::from_str(line).expect("each line is one object");
        assert_eq!(envelope["version"], 2, "{envelope}");
        assert_eq!(envelope["command"], "content export", "{envelope}");
        titles.push(
            envelope["data"]["title"]
                .as_str()
                .expect("a title")
                .to_owned(),
        );
    }
    titles.sort_unstable();
    assert_eq!(
        titles,
        ["First post", "Fourth post", "Second post", "Third post"],
        "every entry streamed once"
    );

    // --json: the whole export as one object, `entries` like `content list`'s.
    let whole = colophon(
        &config,
        &[
            "content",
            "export",
            "articles",
            "--project",
            &project,
            "--json",
        ],
    );
    let whole = json_out(&whole);
    assert_eq!(whole["command"], "content export");
    assert_eq!(whole["data"]["collection"], "articles", "{whole}");
    assert_eq!(
        whole["data"]["entries"].as_array().expect("entries").len(),
        4,
        "{whole}"
    );

    // Round-trip: the text export's own stdout is the file another project imports.
    let exported = files.join("exported.jsonl");
    std::fs::write(&exported, &*text).expect("written");
    let other = common::project(&owner).await;
    let back = colophon(
        &config,
        &[
            "content",
            "import",
            "articles",
            "--project",
            &other,
            "--file",
            exported.to_str().expect("utf-8"),
            "--json",
        ],
    );
    assert_eq!(
        back.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&back.stderr)
    );
    assert_eq!(
        json_out(&back)["data"]["imported"],
        4,
        "the file came back whole"
    );
    let verify = colophon(
        &config,
        &[
            "content",
            "export",
            "articles",
            "--project",
            &other,
            "--jsonl",
        ],
    );
    assert_eq!(
        String::from_utf8_lossy(&verify.stdout).lines().count(),
        4,
        "and exports as four entries again"
    );

    // A file that isn't any of the shapes is a usage error before the request is made.
    let bad = files.join("bad.jsonl");
    std::fs::write(&bad, format!("{}\nnot json\n", article("Fine"))).expect("written");
    let malformed = colophon(
        &config,
        &[
            "content",
            "import",
            "articles",
            "--project",
            &project,
            "--file",
            bad.to_str().expect("utf-8"),
            "--json",
        ],
    );
    assert_eq!(malformed.status.code(), Some(2), "usage, exit 2");
    let envelope = json_out(&malformed);
    assert_eq!(envelope["error"]["code"], "usage", "{envelope}");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("line 2")),
        "the CLI's own check names the line: {envelope}"
    );
    assert!(
        String::from_utf8_lossy(&malformed.stderr).contains("error: line 2"),
        "stderr says it in words: {}",
        String::from_utf8_lossy(&malformed.stderr)
    );

    // An empty file is a usage error, not a silent no-op.
    let empty = files.join("empty.jsonl");
    std::fs::write(&empty, "  \n").expect("written");
    let nothing = colophon(
        &config,
        &[
            "content",
            "import",
            "articles",
            "--project",
            &project,
            "--file",
            empty.to_str().expect("utf-8"),
            "--json",
        ],
    );
    assert_eq!(nothing.status.code(), Some(2));
    assert_eq!(json_out(&nothing)["error"]["code"], "usage");

    // A read-only member: export reads every field, so it streams; import needs
    // `content.create`, so the API's refusal arrives whole on stdout, exit 3.
    let guest_config = std::env::temp_dir().join(format!("colophon-cli-{}", uuid::Uuid::now_v7()));
    let output = login(&guest_config, &guest).await;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let theirs = colophon(
        &guest_config,
        &["content", "export", "articles", "--project", &project],
    );
    assert_eq!(
        theirs.status.code(),
        Some(0),
        "a role that reads every field exports: {}",
        String::from_utf8_lossy(&theirs.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&theirs.stdout).lines().count(), 4);
    let denied = colophon(
        &guest_config,
        &[
            "content",
            "import",
            "articles",
            "--project",
            &project,
            "--file",
            good.to_str().expect("utf-8"),
            "--json",
        ],
    );
    assert_eq!(
        denied.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&denied.stderr)
    );
    let envelope = json_out(&denied);
    assert_eq!(envelope["command"], "content import");
    assert_eq!(envelope["error"]["code"], "forbidden", "{envelope}");
    assert_eq!(
        envelope["error"]["details"]["reason"], "missing_permission",
        "{envelope}"
    );
    assert_eq!(
        envelope["error"]["details"]["permission"], "content.create",
        "{envelope}"
    );
    assert!(envelope["error"]["request_id"].is_string(), "{envelope}");

    // --json and --jsonl pick one format; together they refuse before anything runs.
    let both = colophon(
        &config,
        &[
            "content",
            "export",
            "articles",
            "--project",
            &project,
            "--json",
            "--jsonl",
        ],
    );
    assert_eq!(both.status.code(), Some(2), "one format, not two");
    assert!(both.stdout.is_empty(), "a usage refusal stays off stdout");

    let _cleanup = std::fs::remove_dir_all(&config);
    let _cleanup = std::fs::remove_dir_all(&guest_config);
    let _cleanup = std::fs::remove_dir_all(&files);
}

/// Writes one JSON-RPC message to `stdin`, as an agent host does.
async fn mcp_send(stdin: &mut ChildStdin, message: &Value) {
    let mut line = message.to_string();
    line.push('\n');
    stdin.write_all(line.as_bytes()).await.expect("write");
    stdin.flush().await.expect("flush");
}

/// Reads lines until the response to `id` arrives; the message.
async fn mcp_answer(lines: &mut Lines<LinesBuf<ChildStdout>>, id: i64) -> Value {
    loop {
        let line = timeout(Duration::from_secs(60), lines.next_line())
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

/// `colophon mcp` is the standalone `colophon-mcp` server, linked rather than spawned
/// (D-106, D-119): the same three environment variables configure it, `initialize`
/// and `tools/list` answer over its own stdin and stdout with the same protocol and
/// the same eighteen safe tools — and closing stdin ends it, exit 0.
#[tokio::test]
#[ignore = "needs the stack, migrations and the colophon binary"]
async fn colophon_mcp_serves_the_stdio_server_in_process() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let made = call(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/tokens"),
        Some(&json!({"name": "cli-mcp", "kind": "full-access", "expires_in_days": 1})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let secret = made.body["data"]["secret"]
        .as_str()
        .expect("a secret")
        .to_owned();

    let mut child = tokio::process::Command::new(colophon_binary())
        .arg("mcp")
        .env_clear()
        .env("COLOPHON_SERVER", ORIGIN)
        .env("COLOPHON_TOKEN", &secret)
        .env("COLOPHON_PROJECT", &project)
        .env("COLOPHON_MCP_LOG", "warn")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("colophon mcp starts");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut lines = LinesBuf::new(child.stdout.take().expect("stdout")).lines();

    let init = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                   "clientInfo": {"name": "cli-test", "version": "1"}}
    });
    mcp_send(&mut stdin, &init).await;
    let answered = mcp_answer(&mut lines, 1).await;
    assert_eq!(
        answered["result"]["protocolVersion"], "2025-11-25",
        "{answered}"
    );
    assert_eq!(
        answered["result"]["serverInfo"]["name"], "colophon-mcp",
        "the same server the colophon-mcp binary runs: {answered}"
    );
    mcp_send(
        &mut stdin,
        &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    )
    .await;

    mcp_send(
        &mut stdin,
        &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    )
    .await;
    let listed = mcp_answer(&mut lines, 2).await;
    let mut tools: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    tools.sort_unstable();
    assert_eq!(
        tools,
        [
            "create_comment",
            "create_draft",
            "create_publish_request",
            "create_task",
            "get_entry",
            "get_migration",
            "get_publish_request",
            "get_release",
            "list_comments",
            "list_content_types",
            "list_entries",
            "list_migration_items",
            "list_migrations",
            "list_releases",
            "list_tasks",
            "query_entries",
            "request_review",
            "update_draft",
        ],
        "the same eighteen safe tools the standalone server offers (D-106)"
    );

    drop(stdin);
    let status = timeout(Duration::from_secs(10), child.wait())
        .await
        .expect("exits once the host closes stdin")
        .expect("wait");
    assert_eq!(status.code(), Some(0));
}
