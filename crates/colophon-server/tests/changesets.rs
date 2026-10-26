//! Changesets and their review end to end (step 2.1, D-044), with two real people: an editor
//! pins two articles and submits them, the owner approves that exact hash, an edit makes the
//! changeset stale and the approval stops counting, a refresh gives it a new hash, and a reviewer
//! can't act on a hash they didn't see. Needs the stack and `scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Reply, Server, Session, article, call, entries, join, key, project, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

fn changesets(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production/changesets")
}

async fn new_article(session: &Session, project: &str, title: &str) -> (String, String) {
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
    (
        reply.body["data"]["id"].as_str().expect("an id").to_owned(),
        reply.etag().expect("an ETag"),
    )
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

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: pin, submit, approve, edit, refresh"
)]
async fn an_approval_binds_to_the_exact_hash_and_an_edit_makes_it_stale() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let editor = sign_in("dev-editor").await;
    let project = project(&owner).await;
    join(&owner, &editor, &project, "editor").await;
    let (first, _) = new_article(&editor, &project, "First").await;
    let (second, _) = new_article(&editor, &project, "Second").await;

    // Pin both; the same key again is the same changeset.
    let create_key = key();
    let body = json!({"title": "Launch posts", "entries": [second, first]});
    let created = call(
        &editor,
        Method::POST,
        &changesets(&project),
        Some(&body),
        &[("idempotency-key", &create_key)],
    )
    .await
    .expect("request");
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(state(&created), "open");
    let id = created.body["data"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let path = format!("{}/{id}", changesets(&project));
    let pinned = hash(&created);
    assert_eq!(created.etag(), Some(format!("\"{pinned}\"")));
    assert_eq!(
        created.body["data"]["items"].as_array().map(Vec::len),
        Some(2)
    );
    let again = call(
        &editor,
        Method::POST,
        &changesets(&project),
        Some(&body),
        &[("idempotency-key", &create_key)],
    )
    .await
    .expect("request");
    assert!(again.replayed());
    assert_eq!(again.body["data"]["id"], id.as_str());

    // Every action needs the hash you looked at.
    let missing = call(&editor, Method::POST, &format!("{path}/submit"), None, &[])
        .await
        .expect("request");
    assert_eq!(missing.status, StatusCode::PRECONDITION_REQUIRED);
    let wrong = act(
        &editor,
        &format!("{path}/submit"),
        &format!("sha256:{}", "0".repeat(64)),
        None,
    )
    .await;
    assert_eq!(wrong.status, StatusCode::CONFLICT);
    assert_eq!(wrong.body["error"]["details"]["reason"], "stale_changeset");
    assert_eq!(
        wrong.etag(),
        Some(format!("\"{pinned}\"")),
        "and says which hash is current"
    );

    // An editor submits; an editor can't review; the owner approves that exact hash.
    let early = act(
        &owner,
        &format!("{path}/reviews"),
        &pinned,
        Some(&json!({"decision": "approve"})),
    )
    .await;
    assert_eq!(
        early.status,
        StatusCode::CONFLICT,
        "not submitted yet: {}",
        early.body
    );
    let submitted = act(&editor, &format!("{path}/submit"), &pinned, None).await;
    assert_eq!(state(&submitted), "awaiting_review");
    let by_editor = act(
        &editor,
        &format!("{path}/reviews"),
        &pinned,
        Some(&json!({"decision": "approve"})),
    )
    .await;
    assert_eq!(by_editor.status, StatusCode::FORBIDDEN);
    let asked = act(
        &owner,
        &format!("{path}/reviews"),
        &pinned,
        Some(&json!({"decision": "request_changes", "comment": "Tighten the intro"})),
    )
    .await;
    assert_eq!(state(&asked), "changes_requested", "{}", asked.body);
    let approved = act(
        &owner,
        &format!("{path}/reviews"),
        &pinned,
        Some(&json!({"decision": "approve"})),
    )
    .await;
    assert_eq!(
        (
            state(&approved),
            approved.body["data"]["status"]["by"].as_str(),
            approved.body["data"]["status"]["self_review"].as_bool()
        ),
        ("approved", Some("Dev Owner"), Some(false))
    );

    // The editor edits one article: the approval stops counting, and nobody can review the old hash.
    let current = call(
        &editor,
        Method::GET,
        &format!("{}/{first}", entries(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    let saved = call(
        &editor,
        Method::PUT,
        &format!("{}/{first}", entries(&project)),
        Some(&article("First, edited", "second")),
        &[
            ("if-match", &current.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request");
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    let stale = call(&owner, Method::GET, &path, None, &[])
        .await
        .expect("request");
    assert_eq!(
        (
            state(&stale),
            stale.body["data"]["status"]["edited_items"].as_u64()
        ),
        ("stale", Some(1))
    );
    let on_stale = act(
        &owner,
        &format!("{path}/reviews"),
        &pinned,
        Some(&json!({"decision": "approve"})),
    )
    .await;
    assert_eq!(
        on_stale.status,
        StatusCode::CONFLICT,
        "refresh first: {}",
        on_stale.body
    );

    // A refresh re-pins the edit: a new hash, awaiting review again, the old decisions kept but not counted.
    let refreshed = act(&editor, &format!("{path}/refresh"), &pinned, None).await;
    let repinned = hash(&refreshed);
    assert_ne!(repinned, pinned);
    assert_eq!(state(&refreshed), "awaiting_review");
    let reviews = refreshed.body["data"]["reviews"]
        .as_array()
        .cloned()
        .expect("reviews");
    assert_eq!(reviews.len(), 2);
    assert!(reviews.iter().all(|review| review["current"] == false));
    let old_hash = act(
        &owner,
        &format!("{path}/reviews"),
        &pinned,
        Some(&json!({"decision": "approve"})),
    )
    .await;
    assert_eq!(
        old_hash.body["error"]["details"]["reason"],
        "stale_changeset"
    );
    let reapproved = act(
        &owner,
        &format!("{path}/reviews"),
        &repinned,
        Some(&json!({"decision": "approve"})),
    )
    .await;
    assert_eq!(state(&reapproved), "approved");
    let listed = call(&owner, Method::GET, &changesets(&project), None, &[])
        .await
        .expect("request");
    assert_eq!(listed.body["data"][0]["status"]["state"], "approved");
    assert_eq!(listed.body["data"][0]["items"], 2);

    // An owner reviewing their own changeset is allowed, and shown.
    let (third, _) = new_article(&owner, &project, "Third").await;
    let own = call(
        &owner,
        Method::POST,
        &changesets(&project),
        Some(&json!({"title": "Mine", "entries": [third]})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    let own_path = format!(
        "{}/{}",
        changesets(&project),
        own.body["data"]["id"].as_str().expect("an id")
    );
    let own_hash = hash(&own);
    act(&owner, &format!("{own_path}/submit"), &own_hash, None).await;
    let self_approved = act(
        &owner,
        &format!("{own_path}/reviews"),
        &own_hash,
        Some(&json!({"decision": "approve"})),
    )
    .await;
    assert_eq!(self_approved.body["data"]["status"]["self_review"], true);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn changesets_refuse_what_they_cannot_pin() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let create = |entries: Value| {
        let owner = &owner;
        let path = changesets(&project);
        async move {
            call(
                owner,
                Method::POST,
                &path,
                Some(&json!({"title": "x", "entries": entries})),
                &[("idempotency-key", &key())],
            )
            .await
            .expect("request")
        }
    };
    let unknown = create(json!([uuid::Uuid::now_v7().to_string()])).await;
    assert_eq!(unknown.body["error"]["details"]["reason"], "unknown_entry");
    let empty = create(json!([])).await;
    assert_eq!(empty.body["error"]["details"]["reason"], "invalid_entries");
    let (entry, _) = new_article(&owner, &project, "Once").await;
    let twice = create(json!([entry, entry])).await;
    assert_eq!(twice.body["error"]["details"]["reason"], "invalid_entries");

    // Another person's project: nothing to see.
    let stranger = sign_in("dev-editor").await;
    let hidden = call(&stranger, Method::GET, &changesets(&project), None, &[])
        .await
        .expect("request");
    assert_eq!(hidden.status, StatusCode::NOT_FOUND);
    // "changesets" names a Studio page, so no content type may take it.
    let reserved = call(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/environments/production/content-types"),
        Some(&json!({"slug": "changesets", "definition": {"name": "C", "title_field": "t", "fields": [{"id": "t", "label": "T", "type": "text", "required": true}]}})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(reserved.body["error"]["details"]["reason"], "reserved_slug");
}
