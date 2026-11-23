//! The sign-up guards (D-235) against the real binary: Cloudflare Turnstile checked server-side
//! (a stub `siteverify` on loopback stands in for Cloudflare), failing closed; the per-address
//! cap counting every attempt, refused ones included, keyed on the address the server derives
//! and never on a header the client writes; the staff auth log naming no email or password; and
//! a taken email answering in about the time a new one does. Needs the stack and
//! `scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use common::{ORIGIN, Reply, Server, app, owner_database, unique_slug};
use reqwest::StatusCode;
use serde_json::{Value, json};

/// What the stub `siteverify` answers.
#[derive(Clone)]
enum Mode {
    /// `success: true`, solved on this hostname.
    Pass(&'static str),
    /// `success: false` with this error code.
    Fail(&'static str),
    /// This HTTP status, no body.
    Status(u16),
    /// Not JSON.
    Garbage,
    /// Answers only after this long.
    Slow(Duration),
}

#[derive(Default)]
struct Seen {
    forms: Vec<Vec<(String, String)>>,
}

struct Stub {
    url: String,
    mode: Arc<Mutex<Mode>>,
    seen: Arc<Mutex<Seen>>,
    task: tokio::task::JoinHandle<()>,
}

impl Stub {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a free port");
        let address = listener.local_addr().expect("an address");
        let mode = Arc::new(Mutex::new(Mode::Pass("localhost")));
        let seen = Arc::new(Mutex::new(Seen::default()));
        let (m, s) = (Arc::clone(&mode), Arc::clone(&seen));
        let app = axum::Router::new().route(
            "/siteverify",
            axum::routing::post(move |body: String| {
                let (mode, seen) = (Arc::clone(&m), Arc::clone(&s));
                async move {
                    let form = url::form_urlencoded::parse(body.as_bytes())
                        .map(|(k, v)| (k.into_owned(), v.into_owned()))
                        .collect();
                    seen.lock().expect("lock").forms.push(form);
                    let mode = mode.lock().expect("lock").clone();
                    answer(mode).await
                }
            }),
        );
        let task = tokio::spawn(async move {
            let _stopped = axum::serve(listener, app).await;
        });
        Self {
            url: format!("http://{address}/siteverify"),
            mode,
            seen,
            task,
        }
    }

    fn set(&self, mode: Mode) {
        *self.mode.lock().expect("lock") = mode;
    }

    fn calls(&self) -> usize {
        self.seen.lock().expect("lock").forms.len()
    }

    fn last(&self) -> Vec<(String, String)> {
        self.seen
            .lock()
            .expect("lock")
            .forms
            .last()
            .cloned()
            .unwrap_or_default()
    }
}

async fn answer(mode: Mode) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    match mode {
        Mode::Pass(host) => {
            axum::Json(json!({"success": true, "hostname": host, "error-codes": []}))
                .into_response()
        }
        Mode::Fail(code) => {
            axum::Json(json!({"success": false, "error-codes": [code]})).into_response()
        }
        Mode::Status(code) => axum::http::StatusCode::from_u16(code)
            .expect("a status")
            .into_response(),
        Mode::Garbage => "<html>not json</html>".into_response(),
        Mode::Slow(wait) => {
            tokio::time::sleep(wait).await;
            axum::Json(json!({"success": true, "hostname": "localhost"})).into_response()
        }
    }
}

/// A staff auth log line as read back: event, identity, email digest, address, detail.
type LogLine = (
    String,
    Option<uuid::Uuid>,
    Option<Vec<u8>>,
    Option<String>,
    Option<String>,
);

const SECRET: &str = "0x4AAAAAAAtest-secret-not-real";
const EVENT_KEY: &str = "an-auth-event-key-for-tests-only-0123456789";

fn guarded(stub: &Stub, cap: &str) -> Vec<(&'static str, String)> {
    vec![
        ("COLOPHON_REGISTRATION", "open".to_owned()),
        (
            "COLOPHON_TURNSTILE_SITE_KEY",
            "1x00000000000000000000AA".to_owned(),
        ),
        ("COLOPHON_TURNSTILE_SECRET", SECRET.to_owned()),
        ("COLOPHON_TURNSTILE_VERIFY_URL", stub.url.clone()),
        ("COLOPHON_AUTH_EVENT_KEY", EVENT_KEY.to_owned()),
        ("COLOPHON_SIGNUPS_PER_ADDRESS_HOUR", cap.to_owned()),
    ]
}

async fn start(env: &[(&'static str, String)]) -> Server {
    let pairs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    Server::start_with(&pairs).await
}

/// Every test here signs up and in from loopback: its counts start at zero.
async fn forget_loopback_count() {
    let db = owner_database().await;
    sqlx::query("DELETE FROM signup_throttle WHERE address IN ('127.0.0.1', '-')")
        .execute(&db)
        .await
        .expect("reset the loopback count");
    sqlx::query("DELETE FROM sign_in_log_budget WHERE address IN ('127.0.0.1', '-')")
        .execute(&db)
        .await
        .expect("reset the loopback log budget");
    db.close().await;
}

fn fresh_email() -> String {
    format!("guard-{}@colophon.test", unique_slug())
}

async fn sign_up(body: &Value, headers: &[(&str, &str)]) -> Reply {
    let mut request = app().post(format!("{ORIGIN}/auth/signup")).json(body);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = request.send().await.expect("request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json::<Value>().await.unwrap_or(Value::Null);
    Reply {
        status,
        headers,
        body,
    }
}

fn reason(reply: &Reply) -> Option<&str> {
    reply.body["error"]["details"]["reason"].as_str()
}

async fn has_account(email: &str) -> bool {
    let db = owner_database().await;
    let found = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM identities WHERE email = $1)")
        .bind(email)
        .fetch_one(&db)
        .await
        .expect("query");
    db.close().await;
    found
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn the_bot_check_refuses_missing_replayed_foreign_and_unverifiable_tokens_failing_closed() {
    forget_loopback_count().await;
    let stub = Stub::start().await;
    let _server = start(&guarded(&stub, "1000")).await;
    let refused = |reply: &Reply| {
        reply.status == StatusCode::UNPROCESSABLE_ENTITY
            && reason(reply) == Some("bot_check_failed")
    };

    // The sign-up page learns the site key from the public providers answer.
    let providers: Value = app()
        .get(format!("{ORIGIN}/auth/providers"))
        .send()
        .await
        .expect("request")
        .json()
        .await
        .expect("json");
    assert_eq!(
        providers["data"]["turnstile_site_key"], "1x00000000000000000000AA",
        "{providers}"
    );

    // No token: refused without asking Cloudflare.
    let email = fresh_email();
    let reply = sign_up(&json!({"email": email, "password": "colophon-dev"}), &[]).await;
    assert!(refused(&reply), "{} {}", reply.status, reply.body);
    assert_eq!(stub.calls(), 0, "a missing token is refused unsent");
    assert!(!has_account(&email).await);

    let cases: [(&str, Mode); 5] = [
        ("replayed", Mode::Fail("timeout-or-duplicate")),
        ("for another hostname", Mode::Pass("evil.example.dev")),
        ("the verifier failing", Mode::Status(500)),
        ("the verifier answering garbage", Mode::Garbage),
        ("the verifier too slow", Mode::Slow(Duration::from_secs(6))),
    ];
    for (case, mode) in cases {
        stub.set(mode);
        let email = fresh_email();
        let started = Instant::now();
        let reply = sign_up(
            &json!({"email": email, "password": "colophon-dev", "turnstile_token": "tok-1"}),
            &[],
        )
        .await;
        assert!(refused(&reply), "{case}: {} {}", reply.status, reply.body);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{case}: the verifier's deadline bounds the wait ({:?})",
            started.elapsed()
        );
        assert!(!has_account(&email).await, "{case}: no account");
    }

    // A good token for this host passes, and Cloudflare was told the secret, the token and the
    // address the server derived (loopback), never one a header claimed.
    stub.set(Mode::Pass("localhost"));
    let email = fresh_email();
    let reply = sign_up(
        &json!({"email": email, "password": "colophon-dev", "turnstile_token": "tok-good"}),
        &[
            ("x-forwarded-for", "6.6.6.6"),
            ("cf-connecting-ip", "6.6.6.6"),
        ],
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let form = stub.last();
    let field = |name: &str| {
        form.iter()
            .find_map(|(k, v)| (k == name).then(|| v.clone()))
    };
    assert_eq!(field("secret").as_deref(), Some(SECRET));
    assert_eq!(field("response").as_deref(), Some("tok-good"));
    assert_eq!(field("remoteip").as_deref(), Some("127.0.0.1"));

    // The verifier gone altogether: still closed.
    stub.task.abort();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let email = fresh_email();
    let reply = sign_up(
        &json!({"email": email, "password": "colophon-dev", "turnstile_token": "tok-2"}),
        &[],
    )
    .await;
    assert!(
        refused(&reply),
        "verifier down: {} {}",
        reply.status,
        reply.body
    );
    assert!(!has_account(&email).await);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn the_signup_cap_counts_refused_attempts_and_believes_no_forwarded_header() {
    forget_loopback_count().await;
    let stub = Stub::start().await;
    stub.set(Mode::Fail("invalid-input-response"));
    let _server = start(&guarded(&stub, "3")).await;
    // Three attempts the bot check refuses, each claiming another address in every header a
    // client could write: all three count against the one address the server sees.
    for spoof in ["1.1.1.1", "2.2.2.2", "3.3.3.3"] {
        let reply = sign_up(
            &json!({"email": fresh_email(), "password": "colophon-dev", "turnstile_token": "t"}),
            &[
                ("x-forwarded-for", spoof),
                ("cf-connecting-ip", spoof),
                ("x-real-ip", spoof),
                ("forwarded", &format!("for={spoof}")),
            ],
        )
        .await;
        assert_eq!(reason(&reply), Some("bot_check_failed"), "{}", reply.body);
    }
    // The fourth is refused before the bot check, even with a token that would pass.
    stub.set(Mode::Pass("localhost"));
    let calls = stub.calls();
    let email = fresh_email();
    let reply = sign_up(
        &json!({"email": email, "password": "colophon-dev", "turnstile_token": "t"}),
        &[("cf-connecting-ip", "4.4.4.4")],
    )
    .await;
    assert_eq!(
        (reply.status, reason(&reply)),
        (StatusCode::TOO_MANY_REQUESTS, Some("rate_limited")),
        "{}",
        reply.body
    );
    let retry_after: u32 = reply
        .headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .expect("a Retry-After in seconds");
    assert!((1..=3600).contains(&retry_after), "{retry_after}");
    assert_eq!(
        stub.calls(),
        calls,
        "a capped attempt never reaches the bot check"
    );
    assert!(!has_account(&email).await);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn the_staff_auth_log_records_sign_up_and_sign_in_with_no_email_or_password() {
    forget_loopback_count().await;
    let stub = Stub::start().await;
    let _server = start(&guarded(&stub, "1000")).await;
    let db = owner_database().await;
    let since: time::OffsetDateTime = sqlx::query_scalar("SELECT now()")
        .fetch_one(&db)
        .await
        .expect("now");
    let email = fresh_email();
    let password = "a-password-the-log-never-holds";
    let made = sign_up(
        &json!({"email": email, "password": password, "turnstile_token": "t"}),
        &[],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let taken = sign_up(
        &json!({"email": email, "password": password, "turnstile_token": "t"}),
        &[],
    )
    .await;
    assert_eq!(reason(&taken), Some("email_taken"), "{}", taken.body);
    let wrong = app()
        .post(format!("{ORIGIN}/auth/signin"))
        .json(&json!({"email": email, "password": "not-it-at-all"}))
        .send()
        .await
        .expect("request");
    assert_eq!(wrong.status(), StatusCode::BAD_REQUEST);

    let lines: Vec<LogLine> = sqlx::query_as(
        "SELECT event, identity_id, email_digest, address, detail FROM staff_auth_events
             WHERE at >= $1 ORDER BY at, id",
    )
    .bind(since)
    .fetch_all(&db)
    .await
    .expect("lines");
    let events: Vec<&str> = lines.iter().map(|line| line.0.as_str()).collect();
    assert_eq!(
        events,
        ["signed_up", "sign_up_refused", "sign_in_failed"],
        "{lines:?}"
    );
    let key = aws_lc_rs::hmac::Key::new(aws_lc_rs::hmac::HMAC_SHA256, EVENT_KEY.as_bytes());
    let digest = aws_lc_rs::hmac::sign(&key, email.as_bytes())
        .as_ref()
        .to_vec();
    assert!(lines[0].1.is_some(), "the sign-up names its account");
    assert_eq!(lines[1].2.as_deref(), Some(digest.as_slice()));
    assert_eq!(lines[1].4.as_deref(), Some("email_taken"));
    assert_eq!(
        lines[2].2.as_deref(),
        Some(digest.as_slice()),
        "a failure names the email by its keyed digest only"
    );
    assert!(
        lines
            .iter()
            .all(|line| line.3.as_deref() == Some("127.0.0.1"))
    );
    // Nothing in any column of the log reads as the email or the password.
    let text: String = sqlx::query_scalar(
        "SELECT coalesce(string_agg(t::text, ' '), '') FROM staff_auth_events t WHERE at >= $1",
    )
    .bind(since)
    .fetch_one(&db)
    .await
    .expect("the rows as text");
    assert!(!text.contains(&email), "the email is in the log: {text}");
    assert!(!text.contains(password), "the password is in the log");
    assert!(
        !text.contains("not-it-at-all"),
        "a tried password is in the log"
    );
    db.close().await;
}

/// A taken email and a new one take about as long to answer: both pay Argon2 and a transaction,
/// so the answer's timing doesn't tell which emails have accounts (the 409 itself does; G1 is
/// reduced, not closed, until slice 2's always-202). Medians of interleaved samples, so a slow
/// moment on a CI runner falls on both sides.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_taken_email_takes_about_as_long_to_refuse_as_a_new_one_to_make() {
    forget_loopback_count().await;
    let _server = Server::start_with(&[("COLOPHON_REGISTRATION", "open")]).await;
    let taken = fresh_email();
    let made = sign_up(&json!({"email": taken, "password": "colophon-dev"}), &[]).await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let (mut new, mut old) = (Vec::new(), Vec::new());
    for _ in 0..15 {
        let started = Instant::now();
        let reply = sign_up(
            &json!({"email": fresh_email(), "password": "colophon-dev"}),
            &[],
        )
        .await;
        new.push(started.elapsed());
        assert_eq!(reply.status, StatusCode::CREATED);
        let started = Instant::now();
        let reply = sign_up(&json!({"email": taken, "password": "colophon-dev"}), &[]).await;
        old.push(started.elapsed());
        assert_eq!(reason(&reply), Some("email_taken"));
    }
    new.sort();
    old.sort();
    let (new, old) = (new[new.len() / 2], old[old.len() / 2]);
    let ratio = old.as_secs_f64() / new.as_secs_f64();
    assert!(
        (0.67..=1.5).contains(&ratio),
        "a taken email's median {old:?} against a new one's {new:?} (ratio {ratio:.2})"
    );
}

/// A spray of failed sign-ins can't grow the append-only log at its own rate (D-235): one address
/// gets `SIGN_IN_FAILURES_LOGGED_PER_HOUR` lines an hour, one more marked `log_capped`, then none,
/// however many emails it tries. The misses are still refused as before.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn failed_sign_ins_past_the_log_budget_add_no_lines() {
    forget_loopback_count().await;
    let _server = Server::start_with(&[("COLOPHON_REGISTRATION", "open")]).await;
    let db = owner_database().await;
    let since: time::OffsetDateTime = sqlx::query_scalar("SELECT now()")
        .fetch_one(&db)
        .await
        .expect("now");
    let budget =
        usize::try_from(colophon_db::staff_auth::SIGN_IN_FAILURES_LOGGED_PER_HOUR).expect("small");
    for _ in 0..budget + 15 {
        let reply = app()
            .post(format!("{ORIGIN}/auth/signin"))
            .json(&json!({"email": fresh_email(), "password": "a guess that misses"}))
            .send()
            .await
            .expect("request");
        assert_eq!(reply.status(), StatusCode::BAD_REQUEST, "still refused");
    }
    let lines: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT detail FROM staff_auth_events
         WHERE event = 'sign_in_failed' AND at >= $1 ORDER BY at, id",
    )
    .bind(since)
    .fetch_all(&db)
    .await
    .expect("lines");
    assert_eq!(lines.len(), budget + 1, "{lines:?}");
    assert!(
        lines[..budget]
            .iter()
            .all(|d| d.as_deref() == Some("password"))
    );
    assert_eq!(lines[budget].as_deref(), Some("log_capped"));
    db.close().await;
}
