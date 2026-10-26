//! The first owner (step 1.8, D-043), against the real binaries: `scripts/migrate.sh` has
//! bootstrapped the development realm's dev-owner, who `/me` calls an administrator and who may
//! create projects; dev-editor, signed in just the same, may not; and `colophon admin bootstrap`
//! refuses to make anyone else an administrator. Needs the stack and `scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use std::{path::PathBuf, process::Command};

use common::{Server, call, local_env, sign_in, unique_slug};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// dev-owner's subject, fixed in `deploy/keycloak/colophon-realm.json`.
const DEV_OWNER: &str = "00000000-0000-4000-8000-00000000c001";

/// `colophon admin bootstrap` with the local settings, as `scripts/migrate.sh` runs it.
fn bootstrap(args: &[&str]) -> (Option<i32>, Value, String) {
    let cli = PathBuf::from(env!("CARGO_BIN_EXE_colophon-server")).with_file_name("colophon");
    let output = Command::new(cli)
        .args(["admin", "bootstrap", "--json"])
        .args(args)
        .env_clear()
        .envs(local_env())
        .output()
        .expect("the CLI runs");
    let stdout = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    (
        output.status.code(),
        stdout,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh, which bootstraps dev-owner"]
async fn only_the_bootstrapped_administrator_creates_projects_and_bootstrap_runs_once() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let editor = sign_in("dev-editor").await;

    let me = call(&owner, Method::GET, "/me", None, &[])
        .await
        .expect("request");
    assert_eq!(
        (
            me.body["data"]["administrator"].as_bool(),
            me.body["data"]["subject"].as_str()
        ),
        (Some(true), Some(DEV_OWNER))
    );
    let them = call(&editor, Method::GET, "/me", None, &[])
        .await
        .expect("request");
    assert_eq!(them.body["data"]["administrator"], false);
    let editor_subject = them.body["data"]["subject"]
        .as_str()
        .expect("a subject")
        .to_owned();

    let new = |slug: String| json!({"slug": slug, "name": "Bootstrap test"});
    let refused = call(
        &editor,
        Method::POST,
        "/api/v1/projects",
        Some(&new(unique_slug())),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
    assert!(
        refused.body["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("administrator")),
        "{}",
        refused.body
    );
    let created = call(
        &owner,
        Method::POST,
        "/api/v1/projects",
        Some(&new(unique_slug())),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    // Bootstrap runs once: again for dev-owner it changes nothing; for anyone else it's refused.
    let (code, out, stderr) = bootstrap(&["--subject", DEV_OWNER]);
    assert_eq!(
        (code, out["status"].as_str()),
        (Some(0), Some("already_bootstrapped")),
        "{stderr}"
    );
    let (code, _, stderr) = bootstrap(&["--subject", &editor_subject]);
    assert_eq!(code, Some(4), "{stderr}");
    assert!(stderr.contains("bootstrap runs once"), "{stderr}");
    let (code, _, stderr) = bootstrap(&["--subject", "x", "--issuer", "http://id.example.com"]);
    assert_eq!(code, Some(2), "a plain-http issuer off localhost: {stderr}");
    let still = call(&editor, Method::GET, "/me", None, &[])
        .await
        .expect("request");
    assert_eq!(still.body["data"]["administrator"], false);
}
