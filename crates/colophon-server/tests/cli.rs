//! The command line against the real server (step 1.6): `colophon login` approved in a browser
//! session, then `whoami`, `content create` and `content list` creating and reading the same
//! records the Studio does, every exit code of the CLI contract, and `logout` revoking the token.
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
};

use common::{ORIGIN, Server, Session, call, sign_in, unique_slug};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

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

/// Runs `colophon login`, approves it as `approver`, and waits for it to finish.
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
    let code = tokio::task::spawn_blocking(move || {
        BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
            .find_map(|line| {
                line.split("shows the code ")
                    .nth(1)
                    .map(|code| code.trim_end_matches('.').to_owned())
            })
    })
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
    tokio::task::spawn_blocking(move || child.wait_with_output().expect("the CLI exits"))
        .await
        .expect("task")
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
    assert_eq!(json_out(&output)["status"], "signed_in");
    let credentials = config.join("credentials.json");
    let mode = std::fs::metadata(&credentials)
        .expect("written")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "only the owner can read the token");

    let whoami = colophon(&config, &["whoami", "--json"]);
    assert_eq!(json_out(&whoami)["me"]["display_name"], "Dev Owner");

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
    assert_eq!(outcome["status"], "created");
    let id = outcome["entry"]["id"].as_str().expect("an id").to_owned();
    let key = outcome["idempotency_key"]
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
        (replay["status"].as_str(), replay["entry"]["id"].as_str()),
        (Some("replayed"), Some(id.as_str()))
    );
    let list = colophon(
        &config,
        &["content", "list", "articles", "--project", &slug, "--json"],
    );
    let entries = json_out(&list)["entries"]
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
