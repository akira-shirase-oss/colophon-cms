//! Crash tests (docs/modules/17-verification.md): the server is killed in the middle of a
//! write, at the two moments that matter, and restarted.
//!
//! - **F01**, killed before the commit: no success was acknowledged and nothing was stored;
//!   retrying with the same key works.
//! - **F02**, killed after the commit but before the response: the write is stored, and retrying
//!   with the same key returns the same result instead of writing again or failing.
//! - **F03**, the worker killed after announcing a release but before recording it: the lease
//!   runs out, another worker claims the job and announces it again with the same idempotency
//!   key, so the destination can tell it's a repeat; the job ends `succeeded` after two attempts,
//!   and nothing claims the first one didn't happen.
//!
//! Sessions live in Postgres, so one sign-in serves every server in the test. Needs the stack,
//! migrations, and the fault points: `cargo test -p colophon-server --features fault-injection
//! --test faults -- --ignored`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{
    Receiver, Server, Session, article, call, destination, entries, finished_job, key, project,
    publish_article, sign_in,
};
use reqwest::{Method, StatusCode};

/// The exit code `fault::point` uses.
const FAULT_EXIT: i32 = 86;

/// Sends `request` to a server started with `COLOPHON_FAULT=fault`, expects no response, and
/// expects the server to have exited at the fault.
async fn crash_during(
    fault: &str,
    request: impl AsyncFnOnce() -> Result<common::Reply, reqwest::Error>,
) {
    let mut server = Server::start_with(&[("COLOPHON_FAULT", fault)]).await;
    let outcome = request().await;
    assert!(
        outcome.is_err(),
        "a crash leaves the request unanswered, never acknowledged: got {:?}",
        outcome.map(|reply| reply.status)
    );
    assert_eq!(
        server.exit_code().await,
        Some(FAULT_EXIT),
        "the server stopped at {fault}"
    );
}

async fn titles(session: &Session, project: &str, entry: &str) -> Vec<String> {
    let history = call(
        session,
        Method::GET,
        &format!("{}/{entry}/revisions", entries(project)),
        None,
        &[],
    )
    .await
    .expect("request");
    history.body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|revision| revision["title"].as_str().map(str::to_owned))
        .collect()
}

#[tokio::test]
#[ignore = "needs the stack, migrations and --features fault-injection"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: F01, then F02"
)]
async fn a_save_killed_before_or_after_its_commit_loses_nothing_and_never_doubles() {
    let (editor, project, entry, first) = {
        let _server = Server::start().await;
        let editor = sign_in("dev-owner").await;
        let project = project(&editor).await;
        let created = call(
            &editor,
            Method::POST,
            &entries(&project),
            Some(&article("First", "one")),
            &[("idempotency-key", &key())],
        )
        .await
        .expect("request");
        assert_eq!(created.status, StatusCode::CREATED);
        let entry = created.body["data"]["id"]
            .as_str()
            .expect("an id")
            .to_owned();
        (editor, project, entry, created.etag().expect("an ETag"))
    };
    let path = format!("{}/{entry}", entries(&project));

    // F01: killed before the commit.
    let before = key();
    let second = article("Second", "two");
    crash_during("entry.save:before-commit", async || {
        call(
            &editor,
            Method::PUT,
            &path,
            Some(&second),
            &[("if-match", &first), ("idempotency-key", &before)],
        )
        .await
    })
    .await;
    let after_f01 = {
        let _server = Server::start().await;
        let read = call(&editor, Method::GET, &path, None, &[])
            .await
            .expect("request");
        assert_eq!(
            read.etag(),
            Some(first.clone()),
            "F01: the draft didn't move"
        );
        assert_eq!(
            titles(&editor, &project, &entry).await,
            ["First"],
            "F01: nothing was stored"
        );
        // The claim rolled back with everything else, so the same key now just works.
        let retry = call(
            &editor,
            Method::PUT,
            &path,
            Some(&second),
            &[("if-match", &first), ("idempotency-key", &before)],
        )
        .await
        .expect("request");
        assert_eq!(retry.status, StatusCode::OK, "{}", retry.body);
        assert!(!retry.replayed(), "a fresh write");
        retry.etag().expect("an ETag")
    };

    // F02: killed after the commit, before the response.
    let after = key();
    let third = article("Third", "three");
    crash_during("entry.save:after-commit", async || {
        call(
            &editor,
            Method::PUT,
            &path,
            Some(&third),
            &[("if-match", &after_f01), ("idempotency-key", &after)],
        )
        .await
    })
    .await;
    let _server = Server::start().await;
    let read = call(&editor, Method::GET, &path, None, &[])
        .await
        .expect("request");
    let committed = read.etag().expect("an ETag");
    assert_ne!(committed, after_f01, "F02: the committed save is the draft");
    assert_eq!(read.body["data"]["content"]["title"], "Third");
    // The client never saw the answer, so it retries exactly the same request.
    let retry = call(
        &editor,
        Method::PUT,
        &path,
        Some(&third),
        &[("if-match", &after_f01), ("idempotency-key", &after)],
    )
    .await
    .expect("request");
    assert_eq!(
        (retry.status, retry.etag()),
        (StatusCode::OK, Some(committed)),
        "F02: the retry gets the same result, not a conflict: {}",
        retry.body
    );
    assert!(retry.replayed(), "a replay of the recorded answer");
    assert_eq!(
        titles(&editor, &project, &entry).await,
        ["Third", "Second", "First"],
        "every acknowledged save, each exactly once"
    );
}

#[tokio::test]
#[ignore = "needs the stack, migrations and --features fault-injection"]
async fn a_create_killed_after_its_commit_is_replayed_not_duplicated() {
    let (editor, project) = {
        let _server = Server::start().await;
        let editor = sign_in("dev-owner").await;
        let project = project(&editor).await;
        (editor, project)
    };
    let list = entries(&project);
    let once = key();
    let body = article("Only once", "x");

    crash_during("entry.create:after-commit", async || {
        call(
            &editor,
            Method::POST,
            &list,
            Some(&body),
            &[("idempotency-key", &once)],
        )
        .await
    })
    .await;
    let _server = Server::start().await;
    let retry = call(
        &editor,
        Method::POST,
        &list,
        Some(&body),
        &[("idempotency-key", &once)],
    )
    .await
    .expect("request");
    assert_eq!(retry.status, StatusCode::CREATED, "{}", retry.body);
    assert!(retry.replayed(), "a replay of the recorded answer");
    let listed = call(&editor, Method::GET, &list, None, &[])
        .await
        .expect("request");
    let ids: Vec<&str> = listed.body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|entry| entry["id"].as_str())
        .collect();
    assert_eq!(
        ids,
        [retry.body["data"]["id"].as_str().expect("an id")],
        "one entry, not two"
    );
}

#[tokio::test]
#[ignore = "needs the stack, migrations and --features fault-injection"]
async fn a_worker_killed_after_announcing_is_followed_by_one_with_the_same_key() {
    let receiver = Receiver::start(&[]).await;
    let owner;
    let project_id;
    {
        // Leases short enough to run out during the test; slow polling, so the release's own
        // response is long gone when the worker crashes.
        let mut server = Server::start_with(&[
            ("COLOPHON_FAULT", "worker:after-dispatch"),
            ("COLOPHON_WORKER_LEASE_MS", "1500"),
            ("COLOPHON_WORKER_ATTEMPT_MS", "500"),
            ("COLOPHON_WORKER_POLL_MS", "300"),
        ])
        .await;
        // Sessions live in Postgres: this sign-in serves the next server too.
        owner = sign_in("dev-owner").await;
        let project = project(&owner).await;
        destination(&owner, &project, &receiver.url).await;
        assert_eq!(publish_article(&owner, &project, "Crashed once").await, 1);
        assert_eq!(
            server.exit_code().await,
            Some(FAULT_EXIT),
            "the worker stopped after announcing"
        );
        project_id = project;
    }
    assert_eq!(
        receiver.seen().len(),
        1,
        "the announcement went out before the crash"
    );

    let _server = Server::start_with(&[
        ("COLOPHON_WORKER_LEASE_MS", "1500"),
        ("COLOPHON_WORKER_ATTEMPT_MS", "500"),
        ("COLOPHON_WORKER_POLL_MS", "50"),
    ])
    .await;
    let job = finished_job(&owner, &project_id, 1, 20).await;
    assert_eq!(
        (job["status"].as_str(), job["attempts"].as_i64()),
        (Some("succeeded"), Some(2)),
        "{job}"
    );
    let seen = receiver.seen();
    assert_eq!(seen.len(), 2, "announced again after the lease ran out");
    assert_eq!(
        seen[0].key, seen[1].key,
        "with the same key, so the destination can drop it"
    );
}
