//! Comments and editorial tasks, end to end (milestone E, step E1; D-102), on the real binary
//! and stack: a comment binds one immutable revision and survives the entry moving on and being
//! deleted; a task carries its lifecycle; both answer the same `content.read`/`content.update`
//! permissions entries answer, with the same `own` condition, and every write is audited,
//! idempotent and preconditioned. Needs the stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Reply, Server, Session, article, call, entries, join, key, project, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

fn comments(project: &str, entry: &str) -> String {
    format!("{}/{entry}/comments", entries(project))
}

fn tasks(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production/tasks")
}

async fn send(
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

/// An article and its first draft revision.
async fn new_article(session: &Session, project: &str, title: &str) -> (String, String) {
    let reply = send(
        session,
        Method::POST,
        &entries(project),
        Some(&article(title, "first")),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    (
        reply.body["data"]["id"].as_str().expect("an id").to_owned(),
        reply.body["data"]["revision"]["id"]
            .as_str()
            .expect("a revision")
            .to_owned(),
    )
}

/// The entry's current draft revision after a save.
async fn save_article(
    session: &Session,
    project: &str,
    entry: &str,
    etag: &str,
    text: &str,
) -> String {
    let reply = send(
        session,
        Method::PUT,
        &format!("{}/{entry}", entries(project)),
        Some(&article("Title", text)),
        &[
            ("if-match", &format!("\"{etag}\"")),
            ("idempotency-key", &key()),
        ],
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    reply.body["data"]["revision"]["id"]
        .as_str()
        .expect("a revision")
        .to_owned()
}

async fn comment_on(
    session: &Session,
    project: &str,
    entry: &str,
    revision: &str,
    body: &str,
    field: Option<&str>,
) -> Reply {
    send(
        session,
        Method::POST,
        &comments(project, entry),
        Some(&json!({"revision": revision, "field": field, "body": body})),
        &[("idempotency-key", &key())],
    )
    .await
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && COLOPHON_TEST_DATABASE=colophon_e1 scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: write, list, stay on the revision, resolve, reopen, outlive the entry"
)]
async fn a_comment_binds_its_revision_and_outlives_the_entry() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let (entry, first) = new_article(&owner, &project, "Launch note").await;

    // Writes need both preconditions.
    let no_key = send(
        &owner,
        Method::POST,
        &comments(&project, &entry),
        Some(&json!({"revision": first, "body": "hi"})),
        &[],
    )
    .await;
    assert_eq!(no_key.status, StatusCode::PRECONDITION_REQUIRED);

    // The body is kept verbatim, markup and all: rendering escapes it, storage never rewrites.
    let markup = "<script>alert(1)</script> needs a <b>look</b>";
    let create_key = key();
    let made = send(
        &owner,
        Method::POST,
        &comments(&project, &entry),
        Some(&json!({"revision": first, "field": "body", "body": markup})),
        &[("idempotency-key", &create_key)],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let comment = made.body["data"]["id"].as_str().expect("an id").to_owned();
    assert_eq!(made.body["data"]["revision_id"], first);
    assert_eq!(made.body["data"]["field"], "body");
    assert_eq!(made.body["data"]["body"], markup);
    assert!(made.body["data"]["resolved_at"].is_null());
    let etag = made.etag().expect("an etag");
    assert!(
        etag.starts_with("\"sha256:"),
        "the comment's ETag is a state hash: {etag}"
    );

    // The same request under the same key replays the recorded answer; a different one is
    // refused.
    let replay = send(
        &owner,
        Method::POST,
        &comments(&project, &entry),
        Some(&json!({"revision": first, "field": "body", "body": markup})),
        &[("idempotency-key", &create_key)],
    )
    .await;
    assert_eq!(replay.status, StatusCode::CREATED, "{}", replay.body);
    assert!(replay.replayed(), "the recorded answer replays");
    let again = send(
        &owner,
        Method::POST,
        &comments(&project, &entry),
        Some(&json!({"revision": first, "body": "different"})),
        &[("idempotency-key", &create_key)],
    )
    .await;
    assert_eq!(again.status, StatusCode::CONFLICT, "{}", again.body);
    assert_eq!(
        again.body["error"]["details"]["reason"],
        "idempotency_key_reused"
    );

    // A comment on a revision the entry doesn't have is refused at the field.
    let bogus = comment_on(
        &owner,
        &project,
        &entry,
        "00000000-0000-4000-8000-000000000000",
        "x",
        None,
    )
    .await;
    assert_eq!(bogus.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        bogus.body["error"]["details"]["reason"],
        "revision_not_found"
    );
    // And a malformed field anchor.
    let bad_field = comment_on(&owner, &project, &entry, &first, "x", Some("Body/!")).await;
    assert_eq!(bad_field.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        bad_field.body["error"]["details"]["reason"],
        "invalid_field_path"
    );

    // The draft moves on: the comment stays on the revision it was written about.
    let second = save_article(&owner, &project, &entry, &first, "second").await;
    let whole = send(&owner, Method::GET, &comments(&project, &entry), None, &[]).await;
    assert_eq!(whole.status, StatusCode::OK, "{}", whole.body);
    assert_eq!(whole.body["data"].as_array().map(Vec::len), Some(1));
    let on_first = send(
        &owner,
        Method::GET,
        &format!("{}?revision={first}", comments(&project, &entry)),
        None,
        &[],
    )
    .await;
    assert_eq!(on_first.body["data"].as_array().map(Vec::len), Some(1));
    let on_second = send(
        &owner,
        Method::GET,
        &format!("{}?revision={second}", comments(&project, &entry)),
        None,
        &[],
    )
    .await;
    assert_eq!(on_second.body["data"].as_array().map(Vec::len), Some(0));

    // Resolve needs the state that was looked at.
    let no_match = send(
        &owner,
        Method::POST,
        &format!("{}/{comment}/resolve", comments(&project, &entry)),
        None,
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(no_match.status, StatusCode::PRECONDITION_REQUIRED);
    let stale = send(
        &owner,
        Method::POST,
        &format!("{}/{comment}/resolve", comments(&project, &entry)),
        None,
        &[
            (
                "if-match",
                "\"sha256:0000000000000000000000000000000000000000000000000000000000000000\"",
            ),
            ("idempotency-key", &key()),
        ],
    )
    .await;
    assert_eq!(stale.status, StatusCode::CONFLICT, "{}", stale.body);
    assert_eq!(stale.body["error"]["details"]["reason"], "comment_moved");

    let resolved = send(
        &owner,
        Method::POST,
        &format!("{}/{comment}/resolve", comments(&project, &entry)),
        None,
        &[("if-match", &etag), ("idempotency-key", &key())],
    )
    .await;
    assert_eq!(resolved.status, StatusCode::OK, "{}", resolved.body);
    let owner_id = owner_identity(&owner, &project).await;
    assert_eq!(resolved.body["data"]["resolved_by"], owner_id);
    assert!(resolved.body["data"]["resolved_at"].is_string());
    let resolved_etag = resolved.etag().expect("a new etag");
    assert_ne!(resolved_etag, etag, "resolution changes the state hash");

    // Resolving the resolved says so, and the open list shrinks.
    let already = send(
        &owner,
        Method::POST,
        &format!("{}/{comment}/resolve", comments(&project, &entry)),
        None,
        &[("if-match", &resolved_etag), ("idempotency-key", &key())],
    )
    .await;
    assert_eq!(already.status, StatusCode::CONFLICT, "{}", already.body);
    assert_eq!(
        already.body["error"]["details"]["reason"],
        "already_resolved"
    );
    let open = send(
        &owner,
        Method::GET,
        &format!("{}?state=open", comments(&project, &entry)),
        None,
        &[],
    )
    .await;
    assert_eq!(open.body["data"].as_array().map(Vec::len), Some(0));
    let done = send(
        &owner,
        Method::GET,
        &format!("{}?state=resolved", comments(&project, &entry)),
        None,
        &[],
    )
    .await;
    assert_eq!(done.body["data"].as_array().map(Vec::len), Some(1));

    // Reopen clears the pair; resolving again is back on the table.
    let reopened = send(
        &owner,
        Method::POST,
        &format!("{}/{comment}/reopen", comments(&project, &entry)),
        None,
        &[("if-match", &resolved_etag), ("idempotency-key", &key())],
    )
    .await;
    assert_eq!(reopened.status, StatusCode::OK, "{}", reopened.body);
    assert!(reopened.body["data"]["resolved_at"].is_null());

    // The write was recorded.
    let audit = send(
        &owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/audit?action=comment.resolved"),
        None,
        &[],
    )
    .await;
    assert_eq!(audit.status, StatusCode::OK, "{}", audit.body);
    let events = audit.body["data"].as_array().expect("events");
    assert_eq!(events.len(), 1, "{}", audit.body);
    assert_eq!(events[0]["subject"], comment);

    // Deleting the entry can't take its comments: they stay in history, still readable and
    // still resolvable — and nothing new may be written on it.
    let draft = send(
        &owner,
        Method::GET,
        &format!("{}/{entry}", entries(&project)),
        None,
        &[],
    )
    .await;
    let deleted = send(
        &owner,
        Method::DELETE,
        &format!("{}/{entry}", entries(&project)),
        None,
        &[
            ("if-match", &draft.etag().expect("an etag")),
            ("idempotency-key", &key()),
        ],
    )
    .await;
    assert_eq!(deleted.status, StatusCode::OK, "{}", deleted.body);
    let history = send(&owner, Method::GET, &comments(&project, &entry), None, &[]).await;
    assert_eq!(history.status, StatusCode::OK, "{}", history.body);
    assert_eq!(history.body["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(history.body["data"][0]["body"], markup);
    let gone = comment_on(&owner, &project, &entry, &first, "too late", None).await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND, "{}", gone.body);
}

/// The member's identity id.
async fn owner_identity(session: &Session, project: &str) -> String {
    let members = send(
        session,
        Method::GET,
        &format!("/api/v1/projects/{project}/members"),
        None,
        &[],
    )
    .await;
    members.body["data"][0]["identity_id"]
        .as_str()
        .expect("an id")
        .to_owned()
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && COLOPHON_TEST_DATABASE=colophon_e1 scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: create, assign, finish, reopen, filter"
)]
async fn a_task_carries_its_lifecycle() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let editor = sign_in("dev-editor").await;
    let project = project(&owner).await;
    join(&owner, &editor, &project, "editor").await;
    let (entry, revision) = new_article(&owner, &project, "Launch note").await;
    let owner_id = owner_identity(&owner, &project).await;
    let editor_id = {
        let members = send(
            &owner,
            Method::GET,
            &format!("/api/v1/projects/{project}/members"),
            None,
            &[],
        )
        .await;
        members.body["data"]
            .as_array()
            .expect("members")
            .iter()
            .map(|member| member["identity_id"].as_str().expect("an id"))
            .find(|id| *id != owner_id)
            .expect("the editor")
            .to_owned()
    };

    // The entry it names must exist; so must a revision, a changeset and the assignee's
    // membership when given.
    for (body, reason) in [
        (
            json!({"entry": "00000000-0000-4000-8000-000000000000", "title": "x"}),
            "not_found",
        ),
        (
            json!({"entry": entry, "title": "x", "revision": "00000000-0000-4000-8000-000000000000"}),
            "revision_not_found",
        ),
        (
            json!({"entry": entry, "title": "x", "changeset": "00000000-0000-4000-8000-000000000000"}),
            "changeset_not_found",
        ),
        (
            json!({"entry": entry, "title": "x", "assignee": "00000000-0000-4000-8000-000000000000"}),
            "assignee_not_member",
        ),
        (json!({"entry": entry, "title": "  "}), "empty_title"),
    ] {
        let refused = send(
            &owner,
            Method::POST,
            &tasks(&project),
            Some(&body),
            &[("idempotency-key", &key())],
        )
        .await;
        let expected = match reason {
            "not_found" => StatusCode::NOT_FOUND,
            _ => StatusCode::UNPROCESSABLE_ENTITY,
        };
        assert_eq!(refused.status, expected, "{reason}: {}", refused.body);
        if expected == StatusCode::UNPROCESSABLE_ENTITY {
            assert_eq!(refused.body["error"]["details"]["reason"], reason);
        }
    }

    let made = send(
        &owner,
        Method::POST,
        &tasks(&project),
        Some(&json!({
            "entry": entry,
            "title": "Check the embargo date",
            "revision": revision,
            "assignee": editor_id,
        })),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let task = made.body["data"]["id"].as_str().expect("an id").to_owned();
    assert_eq!(made.body["data"]["status"], "open");
    assert_eq!(made.body["data"]["assignee_id"], editor_id);
    assert_eq!(made.body["data"]["collection"], "articles");
    let etag = made.etag().expect("an etag");

    // The filters find it where it belongs and nowhere else.
    let count = |reply: Reply| reply.body["data"].as_array().map(Vec::len);
    let list = |query: &str| format!("{}{query}", tasks(&project));
    assert_eq!(
        count(send(&owner, Method::GET, &list(""), None, &[]).await),
        Some(1)
    );
    assert_eq!(
        count(
            send(
                &owner,
                Method::GET,
                &list(&format!("?entry={entry}")),
                None,
                &[]
            )
            .await
        ),
        Some(1)
    );
    assert_eq!(
        count(
            send(
                &owner,
                Method::GET,
                &list(&format!("?assignee={editor_id}")),
                None,
                &[]
            )
            .await
        ),
        Some(1)
    );
    assert_eq!(
        count(send(&owner, Method::GET, &list("?assignee=none"), None, &[]).await),
        Some(0)
    );
    assert_eq!(
        count(send(&owner, Method::GET, &list("?status=done"), None, &[]).await),
        Some(0)
    );

    // The assignee finishes it; done_by and done_at record who and when, as a pair.
    let path = format!("{}/{task}", tasks(&project));
    let current = send(&editor, Method::GET, &path, None, &[]).await;
    assert_eq!(current.status, StatusCode::OK, "{}", current.body);
    let finished = send(
        &editor,
        Method::PATCH,
        &path,
        Some(&json!({"status": "done"})),
        &[
            ("if-match", &current.etag().expect("an etag")),
            ("idempotency-key", &key()),
        ],
    )
    .await;
    assert_eq!(finished.status, StatusCode::OK, "{}", finished.body);
    assert_eq!(finished.body["data"]["status"], "done");
    assert_eq!(finished.body["data"]["done_by"], editor_id);
    assert!(finished.body["data"]["done_at"].is_string());
    let done_etag = finished.etag().expect("a new etag");
    assert_ne!(done_etag, etag);

    // A change against what it no longer is loses.
    let moved = send(
        &owner,
        Method::PATCH,
        &path,
        Some(&json!({"status": "cancelled"})),
        &[("if-match", &etag), ("idempotency-key", &key())],
    )
    .await;
    assert_eq!(moved.status, StatusCode::CONFLICT, "{}", moved.body);
    assert_eq!(moved.body["error"]["details"]["reason"], "task_moved");

    // Reopening clears the pair; a retitle and unassign ride along.
    let reopened = send(
        &owner,
        Method::PATCH,
        &path,
        Some(
            &json!({"status": "open", "title": "Check the embargo date, again", "assignee": null}),
        ),
        &[("if-match", &done_etag), ("idempotency-key", &key())],
    )
    .await;
    assert_eq!(reopened.status, StatusCode::OK, "{}", reopened.body);
    assert_eq!(reopened.body["data"]["status"], "open");
    assert_eq!(
        reopened.body["data"]["title"],
        "Check the embargo date, again"
    );
    assert!(reopened.body["data"]["assignee_id"].is_null());
    assert!(reopened.body["data"]["done_by"].is_null());
    assert!(reopened.body["data"]["done_at"].is_null());

    // Nothing to change is refused rather than written.
    let nothing = send(
        &owner,
        Method::PATCH,
        &path,
        Some(&json!({})),
        &[
            ("if-match", &reopened.etag().expect("an etag")),
            ("idempotency-key", &key()),
        ],
    )
    .await;
    assert_eq!(nothing.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        nothing.body["error"]["details"]["reason"],
        "nothing_to_change"
    );
    let bad_status = send(
        &owner,
        Method::PATCH,
        &path,
        Some(&json!({"status": "sorted"})),
        &[
            ("if-match", &reopened.etag().expect("an etag")),
            ("idempotency-key", &key()),
        ],
    )
    .await;
    assert_eq!(bad_status.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        bad_status.body["error"]["details"]["reason"],
        "invalid_status"
    );

    // Both writes were recorded.
    let audit = send(
        &owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/audit"),
        None,
        &[],
    )
    .await;
    let actions: Vec<&str> = audit.body["data"]
        .as_array()
        .expect("events")
        .iter()
        .map(|event| event["action"].as_str().expect("an action"))
        .collect();
    assert!(actions.contains(&"task.created"), "{actions:?}");
    assert!(actions.contains(&"task.updated"), "{actions:?}");
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && COLOPHON_TEST_DATABASE=colophon_e1 scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: readers, writers, outsiders and the own condition"
)]
async fn comments_and_tasks_follow_content_permissions() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let reader = sign_in("dev-editor").await;
    let project = project(&owner).await;

    // A role that reads articles and nothing more.
    let made = send(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/roles"),
        Some(&json!({
            "key": "reader",
            "name": "Reader",
            "permissions": [{"action": "content.read", "subject": "articles"}]
        })),
        &[],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    join(&owner, &reader, &project, "reader").await;

    let (entry, revision) = new_article(&owner, &project, "Shared note").await;
    let written = comment_on(&owner, &project, &entry, &revision, "Looks right", None).await;
    assert_eq!(written.status, StatusCode::CREATED);
    let comment = written.body["data"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let task_made = send(
        &owner,
        Method::POST,
        &tasks(&project),
        Some(&json!({"entry": entry, "title": "Copyedit"})),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(task_made.status, StatusCode::CREATED);
    let task = task_made.body["data"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();

    // The reader sees both and may change neither, with the permission named.
    for (method, path) in [
        (Method::GET, comments(&project, &entry)),
        (Method::GET, tasks(&project)),
        (Method::GET, format!("{}/{task}", tasks(&project))),
    ] {
        let seen = send(&reader, method, &path, None, &[]).await;
        assert_eq!(seen.status, StatusCode::OK, "{}", seen.body);
    }
    let no_write = comment_on(&reader, &project, &entry, &revision, "noted", None).await;
    assert_eq!(no_write.status, StatusCode::FORBIDDEN, "{}", no_write.body);
    assert_eq!(
        no_write.body["error"]["details"]["reason"],
        "missing_permission"
    );
    assert_eq!(
        no_write.body["error"]["details"]["permission"],
        "content.update"
    );
    let no_resolve = send(
        &reader,
        Method::POST,
        &format!("{}/{comment}/resolve", comments(&project, &entry)),
        None,
        &[
            ("if-match", &written.etag().expect("an etag")),
            ("idempotency-key", &key()),
        ],
    )
    .await;
    assert_eq!(no_resolve.status, StatusCode::FORBIDDEN);
    let no_task = send(
        &reader,
        Method::POST,
        &tasks(&project),
        Some(&json!({"entry": entry, "title": "try"})),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(no_task.status, StatusCode::FORBIDDEN);
    let no_patch = send(
        &reader,
        Method::PATCH,
        &format!("{}/{task}", tasks(&project)),
        Some(&json!({"status": "done"})),
        &[
            ("if-match", &task_made.etag().expect("an etag")),
            ("idempotency-key", &key()),
        ],
    )
    .await;
    assert_eq!(no_patch.status, StatusCode::FORBIDDEN);

    // Nobody signed in answers 401, and a project the caller isn't in answers 404.
    let (status, _) = common::get(None, &comments(&project, &entry)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let elsewhere = send(
        &reader,
        Method::GET,
        &format!(
            "/api/v1/projects/{}/environments/production/collections/articles/entries/{entry}/comments",
            "00000000-0000-4000-8000-000000000000"
        ),
        None,
        &[],
    )
    .await;
    assert_eq!(elsewhere.status, StatusCode::NOT_FOUND);

    // The `own` condition holds on comments and tasks the way it holds on entries: their own
    // entries' only.
    let made = send(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/roles"),
        Some(&json!({
            "key": "author",
            "name": "Author",
            "permissions": [
                {"action": "content.read", "subject": "articles", "condition": "own"},
                {"action": "content.create", "subject": "articles"},
                {"action": "content.update", "subject": "articles", "condition": "own"}
            ]
        })),
        &[],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let author = sign_in("dev-editor").await;
    // Move the reader to the author role: same person, new policy.
    let owner_id = owner_identity(&owner, &project).await;
    let members = send(
        &owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/members"),
        None,
        &[],
    )
    .await;
    let author_id = members.body["data"]
        .as_array()
        .expect("members")
        .iter()
        .map(|member| member["identity_id"].as_str().expect("an id"))
        .find(|id| *id != owner_id)
        .expect("the member")
        .to_owned();
    let moved = send(
        &owner,
        Method::PUT,
        &format!("/api/v1/projects/{project}/members/{author_id}"),
        Some(&json!({"role": "author"})),
        &[],
    )
    .await;
    assert_eq!(moved.status, StatusCode::NO_CONTENT, "{}", moved.body);

    let (mine, mine_rev) = new_article(&author, &project, "My own note").await;
    // Their own entry's comments: in reach. The owner's entry's: not found.
    let own_list = send(&author, Method::GET, &comments(&project, &mine), None, &[]).await;
    assert_eq!(own_list.status, StatusCode::OK, "{}", own_list.body);
    let theirs_list = send(&author, Method::GET, &comments(&project, &entry), None, &[]).await;
    assert_eq!(theirs_list.status, StatusCode::NOT_FOUND);
    let own_write = comment_on(&author, &project, &mine, &mine_rev, "mine", None).await;
    assert_eq!(own_write.status, StatusCode::CREATED, "{}", own_write.body);
    let theirs_write = comment_on(&author, &project, &entry, &revision, "theirs", None).await;
    assert_eq!(theirs_write.status, StatusCode::FORBIDDEN);

    // The task list filters to what the role may read: their own entry's tasks only.
    let own_task = send(
        &author,
        Method::POST,
        &tasks(&project),
        Some(&json!({"entry": mine, "title": "finish"})),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(own_task.status, StatusCode::CREATED, "{}", own_task.body);
    let mine_tasks = send(&author, Method::GET, &tasks(&project), None, &[]).await;
    assert_eq!(mine_tasks.status, StatusCode::OK, "{}", mine_tasks.body);
    let listed: Vec<&str> = mine_tasks.body["data"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["entry_id"].as_str().expect("an entry"))
        .collect();
    assert_eq!(listed, [mine.as_str()], "only their own entry's tasks");
}
