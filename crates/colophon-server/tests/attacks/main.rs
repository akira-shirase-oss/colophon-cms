//! The attack suite (D-231): real requests against the real binary and the local stack, one module
//! per attack class in docs/security/attack-surface.md, each asserting the refusal — the status, the
//! code where there is one, and that nothing changed. A gap the survey found is a test here that
//! failed before its fix. Run in CI's integration job with the other stack tests
//! (`--ignored --test-threads=1`); locally, by exact name only.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

#[path = "../common/mod.rs"]
mod common;

mod bodies;
mod injection;
mod limits;
mod media;
mod replay;
mod sessions;
mod ssrf;
mod tenancy;
mod tokens;

use common::{ORIGIN, Reply, Session, app, call, key};
use reqwest::{Method, StatusCode, header};
use serde_json::Value;

/// A request with no credentials at all, as a stranger sends it.
async fn anonymous(method: Method, path: &str, body: Option<&Value>) -> Reply {
    let mut request = app().request(method, format!("{ORIGIN}{path}"));
    if let Some(body) = body {
        request = request.json(body);
    }
    reply(request.send().await.expect("request")).await
}

/// A request made with an API token's secret, as a script would send it.
async fn as_token(secret: &str, method: Method, path: &str, body: Option<&Value>) -> Reply {
    let mut request = app()
        .request(method, format!("{ORIGIN}{path}"))
        .header(header::AUTHORIZATION, format!("Bearer {secret}"))
        .header("idempotency-key", key());
    if let Some(body) = body {
        request = request.json(body);
    }
    reply(request.send().await.expect("request")).await
}

async fn reply(response: reqwest::Response) -> Reply {
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json().await.unwrap_or(Value::Null);
    Reply {
        status,
        headers,
        body,
    }
}

/// A session's request, with any extra headers; the reply.
async fn send(session: &Session, method: Method, path: &str, body: Option<&Value>) -> Reply {
    call(session, method, path, body, &[("idempotency-key", &key())])
        .await
        .expect("request")
}

/// Makes an API token in `project`; its secret.
async fn token(owner: &Session, project: &str, body: &Value) -> String {
    let made = send(
        owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/tokens"),
        Some(body),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    made.body["data"]["secret"]
        .as_str()
        .expect("the secret")
        .to_owned()
}

/// The signed-in person's identity id.
async fn identity_of(session: &Session) -> String {
    let me = send(session, Method::GET, "/me", None).await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.body);
    me.body["data"]["id"].as_str().expect("an id").to_owned()
}

/// A refusal: a 4xx, never a 2xx and never a 500 — whatever the attack, the server answered it.
fn refused(reply: &Reply, what: &str) {
    assert!(
        reply.status.is_client_error(),
        "{what}: expected a refusal, got {} {}",
        reply.status,
        reply.body
    );
}
