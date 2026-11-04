//! The request quota, end to end (migration 0045; D-118): a token's `requests_per_minute` is a
//! rolling window counted by the admission machinery — Postgres's `api_token_limits` row stays
//! the only authority for the ceiling — so a token over it hears 429 with `Retry-After` while a
//! person's session on the same project is untouched (L08: the noisy agent is throttled; the
//! editors keep their latency). The episode's first refusal is the audit log's one
//! `token.rate_limited` event; the window's reset and Valkey's absence behave the way the
//! shared limiter's do. Needs the stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use std::time::Duration;

use common::{Reply, Server, Session, Stopped, app, call, entries, key, project, sign_in};
use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};

/// The window these tests run: two seconds instead of the minute it names, so a reset is
/// seconds of sleep rather than a minute of them (`COLOPHON_QUOTA_WINDOW_MS`).
const WINDOW: (&str, &str) = ("COLOPHON_QUOTA_WINDOW_MS", "2000");

fn tokens(project: &str) -> String {
    format!("/api/v1/projects/{project}/tokens")
}

async fn send(session: &Session, method: Method, path: &str, body: Option<&Value>) -> Reply {
    call(session, method, path, body, &[])
        .await
        .expect("request")
}

/// A request made with an API token's secret, as a script would send it.
async fn as_token(secret: &str, method: Method, path: &str) -> Reply {
    let response = app()
        .request(method, format!("{}{path}", common::ORIGIN))
        .header(header::AUTHORIZATION, format!("Bearer {secret}"))
        .header("idempotency-key", key())
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

/// Makes a token carrying `limits`; its secret, its id and the identity it acts as.
async fn make(session: &Session, project: &str, limits: &Value) -> (String, String, String) {
    let made = send(
        session,
        Method::POST,
        &tokens(project),
        Some(&json!({
            "name": "Quota-bound", "kind": "full-access", "expires_in_days": 1,
            "limits": limits
        })),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let secret = made.body["data"]["secret"]
        .as_str()
        .expect("the secret, once")
        .to_owned();
    let id = made.body["data"]["id"].as_str().expect("an id").to_owned();
    let identity = made.body["data"]["identity_id"]
        .as_str()
        .expect("an identity")
        .to_owned();
    (secret, id, identity)
}

/// `Retry-After`'s seconds, required on a 429.
fn retry_after(reply: &Reply) -> u64 {
    reply
        .headers
        .get(header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok()?.parse().ok())
        .expect("Retry-After in seconds")
}

/// The `token.rate_limited` events the project logged so far, newest first.
async fn rate_limited_events(owner: &Session, project: &str) -> Vec<Value> {
    let log = send(
        owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/audit?action=token.rate_limited"),
        None,
    )
    .await;
    assert_eq!(log.status, StatusCode::OK, "{}", log.body);
    log.body["data"].as_array().expect("events").clone()
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn an_over_quota_token_is_throttled_while_people_work_and_audited_once_per_episode() {
    let _server = Server::start_with(&[WINDOW]).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    // Three requests a window: the fourth hears about it, the third never has to wait long.
    let (secret, id, identity) = make(&owner, &project, &json!({"requests_per_minute": 3})).await;
    let list = entries(&project);

    for n in 0..3 {
        let reply = as_token(&secret, Method::GET, &list).await;
        assert_eq!(reply.status, StatusCode::OK, "request {n}: {}", reply.body);
    }

    // Over the ceiling: 429, the envelope's `rate_limited`, and `Retry-After` says when.
    let mut wait = 0;
    for _ in 0..3 {
        let refused = as_token(&secret, Method::GET, &list).await;
        assert_eq!(
            (
                refused.status,
                &refused.body["error"]["code"],
                &refused.body["error"]["details"]["reason"]
            ),
            (
                StatusCode::TOO_MANY_REQUESTS,
                &json!("rate_limited"),
                &json!("rate_limited")
            ),
            "{}",
            refused.body
        );
        wait = wait.max(retry_after(&refused));
    }
    assert!(wait >= 1, "a wait to report");

    // While it is throttled, the people on the project are not: its ceiling is its own (L08).
    for _ in 0..4 {
        let reply = send(&owner, Method::GET, &list, None).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    }

    // The window it spent is the usage the token's view shows.
    let one = send(
        &owner,
        Method::GET,
        &format!("{}/{id}", tokens(&project)),
        None,
    )
    .await;
    let spent = one.body["data"]["usage"]["requests_last_minute"]
        .as_i64()
        .expect("a count");
    assert!(
        (1..=3).contains(&spent),
        "the limiter's window, not the ledger's 0: {}",
        one.body
    );

    // Three refusals, one episode: the log heard the transition once, naming the token and
    // what its ceiling was — the actor is the token itself.
    let events = rate_limited_events(&owner, &project).await;
    assert_eq!(events.len(), 1, "one event per episode: {events:?}");
    assert_eq!(
        (
            &events[0]["subject"],
            &events[0]["actor"]["id"],
            &events[0]["actor"]["kind"],
            &events[0]["detail"]["requests_per_minute"]
        ),
        (&json!(id), &json!(identity), &json!("token"), &json!(3)),
        "{events:?}"
    );

    // The wait the header named is real: it is admitted again, and a new episode records a
    // new event — transition, not spam, but never silent either. (Two refusals: the first
    // opens the second episode, the second stays inside it.)
    tokio::time::sleep(Duration::from_secs(wait)).await;
    let admitted = as_token(&secret, Method::GET, &list).await;
    assert_eq!(admitted.status, StatusCode::OK, "{}", admitted.body);
    for _ in 0..2 {
        let refused = as_token(&secret, Method::GET, &list).await;
        assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
    }
    assert_eq!(
        rate_limited_events(&owner, &project).await.len(),
        2,
        "a new episode is a new event"
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_token_without_the_ceiling_is_never_quota_limited() {
    let _server = Server::start_with(&[WINDOW]).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;

    // No limits row at all, and a row that bounds something else: both unconstrained here.
    let (unlimited, ..) = make(&owner, &project, &json!({"documents_per_day": 1000})).await;
    let made = send(
        &owner,
        Method::POST,
        &tokens(&project),
        Some(&json!({
            "name": "Plain", "kind": "read-only", "expires_in_days": 1
        })),
    )
    .await;
    let plain = made.body["data"]["secret"]
        .as_str()
        .expect("the secret, once")
        .to_owned();
    let list = entries(&project);
    for secret in [&unlimited, &plain] {
        for _ in 0..6 {
            let reply = as_token(secret, Method::GET, &list).await;
            assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        }
    }
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn without_valkey_a_token_is_still_limited_never_unlimited() {
    let _valkey = Stopped::new("valkey");
    let _server = Server::start_with(&[WINDOW]).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let (secret, ..) = make(&owner, &project, &json!({"requests_per_minute": 3})).await;
    let list = entries(&project);

    // Valkey away fails open only at the shared layer: this process's own count still
    // refuses — looser across a cluster, never unlimited, the same way the admission
    // limits degrade (S.2).
    let mut statuses = Vec::new();
    for _ in 0..6 {
        statuses.push(as_token(&secret, Method::GET, &list).await.status);
    }
    let admitted = statuses
        .iter()
        .filter(|status| **status == StatusCode::OK)
        .count();
    let refused = statuses
        .iter()
        .filter(|status| **status == StatusCode::TOO_MANY_REQUESTS)
        .count();
    assert_eq!((admitted, refused), (3, 3), "{statuses:?}");
    assert!(
        statuses.ends_with(&[StatusCode::TOO_MANY_REQUESTS; 3]),
        "{statuses:?}"
    );
}
