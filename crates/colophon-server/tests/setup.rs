//! A fresh installation's setup code (D-061, amending D-043), against the real binary and a
//! database of its own that nobody has bootstrapped: the server prints a code to standard error
//! when it starts, and only there; a signed-in person who enters it becomes the first
//! administrator, once; the code expires, attempts are rate-limited, and once anyone is an
//! administrator the code and every later start do nothing. The code never appears in a response
//! or in the structured log. Needs the stack and its Keycloak.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use std::{
    fs::File,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use common::{ORIGIN, PORT, Reply, Session, call, local_env, sign_in, unique_slug};
use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};
use sqlx::AssertSqlSafe;

/// A database of its own, migrated and never bootstrapped, with the URLs the server and the
/// migrator use for it.
struct FreshDatabase {
    name: String,
    admin: sqlx::PgPool,
}

/// `postgres://user:password@host/` from the stack's migration URL.
fn server_url(key: &str) -> String {
    let url = local_env()
        .into_iter()
        .find_map(|(name, value)| (name == key).then_some(value))
        .expect("deploy/local.env names it");
    let (server, _database) = url.rsplit_once('/').expect("a database in the URL");
    format!("{server}/")
}

impl FreshDatabase {
    async fn new() -> Self {
        let admin = sqlx::PgPool::connect(&format!(
            "{}postgres",
            server_url("COLOPHON_MIGRATION_DATABASE_URL")
        ))
        .await
        .expect("the stack's database");
        let name = format!("colophon_setup_{}", uuid::Uuid::now_v7().simple());
        // Generated above, never input.
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .execute(&admin)
            .await
            .expect("create database");
        let owner = sqlx::PgPool::connect(&format!(
            "{}{name}",
            server_url("COLOPHON_MIGRATION_DATABASE_URL")
        ))
        .await
        .expect("connect as owner");
        colophon_db::migrate(&owner)
            .await
            .expect("migrations apply");
        owner.close().await;
        Self { name, admin }
    }

    fn url(&self, key: &str) -> String {
        format!("{}{}", server_url(key), self.name)
    }

    async fn owner(&self) -> sqlx::PgPool {
        sqlx::PgPool::connect(&self.url("COLOPHON_MIGRATION_DATABASE_URL"))
            .await
            .expect("connect as owner")
    }

    async fn drop(self) {
        sqlx::query(AssertSqlSafe(format!(
            "DROP DATABASE {} WITH (FORCE)",
            self.name
        )))
        .execute(&self.admin)
        .await
        .expect("drop database");
    }
}

/// The real binary on the fresh database, with everything it writes kept in files.
struct Fresh {
    child: Child,
    stdout: PathBuf,
    stderr: PathBuf,
}

impl Fresh {
    async fn start(database: &FreshDatabase, dir: &Path, run: usize) -> Self {
        let stdout = dir.join(format!("stdout-{run}.log"));
        let stderr = dir.join(format!("stderr-{run}.log"));
        let mut command = Command::new(env!("CARGO_BIN_EXE_colophon-server"));
        command
            .env_clear()
            .envs(local_env())
            .env(
                "COLOPHON_DATABASE_URL",
                database.url("COLOPHON_DATABASE_URL"),
            )
            .env(
                "COLOPHON_MIGRATION_DATABASE_URL",
                database.url("COLOPHON_MIGRATION_DATABASE_URL"),
            )
            .env("COLOPHON_LISTEN_ADDR", format!("127.0.0.1:{PORT}"))
            .env("COLOPHON_PUBLIC_URL", ORIGIN)
            // As much logging as there is, in the shape production ships: the code must be in
            // none of it.
            .env("COLOPHON_LOG", "debug")
            .env("COLOPHON_LOG_FORMAT", "json")
            .stdout(Stdio::from(File::create(&stdout).expect("stdout file")))
            .stderr(Stdio::from(File::create(&stderr).expect("stderr file")));
        let child = command.spawn().expect("the server starts");
        let deadline = Instant::now() + Duration::from_secs(10);
        while reqwest::get(format!("{ORIGIN}/livez")).await.is_err() {
            assert!(Instant::now() < deadline, "the server never answered");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Self {
            child,
            stdout,
            stderr,
        }
    }

    /// The setup code this start printed, waiting up to five seconds for it.
    async fn code(&self) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let printed = std::fs::read_to_string(&self.stderr).unwrap_or_default();
            if let Some(code) = printed
                .lines()
                .find_map(|line| line.trim().strip_prefix("Setup code: "))
            {
                return code.to_owned();
            }
            assert!(
                Instant::now() < deadline,
                "no setup code was printed: {printed}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Everything the process wrote to each stream so far.
    fn output(&self) -> (String, String) {
        (
            std::fs::read_to_string(&self.stdout).unwrap_or_default(),
            std::fs::read_to_string(&self.stderr).unwrap_or_default(),
        )
    }

    /// Stops the process and returns what it wrote.
    fn stop(mut self) -> (String, String) {
        let _already_gone = self.child.kill();
        let _reaped = self.child.wait();
        self.output()
    }
}

impl Drop for Fresh {
    fn drop(&mut self) {
        let _already_gone = self.child.kill();
        let _reaped = self.child.wait();
    }
}

/// Every response a test saw, as text, to search for the code afterwards.
#[derive(Default)]
struct Seen(String);

impl Seen {
    fn note(&mut self, reply: &Reply) {
        for (name, value) in &reply.headers {
            self.0.push_str(name.as_str());
            self.0.push_str(": ");
            self.0.push_str(value.to_str().unwrap_or_default());
            self.0.push('\n');
        }
        self.0.push_str(&reply.body.to_string());
        self.0.push('\n');
    }

    async fn call(
        &mut self,
        session: &Session,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Reply {
        let reply = call(session, method, path, body, &[])
            .await
            .expect("request");
        self.note(&reply);
        reply
    }

    async fn present(&mut self, session: &Session, code: &str) -> Reply {
        self.call(
            session,
            Method::POST,
            "/api/v1/setup",
            Some(&json!({ "code": code })),
        )
        .await
    }
}

fn reason(reply: &Reply) -> Option<&str> {
    reply.body["error"]["details"]["reason"].as_str()
}

/// A scratch directory for one test's output files.
fn scratch(test: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("colophon-{test}-{}", uuid::Uuid::now_v7().simple()));
    std::fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

/// The code, however it was spelled, appears nowhere but in the banner on standard error of the
/// start that issued it.
fn printed_only_to_the_terminal(code: &str, outputs: &[(String, String)], seen: &Seen) {
    let spellings = [
        code.to_owned(),
        code.replace('-', ""),
        code.to_lowercase(),
        code.to_lowercase().replace('-', " "),
    ];
    for (stdout, _) in outputs {
        for spelling in &spellings {
            assert!(
                !stdout.contains(spelling.as_str()),
                "the setup code is in the structured log: {stdout}"
            );
        }
    }
    for spelling in &spellings {
        assert!(
            !seen.0.contains(spelling.as_str()),
            "a response carried the setup code"
        );
    }
    // One banner, from the start that issued it (the banner names the code twice: on its own
    // line and in the link that fills it in).
    let printed: usize = outputs
        .iter()
        .map(|(_, stderr)| stderr.matches(&format!("Setup code: {code}")).count())
        .sum();
    assert_eq!(printed, 1, "printed once, by the start that issued it");
    let linked: usize = outputs
        .iter()
        .map(|(_, stderr)| stderr.matches(&format!("#setup={code}")).count())
        .sum();
    assert_eq!(linked, 1, "with one link that fills it in");
}

/// The owner of `project` may publish, and may approve their own publish request (D-044), so the
/// Studio offers one-click publishing (D-059).
async fn owner_publishes_in_one_click(seen: &mut Seen, owner: &Session, project: &str) {
    let view = seen
        .call(
            owner,
            Method::GET,
            &format!("/api/v1/projects/{project}"),
            None,
        )
        .await;
    assert_eq!(view.body["data"]["role"], "owner");
    assert_eq!(view.body["data"]["self_review"], true, "{}", view.body);
    assert!(
        view.body["data"]["permissions"]
            .as_array()
            .is_some_and(|grants| grants
                .iter()
                .any(|grant| grant["action"] == "content.publish" && grant["subject"] == "*")),
        "{}",
        view.body
    );
}

/// `GET /api/v1/installation` without signing in: its body, after checking it isn't cached.
async fn installation() -> Value {
    let response = reqwest::get(format!("http://127.0.0.1:{PORT}/api/v1/installation"))
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok()),
        Some("no-store")
    );
    response.json().await.expect("JSON")
}

#[tokio::test]
#[ignore = "needs the stack and its Keycloak"]
#[expect(
    clippy::too_many_lines,
    reason = "one journey from a fresh installation to its first administrator, checked at each step"
)]
async fn a_setup_code_from_the_terminal_makes_the_first_administrator_once() {
    let database = FreshDatabase::new().await;
    let dir = scratch("setup-once");
    let server = Fresh::start(&database, &dir, 1).await;
    let code = server.code().await;
    let mut seen = Seen::default();
    let editor = sign_in("dev-editor").await;
    let owner = sign_in("dev-owner").await;

    // Signed out, the front door can tell the installation is new (D-082), and not who runs it.
    assert_eq!(
        installation().await,
        json!({"data": {"has_administrator": false}})
    );
    let me = seen.call(&editor, Method::GET, "/me", None).await;
    assert_eq!(
        (
            me.body["data"]["administrator"].as_bool(),
            me.body["data"]["installation"]["has_administrator"].as_bool()
        ),
        (Some(false), Some(false)),
        "{}",
        me.body
    );
    let refused = seen
        .call(
            &editor,
            Method::POST,
            "/api/v1/projects",
            Some(&json!({"slug": unique_slug(), "name": "Too soon"})),
        )
        .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert!(
        refused.body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("setup code")),
        "{}",
        refused.body
    );

    // A wrong code, and one that isn't even a code, change nothing.
    let mut wrong = code.clone();
    let last = if wrong.ends_with('0') { "1" } else { "0" };
    wrong.replace_range(wrong.len() - 1.., last);
    for attempt in [wrong.as_str(), "not a code"] {
        let reply = seen.present(&editor, attempt).await;
        assert_eq!(
            (reply.status, reason(&reply)),
            (StatusCode::UNPROCESSABLE_ENTITY, Some("setup_code_invalid")),
            "{}",
            reply.body
        );
    }

    // The right one, typed carelessly, works once.
    let typed = code.to_lowercase().replace('-', " ");
    let reply = seen.present(&editor, &typed).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["data"]["administrator"], true);
    let me = seen.call(&editor, Method::GET, "/me", None).await;
    assert_eq!(
        (
            me.body["data"]["administrator"].as_bool(),
            me.body["data"]["installation"]["has_administrator"].as_bool()
        ),
        (Some(true), Some(true))
    );
    assert_eq!(
        installation().await,
        json!({"data": {"has_administrator": true}})
    );
    let made = seen
        .call(
            &editor,
            Method::POST,
            "/api/v1/projects",
            Some(&json!({"slug": unique_slug(), "name": "First project"})),
        )
        .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let id = made.body["data"]["id"].as_str().expect("an id");
    owner_publishes_in_one_click(&mut seen, &editor, id).await;

    // Nobody else becomes an administrator with it, or with anything else.
    for attempt in [code.as_str(), wrong.as_str()] {
        let reply = seen.present(&owner, attempt).await;
        assert_eq!(
            (reply.status, reason(&reply)),
            (StatusCode::CONFLICT, Some("already_set_up")),
            "{}",
            reply.body
        );
    }
    let them = seen.call(&owner, Method::GET, "/me", None).await;
    assert_eq!(them.body["data"]["administrator"], false);
    let first = server.stop();

    // A later start of a set-up installation prints no code.
    let again = Fresh::start(&database, &dir, 2).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let reply = seen.present(&owner, &code).await;
    assert_eq!(reply.status, StatusCode::CONFLICT);
    let second = again.stop();
    assert!(
        !second.1.contains("Setup code"),
        "a set-up installation printed a code: {}",
        second.1
    );
    let owner_pool = database.owner().await;
    let codes: i64 = sqlx::query_scalar("SELECT count(*) FROM setup_codes")
        .fetch_one(&owner_pool)
        .await
        .expect("count");
    assert_eq!(codes, 1, "the second start issued nothing");
    owner_pool.close().await;

    printed_only_to_the_terminal(&code, &[first, second], &seen);
    database.drop().await;
    let _cleaned = std::fs::remove_dir_all(dir);
}

#[tokio::test]
#[ignore = "needs the stack and its Keycloak"]
async fn setup_codes_need_a_signed_in_caller_expire_and_are_rate_limited() {
    let database = FreshDatabase::new().await;
    let dir = scratch("setup-limits");
    let server = Fresh::start(&database, &dir, 1).await;
    let code = server.code().await;
    let mut seen = Seen::default();

    // Signed out, or a cookie without its CSRF token: refused before anything is looked at.
    let anonymous = common::app()
        .post(format!("{ORIGIN}/api/v1/setup"))
        .json(&json!({ "code": code }))
        .send()
        .await
        .expect("request");
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    let editor = sign_in("dev-editor").await;
    let (status, body) = common::post(
        &editor,
        "/api/v1/setup",
        &json!({ "code": code }),
        None,
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    seen.0.push_str(&body.to_string());

    // An expired code is dead, and says no more than a wrong one.
    let owner_pool = database.owner().await;
    sqlx::query(
        "UPDATE setup_codes SET issued_at = now() - interval '2 hours', expires_at = now() - interval '1 second'",
    )
    .execute(&owner_pool)
    .await
    .expect("age the code");
    let expired = seen.present(&editor, &code).await;
    assert_eq!(
        (expired.status, reason(&expired)),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("setup_code_invalid"))
    );

    // Five attempts in fifteen minutes; the sixth waits, whatever it presents.
    for _ in 0..4 {
        let reply = seen.present(&editor, "AAAAA-AAAAA-AAAAA-AAAAA").await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    let first = server.stop();
    let server = Fresh::start(&database, &dir, 2).await;
    let fresh = server.code().await;
    let limited = seen.present(&editor, &fresh).await;
    assert_eq!(
        limited.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        limited.body
    );
    assert_eq!(limited.body["error"]["code"], "rate_limited");
    let wait: u32 = limited.headers[header::RETRY_AFTER]
        .to_str()
        .expect("ascii")
        .parse()
        .expect("seconds");
    assert!((850..=900).contains(&wait), "{wait}");
    let me = seen.call(&editor, Method::GET, "/me", None).await;
    assert_eq!(me.body["data"]["administrator"], false, "the limit held");

    // Someone else, within their own limit, may.
    let owner = sign_in("dev-owner").await;
    let reply = seen.present(&owner, &fresh).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM setup_attempts")
        .fetch_one(&owner_pool)
        .await
        .expect("count");
    assert_eq!(attempts, 6, "the refused attempt wasn't recorded");
    owner_pool.close().await;
    let second = server.stop();

    printed_only_to_the_terminal(&code, &[first.clone(), second.clone()], &seen);
    printed_only_to_the_terminal(&fresh, &[first, second], &seen);
    database.drop().await;
    let _cleaned = std::fs::remove_dir_all(dir);
}
