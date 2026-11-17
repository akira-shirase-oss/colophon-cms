//! The worker end to end (step 2.3, D-046): a committed release is announced to its destination
//! with a stable idempotency key, retried after a server error, given up on after a refusal, and
//! an older release still waiting when a newer one commits is superseded, never sent. Needs the
//! stack and `scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{
    Receiver, Server, call, destination, finished_job, join, project, publish_article, sign_in,
};
use reqwest::{Method, StatusCode};
use serde_json::json;

/// Workers that look for work often, so the tests don't wait.
const FAST: [(&str, &str); 1] = [("COLOPHON_WORKER_POLL_MS", "50")];

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_release_is_announced_once_with_a_stable_key() {
    let _server = Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let receiver = Receiver::start(&[]).await;
    destination(&owner, &project, &receiver.url).await;

    let generation = publish_article(&owner, &project, "Announced").await;
    let job = finished_job(&owner, &project, generation, 15).await;
    assert_eq!(
        (job["status"].as_str(), job["attempts"].as_i64()),
        (Some("succeeded"), Some(1)),
        "{job}"
    );
    let seen = receiver.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].generation, generation.to_string());
    assert!(seen[0].key.starts_with("colophon:"), "{}", seen[0].key);
    assert_eq!(seen[0].body["event"], "release.committed");
    assert_eq!(seen[0].body["entries"][0]["title"], "Announced");

    // Editors can read deliveries but not destinations, whose addresses may carry secrets.
    let editor = sign_in("dev-editor").await;
    join(&owner, &editor, &project, "editor").await;
    let base = format!("/api/v1/projects/{project}/environments/production");
    let jobs = call(&editor, Method::GET, &format!("{base}/jobs"), None, &[])
        .await
        .expect("request");
    assert_eq!(jobs.status, StatusCode::OK);
    let hidden = call(
        &editor,
        Method::GET,
        &format!("{base}/destinations"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(hidden.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn server_errors_are_retried_and_refusals_are_final() {
    let _server = Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;

    // 503 then 200: delivered on the second attempt, with the same key both times.
    let flaky = project(&owner).await;
    let receiver = Receiver::start(&[503]).await;
    destination(&owner, &flaky, &receiver.url).await;
    let generation = publish_article(&owner, &flaky, "Second time lucky").await;
    let job = finished_job(&owner, &flaky, generation, 15).await;
    assert_eq!(
        (job["status"].as_str(), job["attempts"].as_i64()),
        (Some("succeeded"), Some(2)),
        "{job}"
    );
    let seen = receiver.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].key, seen[1].key, "a retry reuses the key");

    // 400: refused, so not retried.
    let refusing = project(&owner).await;
    let receiver = Receiver::start(&[400]).await;
    destination(&owner, &refusing, &receiver.url).await;
    let generation = publish_article(&owner, &refusing, "Refused").await;
    let job = finished_job(&owner, &refusing, generation, 15).await;
    assert_eq!(
        (
            job["status"].as_str(),
            job["attempts"].as_i64(),
            job["last_http_status"].as_i64()
        ),
        (Some("failed"), Some(1), Some(400)),
        "{job}"
    );
    assert_eq!(receiver.seen().len(), 1);

    // Addresses are checked when a destination is saved.
    let base = format!("/api/v1/projects/{refusing}/environments/production/destinations");
    for url in [
        "ftp://example.com/hook",
        "https://user:secret@example.com/hook",
        "not a url",
    ] {
        let reply = call(
            &owner,
            Method::POST,
            &base,
            Some(&json!({"name": "Bad", "url": url})),
            &[],
        )
        .await
        .expect("request");
        assert_eq!(
            reply.body["error"]["details"]["reason"], "invalid_url",
            "{url}"
        );
    }
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn an_older_release_waiting_when_a_newer_commits_is_never_sent() {
    let owner_project;
    let receiver = Receiver::start(&[]).await;
    {
        // An API-only server: releases commit, nothing is delivered yet.
        let _server = Server::start_with(&[("COLOPHON_ROLE", "api")]).await;
        let owner = sign_in("dev-owner").await;
        let project = project(&owner).await;
        destination(&owner, &project, &receiver.url).await;
        assert_eq!(publish_article(&owner, &project, "First").await, 1);
        assert_eq!(publish_article(&owner, &project, "Second").await, 2);
        owner_project = project;
    }
    let _server = Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let second = finished_job(&owner, &owner_project, 2, 15).await;
    assert_eq!(second["status"], "succeeded", "{second}");
    let first = finished_job(&owner, &owner_project, 1, 15).await;
    assert_eq!(
        (first["status"].as_str(), first["attempts"].as_i64()),
        (Some("superseded"), Some(0)),
        "{first}"
    );
    let generations: Vec<String> = receiver
        .seen()
        .into_iter()
        .map(|request| request.generation)
        .collect();
    assert_eq!(
        generations,
        ["2"],
        "release 1's announcement never went out (F05)"
    );
}

/// SSRF (D-149): a loopback listen address is no evidence of a development machine — a
/// production server behind a local reverse proxy binds loopback too — so private
/// destinations are an explicit opt-in, `COLOPHON_ALLOW_PRIVATE_DESTINATIONS=1`, never
/// inferred. Without it a loopback-bound server refuses to save a private or plain-http
/// address, and its worker refuses to send to one saved earlier.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_loopback_server_refuses_private_destinations_unless_told_otherwise() {
    let receiver = Receiver::start(&[]).await;
    let owner_project;
    {
        // The development stack opts in (deploy/local.env): a local receiver is fine there.
        let _server = Server::start_with(&[("COLOPHON_ALLOW_PRIVATE_DESTINATIONS", "1")]).await;
        let owner = sign_in("dev-owner").await;
        let project = project(&owner).await;
        destination(&owner, &project, &receiver.url).await;
        owner_project = project;
    }

    // The same loopback bind without the opt-in: the production default.
    let mut strict = FAST.to_vec();
    strict.push(("COLOPHON_ALLOW_PRIVATE_DESTINATIONS", "0"));
    let _server = Server::start_with(&strict).await;
    let owner = sign_in("dev-owner").await;
    let base = format!("/api/v1/projects/{owner_project}/environments/production/destinations");
    for url in [
        receiver.url.as_str(),
        "https://127.0.0.1/hook",
        "https://169.254.169.254/latest/meta-data/",
        "https://10.0.0.5/hook",
        "https://[::1]/hook",
        "http://example.com/hook",
    ] {
        let reply = call(
            &owner,
            Method::POST,
            &base,
            Some(&json!({"name": "Private", "url": url})),
            &[],
        )
        .await
        .expect("request");
        assert_eq!(
            reply.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{url}: {}",
            reply.body
        );
        assert_eq!(
            reply.body["error"]["details"]["reason"], "invalid_url",
            "{url}"
        );
    }

    // The destination saved while private addresses were allowed is never reached: the
    // worker checks the address again at send time and gives the job up.
    let generation = publish_article(&owner, &owner_project, "Never sent").await;
    let job = finished_job(&owner, &owner_project, generation, 15).await;
    assert_eq!(job["status"].as_str(), Some("failed"), "{job}");
    assert!(
        receiver.seen().is_empty(),
        "nothing reached the private receiver"
    );
}

/// A proxy named in the environment never carries a delivery (D-149): the worker's address
/// checks run in its own resolver, and a proxy resolves the destination's name itself, so a
/// request sent through one would go wherever the proxy chose. With `HTTP_PROXY` and friends
/// pointing at a listener, the delivery still reaches the receiver directly, and no request
/// the listener sees names the receiver. (Other clients — a configured identity provider's,
/// say — may use an operator's proxy; only destinations must not.)
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_proxy_in_the_environment_never_carries_a_delivery() {
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a proxy port");
    let proxy = format!("http://{}", listener.local_addr().expect("an address"));
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = Arc::clone(&requests);
    let task = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut head = vec![0_u8; 2048];
            let read = stream.read(&mut head).await.unwrap_or(0);
            let line = String::from_utf8_lossy(&head[..read])
                .lines()
                .next()
                .unwrap_or_default()
                .to_owned();
            seen.lock().expect("not poisoned").push(line);
            let _ = stream
                .write_all(b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\n\r\n")
                .await;
        }
    });
    let mut settings: Vec<(&str, &str)> = FAST.to_vec();
    for name in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        settings.push((name, proxy.as_str()));
    }
    settings.extend([("NO_PROXY", ""), ("no_proxy", "")]);
    let _server = Server::start_with(&settings).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let receiver = Receiver::start(&[]).await;
    destination(&owner, &project, &receiver.url).await;

    let generation = publish_article(&owner, &project, "Not through a proxy").await;
    let job = finished_job(&owner, &project, generation, 15).await;
    assert_eq!(job["status"].as_str(), Some("succeeded"), "{job}");
    assert_eq!(receiver.seen().len(), 1);
    let address = receiver
        .url
        .trim_start_matches("http://")
        .trim_end_matches("/hook")
        .to_owned();
    let through: Vec<String> = requests
        .lock()
        .expect("not poisoned")
        .iter()
        .filter(|line| line.contains(&address))
        .cloned()
        .collect();
    assert!(
        through.is_empty(),
        "deliveries went through the proxy: {through:?}"
    );
    task.abort();
}
