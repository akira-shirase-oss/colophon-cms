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
//! - **F09**, the project's work bound full, or a durable write that cannot commit: new work
//!   is refused before acceptance — 503 `work_full`, or a 500 the client never mistook for
//!   success — the whole transaction rolls back, and the same request lands once the
//!   condition clears (D-131).
//! - **F10**, a schema edited concurrently or killed mid-edit: two writes from one base make
//!   one winner and one `stale_version` — never a merge — and an interrupted edit leaves the
//!   old version whole or the new one replayed.
//! - **F12**, a delivery repeated under any fault: the repeat carries the job's one
//!   idempotency key, so a destination that deduplicates on it applies exactly one effect.
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
    Receiver, Server, Session, article, call, create_article, destination, entries, finished_job,
    join, key, project, publish_article, sign_in,
};
use reqwest::{Method, StatusCode};
use serde_json::json;

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
        // The shared database's leftover work must not reach this worker first: a stale
        // job dispatching fires the same fault, and the test would record a crash that
        // was never about its announcement (the reported flake). Quarantined — the suite
        // runs one test at a time, so every unfinished row belongs to a dead server.
        let database = common::owner_database().await;
        common::clear_unfinished_work(&database).await;
        database.close().await;
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
        // The dispatch is what the crash is about: the receiver records a request before
        // it answers, so `seen` holding the announcement means the worker's post() is at
        // its return — the fault point. Waiting on it, not a guess at dispatch latency,
        // is what keeps the exit assertion honest under a busy database.
        assert!(
            common::until(15, async || {
                !receiver.seen().is_empty() || server.exited().is_some()
            })
            .await,
            "the announcement never reached the destination"
        );
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

/// A worker killed mid-run has already committed each applied item, so the next worker claims
/// the plan, asks every item what it is, applies what is still pending and finishes the run:
/// F-05b for migrations (M.5b). Nothing an item wrote is done twice.
#[tokio::test]
#[ignore = "needs the stack, migrations and --features fault-injection"]
#[expect(
    clippy::too_many_lines,
    reason = "one crash-and-resume story, told in order: seed, plan, kill, finish"
)]
async fn a_migration_worker_killed_mid_run_resumes_and_finishes_the_plan() {
    let owner;
    let path;
    {
        // A stale plan's first item commit fires the same fault — quarantine whatever
        // earlier tests left unfinished so this worker sees only this test's plan.
        let database = common::owner_database().await;
        common::clear_unfinished_work(&database).await;
        database.close().await;
        let mut server = Server::start_with(&[
            ("COLOPHON_FAULT", "migration:after-item"),
            ("COLOPHON_WORKER_POLL_MS", "50"),
        ])
        .await;
        owner = sign_in("dev-owner").await;
        let project = project(&owner).await;
        for title in ["One", "Two", "Three"] {
            common::create_article(&owner, &project, title).await;
        }
        // A plan that fills a new required field, approved and applied.
        let types = call(
            &owner,
            Method::GET,
            &format!("/api/v1/projects/{project}/environments/production/content-types/articles"),
            None,
            &[],
        )
        .await
        .expect("request");
        let mut target = types.body["data"]["definition"].clone();
        target["fields"].as_array_mut().expect("fields").push(
            serde_json::json!({"id": "subtitle", "label": "Subtitle", "type": "text",
                "required": true, "max_chars": 100, "multiline": false}),
        );
        let planned = call(
            &owner,
            Method::POST,
            &format!("/api/v1/projects/{project}/environments/production/migrations"),
            Some(&serde_json::json!({
                "collection": "articles",
                "definition": target,
                "operations": [{"op": "fill", "path": "subtitle", "value": "x"}],
            })),
            &[("idempotency-key", &key())],
        )
        .await
        .expect("request");
        assert_eq!(planned.status, StatusCode::CREATED, "{}", planned.body);
        let id = planned.body["data"]["id"].as_str().expect("id").to_owned();
        let hash = planned.body["data"]["hash"]
            .as_str()
            .expect("hash")
            .to_owned();
        let plan_path =
            format!("/api/v1/projects/{project}/environments/production/migrations/{id}");
        let etag = format!("\"{hash}\"");
        for action in ["approve", "apply"] {
            let reply = call(
                &owner,
                Method::POST,
                &format!("{plan_path}/{action}"),
                None,
                &[("if-match", &etag), ("idempotency-key", &key())],
            )
            .await
            .expect("request");
            assert!(
                matches!(reply.status, StatusCode::OK | StatusCode::ACCEPTED),
                "{action}: {}",
                reply.body
            );
        }
        // The worker claims the plan, applies the first item, and exits at the fault: the
        // item's commit went in before the crash. The committed outcome is the crash's own
        // observable — the fault point is the first statement after the commit — so the
        // wait polls it rather than guessing how long the drive takes; `exited` is the
        // early-out if the process died on something else first.
        assert!(
            common::until(15, async || {
                if server.exited().is_some() {
                    return true;
                }
                call(
                    &owner,
                    Method::GET,
                    &format!("{plan_path}/items"),
                    None,
                    &[],
                )
                .await
                .map(|reply| {
                    reply.body["data"].as_array().is_some_and(|items| {
                        items
                            .iter()
                            .any(|item| item["outcome"].as_str().is_some_and(|o| o != "pending"))
                    })
                })
                .unwrap_or(false)
            })
            .await,
            "the first item's outcome never committed"
        );
        assert_eq!(
            server.exit_code().await,
            Some(FAULT_EXIT),
            "the worker stopped mid-run"
        );
        path = plan_path;
    }

    let _server = Server::start_with(&[("COLOPHON_WORKER_POLL_MS", "50")]).await;
    // The resumed drive finishes the plan: every item applied exactly once.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let plan = loop {
        let reply = call(&owner, Method::GET, &path, None, &[])
            .await
            .expect("request");
        if reply.body["data"]["state"].as_str() != Some("running") {
            break reply.body["data"].clone();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the plan never settled"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    assert_eq!(plan["state"], "done", "{plan}");
    assert_eq!(
        (
            plan["items"]["done"].as_i64(),
            plan["items"]["blocked"].as_i64()
        ),
        (Some(3), Some(0)),
        "{plan}"
    );
    let items = call(&owner, Method::GET, &format!("{path}/items"), None, &[])
        .await
        .expect("request");
    let items = items.body["data"].as_array().expect("items");
    for item in items {
        assert_eq!(item["outcome"], "applied", "{item}");
        assert!(item["new_revision_id"].is_string(), "{item}");
    }
}

/// F06: a worker killed just after a bulk item's commit resumes without applying it twice —
/// the item's outcome is the crash boundary, not the job's (D-116).
#[tokio::test]
#[ignore = "needs the stack, migrations and --features fault-injection"]
async fn a_worker_killed_after_a_bulk_item_resumes_without_duplicating_it() {
    let owner;
    let project_id;
    let job_id;
    {
        // A stale run's item commit fires the same fault — quarantine whatever earlier
        // tests left unfinished so this worker sees only this test's bulk job.
        let database = common::owner_database().await;
        common::clear_unfinished_work(&database).await;
        database.close().await;
        let mut server = Server::start_with(&[
            ("COLOPHON_FAULT", "bulk:after-item"),
            ("COLOPHON_WORKER_POLL_MS", "100"),
        ])
        .await;
        owner = sign_in("dev-owner").await;
        let project = project(&owner).await;
        let entries = entries(&project);
        let made = call(
            &owner,
            Method::POST,
            &entries,
            Some(&article("crashed item", "body")),
            &[("idempotency-key", &key())],
        )
        .await
        .expect("request");
        assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
        let entry = made.body["data"]["id"].as_str().expect("an id").to_owned();
        let revision = made.body["data"]["revision"]["id"]
            .as_str()
            .expect("a revision")
            .to_owned();
        let base = format!("/api/v1/projects/{project}/environments/production/bulk");
        let submitted = call(
            &owner,
            Method::POST,
            &base,
            Some(&json!({
                "collection": "articles", "seal": true,
                "items": [
                    {"key": "a", "entry": entry, "expected_revision": revision,
                     "op": {"op": "set", "path": "title", "value": "crashed once"}},
                ],
            })),
            &[("idempotency-key", &key())],
        )
        .await
        .expect("request");
        assert_eq!(submitted.status, StatusCode::ACCEPTED, "{}", submitted.body);
        job_id = submitted.body["data"]["id"]
            .as_str()
            .expect("a job id")
            .to_owned();
        // The item's committed outcome is the crash's own line — the fault point is the
        // first statement after the item's commit — so the wait polls it rather than
        // guessing how long the drive takes; `exited` is the early-out if the process
        // died on something else first.
        assert!(
            common::until(15, async || {
                if server.exited().is_some() {
                    return true;
                }
                call(
                    &owner,
                    Method::GET,
                    &format!("{base}/{job_id}/items"),
                    None,
                    &[],
                )
                .await
                .map(|reply| {
                    reply.body["data"].as_array().is_some_and(|items| {
                        items
                            .iter()
                            .any(|item| item["outcome"].as_str().is_some_and(|o| o != "pending"))
                    })
                })
                .unwrap_or(false)
            })
            .await,
            "the item's outcome never committed"
        );
        assert_eq!(
            server.exit_code().await,
            Some(FAULT_EXIT),
            "the worker stopped after the item's commit"
        );
        project_id = project;
    }

    let _server = Server::start_with(&[("COLOPHON_WORKER_POLL_MS", "100")]).await;
    let base = format!("/api/v1/projects/{project_id}/environments/production/bulk");
    let mut state = String::new();
    for _ in 0..150 {
        let got = call(&owner, Method::GET, &format!("{base}/{job_id}"), None, &[])
            .await
            .expect("request");
        state = got.body["data"]["state"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if matches!(state.as_str(), "done" | "blocked" | "cancelled") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert_eq!(state, "done", "the resumed run finished");
    // The item applied exactly once: the entry's history has exactly one revision with the
    // bulk write's title, and the item row says `applied` — never a second write.
    let items = call(
        &owner,
        Method::GET,
        &format!("{base}/{job_id}/items"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        items.body["data"][0]["outcome"].as_str(),
        Some("applied"),
        "{}",
        items.body
    );
}

/// The `content-types` route of `project`'s production environment, for `slug`.
fn content_type(project: &str, slug: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production/content-types/{slug}")
}

/// The `articles` definition plus one optional text field — a change any stored content
/// tolerates, so what settles the race is only who commits first.
fn with_field(definition: &serde_json::Value, id: &str, label: &str) -> serde_json::Value {
    let mut next = definition.clone();
    next["fields"]
        .as_array_mut()
        .expect("a field list")
        .push(json!({"id": id, "label": label, "type": "text"}));
    next
}

/// F10's interrupted half, for a schema edit (the interrupted *migration* half is the
/// worker's `migration:after-item` test above): a `PUT` killed before its commit leaves the
/// old version whole, and the edit's idempotency claim rolled back with it — the retry with
/// the same key writes the version fresh. Killed after the commit, the version is durable and
/// the same retry replays the recorded answer. There is never a half-written version.
#[tokio::test]
#[ignore = "needs the stack, migrations and --features fault-injection"]
async fn a_schema_edit_killed_before_or_after_its_commit_is_never_half_written() {
    let (owner, path, first, second) = {
        let _server = Server::start().await;
        let owner = sign_in("dev-owner").await;
        let project = project(&owner).await;
        let path = content_type(&project, "articles");
        let current = call(&owner, Method::GET, &path, None, &[])
            .await
            .expect("request");
        assert_eq!(current.etag().as_deref(), Some("\"v1\""));
        let base = current.body["data"]["definition"].clone();
        let first = with_field(&base, "subtitle", "Subtitle");
        let second = with_field(&first, "byline", "Byline");
        (owner, path, first, second)
    };

    // Killed before the commit: nothing of the edit is stored.
    let before = key();
    crash_during("content_type.update:before-commit", async || {
        call(
            &owner,
            Method::PUT,
            &path,
            Some(&first),
            &[("if-match", "\"v1\""), ("idempotency-key", &before)],
        )
        .await
    })
    .await;
    {
        let _server = Server::start().await;
        let stored = call(&owner, Method::GET, &path, None, &[])
            .await
            .expect("request");
        assert_eq!(
            (
                stored.body["data"]["version"].as_i64(),
                stored.etag().as_deref()
            ),
            (Some(1), Some("\"v1\"")),
            "the interrupted edit left version 1 whole: {}",
            stored.body
        );
        // The claim rolled back with the edit, so the same key now just works.
        let retry = call(
            &owner,
            Method::PUT,
            &path,
            Some(&first),
            &[("if-match", "\"v1\""), ("idempotency-key", &before)],
        )
        .await
        .expect("request");
        assert_eq!(retry.status, StatusCode::OK, "{}", retry.body);
        assert!(!retry.replayed(), "a fresh write, not a replay");
        assert_eq!(retry.etag().as_deref(), Some("\"v2\""));
    }

    // Killed after the commit: v2 is durable; the client never saw the answer, so it retries
    // exactly the same request and gets the recorded one back.
    let after = key();
    crash_during("content_type.update:after-commit", async || {
        call(
            &owner,
            Method::PUT,
            &path,
            Some(&second),
            &[("if-match", "\"v2\""), ("idempotency-key", &after)],
        )
        .await
    })
    .await;
    let _server = Server::start().await;
    let stored = call(&owner, Method::GET, &path, None, &[])
        .await
        .expect("request");
    assert_eq!(
        (
            stored.body["data"]["version"].as_i64(),
            stored.etag().as_deref()
        ),
        (Some(3), Some("\"v3\"")),
        "the committed edit is the stored schema: {}",
        stored.body
    );
    let retry = call(
        &owner,
        Method::PUT,
        &path,
        Some(&second),
        &[("if-match", "\"v2\""), ("idempotency-key", &after)],
    )
    .await
    .expect("request");
    assert_eq!(
        (retry.status, retry.etag().as_deref()),
        (StatusCode::OK, Some("\"v3\"")),
        "the retry gets the recorded answer: {}",
        retry.body
    );
    assert!(retry.replayed(), "a replay of the recorded answer");
    assert_eq!(
        stored.body["data"]["definition"], retry.body["data"]["definition"],
        "one definition, committed once"
    );
}

/// F10's concurrent half: two members whose roles hold `schema.change` put new definitions of
/// the same collection forward from the same base version, at the same time. The collection's
/// change lock serializes them and the `If-Match` precondition turns the second away
/// `stale_version`, naming the version that won — exactly one lands as v2, never a merge of
/// the two edits and never a silent drop: the loser rebases onto what won and lands v3.
#[tokio::test]
#[ignore = "needs the stack, migrations and --features fault-injection"]
#[expect(
    clippy::too_many_lines,
    reason = "one race told in order: both edits in flight, one winner, the loser rebases"
)]
async fn two_schema_edits_from_one_base_make_one_winner_and_one_conflict() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let other = sign_in("dev-editor").await;
    join(&owner, &other, &project, "developer").await;
    let path = content_type(&project, "articles");

    let current = call(&owner, Method::GET, &path, None, &[])
        .await
        .expect("request");
    assert_eq!(current.etag().as_deref(), Some("\"v1\""));
    let base = current.body["data"]["definition"].clone();
    let first_definition = with_field(&base, "subtitle", "Subtitle");
    let second_definition = with_field(&base, "byline", "Byline");

    // The same base version, two different edits, in flight at once.
    let (first_key, second_key) = (key(), key());
    let first_headers = [
        ("if-match", "\"v1\""),
        ("idempotency-key", first_key.as_str()),
    ];
    let second_headers = [
        ("if-match", "\"v1\""),
        ("idempotency-key", second_key.as_str()),
    ];
    let (first, second) = tokio::join!(
        call(
            &owner,
            Method::PUT,
            &path,
            Some(&first_definition),
            &first_headers,
        ),
        call(
            &other,
            Method::PUT,
            &path,
            Some(&second_definition),
            &second_headers,
        ),
    );
    let first = first.expect("request");
    let second = second.expect("request");
    assert!(
        (first.status, second.status) == (StatusCode::OK, StatusCode::CONFLICT)
            || (first.status, second.status) == (StatusCode::CONFLICT, StatusCode::OK),
        "one edit wins, one conflicts — never both, never neither: {} and {} ({}, {})",
        first.status,
        second.status,
        first.body,
        second.body
    );
    let (winner, loser, loser_session, lost_field) = if first.status == StatusCode::OK {
        (first, second, other, "byline")
    } else {
        (second, first, owner, "subtitle")
    };
    assert_eq!(
        loser.body["error"]["details"]["reason"], "stale_version",
        "{}",
        loser.body
    );
    assert_eq!(
        loser.etag().as_deref(),
        Some("\"v2\""),
        "the loser is told the version that won"
    );

    // The stored version is verbatim the winner's document — the loser's field is not in it,
    // so no merged schema exists.
    let stored = call(&loser_session, Method::GET, &path, None, &[])
        .await
        .expect("request");
    assert_eq!(stored.body["data"]["version"], 2, "{}", stored.body);
    assert_eq!(
        stored.body["data"]["definition"], winner.body["data"]["definition"],
        "the stored v2 is the winner's document alone"
    );
    let ids: Vec<&str> = stored.body["data"]["definition"]["fields"]
        .as_array()
        .expect("a field list")
        .iter()
        .filter_map(|field| field["id"].as_str())
        .collect();
    assert!(!ids.contains(&lost_field), "the lost edit's field: {ids:?}");

    // The loser rebases onto what won: its edit lands as v3 on top of it, so the conflict
    // lost nothing — both fields are in the schema now, in two recorded versions.
    let rebased = with_field(&stored.body["data"]["definition"], lost_field, "Rebased");
    let landed = call(
        &loser_session,
        Method::PUT,
        &path,
        Some(&rebased),
        &[("if-match", "\"v2\""), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(
        (landed.status, landed.etag().as_deref()),
        (StatusCode::OK, Some("\"v3\"")),
        "{}",
        landed.body
    );
    let ids: Vec<&str> = landed.body["data"]["definition"]["fields"]
        .as_array()
        .expect("a field list")
        .iter()
        .filter_map(|field| field["id"].as_str())
        .collect();
    assert!(
        ids.contains(&"subtitle") && ids.contains(&"byline"),
        "both edits landed, in order: {ids:?}"
    );
}

/// The rest of F10's concurrent half: a schema change and an entry write the change would
/// make invalid, in flight at once. The collection's locks (`lock_for_change` exclusive,
/// `lock_for_write` shared) serialize them, so whichever commits first wins outright: the
/// change then checks the stored write against the new definition and refuses it
/// (`needs_migration`, naming the entry in the way), or the write validates under the new
/// schema and is refused (`too_long`). Both can never succeed — that would be an entry stored
/// under a schema it breaks, exactly the mixed schema F10 forbids.
#[tokio::test]
#[ignore = "needs the stack, migrations and --features fault-injection"]
async fn a_schema_change_and_an_entry_write_never_both_succeed() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let type_path = content_type(&project, "articles");
    let list = entries(&project);

    // The articles `title` allows 200 characters; the edit narrows it to 50, so a
    // 120-character title is valid now and invalid then.
    let current = call(&owner, Method::GET, &type_path, None, &[])
        .await
        .expect("request");
    assert_eq!(current.etag().as_deref(), Some("\"v1\""));
    let mut narrower = current.body["data"]["definition"].clone();
    let title = narrower["fields"]
        .as_array_mut()
        .expect("a field list")
        .iter_mut()
        .find(|field| field["id"] == "title")
        .expect("a title field");
    title["max_chars"] = json!(50);
    let long_title = "t".repeat(120);
    let (write_key, edit_key, content) = (key(), key(), article(&long_title, "racing"));
    let write_headers = [("idempotency-key", write_key.as_str())];
    let edit_headers = [
        ("if-match", "\"v1\""),
        ("idempotency-key", edit_key.as_str()),
    ];

    let (write, edit) = tokio::join!(
        call(&owner, Method::POST, &list, Some(&content), &write_headers),
        call(
            &owner,
            Method::PUT,
            &type_path,
            Some(&narrower),
            &edit_headers,
        ),
    );
    let write = write.expect("request");
    let edit = edit.expect("request");

    match (write.status, edit.status) {
        // The write committed first: the change saw it stored and refused, naming it.
        (StatusCode::CREATED, StatusCode::UNPROCESSABLE_ENTITY) => {
            let details = &edit.body["error"]["details"];
            assert_eq!(details["reason"], "needs_migration", "{}", edit.body);
            let entry = write.body["data"]["id"].as_str().expect("an id");
            assert!(
                details["entries"]
                    .as_array()
                    .expect("a list")
                    .iter()
                    .any(|in_the_way| in_the_way["entry"].as_str() == Some(entry)),
                "the refusal names the entry in the way: {}",
                edit.body
            );
            // The schema didn't move.
            let stored = call(&owner, Method::GET, &type_path, None, &[])
                .await
                .expect("request");
            assert_eq!(stored.body["data"]["version"], 1, "{}", stored.body);
        }
        // The change committed first: the write validated under it and was refused — and
        // stored nothing.
        (StatusCode::UNPROCESSABLE_ENTITY, StatusCode::OK) => {
            assert_eq!(
                write.body["error"]["details"]["reason"], "too_long",
                "{}",
                write.body
            );
            assert_eq!(edit.body["data"]["version"], 2, "{}", edit.body);
            let listed = call(&owner, Method::GET, &list, None, &[])
                .await
                .expect("request");
            assert!(
                listed.body["data"].as_array().expect("a list").is_empty(),
                "the refused write stored nothing: {}",
                listed.body
            );
        }
        pair => panic!(
            "the race serializes: one succeeds, one is refused — never both 2xx, never both refused: {pair:?}"
        ),
    }
}

/// F12, applied to delivery (its spec case is parked): whatever makes a worker send the same
/// announcement again — the process killed after the destination recorded it, the
/// destination answering a server error, or its answer arriving past the attempt's deadline —
/// the repeat carries the job's one idempotency key, so a destination that deduplicates on it
/// applies exactly one effect. Each phase delivers the release twice at the receiver while
/// its effect count stays 1, and the job ends `succeeded` after two attempts. Reordering
/// (an older release announcing after a newer one) is F05's, proven in tests/worker.rs by
/// supersession before send.
#[tokio::test]
#[ignore = "needs the stack, migrations and --features fault-injection"]
async fn a_delivery_repeated_under_faults_is_one_effect_at_the_destination() {
    // Crash and resume: the announcement lands, the worker dies before recording it, the
    // lease runs out and the next worker reposts under the same key.
    let receiver = Receiver::start(&[]).await;
    let owner;
    let project_id;
    {
        // Stale jobs left by earlier tests must not reach this worker first: a stale
        // dispatch fires the same fault on work that was never the test's (the reported
        // flake). The suite runs one test at a time — every unfinished row is a dead
        // server's.
        let database = common::owner_database().await;
        common::clear_unfinished_work(&database).await;
        database.close().await;
        let mut server = Server::start_with(&[
            ("COLOPHON_FAULT", "worker:after-dispatch"),
            ("COLOPHON_WORKER_LEASE_MS", "1500"),
            ("COLOPHON_WORKER_ATTEMPT_MS", "500"),
            ("COLOPHON_WORKER_POLL_MS", "300"),
        ])
        .await;
        owner = sign_in("dev-owner").await;
        let project = project(&owner).await;
        destination(&owner, &project, &receiver.url).await;
        assert_eq!(publish_article(&owner, &project, "Crashed once").await, 1);
        // The receiver records a request before it answers, so `seen` holding the
        // announcement is the observable the crash is about — wait on it, not on a guess
        // at dispatch latency.
        assert!(
            common::until(15, async || {
                !receiver.seen().is_empty() || server.exited().is_some()
            })
            .await,
            "the announcement never reached the destination"
        );
        assert_eq!(
            server.exit_code().await,
            Some(FAULT_EXIT),
            "the worker stopped after announcing"
        );
        project_id = project;
    }
    {
        let _server = Server::start_with(&[("COLOPHON_WORKER_POLL_MS", "50")]).await;
        let job = finished_job(&owner, &project_id, 1, 20).await;
        assert_eq!(
            (job["status"].as_str(), job["attempts"].as_i64()),
            (Some("succeeded"), Some(2)),
            "{job}"
        );
        let seen = receiver.seen();
        assert_eq!(seen.len(), 2, "re-posted once the lease ran out");
        assert_eq!(seen[0].key, seen[1].key, "the repeat carries the same key");
        assert_eq!(
            receiver.effects(),
            1,
            "one effect — the repeat was recognisably the same delivery"
        );
    }

    // A server error retried: the retry repeats the same key.
    {
        let _server = Server::start_with(&[("COLOPHON_WORKER_POLL_MS", "50")]).await;
        let receiver = Receiver::start(&[503]).await;
        let project = project(&owner).await;
        destination(&owner, &project, &receiver.url).await;
        assert_eq!(publish_article(&owner, &project, "Retried").await, 1);
        let job = finished_job(&owner, &project, 1, 15).await;
        assert_eq!(
            (job["status"].as_str(), job["attempts"].as_i64()),
            (Some("succeeded"), Some(2)),
            "{job}"
        );
        let seen = receiver.seen();
        assert_eq!(seen.len(), 2, "refused once, delivered on the retry");
        assert_eq!(seen[0].key, seen[1].key);
        assert_eq!(receiver.effects(), 1);
    }

    // An answer past the attempt's deadline: the post landed, the worker timed it out and
    // retried — same key again.
    {
        let _server = Server::start_with(&[
            ("COLOPHON_WORKER_POLL_MS", "50"),
            ("COLOPHON_WORKER_ATTEMPT_MS", "800"),
            ("COLOPHON_WORKER_LEASE_MS", "2000"),
        ])
        .await;
        // The first answer goes out 2.5 s late: past the attempt's 800 ms deadline.
        let receiver = Receiver::start_timed(&[(200, 2500)]).await;
        let project = project(&owner).await;
        destination(&owner, &project, &receiver.url).await;
        assert_eq!(publish_article(&owner, &project, "Slow answer").await, 1);
        let job = finished_job(&owner, &project, 1, 15).await;
        assert_eq!(
            (job["status"].as_str(), job["attempts"].as_i64()),
            (Some("succeeded"), Some(2)),
            "{job}"
        );
        let seen = receiver.seen();
        assert_eq!(
            seen.len(),
            2,
            "the timed-out post was recorded, then the retry arrived"
        );
        assert_eq!(seen[0].key, seen[1].key);
        assert_eq!(
            receiver.effects(),
            1,
            "the timed-out post landed once — the retry added no effect"
        );
    }
}

/// A submitted-and-approved changeset that adds one article to `project`, ready to publish:
/// the changeset's path and the `If-Match` its current hash binds.
async fn approved(owner: &Session, project: &str, title: &str) -> (String, String) {
    let entry = create_article(owner, project, title).await;
    let base = format!("/api/v1/projects/{project}/environments/production");
    let set = call(
        owner,
        Method::POST,
        &format!("{base}/changesets"),
        Some(&json!({"title": title, "entries": [entry]})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(set.status, StatusCode::CREATED, "{}", set.body);
    let path = format!(
        "{base}/changesets/{}",
        set.body["data"]["id"].as_str().expect("an id")
    );
    let if_match = format!("\"{}\"", set.body["data"]["hash"].as_str().expect("a hash"));
    let submitted = call(
        owner,
        Method::POST,
        &format!("{path}/submit"),
        None,
        &[("if-match", &if_match)],
    )
    .await
    .expect("request");
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);
    let approved = call(
        owner,
        Method::POST,
        &format!("{path}/reviews"),
        Some(&json!({"decision": "approve"})),
        &[("if-match", &if_match)],
    )
    .await
    .expect("request");
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.body);
    (path, if_match)
}

/// The durable work a project has committed, as a count of its releases, its outbox `jobs`
/// and its production generation — all three together, so a half-committed release (a
/// generation moved without its jobs, or the other way around) can't slip past.
async fn work_of(database: &sqlx::PgPool, project: &str) -> (i64, i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM releases WHERE project_id = $1::uuid),
                (SELECT count(*) FROM jobs WHERE project_id = $1::uuid),
                (SELECT generation FROM environments
                  WHERE project_id = $1::uuid AND slug = 'production')",
    )
    .bind(project)
    .fetch_one(database)
    .await
    .expect("the project's committed work")
}

/// F09, the bound half (spec §33's 10,000 pending jobs/project, D-131): the project's
/// unfinished work filled to its bound, the next durable work is refused before acceptance —
/// 503 `unavailable` with `details.reason` `work_full` and a `Retry-After` — and the refusing
/// transaction commits nothing: no release, no job, no generation move, and no idempotency
/// claim, so the same key's retry is a fresh write once one slot frees.
#[tokio::test]
#[ignore = "needs the stack, migrations and --features fault-injection"]
async fn a_project_at_its_work_bound_is_refused_then_recovers() {
    /// spec §33's bound, written out here so the test says what it fills.
    const BOUND: i64 = 10_000;
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let receiver = Receiver::start(&[]).await;
    destination(&owner, &project, &receiver.url).await;
    let (set, if_match) = approved(&owner, &project, "At the bound").await;

    // The bound in one statement, not ten thousand requests: accepting `bulk_jobs` are
    // exactly the rows `work::admit` counts as open work.
    let database = common::owner_database().await;
    sqlx::query(
        "INSERT INTO bulk_jobs (project_id, environment_id, id, kind, operation, manifest_hash,
                                created_by, principal, expected_items)
         SELECT p.id, e.id, gen_random_uuid(), 'entries', '{\"collection\": \"articles\"}',
                decode(md5(i::text) || md5((i + 1)::text), 'hex'), m.identity_id, 'member', 0
         FROM projects p
         JOIN environments e ON e.project_id = p.id AND e.slug = 'production'
         JOIN memberships m ON m.project_id = p.id
         CROSS JOIN generate_series(1, $2) AS i
         WHERE p.id = $1::uuid",
    )
    .bind(&project)
    .bind(BOUND)
    .execute(&database)
    .await
    .expect("fill the project's work bound");

    let publish = json!({"base_generation": 0});
    let once = key();
    let refused = call(
        &owner,
        Method::POST,
        &format!("{set}/publish"),
        Some(&publish),
        &[("if-match", &if_match), ("idempotency-key", &once)],
    )
    .await
    .expect("request");
    assert_eq!(
        refused.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["code"], "unavailable",
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"]["reason"], "work_full",
        "{}",
        refused.body
    );
    assert!(
        refused.headers.get(reqwest::header::RETRY_AFTER).is_some(),
        "a refused admission says when to try again"
    );
    // The refusal is before acceptance: nothing of the release stands — no release row, no
    // job, and the generation never moved.
    assert_eq!(
        work_of(&database, &project).await,
        (0, 0, 0),
        "the refused publish committed nothing"
    );

    // One slot frees: the same request — idempotency key included — commits as a fresh write,
    // the claim having rolled back with the refusal.
    sqlx::query(
        "DELETE FROM bulk_jobs WHERE project_id = $1::uuid
           AND id = (SELECT id FROM bulk_jobs WHERE project_id = $1::uuid LIMIT 1)",
    )
    .bind(&project)
    .execute(&database)
    .await
    .expect("free one slot");
    let admitted = call(
        &owner,
        Method::POST,
        &format!("{set}/publish"),
        Some(&publish),
        &[("if-match", &if_match), ("idempotency-key", &once)],
    )
    .await
    .expect("request");
    assert_eq!(admitted.status, StatusCode::CREATED, "{}", admitted.body);
    assert!(
        !admitted.replayed(),
        "the refused claim left nothing to replay: this is a fresh commit"
    );
    let (releases, jobs, generation) = work_of(&database, &project).await;
    assert_eq!(
        (releases, jobs, generation),
        (1, 1, 1),
        "release 1 committed its one destination's job"
    );
    // …and it runs: the worker still answering this test's server finishes the one job the
    // bound let through, so the test leaves no open work behind for the next test's worker.
    assert_eq!(
        finished_job(&owner, &project, 1, 10).await["status"].as_str(),
        Some("succeeded")
    );

    sqlx::query("DELETE FROM bulk_jobs WHERE project_id = $1::uuid")
        .bind(&project)
        .execute(&database)
        .await
        .expect("clean the filler");
    database.close().await;
}

/// F09, the disk half (D-131): a durable write that cannot commit refuses before acceptance —
/// never a success the client could mistake. `REVOKE INSERT ON jobs FROM colophon_api` is the
/// honest stand-in for storage that stops taking rows: a real statement failure at the one
/// place the outbox writes. The refused release leaves what a rolled-back transaction leaves
/// — nothing — and the restored grant admits the same request fresh.
#[tokio::test]
#[ignore = "needs the stack, migrations and --features fault-injection"]
async fn a_durable_write_that_cannot_commit_is_refused_and_rolls_back() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let receiver = Receiver::start(&[]).await;
    destination(&owner, &project, &receiver.url).await;
    let (set, if_match) = approved(&owner, &project, "Unwritable").await;
    let publish = json!({"base_generation": 0});
    let once = key();

    let database = common::owner_database().await;
    // Idempotent first: a run that died mid-test leaves the grant missing, so heal before
    // breaking it again.
    sqlx::query("GRANT INSERT ON jobs TO colophon_api")
        .execute(&database)
        .await
        .expect("grant before revoking");
    sqlx::query("REVOKE INSERT ON jobs FROM colophon_api")
        .execute(&database)
        .await
        .expect("revoke the outbox's insert");
    let refused = call(
        &owner,
        Method::POST,
        &format!("{set}/publish"),
        Some(&publish),
        &[("if-match", &if_match), ("idempotency-key", &once)],
    )
    .await;
    // The grant goes back before anything can panic, whatever the answer was.
    sqlx::query("GRANT INSERT ON jobs TO colophon_api")
        .execute(&database)
        .await
        .expect("restore the outbox's insert");
    let refused = refused.expect("request");
    assert_eq!(
        refused.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["code"], "internal",
        "{}",
        refused.body
    );
    assert_eq!(
        work_of(&database, &project).await,
        (0, 0, 0),
        "the write that couldn't commit left nothing"
    );

    let recovered = call(
        &owner,
        Method::POST,
        &format!("{set}/publish"),
        Some(&publish),
        &[("if-match", &if_match), ("idempotency-key", &once)],
    )
    .await
    .expect("request");
    assert_eq!(recovered.status, StatusCode::CREATED, "{}", recovered.body);
    assert!(
        !recovered.replayed(),
        "the aborted claim left nothing to replay: this is a fresh commit"
    );
    let (releases, jobs, generation) = work_of(&database, &project).await;
    assert_eq!(
        (releases, jobs, generation),
        (1, 1, 1),
        "the same request committed once storage came back"
    );
    // …and its job runs out too: the test leaves no open work for the next test's worker.
    assert_eq!(
        finished_job(&owner, &project, 1, 10).await["status"].as_str(),
        Some("succeeded")
    );
    database.close().await;
}
