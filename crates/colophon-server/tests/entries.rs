//! Entries end to end: the real binary, a real sign-in, and every rule a save must keep, over
//! HTTP. Needs the stack (scripts/stack.sh up) and migrations (scripts/migrate.sh).
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Server, Session, article, call, entries, key, project, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::json;

async fn create(session: &Session, project: &str, title: &str) -> (String, String) {
    let reply = call(
        session,
        Method::POST,
        &entries(project),
        Some(&article(title, "Hello")),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let id = reply.body["data"]["id"].as_str().expect("an id").to_owned();
    (id, reply.etag().expect("an ETag"))
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_draft_is_created_read_saved_and_kept_in_history() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let project = project(&editor).await;

    let body = article("First \"draft\"", "Hello");
    let created = call(
        &editor,
        Method::POST,
        &entries(&project),
        Some(&body),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = created.body["data"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let first = created.etag().expect("an ETag");
    let revision = created.body["data"]["revision"]["id"]
        .as_str()
        .expect("a revision");
    assert_eq!(
        first,
        format!("\"{revision}\""),
        "the ETag names the revision"
    );
    assert_eq!(
        created.body["data"]["content"], body,
        "the content comes back as sent"
    );
    assert!(
        created.body["data"]["revision"]["content_hash"]
            .as_str()
            .is_some_and(|hash| hash.starts_with("sha256:") && hash.len() == 71)
    );

    let read = call(
        &editor,
        Method::GET,
        &format!("{}/{id}", entries(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        (read.status, read.etag()),
        (StatusCode::OK, Some(first.clone()))
    );

    let saved = call(
        &editor,
        Method::PUT,
        &format!("{}/{id}", entries(&project)),
        Some(&article("Second", "Hello again")),
        &[("if-match", &first), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    let second = saved.etag().expect("an ETag");
    assert_ne!(second, first);
    assert_eq!(
        saved.body["data"]["revision"]["base_revision_id"].as_str(),
        Some(revision)
    );

    let history = call(
        &editor,
        Method::GET,
        &format!("{}/{id}/revisions", entries(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    let titles: Vec<&str> = history.body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|revision| revision["title"].as_str())
        .collect();
    assert_eq!(titles, ["Second", "First \"draft\""]);
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_stale_save_is_refused_with_the_current_etag_and_writes_nothing() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let project = project(&editor).await;
    let (id, first) = create(&editor, &project, "First").await;
    let path = format!("{}/{id}", entries(&project));

    let saved = call(
        &editor,
        Method::PUT,
        &path,
        Some(&article("Second", "B")),
        &[("if-match", &first), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    let second = saved.etag().expect("an ETag");

    // Someone still holding the first revision saves on top of it.
    let stale = call(
        &editor,
        Method::PUT,
        &path,
        Some(&article("Late", "C")),
        &[("if-match", &first), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(stale.status, StatusCode::CONFLICT, "{}", stale.body);
    assert_eq!(stale.body["error"]["details"]["reason"], "stale_revision");
    assert_eq!(
        stale.etag(),
        Some(second.clone()),
        "the 409 names the current revision"
    );

    let read = call(&editor, Method::GET, &path, None, &[])
        .await
        .expect("request");
    assert_eq!(read.etag(), Some(second));
    assert_eq!(
        read.body["data"]["content"]["title"], "Second",
        "nothing was written"
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "each refusal, then the replay, in one session"
)]
async fn writes_need_their_preconditions_and_keys_replay_or_refuse() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let project = project(&editor).await;
    let list = entries(&project);

    let no_key = call(&editor, Method::POST, &list, Some(&article("T", "x")), &[])
        .await
        .expect("request");
    assert_eq!(
        (no_key.status, no_key.body["error"]["code"].as_str()),
        (
            StatusCode::PRECONDITION_REQUIRED,
            Some("precondition_required")
        )
    );

    let invalid = call(
        &editor,
        Method::POST,
        &list,
        Some(&json!({"title": " padded", "body": article("T", "x")["body"]})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(invalid.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        (
            invalid.body["error"]["details"]["reason"].as_str(),
            invalid.body["error"]["details"]["pointer"].as_str()
        ),
        (Some("invalid_text"), Some("/title"))
    );

    // The same request with the same key: one entry, and the recorded answer again.
    let once = key();
    let body = article("Once", "x");
    let first = call(
        &editor,
        Method::POST,
        &list,
        Some(&body),
        &[("idempotency-key", &once)],
    )
    .await
    .expect("request");
    let again = call(
        &editor,
        Method::POST,
        &list,
        Some(&body),
        &[("idempotency-key", &once)],
    )
    .await
    .expect("request");
    assert_eq!(first.status, StatusCode::CREATED);
    assert_eq!(again.status, StatusCode::CREATED);
    assert_eq!(again.body, first.body, "a replay is the same answer");
    assert_eq!(again.etag(), first.etag());
    assert!(again.replayed(), "a replay of the recorded answer");
    assert!(!first.replayed());

    // The same key with a different body is refused.
    let reused = call(
        &editor,
        Method::POST,
        &list,
        Some(&article("Different", "y")),
        &[("idempotency-key", &once)],
    )
    .await
    .expect("request");
    assert_eq!(reused.status, StatusCode::CONFLICT);
    assert_eq!(
        reused.body["error"]["details"]["reason"],
        "idempotency_key_reused"
    );

    // Saving without If-Match, or with something that isn't one.
    let id = first.body["data"]["id"].as_str().expect("an id");
    let path = format!("{list}/{id}");
    let no_match = call(
        &editor,
        Method::PUT,
        &path,
        Some(&body),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(no_match.status, StatusCode::PRECONDITION_REQUIRED);
    let star = call(
        &editor,
        Method::PUT,
        &path,
        Some(&body),
        &[("if-match", "*"), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(star.status, StatusCode::BAD_REQUEST);

    let listed = call(&editor, Method::GET, &list, None, &[])
        .await
        .expect("request");
    assert_eq!(
        listed.body["data"].as_array().map(Vec::len),
        Some(1),
        "the replayed create made one entry: {}",
        listed.body
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn lists_page_with_a_cursor_that_only_works_where_it_came_from() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let project = project(&editor).await;
    for title in ["One", "Two", "Three"] {
        create(&editor, &project, title).await;
    }
    let first = call(
        &editor,
        Method::GET,
        &format!("{}?limit=2", entries(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    let titles = |reply: &common::Reply| -> Vec<String> {
        reply.body["data"]
            .as_array()
            .expect("a list")
            .iter()
            .filter_map(|entry| entry["revision"]["title"].as_str().map(str::to_owned))
            .collect()
    };
    assert_eq!(titles(&first), ["Three", "Two"], "newest first");
    let cursor = first.body["next_cursor"].as_str().expect("another page");
    let second = call(
        &editor,
        Method::GET,
        &format!("{}?limit=2&cursor={cursor}", entries(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(titles(&second), ["One"]);
    assert!(second.body["next_cursor"].is_null(), "the last page");

    // The cursor belongs to this project's list: elsewhere it is refused.
    let other = common::project(&editor).await;
    let elsewhere = call(
        &editor,
        Method::GET,
        &format!("{}?cursor={cursor}", entries(&other)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(elsewhere.status, StatusCode::BAD_REQUEST);
    let too_many = call(
        &editor,
        Method::GET,
        &format!("{}?limit=101", entries(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(too_many.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn nobody_outside_the_project_can_read_or_write_its_entries() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let project = project(&editor).await;
    let (id, etag) = create(&editor, &project, "Private").await;
    let path = format!("{}/{id}", entries(&project));

    let stranger = sign_in("dev-editor").await;
    let attempts = [
        (Method::GET, entries(&project), None),
        (Method::GET, path.clone(), None),
        (Method::GET, format!("{path}/revisions"), None),
        (
            Method::GET,
            format!("{path}/revisions/{}", etag.trim_matches('"')),
            None,
        ),
        (Method::POST, entries(&project), Some(article("Mine", "x"))),
        (Method::PUT, path.clone(), Some(article("Mine", "x"))),
    ];
    for (method, target, body) in attempts {
        let reply = call(
            &stranger,
            method.clone(),
            &target,
            body.as_ref(),
            &[("if-match", &etag), ("idempotency-key", &key())],
        )
        .await
        .expect("request");
        assert_eq!(
            (reply.status, reply.body["error"]["code"].as_str()),
            (StatusCode::NOT_FOUND, Some("not_found")),
            "{method} {target}"
        );
    }

    // Paths that can't name anything are 404 too, never 400 or 500.
    for target in [
        format!(
            "/api/v1/projects/not-a-uuid/environments/production/collections/articles/entries/{id}"
        ),
        format!(
            "/api/v1/projects/{project}/environments/staging/collections/articles/entries/{id}"
        ),
        format!(
            "/api/v1/projects/{project}/environments/production/collections/pages/entries/{id}"
        ),
        format!("{}/not-a-uuid", entries(&project)),
    ] {
        let reply = call(&editor, Method::GET, &target, None, &[])
            .await
            .expect("request");
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{target}");
    }

    // A write without the CSRF token is refused before anything else.
    let response = common::app()
        .put(format!("{}{path}", common::ORIGIN))
        .header("cookie", &editor.cookie)
        .header("origin", common::ORIGIN)
        .header("if-match", &etag)
        .header("idempotency-key", key())
        .json(&article("No token", "x"))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

fn titles(reply: &common::Reply) -> Vec<String> {
    reply.body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|entry| entry["revision"]["title"].as_str().map(str::to_owned))
        .collect()
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_list_searches_draft_titles_and_wildcards_match_only_themselves() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let project = project(&editor).await;
    for title in ["Field notes", "Release notes: 100% done", "Kitchen sink"] {
        create(&editor, &project, title).await;
    }
    let search = async |q: &str| {
        let reply = call(
            &editor,
            Method::GET,
            &format!("{}?q={q}", entries(&project)),
            None,
            &[],
        )
        .await
        .expect("request");
        if reply.status == StatusCode::OK {
            (reply.status, titles(&reply))
        } else {
            (reply.status, Vec::new())
        }
    };
    let found = |titles: &[&str]| -> (StatusCode, Vec<String>) {
        (
            StatusCode::OK,
            titles.iter().map(|title| (*title).to_owned()).collect(),
        )
    };
    // Case-insensitive, anywhere in the title. As wildcards, `s%d` would match both notes and
    // `n_sink` the sink.
    assert_eq!(
        search("NOTES").await,
        found(&["Release notes: 100% done", "Field notes"])
    );
    assert_eq!(search("100%25").await, found(&["Release notes: 100% done"]));
    assert_eq!(search("s%25d").await, found(&[]));
    assert_eq!(search("n_sink").await, found(&[]));
    assert_eq!(
        search("%20%20").await.1.len(),
        3,
        "a blank search lists everything"
    );
    assert_eq!(
        search(&"x".repeat(101)).await.0,
        StatusCode::BAD_REQUEST,
        "a search is bounded"
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_list_says_what_is_live_and_the_live_revision_reads_back() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let project = project(&editor).await;
    let (field, field_etag) = create(&editor, &project, "Field notes").await;
    let (kitchen, _) = create(&editor, &project, "Kitchen sink").await;
    let first_revision = field_etag.trim_matches('"').to_owned();
    let generation = common::release(
        &editor,
        &project,
        &json!({"title": "Field notes", "entries": [field]}),
    )
    .await;
    let edited = call(
        &editor,
        Method::PUT,
        &format!("{}/{field}", entries(&project)),
        Some(&article("Field notes, revised", "Hello again")),
        &[("if-match", &field_etag), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(edited.status, StatusCode::OK, "{}", edited.body);

    // Every row says what readers get now: the published entry names its live revision, which
    // the edit has moved on from.
    let all = call(&editor, Method::GET, &entries(&project), None, &[])
        .await
        .expect("request");
    let row = |id: &str| {
        all.body["data"]
            .as_array()
            .expect("a list")
            .iter()
            .find(|entry| entry["id"] == id)
            .expect("listed")
            .clone()
    };
    let live = row(&field);
    assert_eq!(live["published"]["revision_id"], first_revision.as_str());
    assert_eq!(live["published"]["generation"], generation);
    assert_ne!(
        live["revision"]["id"],
        first_revision.as_str(),
        "changed since"
    );
    assert!(row(&kitchen)["published"].is_null(), "a draft isn't live");

    // The live revision reads back with its own content, for comparing.
    let path = format!("{}/{field}/revisions", entries(&project));
    let old = call(
        &editor,
        Method::GET,
        &format!("{path}/{first_revision}"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(old.status, StatusCode::OK, "{}", old.body);
    assert_eq!(old.body["data"]["revision"]["title"], "Field notes");
    assert_eq!(old.body["data"]["content"], article("Field notes", "Hello"));
    // A revision of another entry, or none at all, isn't found under this one.
    for target in [
        format!("{}/{kitchen}/revisions/{first_revision}", entries(&project)),
        format!("{path}/01a17000-0000-7000-8000-000000000000"),
        format!("{path}/not-a-uuid"),
    ] {
        let reply = call(&editor, Method::GET, &target, None, &[])
            .await
            .expect("request");
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{target}");
    }

    // A changeset item says which revision it changes from.
    let set = call(
        &editor,
        Method::POST,
        &format!("/api/v1/projects/{project}/environments/production/changesets"),
        Some(&json!({"title": "Revise", "entries": [field]})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(set.status, StatusCode::CREATED, "{}", set.body);
    assert_eq!(
        set.body["data"]["items"][0]["published_revision_id"],
        first_revision.as_str()
    );
}

/// Holds `entry`'s row from a connection of the test's own until the returned transaction ends.
async fn hold_row(
    database: &sqlx::PgPool,
    entry: &str,
) -> sqlx::Transaction<'static, sqlx::Postgres> {
    let mut holder = database.begin().await.expect("begin");
    sqlx::query("SELECT 1 FROM entries WHERE id = $1::uuid FOR UPDATE")
        .bind(entry)
        .execute(&mut *holder)
        .await
        .expect("hold the entry");
    holder
}

/// The entry's revision titles, newest first.
async fn history(session: &Session, project: &str, id: &str) -> Vec<String> {
    let reply = call(
        session,
        Method::GET,
        &format!("{}/{id}/revisions", entries(project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    reply.body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|revision| revision["title"].as_str().map(str::to_owned))
        .collect()
}

/// D-239: a save queued behind a row someone holds for longer than the request pool's lock
/// timeout is refused as contention (503 with Retry-After) once that timeout passes, before the
/// statement timeout: nothing was committed, so the same request sent again, same key, once the
/// row is free, lands exactly once. Before D-239 it waited out the statement timeout and answered
/// 500 internal (D-216's row says contention is "never a 500").
#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_save_behind_a_held_row_is_told_to_try_again_and_lands_once_retried() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let project = project(&editor).await;
    let (id, base) = create(&editor, &project, "Base").await;
    let path = format!("{}/{id}", entries(&project));
    let database = common::owner_database().await;
    let holder = hold_row(&database, &id).await;

    let idempotency = key();
    let headers = [
        ("if-match", base.as_str()),
        ("idempotency-key", idempotency.as_str()),
    ];
    let body = article("Second", "B");
    let started = std::time::Instant::now();
    let refused = call(&editor, Method::PUT, &path, Some(&body), &headers)
        .await
        .expect("request");
    let waited = started.elapsed();
    assert_eq!(
        (refused.status, refused.body["error"]["code"].as_str()),
        (StatusCode::SERVICE_UNAVAILABLE, Some("unavailable")),
        "{}",
        refused.body
    );
    assert_eq!(
        refused
            .headers
            .get("retry-after")
            .and_then(|value| value.to_str().ok()),
        Some("1")
    );
    let lock_timeout = colophon_db::REQUEST_LOCK_TIMEOUT;
    assert!(
        waited >= lock_timeout && waited < std::time::Duration::from_secs(5),
        "refused at the lock timeout, not the statement timeout: {waited:?}"
    );
    holder.rollback().await.expect("release");
    assert_eq!(
        history(&editor, &project, &id).await,
        ["Base"],
        "nothing was committed"
    );

    let retried = call(&editor, Method::PUT, &path, Some(&body), &headers)
        .await
        .expect("request");
    assert_eq!(retried.status, StatusCode::OK, "{}", retried.body);
    let again = call(&editor, Method::PUT, &path, Some(&body), &headers)
        .await
        .expect("request");
    assert_eq!(
        (again.status, again.etag()),
        (StatusCode::OK, retried.etag()),
        "the same key replays the one save"
    );
    assert_eq!(history(&editor, &project, &id).await, ["Second", "Base"]);
    database.close().await;
}

/// D-239's other half: a statement slow at its own work, no lock involved, still fails at the
/// statement timeout as a 500, and the server names it a statement timeout on its route, so a
/// bad plan or a missing index is seen rather than retried. A trigger on this one entry's row
/// stands in for the slow statement.
#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_slow_statement_is_still_a_failure_named_as_a_statement_timeout() {
    let log_path = std::env::temp_dir().join(format!("colophon-slow-{}.log", key()));
    let log = std::fs::File::create(&log_path).expect("a log file");
    let _server = Server::start_logged_to(
        log,
        // INFO keeps the request span, which carries the route; JSON prints it with each line.
        &[("COLOPHON_LOG", "info"), ("COLOPHON_LOG_FORMAT", "json")],
    )
    .await;
    let editor = sign_in("dev-owner").await;
    let project = project(&editor).await;
    let (id, base) = create(&editor, &project, "Base").await;
    let database = common::owner_database().await;
    let name = format!("colophon_test_slow_{}", id.replace('-', ""));
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE FUNCTION {name}() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN PERFORM pg_sleep(6); RETURN NEW; END $$;
         CREATE TRIGGER {name} BEFORE UPDATE ON entries FOR EACH ROW
         WHEN (OLD.id = '{id}'::uuid) EXECUTE FUNCTION {name}();"
    )))
    .execute(&database)
    .await
    .expect("the slow trigger");

    let saved = call(
        &editor,
        Method::PUT,
        &format!("{}/{id}", entries(&project)),
        Some(&article("Second", "B")),
        &[("if-match", &base), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {name} ON entries; DROP FUNCTION {name}();"
    )))
    .execute(&database)
    .await
    .expect("drop the slow trigger");
    database.close().await;
    assert_eq!(
        (saved.status, saved.body["error"]["code"].as_str()),
        (StatusCode::INTERNAL_SERVER_ERROR, Some("internal")),
        "{}",
        saved.body
    );
    let logged = std::fs::read_to_string(&log_path).expect("the log");
    let _ = std::fs::remove_file(&log_path);
    let line = logged
        .lines()
        .find(|line| line.contains("request failed: statement timeout"))
        .unwrap_or_else(|| panic!("no statement timeout logged:\n{logged}"));
    assert!(line.contains(r#""fault":"statement_timeout""#), "{line}");
    assert!(
        line.contains("/collections/{collection}/entries/{entry}"),
        "the route: {line}"
    );
}

/// D-239 doesn't turn ordinary contention into refusals: a save waiting on its collection's
/// lock (held exclusively, as a content type change holds it) or on its row, for less than the
/// lock timeout, still lands.
#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_save_waiting_less_than_the_lock_timeout_still_lands() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let project = project(&editor).await;
    let (id, base) = create(&editor, &project, "Base").await;
    let path = format!("{}/{id}", entries(&project));
    let database = common::owner_database().await;
    let hold = std::time::Duration::from_millis(1_500);

    // The collection's lock, exclusively: content_types::lock_for_change.
    let mut change = database.begin().await.expect("begin");
    sqlx::query(
        "SELECT pg_advisory_xact_lock(x'4354'::int,
                hashtext(project_id::text || '/' || environment_id::text || '/' || collection))
         FROM entries WHERE id = $1::uuid",
    )
    .bind(&id)
    .execute(&mut *change)
    .await
    .expect("hold the collection");
    let released = tokio::spawn(async move {
        tokio::time::sleep(hold).await;
        change.rollback().await.expect("release");
    });
    let started = std::time::Instant::now();
    let saved = call(
        &editor,
        Method::PUT,
        &path,
        Some(&article("Second", "B")),
        &[("if-match", &base), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    released.await.expect("released");
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    assert!(
        started.elapsed() >= hold,
        "it waited: {:?}",
        started.elapsed()
    );

    // Its row, the same way.
    let current = saved.etag().expect("an ETag");
    let holder = hold_row(&database, &id).await;
    let released = tokio::spawn(async move {
        tokio::time::sleep(hold).await;
        holder.rollback().await.expect("release");
    });
    let started = std::time::Instant::now();
    let saved = call(
        &editor,
        Method::PUT,
        &path,
        Some(&article("Third", "C")),
        &[("if-match", &current), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    released.await.expect("released");
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    assert!(
        started.elapsed() >= hold,
        "it waited: {:?}",
        started.elapsed()
    );
    assert_eq!(
        history(&editor, &project, &id).await,
        ["Third", "Second", "Base"]
    );
    database.close().await;
}

/// F04 on the real binary under real concurrency (module 17): two editors, signed in apart, save
/// the same base revision at the same moment, fifty times over. Each time exactly one save lands
/// and the other is refused `stale_revision` naming the revision that won; the loser retried with
/// its own Idempotency-Key gets that same refusal back, never a late success; and the entry's
/// history is the base and the fifty winners, in order: no loser's revision is ever stored. (The
/// database half, the Studio's two browsers and the simulator are in module 17's F04 row.)
#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one entry's fifty contested saves, then its whole history"
)]
async fn two_editors_saving_one_base_at_once_one_wins_and_nothing_is_lost() {
    const ROUNDS: usize = 50;
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let editor = sign_in("dev-editor").await;
    common::join(&owner, &editor, &project, "editor").await;
    let (id, mut base) = create(&owner, &project, "Base").await;
    let path = format!("{}/{id}", entries(&project));
    let mut kept = vec!["Base".to_owned()];

    for round in 0..ROUNDS {
        let keys = [key(), key()];
        let titles = [format!("Owner {round}"), format!("Editor {round}")];
        let sessions = [&owner, &editor];
        let bodies = [article(&titles[0], "A"), article(&titles[1], "B")];
        let headers = [
            [
                ("if-match", base.as_str()),
                ("idempotency-key", keys[0].as_str()),
            ],
            [
                ("if-match", base.as_str()),
                ("idempotency-key", keys[1].as_str()),
            ],
        ];
        let (first, second) = tokio::join!(
            call(
                sessions[0],
                Method::PUT,
                &path,
                Some(&bodies[0]),
                &headers[0]
            ),
            call(
                sessions[1],
                Method::PUT,
                &path,
                Some(&bodies[1]),
                &headers[1]
            ),
        );
        let replies = [first.expect("request"), second.expect("request")];
        let won: Vec<usize> = (0..2)
            .filter(|&n| replies[n].status == StatusCode::OK)
            .collect();
        let lost: Vec<usize> = (0..2)
            .filter(|&n| replies[n].status == StatusCode::CONFLICT)
            .collect();
        assert_eq!(
            (won.len(), lost.len()),
            (1, 1),
            "round {round}: {} and {}",
            replies[0].status,
            replies[1].status
        );
        let (winner, loser) = (won[0], lost[0]);
        let current = replies[winner].etag().expect("the winner's ETag");
        assert_eq!(
            replies[loser].body["error"]["details"]["reason"], "stale_revision",
            "round {round}: {}",
            replies[loser].body
        );
        assert_eq!(
            replies[loser].etag(),
            Some(current.clone()),
            "round {round}: the 409 names the revision that won"
        );

        // The loser sends the same request again, same key: the same answer, not a late success.
        let retried = call(
            sessions[loser],
            Method::PUT,
            &path,
            Some(&bodies[loser]),
            &headers[loser],
        )
        .await
        .expect("request");
        assert_eq!(
            (retried.status, &retried.body["error"]["details"]["reason"]),
            (StatusCode::CONFLICT, &json!("stale_revision")),
            "round {round}: the retry is refused again: {}",
            retried.body
        );

        kept.push(titles[winner].clone());
        base = current;
    }

    // The history is the base and every winner, newest first: no loser was ever stored.
    let history = call(
        &owner,
        Method::GET,
        &format!("{}/{id}/revisions?limit=100", entries(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(history.status, StatusCode::OK, "{}", history.body);
    let mut stored: Vec<String> = history.body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|revision| revision["title"].as_str().map(str::to_owned))
        .collect();
    stored.reverse();
    assert_eq!(stored, kept);
}

/// F04 with both saves provably in flight together: the entry's row is held, both editors save
/// from the same base, and only once both of the server's backends wait (one on the holder, the
/// other queued behind it) is it released, well inside the statement timeout. One lands, the other is refused naming it, and the history holds the base and the
/// winner alone.
#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn two_saves_held_in_flight_together_still_make_one_winner() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let editor = sign_in("dev-editor").await;
    common::join(&owner, &editor, &project, "editor").await;
    let (id, base) = create(&owner, &project, "Base").await;
    let path = format!("{}/{id}", entries(&project));

    let database = common::owner_database().await;
    let mut holder = database.begin().await.expect("begin");
    sqlx::query("SELECT 1 FROM entries WHERE id = $1::uuid FOR UPDATE")
        .bind(&id)
        .execute(&mut *holder)
        .await
        .expect("hold the entry");
    let holder_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *holder)
        .await
        .expect("its backend");
    let saves = ["Owner", "Editor"].map(|title| {
        let (session, path, base) = (
            if title == "Owner" {
                owner.clone()
            } else {
                editor.clone()
            },
            path.clone(),
            base.clone(),
        );
        tokio::spawn(async move {
            call(
                &session,
                Method::PUT,
                &path,
                Some(&article(title, "body")),
                &[("if-match", &base), ("idempotency-key", &key())],
            )
            .await
            .expect("request")
        })
    });
    // Released before the request pool's lock timeout (D-239) can end either wait: past it,
    // the first save would be refused as contention instead of racing the second.
    let deadline = std::time::Instant::now()
        + colophon_db::REQUEST_LOCK_TIMEOUT.saturating_sub(std::time::Duration::from_secs(1));
    loop {
        // The two queue: the first waits on the holder, the second on the first.
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND pid <> $1
               AND cardinality(pg_blocking_pids(pid)) > 0",
        )
        .bind(holder_pid)
        .fetch_one(&database)
        .await
        .expect("pg_stat_activity");
        if waiting >= 2 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "both saves never waited together ({waiting} waiting)"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    holder.rollback().await.expect("release");

    let mut statuses = Vec::new();
    let mut bodies = Vec::new();
    let mut winner = None;
    for save in saves {
        let reply = save.await.expect("joined");
        if reply.status == StatusCode::OK {
            winner = Some(reply.body["data"]["content"]["title"].clone());
        }
        statuses.push(reply.status);
        bodies.push(reply.body);
    }
    statuses.sort_unstable();
    assert_eq!(
        statuses,
        [StatusCode::OK, StatusCode::CONFLICT],
        "the two saves: {bodies:?}"
    );
    let history = call(
        &owner,
        Method::GET,
        &format!("{}/{id}/revisions", entries(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    let titles: Vec<&serde_json::Value> = history.body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|revision| &revision["title"])
        .collect();
    assert_eq!(
        titles,
        [&winner.expect("a winner"), &json!("Base")],
        "the base and the winner alone"
    );
    database.close().await;
}
