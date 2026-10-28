//! Releases and public delivery end to end (step 2.2, D-045): an approved changeset becomes
//! release 1, delivery serves it to anyone with no sign-in, a newer draft never leaks into it,
//! release 2 changes one entry while generation 1 still reads as it was, and two releases racing
//! on one base can't both commit. Unpublishing (step 3.2, D-049) is reviewed like publishing: it
//! pins the published revision, goes stale when that changes, and its release says what it took
//! down. Needs the stack and `scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{
    ORIGIN, Reply, Server, Session, app, article, call, entries, join, key, project, sign_in,
};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
}

async fn new_article(session: &Session, project: &str, title: &str, text: &str) -> String {
    let reply = call(
        session,
        Method::POST,
        &entries(project),
        Some(&article(title, text)),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    reply.body["data"]["id"].as_str().expect("an id").to_owned()
}

async fn edit(session: &Session, project: &str, entry: &str, title: &str, text: &str) {
    let path = format!("{}/{entry}", entries(project));
    let current = call(session, Method::GET, &path, None, &[])
        .await
        .expect("request");
    let saved = call(
        session,
        Method::PUT,
        &path,
        Some(&article(title, text)),
        &[
            ("if-match", &current.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request");
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
}

/// A changeset of `entries`, submitted by `author` and approved by `approver`; its path and hash.
async fn approved(
    author: &Session,
    approver: &Session,
    project: &str,
    entries: &[&str],
) -> (String, String) {
    let body = json!({"title": "Release", "entries": entries});
    approved_changeset(author, approver, project, &body).await
}

/// The changeset `body`, submitted by `author` and approved by `approver`; its path and hash.
async fn approved_changeset(
    author: &Session,
    approver: &Session,
    project: &str,
    body: &Value,
) -> (String, String) {
    let created = call(
        author,
        Method::POST,
        &format!("{}/changesets", base(project)),
        Some(body),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let path = format!(
        "{}/changesets/{}",
        base(project),
        created.body["data"]["id"].as_str().expect("an id")
    );
    let hash = created.body["data"]["hash"]
        .as_str()
        .expect("a hash")
        .to_owned();
    let if_match = format!("\"{hash}\"");
    let submitted = call(
        author,
        Method::POST,
        &format!("{path}/submit"),
        None,
        &[("if-match", &if_match)],
    )
    .await
    .expect("request");
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);
    let approval = call(
        approver,
        Method::POST,
        &format!("{path}/reviews"),
        Some(&json!({"decision": "approve"})),
        &[("if-match", &if_match)],
    )
    .await
    .expect("request");
    assert_eq!(
        approval.body["data"]["status"]["state"], "approved",
        "{}",
        approval.body
    );
    (path, hash)
}

async fn publish(
    session: &Session,
    path: &str,
    hash: &str,
    base_generation: i64,
    idempotency: &str,
) -> Reply {
    call(
        session,
        Method::POST,
        &format!("{path}/publish"),
        Some(&json!({"base_generation": base_generation})),
        &[
            ("if-match", &format!("\"{hash}\"")),
            ("idempotency-key", idempotency),
        ],
    )
    .await
    .expect("request")
}

/// A public delivery read, with no cookie at all; the body and `Colophon-Generation`.
async fn deliver(path: &str) -> (StatusCode, Value, Option<i64>) {
    let response = app()
        .get(format!("{ORIGIN}{path}"))
        .send()
        .await
        .expect("request");
    let status = response.status();
    let generation = response
        .headers()
        .get("colophon-generation")
        .and_then(|value| value.to_str().ok()?.parse().ok());
    (
        status,
        response.json().await.unwrap_or(Value::Null),
        generation,
    )
}

fn texts(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|entry| {
            entry["content"]["body"]["doc"]["content"][0]["content"][0]["text"]
                .as_str()
                .unwrap_or("?")
                .to_owned()
        })
        .collect()
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: release 1, a draft, release 2, a race"
)]
async fn releases_are_numbered_delivered_and_never_leak_drafts() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let editor = sign_in("dev-editor").await;
    let project = project(&owner).await;
    join(&owner, &editor, &project, "editor").await;
    let content = format!("/content/v1/{project}/production/articles");

    // Nothing published yet: an empty read at generation 0.
    let (status, body, generation) = deliver(&content).await;
    assert_eq!(
        (status, generation, body["data"].as_array().map(Vec::len)),
        (StatusCode::OK, Some(0), Some(0))
    );

    let first = new_article(&editor, &project, "First", "one").await;
    let second = new_article(&editor, &project, "Second", "two").await;
    let (path, hash) = approved(&editor, &owner, &project, &[&first, &second]).await;
    let by_editor = publish(&editor, &path, &hash, 0, &key()).await;
    assert_eq!(
        by_editor.status,
        StatusCode::FORBIDDEN,
        "editors don't publish"
    );
    assert_eq!(
        (
            &by_editor.body["error"]["details"]["reason"],
            &by_editor.body["error"]["details"]["permission"]
        ),
        (&json!("missing_permission"), &json!("content.publish")),
        "a refusal says why in codes a client can act on: {}",
        by_editor.body
    );
    let publish_key = key();
    let released = publish(&owner, &path, &hash, 0, &publish_key).await;
    assert_eq!(released.status, StatusCode::CREATED, "{}", released.body);
    assert_eq!(
        (
            released.body["data"]["generation"].as_i64(),
            released.body["data"]["entries_changed"].as_u64()
        ),
        (Some(1), Some(2))
    );
    let replay = publish(&owner, &path, &hash, 0, &publish_key).await;
    assert!(
        replay.replayed(),
        "a retry after a lost answer gets the same release"
    );
    assert_eq!(replay.body["data"]["generation"], 1);
    let again = publish(&owner, &path, &hash, 1, &key()).await;
    assert_eq!(
        again.status,
        StatusCode::CONFLICT,
        "published once: {}",
        again.body
    );
    assert_eq!(
        again.body["error"]["details"]["reason"],
        "already_published"
    );
    let shown = call(&owner, Method::GET, &path, None, &[])
        .await
        .expect("request");
    assert_eq!(
        (
            shown.body["data"]["status"]["state"].as_str(),
            shown.body["data"]["status"]["generation"].as_i64()
        ),
        (Some("published"), Some(1))
    );

    // Anyone can read it, with no sign-in; a newer draft never shows.
    let (_, body, generation) = deliver(&content).await;
    assert_eq!(generation, Some(1));
    assert_eq!(texts(&body), ["two", "one"], "newest first");
    edit(&editor, &project, &first, "First", "one, edited").await;
    let (_, body, _) = deliver(&content).await;
    assert_eq!(texts(&body), ["two", "one"], "the draft isn't published");

    // Release 2 publishes the edit; generation 1 still reads as it was.
    let (path2, hash2) = approved(&editor, &owner, &project, &[&first]).await;
    let stale_base = publish(&owner, &path2, &hash2, 0, &key()).await;
    assert_eq!(
        stale_base.body["error"]["details"]["reason"],
        "generation_moved"
    );
    assert_eq!(
        stale_base
            .headers
            .get("colophon-generation")
            .and_then(|v| v.to_str().ok()),
        Some("1")
    );
    let second_release = publish(&owner, &path2, &hash2, 1, &key()).await;
    assert_eq!(
        (
            second_release.body["data"]["generation"].as_i64(),
            second_release.body["data"]["entries_changed"].as_u64()
        ),
        (Some(2), Some(1))
    );
    let (_, now, generation) = deliver(&content).await;
    assert_eq!(
        (generation, texts(&now)),
        (Some(2), vec!["two".to_owned(), "one, edited".to_owned()])
    );
    let (_, then, generation) = deliver(&format!("{content}?generation=1")).await;
    assert_eq!(
        (generation, texts(&then)),
        (Some(1), vec!["two".to_owned(), "one".to_owned()]),
        "a pinned read"
    );
    let (_, one, _) = deliver(&format!("{content}/{second}")).await;
    assert_eq!(
        one["data"]["published_in"], 1,
        "unchanged entries keep their release"
    );

    // A list read page by page stays on the generation of its first page.
    let (_, page, _) = deliver(&format!("{content}?limit=1")).await;
    let cursor = page["next_cursor"].as_str().expect("a cursor").to_owned();

    // Two releases on one base: exactly one commits.
    let third = new_article(&editor, &project, "Third", "three").await;
    let fourth = new_article(&editor, &project, "Fourth", "four").await;
    let (a, hash_a) = approved(&editor, &owner, &project, &[&third]).await;
    let (b, hash_b) = approved(&editor, &owner, &project, &[&fourth]).await;
    let (key_a, key_b) = (key(), key());
    let (left, right) = tokio::join!(
        publish(&owner, &a, &hash_a, 2, &key_a),
        publish(&owner, &b, &hash_b, 2, &key_b)
    );
    let mut statuses = [left.status, right.status];
    statuses.sort();
    assert_eq!(
        statuses,
        [StatusCode::CREATED, StatusCode::CONFLICT],
        "{} / {}",
        left.body,
        right.body
    );
    let loser = if left.status == StatusCode::CONFLICT {
        &left
    } else {
        &right
    };
    assert_eq!(loser.body["error"]["details"]["reason"], "generation_moved");

    let (_, next, generation) = deliver(&format!("{content}?limit=1&cursor={cursor}")).await;
    assert_eq!(
        (generation, texts(&next)),
        (Some(2), vec!["one, edited".to_owned()]),
        "the cursor's generation, not the newest"
    );
    let (_, latest, generation) = deliver(&content).await;
    assert_eq!(
        (generation, latest["data"].as_array().map(Vec::len)),
        (Some(3), Some(3))
    );

    let listed = call(
        &editor,
        Method::GET,
        &format!("{}/releases", base(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    let generations: Vec<i64> = listed.body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|r| r["generation"].as_i64())
        .collect();
    assert_eq!(generations, [3, 2, 1]);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn delivery_refuses_what_it_cannot_serve() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let content = format!("/content/v1/{project}/production/articles");
    let (status, body, _) = deliver(&format!("{content}?generation=1")).await;
    assert_eq!(
        (status, body["error"]["details"]["reason"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_generation")),
        "not committed yet"
    );
    let (status, _, _) = deliver(&format!(
        "/content/v1/{}/production/articles",
        uuid::Uuid::now_v7()
    ))
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = deliver(&format!("/content/v1/{project}/staging/articles")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let entry = new_article(&owner, &project, "Draft only", "secret").await;
    let (status, _, _) = deliver(&format!("{content}/{entry}")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a draft that was never published"
    );
    let (status, _, _) = deliver(&format!("{content}?cursor=nonsense")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // An unapproved changeset can't be published.
    let created = call(
        &owner,
        Method::POST,
        &format!("{}/changesets", base(&project)),
        Some(&json!({"title": "x", "entries": [entry]})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    let path = format!(
        "{}/changesets/{}",
        base(&project),
        created.body["data"]["id"].as_str().expect("an id")
    );
    let hash = created.body["data"]["hash"]
        .as_str()
        .expect("a hash")
        .to_owned();
    let refused = publish(&owner, &path, &hash, 0, &key()).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
}

async fn create_changeset(session: &Session, project: &str, body: &Value) -> Reply {
    call(
        session,
        Method::POST,
        &format!("{}/changesets", base(project)),
        Some(body),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request")
}

/// Unpublishing through review (D-049): only a published entry can be taken down, never in the
/// same changeset that publishes it; the item pins the published revision, goes stale when a newer
/// one is published, and refreshes onto it; the release reports what it took down, and delivery
/// answers 410 for it.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: refusals, a takedown, a newer release, refresh, release"
)]
async fn unpublishing_is_reviewed_pins_what_is_published_and_says_what_it_took_down() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let id = project(&owner).await;
    let live = new_article(&owner, &id, "Live", "first").await;
    let draft = new_article(&owner, &id, "Draft", "never published").await;
    let (path, hash) = approved(&owner, &owner, &id, &[&live]).await;
    assert_eq!(
        publish(&owner, &path, &hash, 0, &key()).await.status,
        StatusCode::CREATED
    );

    let refused =
        create_changeset(&owner, &id, &json!({"title": "No", "unpublish": [draft]})).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(refused.body["error"]["details"]["reason"], "not_published");
    let both = json!({"title": "No", "entries": [live], "unpublish": [live]});
    let refused = create_changeset(&owner, &id, &both).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"]["reason"],
        "invalid_entries"
    );
    let empty = create_changeset(&owner, &id, &json!({"title": "No"})).await;
    assert_eq!(
        empty.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        empty.body
    );

    let takedown = json!({"title": "Take it down", "unpublish": [live]});
    let (down_path, down_hash) = approved_changeset(&owner, &owner, &id, &takedown).await;
    let (_, shown) = common::get(Some(&owner), &down_path).await;
    let item = &shown["data"]["items"][0];
    assert_eq!(item["kind"], "unpublish");
    assert_eq!(item["edited"], false);
    let pinned = item["revision_id"].as_str().expect("a revision").to_owned();
    assert_eq!(item["current_revision_id"], pinned.as_str());
    let (_, listed) = common::get(Some(&owner), &format!("{}/changesets", base(&id))).await;
    let summary = listed["data"]
        .as_array()
        .and_then(|rows| {
            rows.iter().find(|row| {
                format!(
                    "{}/changesets/{}",
                    base(&id),
                    row["id"].as_str().unwrap_or_default()
                ) == down_path
            })
        })
        .expect("listed");
    assert_eq!(
        (summary["items"].as_i64(), summary["unpublish"].as_i64()),
        (Some(1), Some(1))
    );

    // A newer revision is published meanwhile: the takedown pinned the old one, so it's stale.
    edit(&owner, &id, &live, "Live", "second").await;
    let (path, hash) = approved(&owner, &owner, &id, &[&live]).await;
    assert_eq!(
        publish(&owner, &path, &hash, 1, &key()).await.status,
        StatusCode::CREATED
    );
    let stale = publish(&owner, &down_path, &down_hash, 2, &key()).await;
    assert_eq!(stale.status, StatusCode::CONFLICT, "{}", stale.body);
    let (_, shown) = common::get(Some(&owner), &down_path).await;
    assert_eq!(shown["data"]["status"]["state"], "stale");
    assert_eq!(shown["data"]["items"][0]["edited"], true);
    let refreshed = call(
        &owner,
        Method::POST,
        &format!("{down_path}/refresh"),
        None,
        &[("if-match", &format!("\"{down_hash}\""))],
    )
    .await
    .expect("request");
    assert_eq!(refreshed.status, StatusCode::OK, "{}", refreshed.body);
    let item = &refreshed.body["data"]["items"][0];
    assert_eq!(
        (item["kind"].as_str(), item["edited"].as_bool()),
        (Some("unpublish"), Some(false))
    );
    assert_ne!(
        item["revision_id"],
        pinned.as_str(),
        "now pins the revision published since"
    );
    let new_hash = refreshed.body["data"]["hash"]
        .as_str()
        .expect("a hash")
        .to_owned();
    assert_ne!(new_hash, down_hash);
    let approval = call(
        &owner,
        Method::POST,
        &format!("{down_path}/reviews"),
        Some(&json!({"decision": "approve"})),
        &[("if-match", &format!("\"{new_hash}\""))],
    )
    .await
    .expect("request");
    assert_eq!(
        approval.body["data"]["status"]["state"], "approved",
        "{}",
        approval.body
    );

    let released = publish(&owner, &down_path, &new_hash, 2, &key()).await;
    assert_eq!(released.status, StatusCode::CREATED, "{}", released.body);
    assert_eq!(released.body["data"]["generation"], 3);
    assert_eq!(
        (
            released.body["data"]["entries_changed"].as_i64(),
            released.body["data"]["entries_unpublished"].as_i64()
        ),
        (Some(0), Some(1))
    );
    let (_, history) = common::get(Some(&owner), &format!("{}/releases", base(&id))).await;
    assert_eq!(
        (
            history["data"][0]["generation"].as_i64(),
            history["data"][0]["unpublished"].as_i64()
        ),
        (Some(3), Some(1))
    );
    for generation in ["", "?generation=1", "?generation=2"] {
        let (status, body, _) = deliver(&format!(
            "/content/v1/{id}/production/articles/{live}{generation}"
        ))
        .await;
        assert_eq!(status, StatusCode::GONE, "{generation}: {body}");
    }
}

/// The caller's identity id, from `/me`.
async fn me(session: &Session) -> String {
    let me = call(session, Method::GET, "/me", None, &[])
        .await
        .expect("request");
    me.body["data"]["id"].as_str().expect("an id").to_owned()
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn an_entry_says_whether_and_where_it_is_live_and_lists_say_whose() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let editor = sign_in("dev-editor").await;
    let project = project(&owner).await;
    join(&owner, &editor, &project, "editor").await;
    let editor_id = me(&editor).await;
    let first = new_article(&editor, &project, "First", "one").await;
    let second = new_article(&editor, &project, "Second", "two").await;
    let read = |entry: String| {
        let (session, path) = (&owner, format!("{}/{entry}", entries(&project)));
        async move {
            call(session, Method::GET, &path, None, &[])
                .await
                .expect("request")
        }
    };

    // Not published: the read says so with an explicit null, and says whose entry it is.
    let draft = read(first.clone()).await;
    assert_eq!(draft.status, StatusCode::OK, "{}", draft.body);
    assert!(
        draft.body["data"]
            .as_object()
            .is_some_and(|data| data.get("published") == Some(&Value::Null)),
        "an unpublished entry reads `published: null`: {}",
        draft.body
    );
    assert_eq!(draft.body["data"]["created_by"], editor_id.as_str());

    let (path, hash) = approved(&editor, &owner, &project, &[&first]).await;
    let released = publish(&owner, &path, &hash, 0, &key()).await;
    assert_eq!(released.status, StatusCode::CREATED, "{}", released.body);

    // Published: the read names the live version and its release, whatever the list holds.
    let live = read(first.clone()).await;
    assert_eq!(live.body["data"]["published"]["generation"], 1);
    assert_eq!(
        live.body["data"]["published"]["revision_id"], live.body["data"]["revision"]["id"],
        "nothing edited since it went live"
    );
    assert_eq!(
        read(second.clone()).await.body["data"]["published"],
        Value::Null
    );

    // Edited after going live: the draft moved on, the live version didn't.
    edit(&editor, &project, &first, "First, edited", "one more").await;
    let moved = read(first.clone()).await;
    assert_eq!(moved.body["data"]["published"]["generation"], 1);
    assert_ne!(
        moved.body["data"]["published"]["revision_id"], moved.body["data"]["revision"]["id"],
        "changed since it went live"
    );

    // A save's answer doesn't look publication up, so it leaves the field out rather than
    // saying `null` (which would read as "not published").
    let saved = call(
        &editor,
        Method::PUT,
        &format!("{}/{first}", entries(&project)),
        Some(&article("First, again", "once more")),
        &[
            ("if-match", &moved.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request");
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    assert!(
        saved.body["data"]
            .as_object()
            .is_some_and(|data| !data.contains_key("published")),
        "{}",
        saved.body
    );

    // Lists say whose: entries by creator, publish requests by creator and submitter.
    let listed = call(&owner, Method::GET, &entries(&project), None, &[])
        .await
        .expect("request");
    let creators: Vec<&Value> = listed.body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|entry| &entry["created_by"])
        .collect();
    assert_eq!(creators, [&json!(editor_id), &json!(editor_id)]);
    let requests = call(
        &owner,
        Method::GET,
        &format!("{}/changesets", base(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    let request = &requests.body["data"][0];
    assert_eq!(
        (&request["created_by"], &request["submitted_by"]),
        (&json!(editor_id), &json!(editor_id)),
        "{}",
        requests.body
    );
}
