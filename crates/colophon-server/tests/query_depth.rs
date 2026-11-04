//! Reference population over real HTTP (DX.5, D-107): `depth` in the query document
//! expands each row's top-level single-reference fields into bounded objects naming
//! their entries — current drafts on `POST …/entries/query`, narrowed to the caller's
//! `content.read` on the target's collection; publications pinned at the page's
//! generation on `POST /content/v1/…/query`. `depth: 0` keeps the stored ids; a target
//! that doesn't resolve — unpublished or revoked on the public side, unreadable by the
//! role on the draft side — keeps its stored id, and a cycle always ends at the bound.
//! Needs the stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{ORIGIN, Reply, Server, Session, app, call, join, key, project, release, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
}

/// A request that may write: content types, entries, roles carry an idempotency key.
async fn send(session: &Session, method: Method, path: &str, body: Option<&Value>) -> Reply {
    call(session, method, path, body, &[("idempotency-key", &key())])
        .await
        .expect("request")
}

/// `POST …/entries/query` — the draft perspective — with `document`, and `?cursor=`
/// when a page boundary was handed back.
async fn ask(
    session: &Session,
    project: &str,
    collection: &str,
    document: &Value,
    cursor: Option<&str>,
) -> Reply {
    let mut path = format!("{}/collections/{collection}/entries/query", base(project));
    if let Some(cursor) = cursor {
        path.push_str("?cursor=");
        path.push_str(cursor);
    }
    send(session, Method::POST, &path, Some(document)).await
}

/// `POST /content/v1/{project}/production/{collection}/query` — the published
/// perspective — anonymously, `?cursor=` and `?generation=` as named.
async fn ask_published(
    project: &str,
    collection: &str,
    document: &Value,
    cursor: Option<&str>,
    generation: Option<i64>,
) -> Reply {
    let mut parameters: Vec<String> = Vec::new();
    if let Some(cursor) = cursor {
        parameters.push(format!("cursor={cursor}"));
    }
    if let Some(generation) = generation {
        parameters.push(format!("generation={generation}"));
    }
    let mut path = format!("/content/v1/{project}/production/{collection}/query");
    if !parameters.is_empty() {
        path.push('?');
        path.push_str(&parameters.join("&"));
    }
    let response = app()
        .post(format!("{ORIGIN}{path}"))
        .json(document)
        .send()
        .await
        .expect("request");
    Reply {
        status: response.status(),
        headers: response.headers().clone(),
        body: response.json().await.unwrap_or(Value::Null),
    }
}

/// `staff`: `name` is the title; `mentor` a single reference to its own collection —
/// the field a cycle of two entries makes.
fn staff() -> Value {
    json!({
        "name": "Staff",
        "title_field": "name",
        "fields": [
            {"id": "name", "label": "Name", "type": "text", "required": true, "index": true},
            {"id": "bio", "label": "Bio", "type": "text", "multiline": true},
            {"id": "mentor", "label": "Mentor", "type": "reference", "to": "staff"}
        ]
    })
}

/// `posts`: `headline` the title, `author` a single reference to `staff`, `related` a
/// `many` reference to its own collection — the spelling that never expands.
fn posts() -> Value {
    json!({
        "name": "Posts",
        "title_field": "headline",
        "fields": [
            {"id": "headline", "label": "Headline", "type": "text", "required": true, "index": true},
            {"id": "author", "label": "Author", "type": "reference", "to": "staff"},
            {"id": "related", "label": "Related", "type": "reference", "to": "posts", "many": true}
        ]
    })
}

/// One entry of `collection`, created by `session`; its id.
async fn entry(session: &Session, project: &str, collection: &str, content: &Value) -> String {
    let made = send(
        session,
        Method::POST,
        &format!("{}/collections/{collection}/entries", base(project)),
        Some(content),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    made.body["data"]["id"].as_str().expect("an id").to_owned()
}

/// Overwrites `entry`'s content (`If-Match` like any save).
async fn save(session: &Session, project: &str, collection: &str, entry: &str, content: &Value) {
    let path = format!("{}/collections/{collection}/entries/{entry}", base(project));
    let read = send(session, Method::GET, &path, None).await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    let saved = call(
        session,
        Method::PUT,
        &path,
        Some(content),
        &[
            ("if-match", &read.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request");
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
}

/// Replaces `role`'s permissions — `If-Match` like any save — so one member's read can
/// widen mid-test (a member holds one role).
async fn set_permissions(owner: &Session, project: &str, role: &str, permissions: Value) {
    let path = format!("/api/v1/projects/{project}/roles/{role}");
    let read = send(owner, Method::GET, &path, None).await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    let saved = call(
        owner,
        Method::PUT,
        &path,
        Some(&json!({"name": "Stringer", "permissions": permissions})),
        &[
            ("if-match", &read.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request");
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
}

/// The project and its seeded graph, as `owner`: `staff` Ada ↔ Ben (each the other's
/// mentor — a cycle) and Carol (none); `posts` Alpha by Ada, Beta by Ben, Gamma by
/// Ada and related to Alpha. Ids come back `(ada, ben, carol, alpha, beta, gamma)`.
async fn seed(owner: &Session) -> (String, [String; 6]) {
    let project = project(owner).await;
    for (slug, definition) in [("staff", staff()), ("posts", posts())] {
        let made = send(
            owner,
            Method::POST,
            &format!("{}/content-types", base(&project)),
            Some(&json!({"slug": slug, "definition": definition})),
        )
        .await;
        assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    }
    let ada = entry(
        owner,
        &project,
        "staff",
        &json!({"name": "Ada", "bio": "first"}),
    )
    .await;
    let ben = entry(
        owner,
        &project,
        "staff",
        &json!({"name": "Ben", "bio": "second", "mentor": ada}),
    )
    .await;
    save(
        owner,
        &project,
        "staff",
        &ada,
        &json!({"name": "Ada", "bio": "first", "mentor": ben}),
    )
    .await;
    let carol = entry(owner, &project, "staff", &json!({"name": "Carol"})).await;
    let alpha = entry(
        owner,
        &project,
        "posts",
        &json!({"headline": "Alpha", "author": ada}),
    )
    .await;
    let beta = entry(
        owner,
        &project,
        "posts",
        &json!({"headline": "Beta", "author": ben, "related": [alpha]}),
    )
    .await;
    let gamma = entry(
        owner,
        &project,
        "posts",
        &json!({"headline": "Gamma", "author": ada}),
    )
    .await;
    (project, [ada, ben, carol, alpha, beta, gamma])
}

/// The page's rows, headline order the answer gave.
fn rows(reply: &Reply) -> &[Value] {
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    reply.body["data"].as_array().expect("a page")
}

/// The row whose `content.headline` is `headline`, of a page sorted on it.
fn row<'a>(page: &'a [Value], headline: &str) -> &'a Value {
    page.iter()
        .find(|entry| entry["content"]["headline"] == headline)
        .expect("the row")
}

/// What an expanded reference shows of `staff` Ada at the level it resolved: her own
/// revision id asserts the read happened; `mentor` still her stored id.
fn ada_object(ada: &Value, ben: &str) {
    assert_eq!(ada["collection"], json!("staff"));
    assert!(ada["revision_id"].is_string(), "{ada}");
    assert_eq!(ada["title"], json!("Ada"));
    assert_eq!(ada["content"]["name"], json!("Ada"));
    assert_eq!(ada["content"]["bio"], json!("first"));
    assert_eq!(ada["content"]["mentor"], json!(ben));
    assert!(ada["hidden_fields"].is_null(), "{ada}");
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn depth_zero_keeps_ids_and_depth_one_names_their_entries() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, [ada, ben, _, alpha, _beta, _]) = seed(&owner).await;

    // `depth` 0 — or absent — is the stored spelling: the id, a `many`'s ids.
    for document in [
        json!({"sort": [{"field": "headline"}]}),
        json!({"sort": [{"field": "headline"}], "depth": 0}),
    ] {
        let reply = ask(&owner, &project, "posts", &document, None).await;
        let page = rows(&reply);
        assert_eq!(row(page, "Alpha")["content"]["author"], json!(ada));
        assert_eq!(row(page, "Beta")["content"]["related"], json!([alpha]));
    }

    // `depth` 1: each row's `author` is the object naming its entry.
    let reply = ask(
        &owner,
        &project,
        "posts",
        &json!({"sort": [{"field": "headline"}], "depth": 1}),
        None,
    )
    .await;
    let page = rows(&reply);
    let expanded = row(page, "Alpha")["content"]["author"].clone();
    assert_eq!(expanded["id"], json!(ada));
    ada_object(&expanded, &ben);
    // A `many` reference keeps its stored list of ids even under `depth`.
    assert_eq!(row(page, "Beta")["content"]["related"], json!([alpha]));
    assert_eq!(row(page, "Beta")["content"]["author"]["id"], json!(ben));
    assert_eq!(
        row(page, "Beta")["content"]["author"]["content"]["name"],
        json!("Ben")
    );
    // The applied echo is the same canonical document — `depth` is in it, so the
    // hashes differ only by it.
    assert_eq!(reply.body["applied"]["query"]["depth"], json!(1));
    let zero = ask(
        &owner,
        &project,
        "posts",
        &json!({"sort": [{"field": "headline"}], "depth": 0}),
        None,
    )
    .await;
    assert_ne!(
        reply.body["applied"]["query_hash"],
        zero.body["applied"]["query_hash"]
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn depth_two_expands_one_more_level_and_a_cycle_ends_at_the_bound() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, [ada, ben, _, _, _, _]) = seed(&owner).await;

    let reply = ask(
        &owner,
        &project,
        "posts",
        &json!({"filters": [{"field": "headline", "op": "eq", "value": "Alpha"}], "depth": 2}),
        None,
    )
    .await;
    let alpha = rows(&reply)[0].clone();
    let author = &alpha["content"]["author"];
    assert_eq!(author["id"], json!(ada));
    // Level 2: Ada's own `mentor` expands into Ben; at the bound his `mentor` — the
    // cycle back to Ada — stays her stored id, not a third object.
    let mentor = &author["content"]["mentor"];
    assert_eq!(mentor["id"], json!(ben));
    assert_eq!(mentor["collection"], json!("staff"));
    assert_eq!(mentor["title"], json!("Ben"));
    assert_eq!(mentor["content"]["mentor"], json!(ada));

    // One level less: the same cycle shows Ada's `mentor` as Ben's stored id.
    let reply = ask(
        &owner,
        &project,
        "posts",
        &json!({"filters": [{"field": "headline", "op": "eq", "value": "Alpha"}], "depth": 1}),
        None,
    )
    .await;
    let alpha = rows(&reply)[0].clone();
    let author = &alpha["content"]["author"];
    assert_eq!(author["id"], json!(ada));
    assert_eq!(author["content"]["mentor"], json!(ben));
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn expansion_is_page_local_and_the_cursor_still_names_the_query() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, [ada, ben, _, _, _, _]) = seed(&owner).await;

    // Three pages of one post each: each expands only its own row's `author`.
    let document = json!({"sort": [{"field": "headline"}], "limit": 1, "depth": 1});
    let mut cursor: Option<String> = None;
    for (headline, author_id) in [("Alpha", &ada), ("Beta", &ben), ("Gamma", &ada)] {
        let reply = ask(&owner, &project, "posts", &document, cursor.as_deref()).await;
        let page = rows(&reply);
        assert_eq!(page.len(), 1, "{headline}: {}", reply.body);
        assert_eq!(page[0]["content"]["headline"], json!(headline));
        assert_eq!(page[0]["content"]["author"]["id"], json!(author_id));
        cursor = reply.body["next_cursor"].as_str().map(str::to_owned);
    }
    assert_eq!(cursor, None, "three posts paged once each");

    // `depth` is inside the canonical query the hash binds: a cursor from the depth-1
    // walk is refused verbatim by the same document at depth 0.
    let first = ask(&owner, &project, "posts", &document, None).await;
    let boundary = first.body["next_cursor"].as_str().expect("a boundary");
    let refused = ask(
        &owner,
        &project,
        "posts",
        &json!({"sort": [{"field": "headline"}], "limit": 1, "depth": 0}),
        Some(boundary),
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
        json!("foreign_cursor")
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_target_the_role_may_not_read_keeps_its_stored_id() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let guest = sign_in("dev-editor").await;
    let (project, [_ada, _, _, _, _, _]) = seed(&owner).await;

    // `stringer` reads posts but nothing of staff: the role may run the query, yet a
    // reference into staff resolves to nothing — the stored id, nothing shown.
    let made = send(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/roles"),
        Some(&json!({
            "key": "stringer",
            "name": "Stringer",
            "permissions": [
                {"action": "content.read", "subject": "posts"},
                {"action": "content.create", "subject": "posts"}
            ]
        })),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    join(&owner, &guest, &project, "stringer").await;

    let reply = ask(
        &guest,
        &project,
        "posts",
        &json!({"sort": [{"field": "headline"}], "depth": 2}),
        None,
    )
    .await;
    // Every author value stayed a string — never an object, never a hint.
    for entry in rows(&reply) {
        assert!(entry["content"]["author"].is_string(), "{}", entry);
    }

    // `stringer` widened to read `staff` under `condition: "own"` — the same role,
    // updated — the guest's own staff expands, the owner's stays the stored id.
    set_permissions(
        &owner,
        &project,
        "stringer",
        json!([
            {"action": "content.read", "subject": "posts"},
            {"action": "content.read", "subject": "staff", "condition": "own"},
            {"action": "content.create", "subject": "posts"},
            {"action": "content.create", "subject": "staff"}
        ]),
    )
    .await;
    let mine = entry(
        &guest,
        &project,
        "staff",
        &json!({"name": "Mine", "bio": "own"}),
    )
    .await;
    entry(
        &guest,
        &project,
        "posts",
        &json!({"headline": "Own post", "author": mine}),
    )
    .await;
    let reply = ask(
        &guest,
        &project,
        "posts",
        &json!({"sort": [{"field": "headline"}], "depth": 1}),
        None,
    )
    .await;
    let page = rows(&reply);
    let own = row(page, "Own post");
    assert_eq!(own["content"]["author"]["id"], json!(mine));
    assert_eq!(own["content"]["author"]["content"]["name"], json!("Mine"));
    // The owner's staff — Ada behind Alpha — still resolves to nothing for this role.
    assert!(row(page, "Alpha")["content"]["author"].is_string());
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_target_shows_only_the_fields_the_role_may_read() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let guest = sign_in("dev-editor").await;
    let (project, [ada, _, _, _, _, _]) = seed(&owner).await;

    // `stringer` reads posts whole but staff's `name` only: the expanded object keeps
    // `name`, hides `bio` and `mentor`, and says which — `hidden_fields`, the same
    // reporting a draft view gives. `name` being staff's title field, the title shows.
    let made = send(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/roles"),
        Some(&json!({
            "key": "names-only",
            "name": "Names only",
            "permissions": [
                {"action": "content.read", "subject": "posts"},
                {"action": "content.read", "subject": "staff", "fields": ["name"]},
                {"action": "content.create", "subject": "posts"}
            ]
        })),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    join(&owner, &guest, &project, "names-only").await;

    let reply = ask(
        &guest,
        &project,
        "posts",
        &json!({"filters": [{"field": "headline", "op": "eq", "value": "Alpha"}], "depth": 2}),
        None,
    )
    .await;
    let author = &row(rows(&reply), "Alpha")["content"]["author"];
    assert_eq!(author["id"], json!(ada));
    assert_eq!(author["collection"], json!("staff"));
    assert_eq!(author["title"], json!("Ada"));
    assert_eq!(author["content"], json!({"name": "Ada"}));
    assert_eq!(author["hidden_fields"], json!(["bio", "mentor"]));

    // The same narrowing, but the title field is hidden too — the same role, updated:
    // `title` comes back empty, like a draft view's.
    set_permissions(
        &owner,
        &project,
        "names-only",
        json!([
            {"action": "content.read", "subject": "posts"},
            {"action": "content.read", "subject": "staff", "fields": ["bio"]},
            {"action": "content.create", "subject": "posts"}
        ]),
    )
    .await;
    let reply = ask(
        &guest,
        &project,
        "posts",
        &json!({"filters": [{"field": "headline", "op": "eq", "value": "Alpha"}], "depth": 1}),
        None,
    )
    .await;
    let author = &row(rows(&reply), "Alpha")["content"]["author"];
    assert_eq!(author["title"], json!(""));
    assert_eq!(author["content"], json!({"bio": "first"}));
    assert_eq!(author["hidden_fields"], json!(["name", "mentor"]));
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one scenario's own sequence: publish, populate, pin, retitle, revoke"
)]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn the_published_perspective_resolves_at_the_pinned_generation() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, [ada, ben, carol, alpha, beta, gamma]) = seed(&owner).await;

    // Publishing the posts without their staff is refused outright — a release may
    // never leave a reference dangling (D-065): so the only target a published
    // reference can't reach is one that was taken down (below).
    let env = base(&project);
    let set = send(
        &owner,
        Method::POST,
        &format!("{env}/changesets"),
        Some(&json!({"title": "Posts only", "entries": [alpha.clone(), beta.clone(), gamma.clone()]})),
    )
    .await;
    assert_eq!(set.status, StatusCode::CREATED, "{}", set.body);
    let path = format!(
        "{env}/changesets/{}",
        set.body["data"]["id"].as_str().expect("an id")
    );
    let if_match = format!("\"{}\"", set.body["data"]["hash"].as_str().expect("a hash"));
    let submitted = call(
        &owner,
        Method::POST,
        &format!("{path}/submit"),
        None,
        &[("if-match", &if_match)],
    )
    .await
    .expect("request");
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);
    let approve = call(
        &owner,
        Method::POST,
        &format!("{path}/reviews"),
        Some(&json!({"decision": "approve"})),
        &[("if-match", &if_match)],
    )
    .await
    .expect("request");
    assert_eq!(approve.status, StatusCode::OK, "{}", approve.body);
    let releases = send(&owner, Method::GET, &format!("{env}/releases"), None).await;
    let current: i64 = releases
        .headers
        .get("colophon-generation")
        .and_then(|value| value.to_str().ok()?.parse().ok())
        .expect("a generation");
    let published = call(
        &owner,
        Method::POST,
        &format!("{path}/publish"),
        Some(&json!({"base_generation": current})),
        &[("if-match", &if_match), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(published.status, StatusCode::CONFLICT, "{}", published.body);
    assert_eq!(
        published.body["error"]["details"]["reason"],
        json!("references_unpublished")
    );

    // Release 1 publishes everyone; depth expands over the published copies exactly
    // as over drafts, cycle included, and nothing is hidden on the public read.
    let first = release(
        &owner,
        &project,
        &json!({"title": "Everyone", "entries": [ada.clone(), ben.clone(), carol.clone(), alpha.clone(), beta.clone(), gamma.clone()]}),
    )
    .await;
    let reply = ask_published(
        &project,
        "posts",
        &json!({"sort": [{"field": "headline"}], "depth": 2}),
        None,
        None,
    )
    .await;
    let page = rows(&reply);
    let author = &row(page, "Alpha")["content"]["author"];
    assert_eq!(author["id"], json!(ada));
    assert_eq!(author["collection"], json!("staff"));
    assert_eq!(author["title"], json!("Ada"));
    assert_eq!(author["content"]["name"], json!("Ada"));
    assert!(author["hidden_fields"].is_null());
    let mentor = &author["content"]["mentor"];
    assert_eq!(mentor["id"], json!(ben));
    assert_eq!(mentor["content"]["mentor"], json!(ada));

    // Release 2 retitles Ada; pinned at release 1 the populated copy stays hers as it
    // was — a populated reference reads exactly the release its row does.
    save(
        &owner,
        &project,
        "staff",
        &ada,
        &json!({"name": "Ada Lovelace", "bio": "first", "mentor": ben}),
    )
    .await;
    let second = release(
        &owner,
        &project,
        &json!({"title": "Retitle Ada", "entries": [ada.clone()]}),
    )
    .await;
    assert_eq!(second, first + 1);
    let pinned = ask_published(
        &project,
        "posts",
        &json!({"filters": [{"field": "headline", "op": "eq", "value": "Alpha"}], "depth": 1}),
        None,
        Some(first),
    )
    .await;
    let author = &row(rows(&pinned), "Alpha")["content"]["author"];
    assert_eq!(author["content"]["name"], json!("Ada"), "{}", pinned.body);
    let latest = ask_published(
        &project,
        "posts",
        &json!({"filters": [{"field": "headline", "op": "eq", "value": "Alpha"}], "depth": 1}),
        None,
        None,
    )
    .await;
    let author = &row(rows(&latest), "Alpha")["content"]["author"];
    assert_eq!(author["content"]["name"], json!("Ada Lovelace"));

    // Ben is the cycle's other half — Ada's `mentor` — and Beta's `author`; both
    // drafts repoint at Carol, then release 3 publishes those and takes Ben down in
    // the same changeset (the only way a taken-down entry stops being named —
    // `still_referenced`). His earlier publications are revoked, so at the
    // generations where the old copies still name him the reference resolves to
    // nothing — the stored id, honestly — at level 1 and at level 2 alike.
    save(
        &owner,
        &project,
        "staff",
        &ada,
        &json!({"name": "Ada Lovelace", "bio": "first", "mentor": carol}),
    )
    .await;
    save(
        &owner,
        &project,
        "posts",
        &beta,
        &json!({"headline": "Beta", "author": carol}),
    )
    .await;
    let third = release(
        &owner,
        &project,
        &json!({"title": "Repoint and take Ben down", "entries": [ada.clone(), beta.clone()], "unpublish": [ben.clone()]}),
    )
    .await;
    assert_eq!(third, second + 1);
    for generation in [Some(first), Some(second)] {
        let reply = ask_published(
            &project,
            "posts",
            &json!({"sort": [{"field": "headline"}], "depth": 2}),
            None,
            generation,
        )
        .await;
        let page = rows(&reply).to_vec();
        // Level 1: Beta's old copy names Ben — revoked — the stored id.
        assert_eq!(
            row(&page, "Beta")["content"]["author"],
            json!(ben),
            "generation {generation:?}"
        );
        // Level 2: Ada's old copy still names him; at the bound that too stays the id.
        let alpha_author = &row(&page, "Alpha")["content"]["author"];
        assert_eq!(alpha_author["id"], json!(ada), "generation {generation:?}");
        assert_eq!(
            alpha_author["content"]["mentor"],
            json!(ben),
            "generation {generation:?}"
        );
    }
    // At the latest, both repointed references resolve — to Carol.
    let latest = ask_published(
        &project,
        "posts",
        &json!({"sort": [{"field": "headline"}], "depth": 2}),
        None,
        None,
    )
    .await;
    let page = rows(&latest);
    assert_eq!(row(page, "Beta")["content"]["author"]["id"], json!(carol));
    let mentor = &row(page, "Alpha")["content"]["author"]["content"]["mentor"];
    assert_eq!(mentor["id"], json!(carol));
    assert_eq!(mentor["content"]["name"], json!("Carol"));
}
