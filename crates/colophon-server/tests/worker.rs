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
