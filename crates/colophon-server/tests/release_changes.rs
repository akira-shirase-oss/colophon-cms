//! What a release changed, a rollback's preview and a publish request's items, as each caller may
//! see them (D-185, the Studio's publishing flow): every list that names entries across
//! collections and authors leaves out what the caller's role can't read and blanks titles it
//! can't read (D-058, step R.3), exactly as the entries API does; the lists are bounded and page
//! by cursor; a stale preview can't turn into a different request; a bad generation answers in
//! the API's own error shape; and nothing crosses projects. Needs the stack and
//! `scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use std::time::{Duration, Instant};

use common::{
    ORIGIN, Reply, Server, Session, app, article, call, create_article, entries, join, key,
    project, release, sign_in,
};
use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};

fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
}

fn notes(project: &str) -> String {
    format!("{}/collections/notes/entries", base(project))
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

/// A GET with no credentials at all, or with an API token's secret.
async fn bare(path: &str, bearer: Option<&str>) -> Reply {
    let mut request = app().get(format!("{ORIGIN}{path}"));
    if let Some(secret) = bearer {
        request = request.header(header::AUTHORIZATION, format!("Bearer {secret}"));
    }
    let response = request.send().await.expect("request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json().await.unwrap_or(Value::Null);
    Reply {
        status,
        headers,
        body,
    }
}

/// Adds the `notes` collection (title and text) to `project`.
async fn add_notes(owner: &Session, project: &str) {
    let made = send(
        owner,
        Method::POST,
        &format!("{}/content-types", base(project)),
        Some(&json!({"slug": "notes", "definition": {
            "name": "Notes", "title_field": "title",
            "fields": [
                {"id": "title", "label": "Title", "type": "text", "required": true},
                {"id": "note", "label": "Note", "type": "text"}
            ]
        }})),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
}

async fn new_note(session: &Session, project: &str, title: &str) -> String {
    let made = send(
        session,
        Method::POST,
        &notes(project),
        Some(&json!({"title": title, "note": "kept private"})),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    made.body["data"]["id"].as_str().expect("an id").to_owned()
}

async fn new_role(owner: &Session, project: &str, key_name: &str, permissions: &Value) {
    let made = send(
        owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/roles"),
        Some(&json!({"key": key_name, "name": key_name, "permissions": permissions})),
        &[],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
}

/// A new person, signed in, holding `role` in `project`.
async fn member(owner: &Session, project: &str, role: &str) -> Session {
    let person = sign_in(&format!("ws3-{role}-{}", &key()[25..])).await;
    join(owner, &person, project, role).await;
    person
}

/// Every page of a release's changes as `session` reads them, at `limit` a page.
async fn all_changes(session: &Session, project: &str, generation: i64, limit: u32) -> Vec<Value> {
    let mut rows = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..10_000 {
        let path = match &cursor {
            Some(at) => format!(
                "{}/releases/{generation}/changes?limit={limit}&cursor={at}",
                base(project)
            ),
            None => format!(
                "{}/releases/{generation}/changes?limit={limit}",
                base(project)
            ),
        };
        let page = send(session, Method::GET, &path, None, &[]).await;
        assert_eq!(page.status, StatusCode::OK, "{path}: {}", page.body);
        let data = page.body["data"].as_array().expect("a page");
        assert!(
            data.len() <= usize::try_from(limit).expect("a limit"),
            "{}",
            page.body
        );
        rows.extend(data.iter().cloned());
        match page.body["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => return rows,
        }
    }
    panic!("a release's changes never ended")
}

fn ids(rows: &[Value]) -> Vec<String> {
    rows.iter()
        .map(|row| row["entry_id"].as_str().expect("an entry").to_owned())
        .collect()
}

fn text(reply: &Reply) -> String {
    reply.body.to_string()
}

/// R1 and R1b (the coordinator's review of the receipt and preview additions): a role scoped to
/// one collection, a role that reads only its own entries and a role that can't read titles never
/// learn a title, entry or version through a receipt's changes, a rollback preview or a publish
/// request's items that the entries API would keep from them. Before the fix each of these
/// answers carried "Secret note" (and the request page did on `main`).
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one project, three roles, four answers each"
)]
async fn a_scoped_role_reads_in_changes_previews_and_requests_only_what_it_may_open() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    add_notes(&owner, &project).await;
    // Release 1: a note and an article. Release 2: the note taken down. Release 3: a newer article.
    let note = new_note(&owner, &project, "Secret note").await;
    let first = create_article(&owner, &project, "Harbour notes").await;
    let generation_one = release(
        &owner,
        &project,
        &json!({"title": "One", "entries": [note, first]}),
    )
    .await;
    assert_eq!(generation_one, 1);
    assert_eq!(
        release(
            &owner,
            &project,
            &json!({"title": "Two", "unpublish": [note]})
        )
        .await,
        2
    );
    let second = create_article(&owner, &project, "Ferry times").await;
    assert_eq!(
        release(
            &owner,
            &project,
            &json!({"title": "Three", "entries": [second]})
        )
        .await,
        3
    );

    new_role(
        &owner,
        &project,
        "desk",
        &json!([
            {"action": "content.read", "subject": "articles"},
            {"action": "content.update", "subject": "articles"}
        ]),
    )
    .await;
    new_role(
        &owner,
        &project,
        "mine",
        &json!([
            {"action": "content.read", "subject": "articles", "condition": "own"},
            {"action": "content.create", "subject": "articles"},
            {"action": "content.update", "subject": "articles", "condition": "own"}
        ]),
    )
    .await;
    new_role(
        &owner,
        &project,
        "untitled",
        &json!([
            {"action": "content.read", "subject": "*", "fields": ["body", "note"]},
            {"action": "content.update", "subject": "*"}
        ]),
    )
    .await;
    let desk = member(&owner, &project, "desk").await;
    let mine = member(&owner, &project, "mine").await;
    let untitled = member(&owner, &project, "untitled").await;
    let own = create_article(&mine, &project, "My own piece").await;
    assert_eq!(
        release(
            &owner,
            &project,
            &json!({"title": "Four", "entries": [own]})
        )
        .await,
        4
    );
    let requests = send(
        &owner,
        Method::GET,
        &format!("{}/changesets", base(&project)),
        None,
        &[],
    )
    .await;
    let one = requests.body["data"]
        .as_array()
        .expect("requests")
        .iter()
        .find(|row| row["title"] == "One")
        .and_then(|row| row["id"].as_str())
        .expect("request One")
        .to_owned();

    // First, every answer each role gets, searched for what it must not learn. Collected, so a
    // failure names every leak at once.
    let receipt_one = format!("{}/releases/1", base(&project));
    let receipt_two = format!("{}/releases/2", base(&project));
    let preview_one = format!("{}/releases/1/rollback", base(&project));
    let request_one = format!("{}/changesets/{one}", base(&project));
    let changes_one = format!("{}/releases/1/changes", base(&project));
    let mut leaks = Vec::new();
    let mut look = async |who: &str, session: &Session, path: &str, secrets: &[&str]| {
        let reply = send(session, Method::GET, path, None, &[]).await;
        let body = text(&reply);
        for secret in secrets {
            if body.contains(secret) {
                leaks.push(format!(
                    "{who} reads {secret:?} in GET {path} ({})",
                    reply.status
                ));
            }
        }
    };
    for path in [
        &receipt_one,
        &receipt_two,
        &preview_one,
        &request_one,
        &changes_one,
    ] {
        look("desk (articles only)", &desk, path, &["Secret note", &note]).await;
        look(
            "mine (own articles only)",
            &mine,
            path,
            &["Secret note", "Harbour notes", "Ferry times", &note, &first],
        )
        .await;
        look(
            "untitled (no title field)",
            &untitled,
            path,
            &["Secret note", "Harbour notes", "Ferry times"],
        )
        .await;
    }
    assert!(
        leaks.is_empty(),
        "{} leaks:\n{}",
        leaks.len(),
        leaks.join("\n")
    );

    // Then what each role is told instead. The owner reads everything: release 1 changed the
    // note and the article.
    let everything = all_changes(&owner, &project, 1, 100).await;
    assert_eq!(everything.len(), 2, "{everything:?}");
    // Desk: the article, whole; the note left out of release 1, and release 2 shows nothing.
    let seen = all_changes(&desk, &project, 1, 100).await;
    assert_eq!(ids(&seen), std::slice::from_ref(&first), "{seen:?}");
    assert_eq!(seen[0]["title"], "Harbour notes");
    assert_eq!(seen[0]["title_hidden"], false);
    assert_eq!(
        all_changes(&desk, &project, 2, 100).await,
        Vec::<Value>::new()
    );
    let preview = send(&desk, Method::GET, &preview_one, None, &[]).await;
    assert_eq!(preview.status, StatusCode::OK, "{}", preview.body);
    // It still says how many stay down in all, so the dialog can say "and 1 you can't open".
    assert_eq!(
        preview.body["data"]["kept_down_total"], 1,
        "{}",
        preview.body
    );
    assert_eq!(
        preview.body["data"]["kept_down"],
        json!([]),
        "{}",
        preview.body
    );
    // The request that made release 1 names the article and counts the note (R1b).
    let request = send(&desk, Method::GET, &request_one, None, &[]).await;
    assert_eq!(request.status, StatusCode::OK, "{}", request.body);
    assert_eq!(
        ids(request.body["data"]["items"].as_array().expect("items")),
        std::slice::from_ref(&first)
    );
    assert_eq!(request.body["data"]["hidden_items"], 1, "{}", request.body);
    // The owner still sees both, under the same hash: hiding changes nothing it pins.
    let whole = send(&owner, Method::GET, &request_one, None, &[]).await;
    assert_eq!(
        whole.body["data"]["items"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(whole.body["data"]["hidden_items"], 0);
    assert_eq!(whole.body["data"]["hash"], request.body["data"]["hash"]);
    // Mine: none of the owner's; its own, whole.
    assert_eq!(
        all_changes(&mine, &project, 1, 100).await,
        Vec::<Value>::new()
    );
    let seen = all_changes(&mine, &project, 4, 100).await;
    assert_eq!(ids(&seen), std::slice::from_ref(&own));
    assert_eq!(seen[0]["title"], "My own piece");
    let request = send(&mine, Method::GET, &request_one, None, &[]).await;
    assert_eq!(request.body["data"]["items"], json!([]));
    assert_eq!(request.body["data"]["hidden_items"], 2);
    // Untitled: every entry, each title blank and saying so.
    let seen = all_changes(&untitled, &project, 1, 100).await;
    assert_eq!(seen.len(), 2, "{seen:?}");
    for row in &seen {
        assert_eq!(row["title"], "", "{row}");
        assert_eq!(row["title_hidden"], true, "{row}");
    }
    let preview = send(&untitled, Method::GET, &preview_one, None, &[]).await;
    assert_eq!(preview.status, StatusCode::OK, "{}", preview.body);
    assert_eq!(
        preview.body["data"]["kept_down"][0]["title_hidden"], true,
        "{}",
        preview.body
    );
    let request = send(&untitled, Method::GET, &request_one, None, &[]).await;
    assert_eq!(
        request.body["data"]["items"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(request.body["data"]["items"][0]["title_hidden"], true);
}

/// The authorization matrix for both additions: every member reads a receipt's changes (as far
/// as their role reads), only a role that may change every pinned entry previews a rollback, and
/// everyone else is refused the way the rest of the API refuses them — 401 without a session or
/// with a revoked token, 404 outside the project (an API token included), 403 for a role.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(clippy::too_many_lines, reason = "one row per caller")]
async fn every_caller_gets_the_answer_its_role_allows() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let first = create_article(&owner, &project, "Harbour notes").await;
    assert_eq!(
        release(
            &owner,
            &project,
            &json!({"title": "One", "entries": [first]})
        )
        .await,
        1
    );
    let second = create_article(&owner, &project, "Ferry times").await;
    assert_eq!(
        release(
            &owner,
            &project,
            &json!({"title": "Two", "entries": [second]})
        )
        .await,
        2
    );
    new_role(
        &owner,
        &project,
        "viewer",
        &json!([{"action": "content.read", "subject": "*"}]),
    )
    .await;
    new_role(
        &owner,
        &project,
        "auditor",
        &json!([{"action": "audit.read", "subject": "*"}]),
    )
    .await;
    let changes = format!("{}/releases/1/changes", base(&project));
    let preview = format!("{}/releases/1/rollback", base(&project));

    // People in the project, by role: (changes status, rows, preview status).
    for (role, rows, previews) in [
        ("publisher", 1, StatusCode::OK),
        ("editor", 1, StatusCode::OK),
        ("developer", 1, StatusCode::OK),
        ("viewer", 1, StatusCode::FORBIDDEN),
        ("auditor", 0, StatusCode::FORBIDDEN),
    ] {
        let person = member(&owner, &project, role).await;
        let listed = send(&person, Method::GET, &changes, None, &[]).await;
        assert_eq!(listed.status, StatusCode::OK, "{role}: {}", listed.body);
        assert_eq!(
            listed.body["data"].as_array().map(Vec::len),
            Some(rows),
            "{role}: {}",
            listed.body
        );
        let previewed = send(&person, Method::GET, &preview, None, &[]).await;
        assert_eq!(previewed.status, previews, "{role}: {}", previewed.body);
        if previews != StatusCode::OK {
            assert!(
                !text(&previewed).contains("Ferry times"),
                "{role}: {}",
                previewed.body
            );
        }
    }
    let listed = send(&owner, Method::GET, &changes, None, &[]).await;
    assert_eq!(listed.body["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        send(&owner, Method::GET, &preview, None, &[]).await.status,
        StatusCode::OK
    );

    // Someone signed in who isn't a member: 404, as for any project they can't see.
    let stranger = sign_in(&format!("ws3-stranger-{}", &key()[25..])).await;
    for path in [
        &changes,
        &preview,
        &format!("{}/releases/1", base(&project)),
    ] {
        let reply = send(&stranger, Method::GET, path, None, &[]).await;
        assert_eq!(
            reply.status,
            StatusCode::NOT_FOUND,
            "{path}: {}",
            reply.body
        );
        assert!(!text(&reply).contains("Harbour notes"));
    }
    // No session at all: 401 in the error envelope.
    for path in [&changes, &preview] {
        let reply = bare(path, None).await;
        assert_eq!(
            reply.status,
            StatusCode::UNAUTHORIZED,
            "{path}: {}",
            reply.body
        );
        assert!(reply.body["error"]["code"].is_string(), "{}", reply.body);
    }

    // API tokens: read-only reads changes but may not preview a rollback; a token whose
    // permissions read no content lists nothing; a revoked one is 401; any token is 404 on
    // another project's URLs.
    let token = |body: Value| {
        let owner = &owner;
        let project = project.clone();
        async move {
            let made = send(
                owner,
                Method::POST,
                &format!("/api/v1/projects/{project}/tokens"),
                Some(&body),
                &[],
            )
            .await;
            assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
            (
                made.body["data"]["secret"]
                    .as_str()
                    .expect("a secret")
                    .to_owned(),
                made.body["data"]["id"].as_str().expect("an id").to_owned(),
            )
        }
    };
    let (reader, reader_id) =
        token(json!({"name": "Site", "kind": "read-only", "expires_in_days": 1})).await;
    let listed = bare(&changes, Some(&reader)).await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(listed.body["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        bare(&preview, Some(&reader)).await.status,
        StatusCode::FORBIDDEN
    );
    let (hooks, _) = token(json!({
        "name": "Hooks", "kind": "custom", "expires_in_days": 1,
        "permissions": [{"action": "destinations.manage", "subject": "*"}]
    }))
    .await;
    let listed = bare(&changes, Some(&hooks)).await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(listed.body["data"], json!([]), "{}", listed.body);
    assert_eq!(
        bare(&preview, Some(&hooks)).await.status,
        StatusCode::FORBIDDEN
    );
    let (writer, _) = token(json!({
        "name": "Writer", "kind": "custom", "expires_in_days": 1,
        "permissions": [
            {"action": "content.read", "subject": "*"},
            {"action": "content.update", "subject": "*"}
        ]
    }))
    .await;
    assert_eq!(bare(&preview, Some(&writer)).await.status, StatusCode::OK);
    let other = project_with_release(&owner, "Other project's piece").await;
    for path in [
        format!("{}/releases/1/changes", base(&other)),
        format!("{}/releases/1/rollback", base(&other)),
        format!("{}/releases/1", base(&other)),
    ] {
        let reply = bare(&path, Some(&writer)).await;
        assert_eq!(
            reply.status,
            StatusCode::NOT_FOUND,
            "{path}: {}",
            reply.body
        );
        assert!(!text(&reply).contains("Other project's piece"));
    }
    let revoked = send(
        &owner,
        Method::DELETE,
        &format!("/api/v1/projects/{project}/tokens/{reader_id}"),
        None,
        &[],
    )
    .await;
    assert_eq!(revoked.status, StatusCode::OK, "{}", revoked.body);
    for path in [&changes, &preview] {
        let reply = bare(path, Some(&reader)).await;
        assert_eq!(
            reply.status,
            StatusCode::UNAUTHORIZED,
            "{path}: {}",
            reply.body
        );
    }
}

async fn project_with_release(owner: &Session, title: &str) -> String {
    let project = project(owner).await;
    let entry = create_article(owner, &project, title).await;
    assert_eq!(
        release(
            owner,
            &project,
            &json!({"title": title, "entries": [entry]})
        )
        .await,
        1
    );
    project
}

/// Cross-project IDOR: a generation, a request id and a cursor from one project, asked for
/// under another project's URL by its owner, answer from that project or not at all.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn nothing_from_one_project_answers_under_another() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let mine = project_with_release(&owner, "Mine, release one").await;
    let theirs = project(&owner).await;
    let mut generation = 0;
    for title in ["Theirs one", "Theirs two", "Theirs three"] {
        let entry = create_article(&owner, &theirs, title).await;
        generation = release(
            &owner,
            &theirs,
            &json!({"title": title, "entries": [entry]}),
        )
        .await;
    }
    assert_eq!(generation, 3);
    let listed = send(
        &owner,
        Method::GET,
        &format!("{}/changesets", base(&theirs)),
        None,
        &[],
    )
    .await;
    let request = listed.body["data"][0]["id"]
        .as_str()
        .expect("a request")
        .to_owned();
    // Their generation 3 under my project: mine has only release 1.
    for path in [
        format!("{}/releases/3", base(&mine)),
        format!("{}/releases/3/changes", base(&mine)),
        format!("{}/changesets/{request}", base(&mine)),
    ] {
        let reply = send(&owner, Method::GET, &path, None, &[]).await;
        assert_eq!(
            reply.status,
            StatusCode::NOT_FOUND,
            "{path}: {}",
            reply.body
        );
    }
    // Generation 1 exists in both: under my URL it is mine.
    let rows = all_changes(&owner, &mine, 1, 100).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["title"], "Mine, release one");
    // A cursor from their list is refused under mine, not followed.
    let page = send(
        &owner,
        Method::GET,
        &format!("{}/releases/3/changes?limit=1", base(&theirs)),
        None,
        &[],
    )
    .await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    // One row, no more pages: make one from a two-row release instead.
    let a = create_article(&owner, &theirs, "Theirs four").await;
    let b = create_article(&owner, &theirs, "Theirs five").await;
    assert_eq!(
        release(
            &owner,
            &theirs,
            &json!({"title": "Pair", "entries": [a, b]})
        )
        .await,
        4
    );
    let page = send(
        &owner,
        Method::GET,
        &format!("{}/releases/4/changes?limit=1", base(&theirs)),
        None,
        &[],
    )
    .await;
    let cursor = page.body["next_cursor"]
        .as_str()
        .expect("a cursor")
        .to_owned();
    let reply = send(
        &owner,
        Method::GET,
        &format!("{}/releases/1/changes?limit=1&cursor={cursor}", base(&mine)),
        None,
        &[],
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.body);
    assert!(reply.body["error"]["code"].is_string(), "{}", reply.body);
    // And on the same project, a cursor from release 4 doesn't page release 3.
    let reply = send(
        &owner,
        Method::GET,
        &format!(
            "{}/releases/3/changes?limit=1&cursor={cursor}",
            base(&theirs)
        ),
        None,
        &[],
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.body);
}

/// R5 and the generation edge cases: a generation that isn't a number, overflows `i64`, is zero,
/// negative, unknown or the live one answers in the API's JSON error envelope — 404 for a release
/// that doesn't exist, `invalid_generation` for a rollback outside `1..latest` — never axum's
/// plain-text rejection.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one table of bad generations, four routes each"
)]
async fn a_bad_generation_answers_in_the_error_envelope() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project_with_release(&owner, "Only one").await;
    let entry = create_article(&owner, &project, "Second").await;
    assert_eq!(
        release(
            &owner,
            &project,
            &json!({"title": "Two", "entries": [entry]})
        )
        .await,
        2
    );
    let bad = [
        "abc",
        "1.5",
        "9223372036854775808",
        "-9223372036854775809",
        "9223372036854775807",
        "0",
        "-1",
        "99",
        "%20",
    ];
    for generation in bad {
        for (path, status) in [
            (
                format!("{}/releases/{generation}", base(&project)),
                StatusCode::NOT_FOUND,
            ),
            (
                format!("{}/releases/{generation}/changes", base(&project)),
                StatusCode::NOT_FOUND,
            ),
        ] {
            let reply = send(&owner, Method::GET, &path, None, &[]).await;
            assert_eq!(reply.status, status, "{path}: {}", reply.body);
            assert_eq!(
                reply.body["error"]["code"], "not_found",
                "{path}: {}",
                reply.body
            );
            assert!(
                reply.headers[header::CONTENT_TYPE]
                    .to_str()
                    .unwrap_or_default()
                    .starts_with("application/json"),
                "{path}"
            );
        }
        // The live release (2) is refused like every other generation outside 1..latest.
        let rollback = format!("{}/releases/{generation}/rollback", base(&project));
        let previewed = send(&owner, Method::GET, &rollback, None, &[]).await;
        assert_eq!(
            previewed.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{rollback}: {}",
            previewed.body
        );
        assert_eq!(
            previewed.body["error"]["details"]["reason"], "invalid_generation",
            "{rollback}: {}",
            previewed.body
        );
        let made = send(
            &owner,
            Method::POST,
            &rollback,
            Some(&json!({"title": "Back"})),
            &[("idempotency-key", &key())],
        )
        .await;
        assert_eq!(
            made.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{rollback}: {}",
            made.body
        );
        assert_eq!(
            made.body["error"]["details"]["reason"],
            "invalid_generation"
        );
    }
    let live = format!("{}/releases/2/rollback", base(&project));
    let previewed = send(&owner, Method::GET, &live, None, &[]).await;
    assert_eq!(
        previewed.body["error"]["details"]["reason"],
        "invalid_generation"
    );
    // A limit outside 1..=100 is refused, not clamped silently.
    for limit in ["0", "101", "-1", "x"] {
        let path = format!("{}/releases/1/changes?limit={limit}", base(&project));
        let reply = send(&owner, Method::GET, &path, None, &[]).await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{path}: {}",
            reply.body
        );
        assert!(
            reply.body["error"]["code"].is_string(),
            "{path}: {}",
            reply.body
        );
    }
}

/// A preview is a promise about what confirming makes. Someone publishing while the dialog is
/// open moves the pins, so the request the confirm sends — `If-Match` with the preview's hash —
/// is refused as stale (409, like any stale changeset) and nothing is made; after a fresh look it is made, with exactly
/// the previewed pins. An entry published since release 1 is among what it takes down, and one
/// deleted since stays down with `entry_deleted`.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_stale_preview_never_becomes_a_request() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project_with_release(&owner, "First").await;
    let second = create_article(&owner, &project, "Second").await;
    assert_eq!(
        release(
            &owner,
            &project,
            &json!({"title": "Two", "entries": [second]})
        )
        .await,
        2
    );
    let rollback = format!("{}/releases/1/rollback", base(&project));
    let looked = send(&owner, Method::GET, &rollback, None, &[]).await;
    assert_eq!(looked.status, StatusCode::OK, "{}", looked.body);
    let seen = looked.etag().expect("a preview names its pins' hash");
    assert_eq!(
        looked.body["data"]["hash"]
            .as_str()
            .map(|hash| format!("\"{hash}\"")),
        Some(seen.clone())
    );
    assert_eq!(looked.body["data"]["unpublish_total"], 1);

    // Someone else publishes a third article while the dialog is open.
    let third = create_article(&owner, &project, "Third").await;
    assert_eq!(
        release(
            &owner,
            &project,
            &json!({"title": "Three", "entries": [third]})
        )
        .await,
        3
    );
    let requests = |reply: &Reply| reply.body["data"].as_array().map(Vec::len);
    let before = send(
        &owner,
        Method::GET,
        &format!("{}/changesets", base(&project)),
        None,
        &[],
    )
    .await;
    let stale = send(
        &owner,
        Method::POST,
        &rollback,
        Some(&json!({"title": "Back to one"})),
        &[("idempotency-key", &key()), ("if-match", &seen)],
    )
    .await;
    assert_eq!(stale.status, StatusCode::CONFLICT, "{}", stale.body);
    let after = send(
        &owner,
        Method::GET,
        &format!("{}/changesets", base(&project)),
        None,
        &[],
    )
    .await;
    assert_eq!(
        requests(&after),
        requests(&before),
        "a stale confirm made nothing"
    );

    // A fresh look takes both down, and the confirm with its hash makes exactly that.
    let fresh = send(&owner, Method::GET, &rollback, None, &[]).await;
    assert_eq!(fresh.body["data"]["unpublish_total"], 2, "{}", fresh.body);
    let made = send(
        &owner,
        Method::POST,
        &rollback,
        Some(&json!({"title": "Back to one"})),
        &[
            ("idempotency-key", &key()),
            ("if-match", &fresh.etag().expect("a hash")),
        ],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    assert_eq!(made.body["data"]["hash"], fresh.body["data"]["hash"]);
    let mut pinned = ids(made.body["data"]["items"].as_array().expect("items"));
    pinned.sort();
    let mut expected = vec![second.clone(), third.clone()];
    expected.sort();
    assert_eq!(pinned, expected);
    // A garbled If-Match is a 400, not a pass.
    let garbled = send(
        &owner,
        Method::POST,
        &rollback,
        Some(&json!({"title": "Back to one"})),
        &[("idempotency-key", &key()), ("if-match", "nonsense")],
    )
    .await;
    assert_eq!(garbled.status, StatusCode::BAD_REQUEST, "{}", garbled.body);
}

/// R2, R3 and the bounds: a rollback that takes 5,000 entries down. Its preview lists at most 100
/// with the totals; its request and release are made; the release's changes come back 100 a page
/// by cursor, every one exactly once, past the old 500 cut — publishes and takedowns alike — and
/// each page and the preview answer within budget on this machine (p99 printed for the report).
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh; makes 5,000 entries"]
#[expect(clippy::too_many_lines, reason = "set up 5,000 entries, then measure")]
async fn a_rollback_of_five_thousand_entries_stays_bounded_and_pages_completely() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    // Release 1: one article at version one, which release 2 replaces — so the rollback both
    // publishes (version one again) and takes down (the 5,000 since).
    let anchor = create_article(&owner, &project, "Anchor").await;
    assert_eq!(
        release(
            &owner,
            &project,
            &json!({"title": "One", "entries": [anchor]})
        )
        .await,
        1
    );
    let path = format!("{}/{anchor}", entries(&project));
    let current = send(&owner, Method::GET, &path, None, &[]).await;
    let saved = send(
        &owner,
        Method::PUT,
        &path,
        Some(&article("Anchor", "version two")),
        &[
            ("if-match", &current.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    assert_eq!(
        release(
            &owner,
            &project,
            &json!({"title": "Two", "entries": [anchor]})
        )
        .await,
        2
    );

    // 5,000 articles, imported 500 a file, published 100 a release (releases 3 to 52).
    let import = format!("{}/collections/articles/import", base(&project));
    for batch in 0..10 {
        let file: Vec<String> = (0..500)
            .map(|index| json!({"title": format!("Bulk {batch:02}-{index:03}")}).to_string())
            .collect();
        let response = app()
            .post(format!("{ORIGIN}{import}"))
            .header(header::COOKIE, &owner.cookie)
            .header("x-csrf-token", &owner.csrf)
            .header(header::ORIGIN, ORIGIN)
            .header(header::CONTENT_TYPE, "application/jsonlines")
            .header("idempotency-key", key())
            .body(file.join("\n"))
            .send()
            .await
            .expect("import");
        assert_eq!(
            response.status(),
            StatusCode::CREATED,
            "{:?}",
            response.text().await
        );
    }
    let mut bulk = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let path = match &cursor {
            Some(at) => format!("{}?limit=100&cursor={at}", entries(&project)),
            None => format!("{}?limit=100", entries(&project)),
        };
        let page = send(&owner, Method::GET, &path, None, &[]).await;
        for row in page.body["data"].as_array().expect("a page") {
            let id = row["id"].as_str().expect("an id");
            if id != anchor {
                bulk.push(id.to_owned());
            }
        }
        match page.body["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    assert_eq!(bulk.len(), 5_000);
    let mut latest = 2;
    for chunk in bulk.chunks(100) {
        latest = release(
            &owner,
            &project,
            &json!({"title": "Bulk", "entries": chunk}),
        )
        .await;
    }
    assert_eq!(latest, 52);

    // The preview: at most 100 lines, with the totals, measured.
    let rollback = format!("{}/releases/1/rollback", base(&project));
    let mut timings = Vec::new();
    let mut preview = Value::Null;
    for _ in 0..50 {
        let started = Instant::now();
        let reply = send(&owner, Method::GET, &rollback, None, &[]).await;
        timings.push(started.elapsed());
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        preview = reply.body;
    }
    let data = &preview["data"];
    assert_eq!(data["publish_total"], 1, "{data}");
    assert_eq!(data["unpublish_total"], 5_000);
    assert_eq!(data["restore"].as_array().map(Vec::len), Some(100));
    assert_eq!(
        data["restore"][0]["kind"], "publish",
        "publishes come first"
    );
    report("rollback preview, 5,001 pins", &mut timings);

    // The request carries all 5,001 (under the 10,000 cap) and publishes as release 53.
    let made = send(
        &owner,
        Method::POST,
        &rollback,
        Some(&json!({"title": "Back to one"})),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED);
    assert_eq!(
        made.body["data"]["items"].as_array().map(Vec::len),
        Some(5_001)
    );
    let request = made.body["data"]["id"].as_str().expect("an id").to_owned();
    let hash = made.body["data"]["hash"]
        .as_str()
        .expect("a hash")
        .to_owned();
    let changeset = format!("{}/changesets/{request}", base(&project));
    let if_match = format!("\"{hash}\"");
    for (step, body) in [
        ("submit", None),
        ("reviews", Some(json!({"decision": "approve"}))),
    ] {
        let reply = send(
            &owner,
            Method::POST,
            &format!("{changeset}/{step}"),
            body.as_ref(),
            &[("if-match", &if_match)],
        )
        .await;
        assert!(reply.status.is_success(), "{step}: {}", reply.body);
    }
    let published = send(
        &owner,
        Method::POST,
        &format!("{changeset}/publish"),
        Some(&json!({"base_generation": 52})),
        &[("if-match", &if_match), ("idempotency-key", &key())],
    )
    .await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);
    assert_eq!(published.body["data"]["generation"], 53);

    // Every change exactly once, 100 a page, past the old cut at 500.
    let rows = all_changes(&owner, &project, 53, 100).await;
    assert_eq!(rows.len(), 5_001);
    let mut seen = ids(&rows);
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 5_001, "no entry twice");
    assert_eq!(rows[0]["kind"], "publish");
    assert_eq!(rows[0]["entry_id"], anchor.as_str());
    assert_eq!(
        rows.iter().filter(|row| row["kind"] == "unpublish").count(),
        5_000,
        "the takedowns past 500 are all there"
    );
    let receipt = send(
        &owner,
        Method::GET,
        &format!("{}/releases/53", base(&project)),
        None,
        &[],
    )
    .await;
    assert_eq!(receipt.body["data"]["unpublished"], 5_000);
    assert!(
        receipt.body["data"].get("changes").is_none(),
        "the receipt pages its changes elsewhere"
    );

    // Page timings: first, middle and last pages, measured.
    let mut cursors = vec![None];
    let mut cursor: Option<String> = None;
    for _ in 0..50 {
        let path = match &cursor {
            Some(at) => format!(
                "{}/releases/53/changes?limit=100&cursor={at}",
                base(&project)
            ),
            None => format!("{}/releases/53/changes?limit=100", base(&project)),
        };
        let page = send(&owner, Method::GET, &path, None, &[]).await;
        cursor = page.body["next_cursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
        cursors.push(cursor.clone());
    }
    let mut timings = Vec::new();
    for round in 0..4 {
        for at in &cursors {
            let path = match at {
                Some(at) => format!(
                    "{}/releases/53/changes?limit=100&cursor={at}",
                    base(&project)
                ),
                None => format!("{}/releases/53/changes?limit=100", base(&project)),
            };
            let started = Instant::now();
            let page = send(&owner, Method::GET, &path, None, &[]).await;
            timings.push(started.elapsed());
            assert_eq!(page.status, StatusCode::OK, "round {round}: {}", page.body);
        }
    }
    report("release changes page of 100, 5,001 changes", &mut timings);
}

/// Prints n, p50, p99 and max, and fails past a generous budget: these are laptop observations
/// for the report, not capacity, and a busy machine shouldn't fail the suite — only an answer
/// that grew with the release (an unbounded read) would.
fn report(what: &str, timings: &mut [Duration]) {
    timings.sort();
    let at = |percent: usize| timings[(timings.len() - 1) * percent / 100];
    let (p50, p99, max) = (at(50), at(99), timings[timings.len() - 1]);
    println!(
        "TIMING {what}: n={} p50={:.1}ms p99={:.1}ms max={:.1}ms",
        timings.len(),
        p50.as_secs_f64() * 1000.0,
        p99.as_secs_f64() * 1000.0,
        max.as_secs_f64() * 1000.0
    );
    assert!(p99 < Duration::from_secs(2), "{what}: p99 {p99:?}");
}
