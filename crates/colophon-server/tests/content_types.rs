//! Content types end to end: the real binary, a real sign-in. Define a type, create entries
//! that fit it and see those that don't refused with a pointer, change the type safely, see a
//! change refused with the entries in its way and accepted once they're fixed (D-055), give each
//! entry its own slug and find it in the public API (D-054), and see that only owners and
//! developers may change types. Needs the stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Server, Session, call, key, project, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

fn types(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production/content-types")
}

fn entries(project: &str, collection: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production/collections/{collection}/entries")
}

fn posts() -> Value {
    json!({
        "name": "Posts",
        "title_field": "headline",
        "fields": [
            {"id": "headline", "label": "Headline", "type": "text", "required": true},
            {"id": "minutes", "label": "Reading time", "type": "number", "integer": true, "min": 1},
            {"id": "section", "label": "Section", "type": "enum", "values": ["News", "Guides"]}
        ]
    })
}

async fn create_posts(session: &Session, project: &str) -> common::Reply {
    call(
        session,
        Method::POST,
        &types(project),
        Some(&json!({"slug": "posts", "definition": posts()})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request")
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_content_type_is_defined_and_its_entries_follow_it() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let project = project(&editor).await;

    let listed = call(&editor, Method::GET, &types(&project), None, &[])
        .await
        .expect("request");
    assert_eq!(
        listed.body["data"][0]["slug"], "articles",
        "every project starts with articles"
    );

    let created = create_posts(&editor, &project).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.etag().as_deref(), Some("\"v1\""));
    assert_eq!(
        created.body["data"]["definition"]["fields"][1]["required"], false,
        "defaults are written out"
    );
    let taken = create_posts(&editor, &project).await;
    assert_eq!(taken.status, StatusCode::CONFLICT);

    let invalid = call(
        &editor,
        Method::POST,
        &types(&project),
        Some(&json!({"slug": "bad", "definition": {"name": "Bad", "title_field": "t", "fields": [{"id": "t", "label": "T", "type": "image"}]}})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(invalid.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        invalid.body["error"]["details"]["pointer"],
        "/definition/fields/0/type"
    );
    let reserved = call(
        &editor,
        Method::POST,
        &types(&project),
        Some(&json!({"slug": "members", "definition": posts()})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(
        reserved.body["error"]["details"]["reason"], "reserved_slug",
        "a Studio page's name"
    );

    // Entries follow the type.
    let post = call(
        &editor,
        Method::POST,
        &entries(&project, "posts"),
        Some(&json!({"section": "Guides", "headline": "Hello", "minutes": 3})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(post.status, StatusCode::CREATED, "{}", post.body);
    assert_eq!(
        post.body["data"]["content"],
        json!({"headline": "Hello", "minutes": 3, "section": "Guides"}),
        "written in definition order"
    );
    for (content, pointer) in [
        (json!({"headline": "Hi", "minutes": 0}), "/minutes"),
        (json!({"headline": "Hi", "section": "Opinion"}), "/section"),
        (json!({"headline": "Hi", "color": "red"}), "/color"),
        (json!({"minutes": 2}), "/headline"),
    ] {
        let refused = call(
            &editor,
            Method::POST,
            &entries(&project, "posts"),
            Some(&content),
            &[("idempotency-key", &key())],
        )
        .await
        .expect("request");
        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{content}"
        );
        assert_eq!(
            refused.body["error"]["details"]["pointer"], pointer,
            "{content}"
        );
    }
    let missing = call(&editor, Method::GET, &entries(&project, "pages"), None, &[])
        .await
        .expect("request");
    assert_eq!(
        missing.status,
        StatusCode::NOT_FOUND,
        "an unknown collection"
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_content_type_changes_only_in_ways_that_keep_its_entries_valid() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let project = project(&editor).await;
    create_posts(&editor, &project).await;
    let post = call(
        &editor,
        Method::POST,
        &entries(&project, "posts"),
        Some(&json!({"headline": "Hello", "minutes": 3})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(post.status, StatusCode::CREATED, "{}", post.body);

    // A safe change: a new optional field and a new option.
    let mut wider = posts();
    wider["fields"][2]["values"] = json!(["News", "Guides", "Opinion"]);
    wider["fields"]
        .as_array_mut()
        .expect("a list")
        .push(json!({"id": "featured", "label": "Featured", "type": "boolean"}));
    let path = format!("{}/posts", types(&project));
    let updated = call(
        &editor,
        Method::PUT,
        &path,
        Some(&wider),
        &[("if-match", "\"v1\""), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);
    assert_eq!(updated.etag().as_deref(), Some("\"v2\""));
    let opinion = call(
        &editor,
        Method::POST,
        &entries(&project, "posts"),
        Some(&json!({"headline": "Hot take", "section": "Opinion", "featured": true})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(opinion.status, StatusCode::CREATED, "{}", opinion.body);
    let id = post.body["data"]["id"].as_str().expect("an id");
    let old = call(
        &editor,
        Method::GET,
        &format!("{}/{id}", entries(&project, "posts")),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        old.status,
        StatusCode::OK,
        "entries from version 1 still read"
    );

    // Stale and unsafe changes are refused.
    let stale = call(
        &editor,
        Method::PUT,
        &path,
        Some(&wider),
        &[("if-match", "\"v1\""), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(
        (stale.status, stale.etag().as_deref()),
        (StatusCode::CONFLICT, Some("\"v2\""))
    );
    let mut narrower = wider.clone();
    narrower["fields"].as_array_mut().expect("a list").remove(1);
    let unsafe_change = call(
        &editor,
        Method::PUT,
        &path,
        Some(&narrower),
        &[("if-match", "\"v2\""), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(unsafe_change.status, StatusCode::UNPROCESSABLE_ENTITY);
    let details = &unsafe_change.body["error"]["details"];
    assert_eq!(details["reason"], "needs_migration");
    // The refusal names the entry in the way and why (D-055).
    assert_eq!(details["total"], 1, "{details}");
    assert_eq!(details["entries"][0]["title"], "Hello");
    assert_eq!(details["entries"][0]["state"], "draft");
    assert_eq!(details["entries"][0]["problem"]["reason"], "unknown_field");
    assert_eq!(details["entries"][0]["problem"]["pointer"], "/minutes");
}

fn notes() -> Value {
    json!({
        "name": "Notes",
        "title_field": "title",
        "fields": [
            {"id": "title", "label": "Title", "type": "text", "required": true},
            {"id": "tag", "label": "Tag", "type": "text"},
            {"id": "done", "label": "Done", "type": "boolean"}
        ]
    })
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_narrowing_change_goes_through_when_nothing_stored_is_in_the_way() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let created = call(
        &owner,
        Method::POST,
        &types(&project),
        Some(&json!({"slug": "notes", "definition": notes()})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let mut ids = Vec::new();
    for body in [json!({"title": "One", "tag": "x"}), json!({"title": "Two"})] {
        let note = call(
            &owner,
            Method::POST,
            &entries(&project, "notes"),
            Some(&body),
            &[("idempotency-key", &key())],
        )
        .await
        .expect("request");
        assert_eq!(note.status, StatusCode::CREATED, "{}", note.body);
        ids.push((
            note.body["data"]["id"].as_str().expect("an id").to_owned(),
            note.etag().expect("an ETag"),
        ));
    }
    let path = format!("{}/notes", types(&project));
    let put = |definition: Value, version: &'static str| {
        let owner = &owner;
        let path = path.clone();
        async move {
            call(
                owner,
                Method::PUT,
                &path,
                Some(&definition),
                &[("if-match", version), ("idempotency-key", &key())],
            )
            .await
            .expect("request")
        }
    };

    // Nobody uses `done`: removing it is fine.
    let mut without_done = notes();
    without_done["fields"].as_array_mut().expect("a list").remove(2);
    let removed = put(without_done.clone(), "\"v1\"").await;
    assert_eq!(removed.status, StatusCode::OK, "{}", removed.body);

    // `Two` has no tag: requiring one is refused, naming it.
    let mut required = without_done.clone();
    required["fields"][1]["required"] = json!(true);
    let refused = put(required.clone(), "\"v2\"").await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", refused.body);
    let details = &refused.body["error"]["details"];
    assert_eq!(
        (&details["reason"], &details["total"], &details["entries"][0]["title"]),
        (&json!("needs_migration"), &json!(1), &json!("Two"))
    );
    assert_eq!(details["entries"][0]["problem"]["reason"], "missing_field");

    // Once `Two` has a tag, the same change goes through.
    let (two, etag) = &ids[1];
    let saved = call(
        &owner,
        Method::PUT,
        &format!("{}/{two}", entries(&project, "notes")),
        Some(&json!({"title": "Two", "tag": "y"})),
        &[("if-match", etag), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    let accepted = put(required, "\"v2\"").await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.body);
    assert_eq!(accepted.etag().as_deref(), Some("\"v3\""));

    // A field turned into a slug is refused whatever is stored.
    let mut into_slug = without_done;
    into_slug["fields"][1] = json!({"id": "tag", "label": "Tag", "type": "slug", "required": true});
    let into = put(into_slug, "\"v3\"").await;
    assert_eq!(into.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(into.body["error"]["details"]["total"], Value::Null, "{}", into.body);
}

fn pages() -> Value {
    json!({
        "name": "Pages",
        "title_field": "title",
        "fields": [
            {"id": "title", "label": "Title", "type": "text", "required": true},
            {"id": "slug", "label": "URL name", "type": "slug", "source": "title"}
        ]
    })
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_slug_belongs_to_one_entry_and_reaches_the_public_api() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let created = call(
        &owner,
        Method::POST,
        &types(&project),
        Some(&json!({"slug": "pages", "definition": pages()})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let make = |body: Value| {
        let owner = &owner;
        let path = entries(&project, "pages");
        async move {
            call(owner, Method::POST, &path, Some(&body), &[("idempotency-key", &key())])
                .await
                .expect("request")
        }
    };
    let about = make(json!({"title": "About", "slug": "about"})).await;
    assert_eq!(about.status, StatusCode::CREATED, "{}", about.body);
    let about_id = about.body["data"]["id"].as_str().expect("an id").to_owned();

    let taken = make(json!({"title": "About us", "slug": "about"})).await;
    assert_eq!(taken.status, StatusCode::CONFLICT, "{}", taken.body);
    let details = &taken.body["error"]["details"];
    assert_eq!(
        (&details["reason"], &details["pointer"]),
        (&json!("slug_taken"), &json!("/slug"))
    );
    assert_eq!(details["entries"][0]["entry"], about_id.as_str());
    assert_eq!(details["entries"][0]["state"], "draft");

    let not_a_slug = make(json!({"title": "About us", "slug": "About Us"})).await;
    assert_eq!(not_a_slug.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(not_a_slug.body["error"]["details"]["reason"], "invalid_slug");

    // Published, the slug is in the public API.
    common::release(
        &owner,
        &project,
        &json!({"title": "About", "entries": [about_id]}),
    )
    .await;
    let (status, body) =
        common::get(None, &format!("/content/v1/{project}/production/pages")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"][0]["slug"], "about");
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn only_owners_and_developers_change_content_types_and_strangers_see_nothing() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    create_posts(&owner, &project).await;

    // dev-editor joins this project as an editor (no membership API yet).
    let other = sign_in("dev-editor").await;
    let stranger = call(&other, Method::GET, &types(&project), None, &[])
        .await
        .expect("request");
    assert_eq!(
        stranger.status,
        StatusCode::NOT_FOUND,
        "not a member: nothing to see"
    );
    let database = common::owner_database().await;
    let me = call(&other, Method::GET, "/me", None, &[])
        .await
        .expect("request");
    let identity =
        uuid::Uuid::parse_str(me.body["data"]["id"].as_str().expect("an id")).expect("a uuid");
    let project_id = uuid::Uuid::parse_str(&project).expect("a uuid");
    sqlx::query(
        "INSERT INTO memberships (project_id, identity_id, role) VALUES ($1, $2, 'editor')",
    )
    .bind(project_id)
    .bind(identity)
    .execute(&database)
    .await
    .expect("membership");
    database.close().await;

    let read = call(
        &other,
        Method::GET,
        &format!("{}/posts", types(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(read.status, StatusCode::OK, "editors may read types");
    let written = call(
        &other,
        Method::PUT,
        &format!("{}/posts", types(&project)),
        Some(&posts()),
        &[("if-match", "\"v1\""), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(written.status, StatusCode::FORBIDDEN, "{}", written.body);
    let entry = call(
        &other,
        Method::POST,
        &entries(&project, "posts"),
        Some(&json!({"headline": "By an editor"})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(
        entry.status,
        StatusCode::CREATED,
        "editors may still write entries"
    );
}
