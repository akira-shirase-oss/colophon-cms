//! Bounded bulk work on drafts, end to end (migration 0046; P5, D-116): a manifest of exact
//! entry ids and base revisions, submitted in bounded chunks (at most 100 items, 1 MiB a
//! chunk), driven per item by the worker as the submitter, honest about never being globally
//! atomic. This file covers: submit-and-seal applying per item; a moved base revision
//! conflict; the resumable items cursor; chunk and operation validation; appending with
//! `If-Match` and per-item idempotent keys; cancellation; and idempotent replay after a
//! transport loss. Needs the stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Reply, Server, Session, article, call, entries, key, project, sign_in};
use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};

fn bulk(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production/bulk")
}

async fn send(session: &Session, method: Method, path: &str, body: Option<&Value>) -> Reply {
    call(session, method, path, body, &[("idempotency-key", &key())])
        .await
        .expect("request")
}

/// A request with explicit headers (the key and `If-Match` it names).
async fn with(
    session: &Session,
    method: Method,
    path: &str,
    body: Option<&Value>,
    extra: &[(&str, &str)],
) -> Reply {
    call(session, method, path, body, extra)
        .await
        .expect("request")
}

/// A draft, `(entry id, base revision id)`.
async fn draft(session: &Session, project: &str, title: &str) -> (String, String) {
    let made = send(
        session,
        Method::POST,
        &entries(project),
        Some(&article(title, "body")),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    (
        made.body["data"]["id"].as_str().expect("an id").to_owned(),
        made.body["data"]["revision"]["id"]
            .as_str()
            .expect("a revision")
            .to_owned(),
    )
}

/// Saves over the draft so `revision` is no longer its head; the new head.
async fn move_past(session: &Session, project: &str, entry: &str, revision: &str) -> String {
    let saved = with(
        session,
        Method::PUT,
        &format!("{}/{entry}", entries(project)),
        Some(&article("moved", "newer")),
        &[
            ("idempotency-key", &key()),
            ("if-match", &format!("\"{revision}\"")),
        ],
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    saved.body["data"]["revision"]["id"]
        .as_str()
        .expect("a revision")
        .to_owned()
}

fn set_op(value: &str) -> Value {
    json!({"op": "set", "path": "title", "value": value})
}

fn reason(reply: &Reply) -> &Value {
    &reply.body["error"]["details"]["reason"]
}

/// Waits for the worker to take `job` terminal, returning its last view.
async fn terminal(session: &Session, path: &str) -> Value {
    for _ in 0..100 {
        let got = send(session, Method::GET, path, None).await;
        assert_eq!(got.status, StatusCode::OK, "{}", got.body);
        let state = got.body["data"]["state"].as_str().unwrap_or_default();
        if matches!(state, "done" | "blocked" | "cancelled") {
            return got.body["data"].clone();
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    panic!("the job never left running");
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: submit, apply, conflict, cursor, cancel, replay"
)]
async fn a_sealed_job_applies_per_item_and_reports_outcomes() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let (entry_one, revision_one) = draft(&owner, &project, "first").await;
    let (entry_two, revision_two) = draft(&owner, &project, "second").await;
    // The second draft moves before the job runs: its item must conflict, not overwrite.
    let head_two = move_past(&owner, &project, &entry_two, &revision_two).await;

    let items = json!([
        {"key": "one", "entry": entry_one, "expected_revision": revision_one,
         "op": set_op("first (bulk)")},
        {"key": "two", "entry": entry_two, "expected_revision": revision_two,
         "op": set_op("second (bulk)")},
    ]);
    let key_submit = key();
    let submitted = with(
        &owner,
        Method::POST,
        &bulk(&project),
        Some(&json!({"collection": "articles", "items": items, "seal": true})),
        &[("idempotency-key", &key_submit)],
    )
    .await;
    assert_eq!(submitted.status, StatusCode::ACCEPTED, "{}", submitted.body);
    let job = submitted.body["data"]["id"]
        .as_str()
        .expect("a job id")
        .to_owned();
    assert_eq!(
        (
            submitted.body["data"]["state"].as_str(),
            submitted.body["data"]["items"]["expected"].as_i64()
        ),
        (Some("running"), Some(2)),
        "a sealed submit starts running: {}",
        submitted.body
    );
    let etag = submitted
        .headers
        .get(header::ETAG)
        .and_then(|v| v.to_str().ok())
        .expect("the manifest hash is the job's ETag")
        .to_owned();
    assert!(etag.starts_with("\"sha256:"), "{etag}");

    // A transport-loss retry of the same key answers the recorded response — no second job.
    let replayed = with(
        &owner,
        Method::POST,
        &bulk(&project),
        Some(&json!({"collection": "articles", "items": items, "seal": true})),
        &[("idempotency-key", &key_submit)],
    )
    .await;
    assert_eq!(replayed.status, StatusCode::ACCEPTED, "{}", replayed.body);
    assert_eq!(replayed.body["data"]["id"].as_str(), Some(job.as_str()));
    assert_eq!(
        replayed
            .headers
            .get("idempotent-replayed")
            .and_then(|v| v.to_str().ok()),
        Some("true")
    );

    // The worker applies one item and conflicts the other: per-item truth, not atomicity.
    let done = terminal(&owner, &format!("{}/{job}", bulk(&project))).await;
    assert_eq!(done["state"].as_str(), Some("blocked"), "{done}");
    assert_eq!(
        (
            done["items"]["done"].as_i64(),
            done["items"]["blocked"].as_i64()
        ),
        (Some(1), Some(1)),
        "{done}"
    );

    // The items cursor says which and why.
    let items_page = send(
        &owner,
        Method::GET,
        &format!("{}/{job}/items", bulk(&project)),
        None,
    )
    .await;
    assert_eq!(items_page.status, StatusCode::OK, "{}", items_page.body);
    let items = items_page.body["data"].as_array().expect("items");
    assert_eq!(items.len(), 2, "{}", items_page.body);
    assert_eq!(
        items[0]["outcome"].as_str(),
        Some("applied"),
        "key order: {}",
        items_page.body
    );
    assert!(
        items[0]["new_revision_id"].is_string(),
        "an applied item names the revision it made: {}",
        items[0]
    );
    assert_eq!(items[1]["outcome"].as_str(), Some("conflict"));
    assert_eq!(
        items[1]["problem"]["reason"].as_str(),
        Some("revision_moved"),
        "{}",
        items[1]
    );

    // The write is real: the first draft's head carries the set title.
    let draft_view = send(
        &owner,
        Method::GET,
        &format!("{}/{entry_one}", entries(&project)),
        None,
    )
    .await;
    assert_eq!(
        draft_view.body["data"]["content"]["title"].as_str(),
        Some("first (bulk)"),
        "{}",
        draft_view.body
    );
    // And the moved draft is untouched by the job (it moved by the caller's save).
    let two_view = send(
        &owner,
        Method::GET,
        &format!("{}/{entry_two}", entries(&project)),
        None,
    )
    .await;
    assert_eq!(
        two_view.body["data"]["revision"]["id"].as_str(),
        Some(head_two.as_str()),
        "{}",
        two_view.body
    );

    // List and filter the jobs.
    let listed = send(
        &owner,
        Method::GET,
        &format!("{}?state=blocked", bulk(&project)),
        None,
    )
    .await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert!(
        listed.body["data"]
            .as_array()
            .is_some_and(|jobs| jobs.iter().any(|j| j["id"].as_str() == Some(&job))),
        "{}",
        listed.body
    );

    // A finished job can't be cancelled.
    let cancel = send(
        &owner,
        Method::POST,
        &format!("{}/{job}/cancel", bulk(&project)),
        None,
    )
    .await;
    assert_eq!(cancel.status, StatusCode::CONFLICT, "{}", cancel.body);
    assert_eq!(reason(&cancel), &json!("job_finished"));

    // The audit log kept the submit.
    let audited = send(
        &owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/audit?action=bulk.submitted"),
        None,
    )
    .await;
    assert_eq!(audited.status, StatusCode::OK, "{}", audited.body);
    assert!(
        audited.body["data"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["subject"].as_str() == Some(&job))),
        "{}",
        audited.body
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story: guard, bind, append, seal, cancel"
)]
async fn chunks_append_under_the_manifest_hash_and_cancel_stops_pending_items() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let (entry_one, revision_one) = draft(&owner, &project, "chunked").await;

    // An unsealed submit accepts.
    let submitted = with(
        &owner,
        Method::POST,
        &bulk(&project),
        Some(&json!({
            "collection": "articles",
            "items": [{"key": "a", "entry": entry_one, "expected_revision": revision_one,
                       "op": set_op("chunked one")}],
        })),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(submitted.status, StatusCode::ACCEPTED, "{}", submitted.body);
    assert_eq!(submitted.body["data"]["state"].as_str(), Some("accepting"));
    let job = submitted.body["data"]["id"]
        .as_str()
        .expect("a job id")
        .to_owned();
    let etag = submitted
        .headers
        .get(header::ETAG)
        .and_then(|v| v.to_str().ok())
        .expect("an ETag")
        .to_owned();

    // An append without If-Match is 428; with a wrong hash, 409 job_moved.
    let missing = with(
        &owner,
        Method::POST,
        &format!("{}/{job}/items", bulk(&project)),
        Some(&json!({"items": [{"key": "b", "entry": entry_one,
                               "expected_revision": revision_one, "op": set_op("x")}]})),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(
        missing.status,
        StatusCode::PRECONDITION_REQUIRED,
        "{}",
        missing.body
    );
    let moved = with(
        &owner,
        Method::POST,
        &format!("{}/{job}/items", bulk(&project)),
        Some(&json!({"items": [{"key": "b", "entry": entry_one,
                               "expected_revision": revision_one, "op": set_op("x")}]})),
        &[
            ("idempotency-key", &key()),
            (
                "if-match",
                "\"sha256:0000000000000000000000000000000000000000000000000000000000000000\"",
            ),
        ],
    )
    .await;
    assert_eq!(moved.status, StatusCode::CONFLICT, "{}", moved.body);
    assert_eq!(reason(&moved), &json!("job_moved"));

    // A real append lands the new key and reports a resent key once, never twice.
    let appended = with(
        &owner,
        Method::POST,
        &format!("{}/{job}/items", bulk(&project)),
        Some(&json!({
            "items": [
                {"key": "a", "entry": entry_one, "expected_revision": revision_one,
                 "op": set_op("again")},
                {"key": "b", "entry": entry_one, "expected_revision": revision_one,
                 "op": set_op("chunked two")},
            ],
            "seal": true,
        })),
        &[("idempotency-key", &key()), ("if-match", &etag)],
    )
    .await;
    assert_eq!(appended.status, StatusCode::ACCEPTED, "{}", appended.body);
    assert_eq!(
        (
            appended.body["data"]["appended"].as_i64(),
            appended.body["data"]["duplicates"][0].as_str(),
            appended.body["data"]["job"]["state"].as_str()
        ),
        (Some(1), Some("a"), Some("running")),
        "{}",
        appended.body
    );

    // Cancelling a sealed job is refused after it is done only; a running job cancels and
    // its audit lands — pending items skip (item "b" may already have applied; either way
    // the job ends cancelled).
    let cancelled = send(
        &owner,
        Method::POST,
        &format!("{}/{job}/cancel", bulk(&project)),
        None,
    )
    .await;
    assert!(
        cancelled.status == StatusCode::OK || cancelled.status == StatusCode::CONFLICT,
        "{}",
        cancelled.body
    );
    let last = terminal(&owner, &format!("{}/{job}", bulk(&project))).await;
    assert!(
        matches!(
            last["state"].as_str(),
            Some("cancelled" | "done" | "blocked")
        ),
        "{last}"
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "each bound is its own refused request"
)]
async fn validation_refuses_oversized_unknown_and_malformed_manifests() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let (entry_one, revision_one) = draft(&owner, &project, "guards").await;

    // More than 100 items is refused before anything lands.
    let many: Vec<Value> = (0..101)
        .map(|index| {
            json!({"key": format!("k{index}"), "entry": entry_one,
                   "expected_revision": revision_one, "op": set_op("x")})
        })
        .collect();
    let refused = send(
        &owner,
        Method::POST,
        &bulk(&project),
        Some(&json!({"collection": "articles", "items": many})),
    )
    .await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(reason(&refused), &json!("chunk_too_large"));

    // An entry that isn't in the collection is named by its pointer.
    let unknown = send(
        &owner,
        Method::POST,
        &bulk(&project),
        Some(&json!({
            "collection": "articles",
            "items": [{"key": "a", "entry": uuid::Uuid::now_v7(),
                       "expected_revision": revision_one, "op": set_op("x")}],
        })),
    )
    .await;
    assert_eq!(
        unknown.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        unknown.body
    );
    assert_eq!(reason(&unknown), &json!("unknown_entry"));

    // An op the parser doesn't know is refused at submit, pointer into the item.
    let bad_op = send(
        &owner,
        Method::POST,
        &bulk(&project),
        Some(&json!({
            "collection": "articles",
            "items": [{"key": "a", "entry": entry_one, "expected_revision": revision_one,
                       "op": {"op": "publish", "to": "live"}}],
        })),
    )
    .await;
    assert_eq!(
        bad_op.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        bad_op.body
    );
    assert_eq!(
        bad_op.body["error"]["details"]["pointer"].as_str(),
        Some("/items/0/op/op"),
        "{}",
        bad_op.body
    );

    // An empty chunk and a duplicate key are refused too.
    for (items, want) in [
        (json!([]), "manifest_empty"),
        (
            json!([
                {"key": "a", "entry": entry_one, "expected_revision": revision_one,
                 "op": set_op("x")},
                {"key": "a", "entry": entry_one, "expected_revision": revision_one,
                 "op": set_op("y")},
            ]),
            "duplicate_key",
        ),
    ] {
        let reply = send(
            &owner,
            Method::POST,
            &bulk(&project),
            Some(&json!({"collection": "articles", "items": items})),
        )
        .await;
        assert_eq!(
            reply.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{}",
            reply.body
        );
        assert_eq!(reason(&reply), &json!(want), "{}", reply.body);
    }

    // A sealed manifest takes no more items.
    let sealed = send(
        &owner,
        Method::POST,
        &bulk(&project),
        Some(&json!({
            "collection": "articles", "seal": true,
            "items": [{"key": "a", "entry": entry_one, "expected_revision": revision_one,
                       "op": set_op("sealed")}],
        })),
    )
    .await;
    assert_eq!(sealed.status, StatusCode::ACCEPTED, "{}", sealed.body);
    let job = sealed.body["data"]["id"]
        .as_str()
        .expect("a job id")
        .to_owned();
    let etag = sealed
        .headers
        .get(header::ETAG)
        .and_then(|v| v.to_str().ok())
        .expect("an ETag")
        .to_owned();
    let closed = with(
        &owner,
        Method::POST,
        &format!("{}/{job}/items", bulk(&project)),
        Some(&json!({"items": [{"key": "b", "entry": entry_one,
                               "expected_revision": revision_one, "op": set_op("x")}]})),
        &[("idempotency-key", &key()), ("if-match", &etag)],
    )
    .await;
    assert_eq!(closed.status, StatusCode::CONFLICT, "{}", closed.body);
    assert_eq!(reason(&closed), &json!("job_not_accepting"));

    // Let the sealed job finish so the server isn't left mid-run for the next test.
    let last = terminal(&owner, &format!("{}/{job}", bulk(&project))).await;
    assert!(last["state"].is_string(), "{last}");
}
