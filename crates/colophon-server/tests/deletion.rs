//! Deleting entries, end to end (D-080): the real binary, real people and real API tokens. A
//! deletion needs the draft's `ETag` and an idempotency key, replays its answer, and leaves nothing
//! any read, list, history or save can find; it is refused, saying why, while the entry is
//! published, named by another entry or pinned by a changeset; what it held comes free; and only
//! a role or token holding `content.delete` (on anyone's entries, or its own) may do it. Needs the
//! stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{
    ORIGIN, Reply, Server, Session, app, article, call, create_article, entries, join, key,
    project, publish_article, sign_in,
};
use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};

fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
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

/// The current `ETag` of an entry at `path`.
async fn etag(session: &Session, path: &str) -> String {
    let read = send(session, Method::GET, path, None, &[]).await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    read.etag().expect("an ETag")
}

/// Deletes the entry at `path`, confirming `etag`, with `key`.
async fn delete(session: &Session, path: &str, etag: &str, key: &str) -> Reply {
    send(
        session,
        Method::DELETE,
        path,
        None,
        &[("if-match", etag), ("idempotency-key", key)],
    )
    .await
}

/// Deletes the entry at `path` as it is now, with a fresh key.
async fn delete_now(session: &Session, path: &str) -> Reply {
    let current = etag(session, path).await;
    delete(session, path, &current, &key()).await
}

fn reason(reply: &Reply) -> &Value {
    &reply.body["error"]["details"]["reason"]
}

/// A request made with an API token's secret, as a script would send it.
async fn as_token(secret: &str, method: Method, path: &str, extra: &[(&str, String)]) -> Reply {
    let mut request = app()
        .request(method, format!("{ORIGIN}{path}"))
        .header(header::AUTHORIZATION, format!("Bearer {secret}"));
    for (name, value) in extra {
        request = request.header(*name, value);
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

/// Makes an API token in `project`; its secret.
async fn token(owner: &Session, project: &str, body: &Value) -> String {
    let made = send(
        owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/tokens"),
        Some(body),
        &[],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    made.body["data"]["secret"]
        .as_str()
        .expect("the secret")
        .to_owned()
}

async fn define(owner: &Session, project: &str, slug: &str, definition: &Value) {
    let made = send(
        owner,
        Method::POST,
        &format!("{}/content-types", base(project)),
        Some(&json!({"slug": slug, "definition": definition})),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
}

async fn create(owner: &Session, project: &str, collection: &str, content: &Value) -> Reply {
    send(
        owner,
        Method::POST,
        &format!("{}/collections/{collection}/entries", base(project)),
        Some(content),
        &[("idempotency-key", &key())],
    )
    .await
}

/// Submits, approves and publishes the changeset `view` (as a create answered it) as `owner`.
async fn publish_changeset(owner: &Session, project: &str, view: &Value) {
    let path = format!(
        "{}/changesets/{}",
        base(project),
        view["id"].as_str().expect("an id")
    );
    let if_match = format!("\"{}\"", view["hash"].as_str().expect("a hash"));
    for (step, body) in [
        ("submit", None),
        ("reviews", Some(json!({"decision": "approve"}))),
    ] {
        let done = send(
            owner,
            Method::POST,
            &format!("{path}/{step}"),
            body.as_ref(),
            &[("if-match", &if_match)],
        )
        .await;
        assert!(done.status.is_success(), "{step}: {}", done.body);
    }
    let releases = send(
        owner,
        Method::GET,
        &format!("{}/releases", base(project)),
        None,
        &[],
    )
    .await;
    let generation: i64 = releases
        .headers
        .get("colophon-generation")
        .and_then(|value| value.to_str().ok()?.parse().ok())
        .expect("a generation");
    let published = send(
        owner,
        Method::POST,
        &format!("{path}/publish"),
        Some(&json!({"base_generation": generation})),
        &[("if-match", &if_match), ("idempotency-key", &key())],
    )
    .await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);
}

fn id(reply: &Reply) -> String {
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    reply.body["data"]["id"].as_str().expect("an id").to_owned()
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_deleted_entry_is_gone_from_every_read_and_its_answer_replays() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let doomed = create_article(&owner, &project, "Doomed").await;
    let other = create_article(&owner, &project, "Other").await;
    let path = format!("{}/{doomed}", entries(&project));
    let first = etag(&owner, &path).await;

    // Both preconditions are required.
    let no_match = send(
        &owner,
        Method::DELETE,
        &path,
        None,
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(no_match.status, StatusCode::PRECONDITION_REQUIRED);
    let no_key = send(&owner, Method::DELETE, &path, None, &[("if-match", &first)]).await;
    assert_eq!(no_key.status, StatusCode::PRECONDITION_REQUIRED);

    // A draft that moved on since it was looked at: refused, with the current ETag.
    let saved = send(
        &owner,
        Method::PUT,
        &path,
        Some(&article("Doomed, edited", "text")),
        &[("if-match", &first), ("idempotency-key", &key())],
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    let current = saved.etag().expect("an ETag");
    let stale = delete(&owner, &path, &first, &key()).await;
    assert_eq!(stale.status, StatusCode::CONFLICT, "{}", stale.body);
    assert_eq!(reason(&stale), "stale_revision");
    assert_eq!(stale.etag().as_deref(), Some(current.as_str()));

    // Deleted, attributed to the caller.
    let once = key();
    let deleted = delete(&owner, &path, &current, &once).await;
    assert_eq!(deleted.status, StatusCode::OK, "{}", deleted.body);
    let data = &deleted.body["data"];
    assert_eq!(data["id"], json!(doomed));
    assert_eq!(data["collection"], "articles");
    assert_eq!(
        format!("\"{}\"", data["revision_id"].as_str().expect("a revision")),
        current
    );
    assert_eq!(data["deleted_by"], saved.body["data"]["created_by"]);
    assert!(data["deleted_at"].is_string());

    // A retry after a lost answer gets the same answer; the same key for another request doesn't.
    let replayed = delete(&owner, &path, &current, &once).await;
    assert_eq!(replayed.status, StatusCode::OK, "{}", replayed.body);
    assert!(replayed.replayed());
    assert_eq!(replayed.body, deleted.body);
    let other_path = format!("{}/{other}", entries(&project));
    let other_etag = etag(&owner, &other_path).await;
    let reused = delete(&owner, &other_path, &other_etag, &once).await;
    assert_eq!(reused.status, StatusCode::CONFLICT, "{}", reused.body);
    assert_eq!(reason(&reused), "idempotency_key_reused");

    // Nothing finds it: read, list, history, one revision, a save, another deletion.
    for gone in [
        path.clone(),
        format!("{path}/revisions"),
        format!(
            "{path}/revisions/{}",
            data["revision_id"].as_str().expect("a revision")
        ),
    ] {
        let read = send(&owner, Method::GET, &gone, None, &[]).await;
        assert_eq!(read.status, StatusCode::NOT_FOUND, "{gone}: {}", read.body);
    }
    let listed = send(&owner, Method::GET, &entries(&project), None, &[]).await;
    let ids: Vec<&str> = listed.body["data"]
        .as_array()
        .expect("a page")
        .iter()
        .filter_map(|summary| summary["id"].as_str())
        .collect();
    assert_eq!(ids, vec![other.as_str()]);
    let save = send(
        &owner,
        Method::PUT,
        &path,
        Some(&article("Back", "text")),
        &[("if-match", &current), ("idempotency-key", &key())],
    )
    .await;
    assert_eq!(save.status, StatusCode::NOT_FOUND, "{}", save.body);
    let again = delete(&owner, &path, &current, &key()).await;
    assert_eq!(again.status, StatusCode::NOT_FOUND, "{}", again.body);

    // The audit log keeps who deleted what.
    let audit = send(
        &owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/audit?action=entry.deleted"),
        None,
        &[],
    )
    .await;
    assert_eq!(audit.status, StatusCode::OK, "{}", audit.body);
    let event = &audit.body["data"][0];
    assert_eq!(event["subject"], json!(doomed));
    assert_eq!(event["detail"]["title"], "Doomed, edited");
    assert_eq!(event["actor"]["id"], data["deleted_by"]);
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_deletion_is_refused_while_the_entry_is_still_needed() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;

    // Published: readers get it, so it must be taken down first.
    let generation = publish_article(&owner, &project, "Live").await;
    assert!(generation >= 1);
    let listed = send(&owner, Method::GET, &entries(&project), None, &[]).await;
    let live = listed.body["data"][0]["id"]
        .as_str()
        .expect("the published article")
        .to_owned();
    let published = delete_now(&owner, &format!("{}/{live}", entries(&project))).await;
    assert_eq!(published.status, StatusCode::CONFLICT, "{}", published.body);
    assert_eq!(reason(&published), "entry_published");

    // Named by another live entry's draft: refused, naming it.
    define(
        &owner,
        &project,
        "notes",
        &json!({"name": "Notes", "title_field": "title", "fields": [
            {"id": "title", "label": "Title", "type": "text", "required": true},
            {"id": "about", "label": "About", "type": "reference", "to": "articles"}
        ]}),
    )
    .await;
    let subject = create_article(&owner, &project, "Subject").await;
    let subject_path = format!("{}/{subject}", entries(&project));
    let note = id(&create(
        &owner,
        &project,
        "notes",
        &json!({"title": "A note", "about": subject}),
    )
    .await);
    let named = delete_now(&owner, &subject_path).await;
    assert_eq!(named.status, StatusCode::CONFLICT, "{}", named.body);
    assert_eq!(reason(&named), "still_referenced");
    assert_eq!(
        named.body["error"]["details"]["referenced_by"],
        json!([{"entry_id": note, "collection": "notes", "field": "about", "in_draft": true, "published": false}])
    );
    // Once the note goes, nothing names it; but a changeset that hasn't been published pins it.
    let note_path = format!("{}/collections/notes/entries/{note}", base(&project));
    assert_eq!(delete_now(&owner, &note_path).await.status, StatusCode::OK);
    let set = send(
        &owner,
        Method::POST,
        &format!("{}/changesets", base(&project)),
        Some(&json!({"title": "Publish the subject", "entries": [subject]})),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(set.status, StatusCode::CREATED, "{}", set.body);
    let pinned = delete_now(&owner, &subject_path).await;
    assert_eq!(pinned.status, StatusCode::CONFLICT, "{}", pinned.body);
    assert_eq!(reason(&pinned), "entry_in_changeset");
    assert_eq!(
        pinned.body["error"]["details"]["changesets"],
        json!([{"id": set.body["data"]["id"], "title": "Publish the subject", "scheduled": false}])
    );
    // Every refusal left the entry exactly as it was.
    let still = send(&owner, Method::GET, &subject_path, None, &[]).await;
    assert_eq!(still.status, StatusCode::OK, "{}", still.body);
    // Published and taken down, it may go; the changesets keep showing what they published.
    publish_changeset(&owner, &project, &set.body["data"]).await;
    common::release(
        &owner,
        &project,
        &json!({"title": "Take it down", "unpublish": [subject]}),
    )
    .await;
    let deleted = delete_now(&owner, &subject_path).await;
    assert_eq!(deleted.status, StatusCode::OK, "{}", deleted.body);
    let history = send(
        &owner,
        Method::GET,
        &format!(
            "{}/changesets/{}",
            base(&project),
            set.body["data"]["id"].as_str().expect("an id")
        ),
        None,
        &[],
    )
    .await;
    assert_eq!(history.status, StatusCode::OK, "{}", history.body);
    let item = &history.body["data"]["items"][0];
    assert_eq!(
        (&item["entry_id"], &item["entry_deleted"], &item["title"]),
        (&json!(subject), &json!(true), &json!("Subject"))
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: a slug, a unique value, a single type, a reference"
)]
async fn what_a_deleted_entry_held_is_free_again_and_nothing_can_name_it() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    define(
        &owner,
        &project,
        "pages",
        &json!({"name": "Pages", "title_field": "title", "fields": [
            {"id": "title", "label": "Title", "type": "text", "required": true},
            {"id": "slug", "label": "URL name", "type": "slug", "source": "title"},
            {"id": "code", "label": "Code", "type": "text", "unique": true}
        ]}),
    )
    .await;
    define(
        &owner,
        &project,
        "site",
        &json!({"name": "Site settings", "title_field": "name", "single": true, "fields": [
            {"id": "name", "label": "Site name", "type": "text", "required": true}
        ]}),
    )
    .await;
    define(
        &owner,
        &project,
        "notes",
        &json!({"name": "Notes", "title_field": "title", "fields": [
            {"id": "title", "label": "Title", "type": "text", "required": true},
            {"id": "about", "label": "About", "type": "reference", "to": "pages"}
        ]}),
    )
    .await;
    let home = id(&create(
        &owner,
        &project,
        "pages",
        &json!({"title": "Home", "slug": "home", "code": "A1"}),
    )
    .await);
    let site = id(&create(&owner, &project, "site", &json!({"name": "Site"})).await);

    // While they live, nothing else takes them.
    let slug = create(
        &owner,
        &project,
        "pages",
        &json!({"title": "Copy", "slug": "home"}),
    )
    .await;
    assert_eq!(reason(&slug), "slug_taken", "{}", slug.body);
    let code = create(
        &owner,
        &project,
        "pages",
        &json!({"title": "Copy", "code": "A1"}),
    )
    .await;
    assert_eq!(reason(&code), "value_taken", "{}", code.body);
    let single = create(&owner, &project, "site", &json!({"name": "Second"})).await;
    assert_eq!(reason(&single), "single_exists", "{}", single.body);

    // Deleted, all three come free.
    for path in [
        format!("{}/collections/pages/entries/{home}", base(&project)),
        format!("{}/collections/site/entries/{site}", base(&project)),
    ] {
        let deleted = delete_now(&owner, &path).await;
        assert_eq!(deleted.status, StatusCode::OK, "{}", deleted.body);
    }
    let new_home = create(
        &owner,
        &project,
        "pages",
        &json!({"title": "New home", "slug": "home", "code": "A1"}),
    )
    .await;
    assert_eq!(new_home.status, StatusCode::CREATED, "{}", new_home.body);
    let new_site = create(&owner, &project, "site", &json!({"name": "New site"})).await;
    assert_eq!(new_site.status, StatusCode::CREATED, "{}", new_site.body);

    // A reference to the deleted entry is refused where it is, as one to no entry at all.
    let dangling = create(
        &owner,
        &project,
        "notes",
        &json!({"title": "About the old home", "about": home}),
    )
    .await;
    assert_eq!(
        dangling.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        dangling.body
    );
    assert_eq!(
        (
            reason(&dangling),
            &dangling.body["error"]["details"]["pointer"]
        ),
        (&json!("reference_not_found"), &json!("/about"))
    );
    // And a changeset can't pin it.
    let pin = send(
        &owner,
        Method::POST,
        &format!("{}/changesets", base(&project)),
        Some(&json!({"title": "Old home", "entries": [home]})),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(pin.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", pin.body);
    assert_eq!(reason(&pin), "unknown_entry");
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: an editor, an author with own-only delete, three tokens"
)]
async fn only_content_delete_lets_a_person_or_a_token_delete() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let editor = sign_in("dev-editor").await;

    // An editor holds no content.delete: refused, naming it, on anyone's entry and its own.
    let project_a = project(&owner).await;
    join(&owner, &editor, &project_a, "editor").await;
    let owners = create_article(&owner, &project_a, "The owner's").await;
    let editors = create_article(&editor, &project_a, "The editor's").await;
    for (entry, who) in [(&owners, "the owner's"), (&editors, "its own")] {
        let refused = delete_now(&editor, &format!("{}/{entry}", entries(&project_a))).await;
        assert_eq!(
            refused.status,
            StatusCode::FORBIDDEN,
            "{who}: {}",
            refused.body
        );
        assert_eq!(
            (
                reason(&refused),
                &refused.body["error"]["details"]["permission"]
            ),
            (&json!("missing_permission"), &json!("content.delete")),
            "{who}"
        );
    }

    // An author may delete its own articles, not anyone else's.
    let project_b = project(&owner).await;
    let made = send(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project_b}/roles"),
        Some(&json!({
            "key": "author",
            "name": "Author",
            "permissions": [
                {"action": "content.read", "subject": "articles"},
                {"action": "content.create", "subject": "articles"},
                {"action": "content.update", "subject": "articles", "condition": "own"},
                {"action": "content.delete", "subject": "articles", "condition": "own"}
            ]
        })),
        &[],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    join(&owner, &editor, &project_b, "author").await;
    let owners = create_article(&owner, &project_b, "The owner's").await;
    let authors = create_article(&editor, &project_b, "The author's").await;
    let refused = delete_now(&editor, &format!("{}/{owners}", entries(&project_b))).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
    assert_eq!(
        refused.body["error"]["details"]["permission"],
        "content.delete"
    );
    let own = delete_now(&editor, &format!("{}/{authors}", entries(&project_b))).await;
    assert_eq!(own.status, StatusCode::OK, "{}", own.body);

    // Tokens: read-only can't; full access can, as itself; one that may delete but only read its
    // own entries finds the owner's entry not there at all.
    let reader = token(
        &owner,
        &project_b,
        &json!({"name": "Reader", "kind": "read-only", "expires_in_days": 1}),
    )
    .await;
    let full = token(
        &owner,
        &project_b,
        &json!({"name": "Cleaner", "kind": "full-access", "expires_in_days": 1}),
    )
    .await;
    let narrow = token(
        &owner,
        &project_b,
        &json!({
            "name": "Narrow", "kind": "custom", "expires_in_days": 1,
            "permissions": [
                {"action": "content.read", "subject": "articles", "condition": "own"},
                {"action": "content.delete", "subject": "articles"}
            ]
        }),
    )
    .await;
    let target = create_article(&owner, &project_b, "For the tokens").await;
    let path = format!("{}/{target}", entries(&project_b));
    let current = etag(&owner, &path).await;
    let with_key = |key: &'static str| {
        [
            ("if-match", current.clone()),
            ("idempotency-key", key.to_owned()),
        ]
    };
    let read_only = as_token(
        &reader,
        Method::DELETE,
        &path,
        &with_key("token-reader-0001"),
    )
    .await;
    assert_eq!(
        read_only.status,
        StatusCode::FORBIDDEN,
        "{}",
        read_only.body
    );
    assert_eq!(
        read_only.body["error"]["details"]["permission"],
        "content.delete"
    );
    let hidden = as_token(
        &narrow,
        Method::DELETE,
        &path,
        &with_key("token-narrow-0001"),
    )
    .await;
    assert_eq!(hidden.status, StatusCode::NOT_FOUND, "{}", hidden.body);
    let deleted = as_token(&full, Method::DELETE, &path, &with_key("token-cleaner-001")).await;
    assert_eq!(deleted.status, StatusCode::OK, "{}", deleted.body);
    let owner_id = send(&owner, Method::GET, "/me", None, &[]).await.body["data"]["id"].clone();
    assert_ne!(
        deleted.body["data"]["deleted_by"], owner_id,
        "a token's deletion names the token's identity, not its maker"
    );
    assert_eq!(
        send(&owner, Method::GET, &path, None, &[]).await.status,
        StatusCode::NOT_FOUND
    );
}
