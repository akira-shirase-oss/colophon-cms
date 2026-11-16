//! Review stages end to end (milestone E, step E2; D-104): an environment names an ordered list
//! of stages, every one needs an approval on the changeset's current hash before it publishes —
//! in any order — a request for changes at any stage blocks, a stage that asks for a different
//! reviewer never counts the submitter's own approval, and an edit makes every stage's approval
//! stale. An environment without the list behaves exactly as it always did. Needs the stack and
//! `scripts/migrate.sh` against `COLOPHON_TEST_DATABASE`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Reply, Server, Session, article, call, entries, join, key, project, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
}

fn changesets(project: &str) -> String {
    format!("{}/changesets", base(project))
}

fn stages(project: &str) -> String {
    format!("{}/review-stages", base(project))
}

async fn new_article(session: &Session, project: &str, title: &str) -> String {
    let reply = call(
        session,
        Method::POST,
        &entries(project),
        Some(&article(title, "first")),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    reply.body["data"]["id"].as_str().expect("an id").to_owned()
}

/// `POST {changeset}/{action}` with `If-Match: "<hash>"`.
async fn act(session: &Session, path: &str, hash: &str, body: Option<&Value>) -> Reply {
    call(
        session,
        Method::POST,
        path,
        body,
        &[("if-match", &format!("\"{hash}\""))],
    )
    .await
    .expect("request")
}

/// `PUT` the stage list, `If-Match` naming the `ETag` the caller saw.
async fn put_stages(session: &Session, project: &str, list: &Value, etag: &str) -> Reply {
    call(
        session,
        Method::PUT,
        &stages(project),
        Some(&json!({"stages": list})),
        &[("if-match", etag), ("idempotency-key", &key())],
    )
    .await
    .expect("request")
}

fn state(reply: &Reply) -> &str {
    reply.body["data"]["status"]["state"]
        .as_str()
        .unwrap_or("?")
}

fn hash(reply: &Reply) -> String {
    reply.body["data"]["hash"]
        .as_str()
        .expect("a hash")
        .to_owned()
}

/// How `reply`'s changeset says the stage `key` stands: `pending`, `approved`,
/// `changes_requested` or `approval_expired`.
fn stage_state(reply: &Reply, key: &str) -> String {
    reply.body["data"]["stages"]
        .as_array()
        .and_then(|stages| {
            stages
                .iter()
                .find(|stage| stage["key"] == key)
                .and_then(|stage| stage["state"].as_str())
        })
        .expect("a stage")
        .to_owned()
}

/// The environment's current release generation, for a publish's `base_generation`.
async fn generation(session: &Session, project: &str) -> i64 {
    let releases = call(
        session,
        Method::GET,
        &format!("{}/releases", base(project)),
        None,
        &[],
    )
    .await
    .expect("request");
    releases
        .headers
        .get("colophon-generation")
        .and_then(|value| value.to_str().ok()?.parse().ok())
        .expect("a generation")
}

/// `POST {changeset}/publish` on `hash`; the refusal, or the release.
async fn publish(session: &Session, path: &str, hash: &str, project: &str) -> Reply {
    let base = generation(session, project).await;
    call(
        session,
        Method::POST,
        &format!("{path}/publish"),
        Some(&json!({"base_generation": base})),
        &[
            ("if-match", &format!("\"{hash}\"")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request")
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: configure, review each stage, publish"
)]
async fn every_stage_needs_an_approval_on_the_current_hash() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let editor = sign_in("dev-editor").await;
    let project = project(&owner).await;
    join(&owner, &editor, &project, "publisher").await;

    // Nothing configured: the list is empty, and its ETag already names it.
    let empty = call(&owner, Method::GET, &stages(&project), None, &[])
        .await
        .expect("request");
    assert_eq!(empty.status, StatusCode::OK, "{}", empty.body);
    assert_eq!(empty.body["data"]["stages"], json!([]));
    let first_etag = empty.etag().expect("an ETag");

    // Configure two stages: Legal asks for a reviewer other than the submitter.
    let list = json!([
        {"key": "editorial", "name": "Editorial"},
        {"key": "legal", "name": "Legal", "different_reviewer": true}
    ]);
    let configured = put_stages(&owner, &project, &list, &first_etag).await;
    assert_eq!(configured.status, StatusCode::OK, "{}", configured.body);
    // The answer spells every field out, the defaults filled in.
    let full = json!([
        {"key": "editorial", "name": "Editorial", "different_reviewer": false},
        {"key": "legal", "name": "Legal", "different_reviewer": true}
    ]);
    assert_eq!(configured.body["data"]["stages"], full);
    let etag = configured.etag().expect("a new ETag");
    assert_ne!(etag, first_etag);

    // The old ETag is stale now; the refusal names the current one.
    let stale = put_stages(&owner, &project, &list, &first_etag).await;
    assert_eq!(stale.status, StatusCode::CONFLICT, "{}", stale.body);
    assert_eq!(stale.body["error"]["details"]["reason"], "stale_stages");
    assert_eq!(stale.etag(), Some(etag.clone()));
    // Someone else's change raced the same ETag and still reads back in order.
    let listed = call(&editor, Method::GET, &stages(&project), None, &[])
        .await
        .expect("request");
    assert_eq!(listed.body["data"]["stages"], full);
    assert_eq!(listed.etag(), Some(etag.clone()));

    // The editor pins an article and submits it; each review must name its stage now.
    let entry = new_article(&editor, &project, "First").await;
    let created = call(
        &editor,
        Method::POST,
        &changesets(&project),
        Some(&json!({"title": "Launch", "entries": [entry]})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = created.body["data"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let path = format!("{}/{id}", changesets(&project));
    let pinned = hash(&created);
    // The changeset echoes each configured stage, in order, still waiting.
    assert_eq!(stage_state(&created, "editorial"), "pending");
    assert_eq!(stage_state(&created, "legal"), "pending");
    let submitted = act(&editor, &format!("{path}/submit"), &pinned, None).await;
    assert_eq!(state(&submitted), "awaiting_review");
    let anonymous = act(
        &owner,
        &format!("{path}/reviews"),
        &pinned,
        Some(&json!({"decision": "approve"})),
    )
    .await;
    assert_eq!(
        anonymous.body["error"]["details"]["reason"],
        "stage_required"
    );
    let bogus = act(
        &owner,
        &format!("{path}/reviews"),
        &pinned,
        Some(&json!({"decision": "approve", "stage": "copydesk"})),
    )
    .await;
    assert_eq!(bogus.body["error"]["details"]["reason"], "unknown_stage");

    // The editor approves Editorial — their own submission, allowed here — and Legal stays
    // waiting, so publish refuses by name.
    let editorial = act(
        &editor,
        &format!("{path}/reviews"),
        &pinned,
        Some(&json!({"decision": "approve", "stage": "editorial"})),
    )
    .await;
    assert_eq!(state(&editorial), "awaiting_review", "{}", editorial.body);
    assert_eq!(stage_state(&editorial, "editorial"), "approved");
    assert_eq!(stage_state(&editorial, "legal"), "pending");
    let early = publish(&owner, &path, &pinned, &project).await;
    assert_eq!(early.status, StatusCode::CONFLICT, "{}", early.body);
    assert_eq!(early.body["error"]["details"]["reason"], "not_approved");

    // Legal asks for a different reviewer: the submitter's own approval is recorded — a review
    // is never lost — but counts for nothing.
    let own_legal = act(
        &editor,
        &format!("{path}/reviews"),
        &pinned,
        Some(&json!({"decision": "approve", "stage": "legal"})),
    )
    .await;
    assert_eq!(own_legal.status, StatusCode::OK, "{}", own_legal.body);
    assert_eq!(stage_state(&own_legal, "legal"), "pending");
    let still = publish(&owner, &path, &pinned, &project).await;
    assert_eq!(
        still.body["error"]["details"]["reason"], "not_approved",
        "the submitter's approval never counts for a different-reviewer stage"
    );

    // The owner decides Legal; both stages approve and the release commits.
    let approved = act(
        &owner,
        &format!("{path}/reviews"),
        &pinned,
        Some(&json!({"decision": "approve", "stage": "legal"})),
    )
    .await;
    assert_eq!(state(&approved), "approved", "{}", approved.body);
    assert_eq!(stage_state(&approved, "legal"), "approved");
    let published = publish(&owner, &path, &pinned, &project).await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);

    // A request for changes at any stage blocks the whole changeset.
    let second = new_article(&editor, &project, "Second").await;
    let next = call(
        &editor,
        Method::POST,
        &changesets(&project),
        Some(&json!({"title": "Follow-up", "entries": [second.clone()]})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    let next_path = format!(
        "{}/{}",
        changesets(&project),
        next.body["data"]["id"].as_str().expect("an id")
    );
    let next_hash = hash(&next);
    act(&editor, &format!("{next_path}/submit"), &next_hash, None).await;
    act(
        &editor,
        &format!("{next_path}/reviews"),
        &next_hash,
        Some(&json!({"decision": "approve", "stage": "editorial"})),
    )
    .await;
    let asked = act(
        &owner,
        &format!("{next_path}/reviews"),
        &next_hash,
        Some(&json!({"decision": "request_changes", "stage": "legal", "comment": "sign-off"})),
    )
    .await;
    assert_eq!(state(&asked), "changes_requested", "{}", asked.body);
    assert_eq!(stage_state(&asked, "legal"), "changes_requested");
    assert_eq!(stage_state(&asked, "editorial"), "approved");
    let blocked = publish(&owner, &next_path, &next_hash, &project).await;
    assert_eq!(blocked.body["error"]["details"]["reason"], "not_approved");

    // An edit after pinning makes the whole changeset stale; a refresh gives it a new hash and
    // every stage's approval — however recent — is history.
    let current = call(
        &editor,
        Method::GET,
        &format!("{}/{second}", entries(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    let saved = call(
        &editor,
        Method::PUT,
        &format!("{}/{second}", entries(&project)),
        Some(&article("Second, edited", "second")),
        &[
            ("if-match", &current.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request");
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    let now_stale = call(&owner, Method::GET, &next_path, None, &[])
        .await
        .expect("request");
    assert_eq!(state(&now_stale), "stale");
    let refreshed = act(&editor, &format!("{next_path}/refresh"), &next_hash, None).await;
    let repinned = hash(&refreshed);
    assert_ne!(repinned, next_hash);
    assert_eq!(state(&refreshed), "awaiting_review");
    assert_eq!(stage_state(&refreshed, "editorial"), "pending");
    assert_eq!(stage_state(&refreshed, "legal"), "pending");
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: refused writes, a guarded write, the cleared list"
)]
async fn stage_definitions_are_guarded() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let editor = sign_in("dev-editor").await;
    let project = project(&owner).await;

    // Nobody outside the project sees the list.
    let hidden = call(&editor, Method::GET, &stages(&project), None, &[])
        .await
        .expect("request");
    assert_eq!(hidden.status, StatusCode::NOT_FOUND);
    join(&owner, &editor, &project, "publisher").await;
    let shown = call(&owner, Method::GET, &stages(&project), None, &[])
        .await
        .expect("request");
    let etag = shown.etag().expect("an ETag");

    // The write asks for both preconditions.
    let unguarded = call(
        &owner,
        Method::PUT,
        &stages(&project),
        Some(&json!({"stages": []})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(unguarded.status, StatusCode::PRECONDITION_REQUIRED);
    let bad_key = put_stages(
        &owner,
        &project,
        &json!([{"key": "Legal Dept", "name": "Legal"}]),
        &etag,
    )
    .await;
    assert_eq!(
        bad_key.body["error"]["details"]["reason"],
        "invalid_stage_key"
    );
    let dup = put_stages(
        &owner,
        &project,
        &json!([{"key": "a", "name": "A"}, {"key": "a", "name": "Another"}]),
        &etag,
    )
    .await;
    assert_eq!(dup.body["error"]["details"]["reason"], "duplicate_stage");
    let no_name = put_stages(
        &owner,
        &project,
        &json!([{"key": "legal", "name": "  "}]),
        &etag,
    )
    .await;
    assert_eq!(
        no_name.body["error"]["details"]["reason"],
        "invalid_stage_name"
    );
    let too_many: Value = (0..=20)
        .map(|i| json!({"key": format!("s{i}"), "name": format!("Stage {i}")}))
        .collect::<Vec<_>>()
        .into();
    let many = put_stages(&owner, &project, &too_many, &etag).await;
    assert_eq!(many.body["error"]["details"]["reason"], "too_many_stages");

    // Changing the gates is for roles that shape the project: a publisher may not.
    let denied = put_stages(
        &editor,
        &project,
        &json!([{"key": "legal", "name": "Legal"}]),
        &etag,
    )
    .await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN, "{}", denied.body);
    assert_eq!(
        denied.body["error"]["details"]["permission"],
        "schema.change"
    );

    // A replace commits, replays under the same key, and the new list orders as given.
    let one = json!([{"key": "legal", "name": "Legal"}]);
    let put_key = key();
    let made = call(
        &owner,
        Method::PUT,
        &stages(&project),
        Some(&json!({"stages": one})),
        &[("if-match", &etag), ("idempotency-key", &put_key)],
    )
    .await
    .expect("request");
    assert_eq!(made.status, StatusCode::OK, "{}", made.body);
    let replayed = call(
        &owner,
        Method::PUT,
        &stages(&project),
        Some(&json!({"stages": one})),
        &[("if-match", &etag), ("idempotency-key", &put_key)],
    )
    .await
    .expect("request");
    assert!(replayed.replayed(), "the same request replays its answer");
    assert_eq!(
        replayed.body["data"]["stages"],
        json!([{"key": "legal", "name": "Legal", "different_reviewer": false}])
    );

    // Reordering keeps the keys; removing them all restores the single implicit review.
    let reordered = put_stages(
        &owner,
        &project,
        &json!([{"key": "legal", "name": "Legal & Compliance"}, {"key": "editorial", "name": "Editorial"}]),
        &made.etag().expect("the new ETag"),
    )
    .await;
    assert_eq!(reordered.status, StatusCode::OK, "{}", reordered.body);
    let cleared = put_stages(
        &owner,
        &project,
        &json!([]),
        &reordered.etag().expect("an ETag"),
    )
    .await;
    assert_eq!(cleared.status, StatusCode::OK, "{}", cleared.body);
    assert_eq!(cleared.body["data"]["stages"], json!([]));

    // And a changeset on the emptied list reviews the way it always did: one approval, no stage.
    let entry = new_article(&owner, &project, "Implicit").await;
    let created = call(
        &owner,
        Method::POST,
        &changesets(&project),
        Some(&json!({"title": "x", "entries": [entry]})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    let path = format!(
        "{}/{}",
        changesets(&project),
        created.body["data"]["id"].as_str().expect("an id")
    );
    let pinned = hash(&created);
    assert_eq!(created.body["data"]["stages"], json!([]));
    act(&owner, &format!("{path}/submit"), &pinned, None).await;
    let approved = act(
        &owner,
        &format!("{path}/reviews"),
        &pinned,
        Some(&json!({"decision": "approve"})),
    )
    .await;
    assert_eq!(state(&approved), "approved", "{}", approved.body);
    let published = publish(&owner, &path, &pinned, &project).await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);
}
