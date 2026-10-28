//! Rate limits, end to end (milestone S, D-076): a credential or an address over its limit gets
//! 429 with `Retry-After` and is admitted again after it; two servers sharing Valkey share one
//! count; with Valkey stopped each server still limits on its own; and health never counts. Each
//! test speaks for a client address of its own (through a trusted proxy's `X-Forwarded-For`) or a
//! token of its own, so counts left in Valkey by other runs don't touch it. Needs the stack.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use common::{PORT, Server};
use reqwest::StatusCode;
use serde_json::Value;

/// A second server's port, beside [`PORT`].
const SECOND_PORT: u16 = 58099;

/// A documentation address (RFC 5737) no other run has used lately.
fn fresh_address() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after 1970")
        .subsec_nanos();
    format!("198.51.{}.{}", (nanos >> 8) % 256, nanos % 254 + 1)
}

/// `GET /api/v1/projects` on `port`, as `client` behind the trusted proxy, with `token` if any.
async fn projects(
    port: u16,
    client: &str,
    token: Option<&str>,
) -> (StatusCode, reqwest::header::HeaderMap, Value) {
    let mut request = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/api/v1/projects"))
        .header("x-forwarded-for", client);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.expect("request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json().await.unwrap_or(Value::Null);
    (status, headers, body)
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_credential_over_its_limit_waits_as_it_is_told_and_health_never_counts() {
    let _server = Server::start_with(&[
        // One a second with a burst of four: six requests can't earn back a refusal however slow
        // the runner (at four a second, one that took 250 ms admitted the sixth).
        ("COLOPHON_RATE_LIMIT_PER_CREDENTIAL", "1/4"),
        ("COLOPHON_RATE_LIMIT_PER_ADDRESS", "off"),
        ("COLOPHON_TRUSTED_PROXIES", "127.0.0.1"),
    ])
    .await;
    let client = fresh_address();
    let token = format!("colophon_api_{}", client.replace('.', "x"));
    let mut statuses = Vec::new();
    let mut refusal = None;
    for _ in 0..6 {
        let (status, headers, body) = projects(PORT, &client, Some(&token)).await;
        statuses.push(status.as_u16());
        if status == StatusCode::TOO_MANY_REQUESTS {
            refusal = Some((headers, body));
        }
    }
    assert_eq!(
        statuses,
        [401, 401, 401, 401, 429, 429],
        "a made-up token still counts"
    );
    let (headers, body) = refusal.expect("a refusal");
    assert_eq!(
        (&body["error"]["code"], &body["error"]["details"]["reason"]),
        (&Value::from("rate_limited"), &Value::from("rate_limited"))
    );
    let wait: u64 = headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok()?.parse().ok())
        .expect("Retry-After in seconds");
    assert!((1..=2).contains(&wait), "{wait}");

    // Another credential has its own count; health has none.
    let (other, _, _) = projects(PORT, &client, Some("colophon_api_someone_else")).await;
    assert_eq!(other, StatusCode::UNAUTHORIZED);
    for _ in 0..20 {
        let ready = reqwest::get(format!("http://127.0.0.1:{PORT}/readyz"))
            .await
            .expect("request");
        assert_ne!(ready.status(), StatusCode::TOO_MANY_REQUESTS);
    }
    tokio::time::sleep(Duration::from_secs(wait)).await;
    let (again, _, _) = projects(PORT, &client, Some(&token)).await;
    assert_eq!(
        again,
        StatusCode::UNAUTHORIZED,
        "admitted after Retry-After"
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn two_servers_sharing_valkey_share_one_count() {
    let settings = [
        ("COLOPHON_RATE_LIMIT_PER_ADDRESS", "1/5"),
        ("COLOPHON_RATE_LIMIT_PER_CREDENTIAL", "off"),
        ("COLOPHON_TRUSTED_PROXIES", "127.0.0.1"),
    ];
    let _first = Server::start_with(&settings).await;
    let _second = Server::start_on(SECOND_PORT, &settings).await;
    let client = fresh_address();
    let mut admitted = [0_u32; 2];
    let mut refused = [0_u32; 2];
    let started = Instant::now();
    for n in 0..20 {
        let (index, port) = if n % 2 == 0 {
            (0, PORT)
        } else {
            (1, SECOND_PORT)
        };
        let (status, _, _) = projects(port, &client, None).await;
        if status == StatusCode::TOO_MANY_REQUESTS {
            refused[index] += 1;
        } else {
            admitted[index] += 1;
        }
    }
    // One count admits the burst of five plus one a second while the requests last; two separate
    // counts would admit at least ten (each server sees ten requests and has a burst of five). A
    // first version allowed six at five a second, and a slow CI runner whose twenty requests took
    // about 400 ms rightly admitted seven.
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(3),
        "the requests took {elapsed:?}: too slow to tell one count from two"
    );
    let total = u128::from(admitted[0] + admitted[1]);
    let allowed = 5 + elapsed.as_millis().div_ceil(1000);
    assert!(
        (5..=allowed).contains(&total),
        "admitted {admitted:?}, refused {refused:?} in {elapsed:?}, at most {allowed} allowed"
    );
    assert!(refused[0] > 0 && refused[1] > 0, "both refuse: {refused:?}");
}

/// Stops a stack service and starts it again when dropped, whatever the test did.
struct Stopped(&'static str);

impl Stopped {
    fn compose(args: &[&str]) {
        let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/compose.yaml");
        let status = Command::new("docker")
            .args(["compose", "-f"])
            .arg(file)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("docker compose runs");
        assert!(status.success(), "docker compose {args:?} failed");
    }

    fn new(service: &'static str) -> Self {
        Self::compose(&["stop", service]);
        Self(service)
    }
}

impl Drop for Stopped {
    fn drop(&mut self) {
        Self::compose(&["start", self.0]);
    }
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn without_valkey_each_server_still_limits_on_its_own() {
    let _valkey = Stopped::new("valkey");
    let _server = Server::start_with(&[
        // One a second with a burst of four, for the same reason as above.
        ("COLOPHON_RATE_LIMIT_PER_ADDRESS", "1/4"),
        ("COLOPHON_RATE_LIMIT_PER_CREDENTIAL", "off"),
        ("COLOPHON_TRUSTED_PROXIES", "127.0.0.1"),
    ])
    .await;
    let client = fresh_address();
    let mut statuses = Vec::new();
    for _ in 0..8 {
        statuses.push(projects(PORT, &client, None).await.0.as_u16());
    }
    let admitted = statuses.iter().filter(|status| **status != 429).count();
    assert!((4..=5).contains(&admitted), "{statuses:?}");
    assert!(statuses.ends_with(&[429, 429]), "{statuses:?}");
}
