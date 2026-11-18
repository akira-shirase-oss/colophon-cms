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
    ORIGIN, Reply, Server, Session, app, call, create_article, join, key, project, release, sign_in,
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

/// The longest collection slug (63) and the longest title in the longest UTF-8 (200 four-byte
/// characters): the worst-case item the rollback cap is sized from (D-185).
const WIDE: &str = "wwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwww";

fn wide_title(index: usize) -> String {
    // 200 characters: a 4-digit number, then 196 four-byte emoji.
    format!("{index:04}{}", "\u{1F600}".repeat(196))
}

/// Imports `count` worst-case entries into the wide collection, 500 a file; their ids.
async fn import_wide(owner: &Session, project: &str, from: usize, count: usize) -> Vec<String> {
    let collection = format!("{}/collections/{WIDE}", base(project));
    let mut at = from;
    while at < from + count {
        let batch = (from + count - at).min(500);
        let file: Vec<String> = (at..at + batch)
            .map(|index| json!({"title": wide_title(index)}).to_string())
            .collect();
        let response = app()
            .post(format!("{ORIGIN}{collection}/import"))
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
        at += batch;
    }
    let mut ids = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let path = match &cursor {
            Some(next) => format!("{collection}/entries?limit=100&cursor={next}"),
            None => format!("{collection}/entries?limit=100"),
        };
        let page = send(owner, Method::GET, &path, None, &[]).await;
        for row in page.body["data"].as_array().expect("a page") {
            ids.push(row["id"].as_str().expect("an id").to_owned());
        }
        match page.body["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    ids.sort();
    ids
}

/// R2, R3, A2 and the bounds (D-185): a rollback is capped at 190 entries, the most whose answer
/// fits the idempotency record with every field at its longest. Worst-case entries (the longest
/// collection slug and title): at 191 the preview says `too_large` with the totals and `POST`
/// refuses `rollback_too_large`, making nothing — never the 500 it gave before; at 190 it is
/// made, recorded (a replay answers it) and published, and that release's 190 changes page by
/// cursor, each exactly once. Then a rollback 5,000 entries wide still previews in bounded time.
/// p50/p99 are printed for the report.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh; makes 5,000 entries"]
#[expect(
    clippy::too_many_lines,
    reason = "set up, the cap from both sides, then 5,000"
)]
async fn a_rollback_is_capped_where_its_answer_fits_and_its_changes_page_completely() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let made = send(
        &owner,
        Method::POST,
        &format!("{}/content-types", base(&project)),
        Some(&json!({"slug": WIDE, "definition": {
            "name": "Wide", "title_field": "title",
            "fields": [{"id": "title", "label": "Title", "type": "text", "required": true}]
        }})),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
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
    let wide = import_wide(&owner, &project, 0, 191).await;
    assert_eq!(wide.len(), 191);
    for chunk in wide.chunks(100) {
        release(
            &owner,
            &project,
            &json!({"title": "Wide", "entries": chunk}),
        )
        .await;
    }
    let rollback = format!("{}/releases/1/rollback", base(&project));

    // 191: too large. The preview says so, with the totals; POST refuses by name; nothing made.
    let preview = send(&owner, Method::GET, &rollback, None, &[]).await;
    assert_eq!(preview.status, StatusCode::OK, "{}", preview.body);
    let data = &preview.body["data"];
    assert_eq!(data["too_large"], true, "{data}");
    assert_eq!(data["max_items"], 190);
    assert_eq!(data["unpublish_total"], 191);
    assert_eq!(data["hash"], Value::Null);
    assert!(preview.etag().is_none());
    // Nothing to name: it can't be made, and reading that many pins would cost the live set.
    assert_eq!(data["restore"], json!([]));
    let before = send(
        &owner,
        Method::GET,
        &format!("{}/changesets", base(&project)),
        None,
        &[],
    )
    .await;
    let refused = send(
        &owner,
        Method::POST,
        &rollback,
        Some(&json!({"title": "Back to one"})),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"]["reason"],
        "rollback_too_large"
    );
    let after = send(
        &owner,
        Method::GET,
        &format!("{}/changesets", base(&project)),
        None,
        &[],
    )
    .await;
    assert_eq!(
        after.body["data"].as_array().map(Vec::len),
        before.body["data"].as_array().map(Vec::len)
    );

    // Take one down: 190, exactly the cap. Made, recorded, replayed, published.
    assert_eq!(
        release(
            &owner,
            &project,
            &json!({"title": "One down", "unpublish": [wide[0]]})
        )
        .await,
        4
    );
    let preview = send(&owner, Method::GET, &rollback, None, &[]).await;
    assert_eq!(preview.body["data"]["too_large"], false, "{}", preview.body);
    assert_eq!(preview.body["data"]["unpublish_total"], 190);
    let seen = preview.etag().expect("a hash");
    let once = key();
    let made = send(
        &owner,
        Method::POST,
        &rollback,
        Some(&json!({"title": "Back to one"})),
        &[("idempotency-key", &once), ("if-match", &seen)],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    assert_eq!(
        made.body["data"]["items"].as_array().map(Vec::len),
        Some(190)
    );
    let again = send(
        &owner,
        Method::POST,
        &rollback,
        Some(&json!({"title": "Back to one"})),
        &[("idempotency-key", &once), ("if-match", &seen)],
    )
    .await;
    assert_eq!(again.status, StatusCode::CREATED, "{}", again.body);
    assert!(again.replayed(), "the 190-item answer was recorded");
    assert_eq!(again.body["data"]["id"], made.body["data"]["id"]);
    let request = made.body["data"]["id"].as_str().expect("an id").to_owned();
    let if_match = format!(
        "\"{}\"",
        made.body["data"]["hash"].as_str().expect("a hash")
    );
    let changeset = format!("{}/changesets/{request}", base(&project));
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
        Some(&json!({"base_generation": 4})),
        &[("if-match", &if_match), ("idempotency-key", &key())],
    )
    .await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);
    assert_eq!(published.body["data"]["generation"], 5);

    // Its 190 changes, 100 a page and 7 a page: each exactly once, the last page short.
    for limit in [100, 7] {
        let rows = all_changes(&owner, &project, 5, limit).await;
        let mut ids = ids(&rows);
        assert_eq!(ids.len(), 190, "limit {limit}");
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 190, "limit {limit}: no entry twice");
    }
    let first = send(
        &owner,
        Method::GET,
        &format!("{}/releases/5/changes?limit=100", base(&project)),
        None,
        &[],
    )
    .await;
    assert_eq!(first.body["data"].as_array().map(Vec::len), Some(100));
    let next = first.body["next_cursor"]
        .as_str()
        .expect("a second page")
        .to_owned();
    let last = send(
        &owner,
        Method::GET,
        &format!(
            "{}/releases/5/changes?limit=100&cursor={next}",
            base(&project)
        ),
        None,
        &[],
    )
    .await;
    assert_eq!(last.body["data"].as_array().map(Vec::len), Some(90));
    assert_eq!(last.body["next_cursor"], Value::Null);

    // 5,000 wide: the preview stays bounded (191 pins read, the rest counted), measured.
    let more = import_wide(&owner, &project, 191, 4_810).await;
    let fresh: Vec<&String> = more.iter().filter(|id| !wide.contains(id)).collect();
    assert_eq!(fresh.len(), 4_810);
    for chunk in fresh.chunks(100) {
        release(
            &owner,
            &project,
            &json!({"title": "More", "entries": chunk}),
        )
        .await;
    }
    // Five unmeasured first: the 49 releases just made still have delivery jobs and hints in
    // flight on this server, and the first reads warm its caches. Said in the report.
    for _ in 0..5 {
        send(&owner, Method::GET, &rollback, None, &[]).await;
    }
    let mut timings = Vec::new();
    for _ in 0..100 {
        let started = Instant::now();
        let reply = send(&owner, Method::GET, &rollback, None, &[]).await;
        timings.push(started.elapsed());
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        assert_eq!(reply.body["data"]["too_large"], true);
        assert_eq!(
            reply.body["data"]["unpublish_total"], 4_810,
            "{}",
            reply.body
        );
    }
    report(
        "rollback preview, 4,810 pins wide (too large)",
        &mut timings,
    );
    let cursor = send(
        &owner,
        Method::GET,
        &format!("{}/releases/5/changes?limit=100", base(&project)),
        None,
        &[],
    )
    .await;
    let next = cursor.body["next_cursor"]
        .as_str()
        .expect("a cursor")
        .to_owned();
    let mut timings = Vec::new();
    for round in 0..100 {
        let path = if round % 2 == 0 {
            format!("{}/releases/5/changes?limit=100", base(&project))
        } else {
            format!(
                "{}/releases/5/changes?limit=100&cursor={next}",
                base(&project)
            )
        };
        let started = Instant::now();
        let page = send(&owner, Method::GET, &path, None, &[]).await;
        timings.push(started.elapsed());
        assert_eq!(page.status, StatusCode::OK);
    }
    report(
        "release changes page of 100 (190-change release)",
        &mut timings,
    );
}

/// Prints n, p50, p99, max and the slowest three, and fails only on the median: these run on a
/// debug build beside the worker still delivering the releases just made, so the tail measures
/// the laptop; a read that grew with the release would move the median. The release-build p99
/// is measured separately (D-185's evidence).
fn report(what: &str, timings: &mut [Duration]) {
    timings.sort();
    let at = |percent: usize| timings[(timings.len() - 1) * percent / 100];
    let (p50, p99, max) = (at(50), at(99), timings[timings.len() - 1]);
    let slowest: Vec<String> = timings
        .iter()
        .rev()
        .take(3)
        .map(|took| format!("{:.0}ms", took.as_secs_f64() * 1000.0))
        .collect();
    println!("TIMING {what}: slowest {}", slowest.join(", "));
    println!(
        "TIMING {what}: n={} p50={:.1}ms p99={:.1}ms max={:.1}ms",
        timings.len(),
        p50.as_secs_f64() * 1000.0,
        p99.as_secs_f64() * 1000.0,
        max.as_secs_f64() * 1000.0
    );
    assert!(
        p50 < Duration::from_millis(500),
        "{what}: p50 {p50:?} (p99 {p99:?})"
    );
}

/// R6 (D-185): a reviewer approves exactly what the request publishes, so a role that holds
/// `content.review` but can't read an item — an own-only reader on someone else's article, or a
/// role that reads only some fields — can't approve it (403 `cannot_review_hidden`); it can still
/// ask for changes. A reviewer who reads everything approves as before. Before the fix both
/// approvals were accepted: an approval of versions the request page hid from them.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "three roles, each one approval and one request for changes"
)]
async fn a_reviewer_cannot_approve_what_their_role_cannot_read() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    new_role(
        &owner,
        &project,
        "own-reviewer",
        &json!([
            {"action": "content.read", "subject": "articles", "condition": "own"},
            {"action": "content.review", "subject": "articles"}
        ]),
    )
    .await;
    new_role(
        &owner,
        &project,
        "title-reviewer",
        &json!([
            {"action": "content.read", "subject": "articles", "fields": ["title"]},
            {"action": "content.review", "subject": "articles"}
        ]),
    )
    .await;
    new_role(
        &owner,
        &project,
        "full-reviewer",
        &json!([
            {"action": "content.read", "subject": "articles"},
            {"action": "content.review", "subject": "articles"}
        ]),
    )
    .await;
    let entry = create_article(&owner, &project, "Owner's piece").await;
    let made = send(
        &owner,
        Method::POST,
        &format!("{}/changesets", base(&project)),
        Some(&json!({"title": "To review", "entries": [entry]})),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let path = format!(
        "{}/changesets/{}",
        base(&project),
        made.body["data"]["id"].as_str().expect("an id")
    );
    let hash = format!(
        "\"{}\"",
        made.body["data"]["hash"].as_str().expect("a hash")
    );
    let submitted = send(
        &owner,
        Method::POST,
        &format!("{path}/submit"),
        None,
        &[("if-match", &hash)],
    )
    .await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);
    for role in ["own-reviewer", "title-reviewer"] {
        let reviewer = member(&owner, &project, role).await;
        let approve = send(
            &reviewer,
            Method::POST,
            &format!("{path}/reviews"),
            Some(&json!({"decision": "approve"})),
            &[("if-match", &hash)],
        )
        .await;
        assert_eq!(
            approve.status,
            StatusCode::FORBIDDEN,
            "{role}: {}",
            approve.body
        );
        assert_eq!(
            approve.body["error"]["details"]["reason"], "cannot_review_hidden",
            "{role}"
        );
        let changes = send(
            &reviewer,
            Method::POST,
            &format!("{path}/reviews"),
            Some(&json!({"decision": "request_changes", "comment": "Can't see it all"})),
            &[("if-match", &hash)],
        )
        .await;
        assert_eq!(changes.status, StatusCode::OK, "{role}: {}", changes.body);
    }
    let full = member(&owner, &project, "full-reviewer").await;
    let approve = send(
        &full,
        Method::POST,
        &format!("{path}/reviews"),
        Some(&json!({"decision": "approve"})),
        &[("if-match", &hash)],
    )
    .await;
    assert_eq!(approve.status, StatusCode::OK, "{}", approve.body);
    assert_eq!(
        approve.body["data"]["status"]["state"], "approved",
        "{}",
        approve.body
    );
}
