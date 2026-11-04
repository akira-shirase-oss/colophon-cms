//! Collection transfer end to end (D-086): the real binary, a real sign-in. A collection's
//! drafts export as JSONL — one entry's canonical content per line — and a JSONL body, a JSON
//! array, or Strapi export entities import back in one transaction, under an
//! `Idempotency-Key`. Needs the stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Reply, Server, Session, app, call, join, key, project, sign_in};
use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};

fn types(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production/content-types")
}

fn collection(project: &str, collection: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production/collections/{collection}")
}

async fn send(session: &Session, method: Method, path: &str, body: Option<&Value>) -> Reply {
    call(session, method, path, body, &[("idempotency-key", &key())])
        .await
        .expect("request")
}

async fn write_type(session: &Session, project: &str, slug: &str, definition: &Value) -> Reply {
    send(
        session,
        Method::POST,
        &types(project),
        Some(&json!({"slug": slug, "definition": definition})),
    )
    .await
}

/// A raw-body POST: an import sends a JSONL file, not a JSON document.
async fn upload(
    session: &Session,
    path: &str,
    body: &str,
    idempotency_key: &str,
) -> (StatusCode, bool, Value) {
    let response = app()
        .post(format!("{}{path}", common::ORIGIN))
        .header(header::COOKIE, &session.cookie)
        .header("x-csrf-token", &session.csrf)
        .header(header::ORIGIN, common::ORIGIN)
        .header(header::CONTENT_TYPE, "application/jsonlines")
        .header("idempotency-key", idempotency_key)
        .body(body.to_owned())
        .send()
        .await
        .expect("request");
    let status = response.status();
    let replayed = response
        .headers()
        .get("idempotent-replayed")
        .is_some_and(|value| value == "true");
    (
        status,
        replayed,
        response.json().await.unwrap_or(Value::Null),
    )
}

/// A dry run: `?dry_run=true` plans rather than writes, so it needs no `Idempotency-Key`
/// (D-114). `path` carries the query (`?dry_run=true`, plus `&type=` when asked).
async fn plan(session: &Session, path: &str, body: &str) -> (StatusCode, Value) {
    let response = app()
        .post(format!("{}{path}", common::ORIGIN))
        .header(header::COOKIE, &session.cookie)
        .header("x-csrf-token", &session.csrf)
        .header(header::ORIGIN, common::ORIGIN)
        .header(header::CONTENT_TYPE, "application/jsonlines")
        .body(body.to_owned())
        .send()
        .await
        .expect("request");
    (
        response.status(),
        response.json().await.unwrap_or(Value::Null),
    )
}

/// The collection's export as raw text plus its headers, or the error body.
async fn download(session: &Session, path: &str) -> (StatusCode, header::HeaderMap, String) {
    let response = app()
        .get(format!("{}{path}", common::ORIGIN))
        .header(header::COOKIE, &session.cookie)
        .send()
        .await
        .expect("request");
    let status = response.status();
    let headers = response.headers().clone();
    (status, headers, response.text().await.expect("text"))
}

fn lines(text: &str) -> Vec<&str> {
    text.lines().filter(|line| !line.is_empty()).collect()
}

const NOTES: &str = "notes";
const NOTES_DEF: &str = r#"{"name": "Notes", "title_field": "title", "fields": [
    {"id": "title", "label": "Title", "type": "text", "required": true},
    {"id": "note", "label": "Note", "type": "text"}
]}"#;

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_collection_exports_and_imports_its_drafts() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let notes: Value = serde_json::from_str(NOTES_DEF).expect("definition");
    assert_eq!(
        write_type(&owner, &project, NOTES, &notes).await.status,
        StatusCode::CREATED
    );
    let export = format!("{}/export", collection(&project, NOTES));
    let import = format!("{}/import", collection(&project, NOTES));

    // An empty collection exports an empty file that still answers as JSONL.
    let (status, headers, body) = download(&owner, &export).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "application/jsonlines",
        "{headers:?}"
    );
    assert!(
        headers[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap_or_default()
            .contains("notes.jsonl"),
        "{headers:?}"
    );
    assert!(lines(&body).is_empty(), "{body}");

    // JSONL: each line one entry's content.
    let file = [
        r#"{"title":"First","note":"one"}"#,
        r#"{"title":"Second","note":"two"}"#,
        r#"{"title":"Third"}"#,
    ]
    .join("\n");
    let (status, _, reply) = upload(&owner, &import, &file, &key()).await;
    assert_eq!(
        (status, &reply["data"]["imported"]),
        (StatusCode::CREATED, &json!(3)),
        "{reply}"
    );
    let (_, _, exported) = download(&owner, &export).await;
    let out = lines(&exported);
    assert_eq!(out.len(), 3, "{exported}");
    // Newest first; every line is a draft's canonical content.
    assert!(out[0].contains(r#""title":"Third""#), "{exported}");
    for line in &out {
        assert!(serde_json::from_str::<Value>(line).is_ok(), "{line}");
    }

    // A JSON array of the same objects imports too.
    let (status, _, reply) = upload(
        &owner,
        &import,
        r#"[{"title":"Fourth","note":"array"}]"#,
        &key(),
    )
    .await;
    assert_eq!(
        (status, &reply["data"]["imported"]),
        (StatusCode::CREATED, &json!(1)),
        "{reply}"
    );

    // Strapi export entities and `{"version":…,"data":{…}}` markers hand validation `data`.
    let strapi = [
        r#"{"type":"api::note.note","data":{"title":"Fifth","note":"strapi"}}"#,
        r#"{"version":2,"data":{"title":"Sixth","note":"marker"}}"#,
    ]
    .join("\n");
    let (status, _, reply) = upload(&owner, &import, &strapi, &key()).await;
    assert_eq!(
        (status, &reply["data"]["imported"]),
        (StatusCode::CREATED, &json!(2)),
        "{reply}"
    );
    let (_, _, exported) = download(&owner, &export).await;
    assert_eq!(lines(&exported).len(), 6, "{exported}");

    // A line that isn't JSON is refused where it sits.
    let (status, _, reply) =
        upload(&owner, &import, "{\"title\":\"Fine\"}\nnot json\n", &key()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
    assert_eq!(reply["error"]["details"]["reason"], "invalid_json");
    assert_eq!(reply["error"]["details"]["pointer"], "/line/2");

    // A valid file with one bad item lands nothing at all: the transaction rolls back.
    let (status, _, reply) = upload(
        &owner,
        &import,
        r#"[{"title":"Fine"},{"note":"no title"}]"#,
        &key(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
    assert_eq!(reply["error"]["details"]["pointer"], "/1/title", "{reply}");
    let (_, _, exported) = download(&owner, &export).await;
    assert_eq!(lines(&exported).len(), 6, "nothing landed: {exported}");

    // A lost response's retry replays the recorded answer instead of importing twice.
    let once = key();
    let (first_status, first_replayed, _) =
        upload(&owner, &import, r#"{"title":"Once"}"#, &once).await;
    let (again_status, again_replayed, reply) =
        upload(&owner, &import, r#"{"title":"Once"}"#, &once).await;
    assert_eq!(first_status, StatusCode::CREATED);
    assert!(!first_replayed);
    assert_eq!(again_status, StatusCode::CREATED);
    assert!(again_replayed, "{reply}");
    let (_, _, exported) = download(&owner, &export).await;
    assert_eq!(lines(&exported).len(), 7, "imported once: {exported}");
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: mirror, single, bound"
)]
async fn an_import_mirrors_references_and_respects_singles_and_bounds() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;

    // A mirrored pair, like D-085's: imported forward links fill the writer's mirror.
    let writers_def = json!({
        "name": "Writers", "title_field": "name",
        "fields": [
            {"id": "name", "label": "Name", "type": "text", "required": true},
            {"id": "posts", "label": "Posts", "type": "reference", "to": "posts", "many": true, "max": 10}
        ]
    });
    let posts_def = json!({
        "name": "Posts", "title_field": "headline",
        "fields": [
            {"id": "headline", "label": "Headline", "type": "text", "required": true},
            {"id": "writer", "label": "Writer", "type": "reference", "to": "writers", "inverse": "posts"}
        ]
    });
    assert_eq!(
        write_type(&owner, &project, "posts", &json!({
            "name": "Posts", "title_field": "headline",
            "fields": [{"id": "headline", "label": "Headline", "type": "text", "required": true}]
        }))
        .await
        .status,
        StatusCode::CREATED
    );
    assert_eq!(
        write_type(&owner, &project, "writers", &writers_def)
            .await
            .status,
        StatusCode::CREATED
    );
    // Posts gains its mirrored side once writers exists to point back.
    let linked = call(
        &owner,
        Method::PUT,
        &format!("{}/posts", types(&project)),
        Some(&posts_def),
        &[("idempotency-key", &key()), ("if-match", "\"v1\"")],
    )
    .await
    .expect("request");
    assert_eq!(linked.status, StatusCode::OK, "{}", linked.body);
    let ada = send(
        &owner,
        Method::POST,
        &format!("{}/entries", collection(&project, "writers")),
        Some(&json!({"name": "Ada"})),
    )
    .await;
    assert_eq!(ada.status, StatusCode::CREATED, "{}", ada.body);
    let ada = ada.body["data"]["id"].as_str().expect("an id").to_owned();

    let file = [
        format!(r#"{{"headline":"One","writer":"{ada}"}}"#),
        format!(r#"{{"headline":"Two","writer":"{ada}"}}"#),
    ]
    .join("\n");
    let (status, _, reply) = upload(
        &owner,
        &format!("{}/import", collection(&project, "posts")),
        &file,
        &key(),
    )
    .await;
    assert_eq!(
        (status, &reply["data"]["imported"]),
        (StatusCode::CREATED, &json!(2)),
        "{reply}"
    );
    let mirror = call(
        &owner,
        Method::GET,
        &format!("{}/entries/{ada}", collection(&project, "writers")),
        None,
        &[],
    )
    .await
    .expect("request");
    let posts = mirror.body["data"]["content"]["posts"]
        .as_array()
        .expect("a list");
    assert_eq!(posts.len(), 2, "{}", mirror.body);

    // A single type takes its one entry, then refuses a second file.
    let single_def = json!({
        "name": "Home", "title_field": "title", "single": true,
        "fields": [{"id": "title", "label": "Title", "type": "text", "required": true}]
    });
    assert_eq!(
        write_type(&owner, &project, "home", &single_def)
            .await
            .status,
        StatusCode::CREATED
    );
    let home = format!("{}/import", collection(&project, "home"));
    let (status, _, reply) = upload(&owner, &home, r#"{"title":"Welcome"}"#, &key()).await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    let (status, _, reply) = upload(&owner, &home, r#"{"title":"Again"}"#, &key()).await;
    assert_eq!(
        (status, &reply["error"]["details"]["reason"]),
        (StatusCode::CONFLICT, &json!("single_exists")),
        "{reply}"
    );

    // Past the bound the file is refused, not truncated.
    let many = (0..=512)
        .map(|index| format!(r#"{{"headline":"Post {index}"}}"#))
        .collect::<Vec<_>>()
        .join("\n");
    let (status, _, reply) = upload(
        &owner,
        &format!("{}/import", collection(&project, "posts")),
        &many,
        &key(),
    )
    .await;
    assert_eq!(
        (status, &reply["error"]["details"]["reason"]),
        (StatusCode::UNPROCESSABLE_ENTITY, &json!("too_many_items")),
        "{reply}"
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn an_export_needs_every_field_but_may_stop_at_ones_own() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let notes: Value = serde_json::from_str(NOTES_DEF).expect("definition");
    assert_eq!(
        write_type(&owner, &project, NOTES, &notes).await.status,
        StatusCode::CREATED
    );
    for title in ["Mine", "Also mine"] {
        let made = send(
            &owner,
            Method::POST,
            &format!("{}/entries", collection(&project, NOTES)),
            Some(&json!({"title": title, "note": title})),
        )
        .await;
        assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    }
    let export = format!("{}/export", collection(&project, NOTES));
    let roles = format!("/api/v1/projects/{project}/roles");

    // A role that reads only the title field can't export: the file wouldn't come back whole.
    let titled = json!({
        "key": "titles-only", "name": "Titles only",
        "permissions": [{"action": "content.read", "subject": "notes", "fields": ["title"]}]
    });
    assert_eq!(
        send(&owner, Method::POST, &roles, Some(&titled))
            .await
            .status,
        StatusCode::CREATED
    );
    let peeker = sign_in("dev-editor").await;
    join(&owner, &peeker, &project, "titles-only").await;
    let (status, _, body) = download(&peeker, &export).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let envelope: Value = serde_json::from_str(&body).expect("the error envelope");
    assert_eq!(
        envelope["error"]["details"]["reason"],
        "fields_not_readable"
    );

    // A role that reads every field of its own entries exports those and no one else's.
    // (`own` can't condition a create — there is no entry yet to be own — so create is plain.)
    let own = json!({
        "key": "own-notes", "name": "Own notes",
        "permissions": [
            {"action": "content.read", "subject": "notes", "condition": "own"},
            {"action": "content.create", "subject": "notes"}
        ]
    });
    assert_eq!(
        send(&owner, Method::POST, &roles, Some(&own)).await.status,
        StatusCode::CREATED
    );
    // The guest's role moves to it; now their export holds only what they wrote themselves.
    let members = call(
        &owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/members"),
        None,
        &[],
    )
    .await
    .expect("request");
    let guest_id = members.body["data"][1]["identity_id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let moved = call(
        &owner,
        Method::PUT,
        &format!("/api/v1/projects/{project}/members/{guest_id}"),
        Some(&json!({"role": "own-notes"})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(moved.status, StatusCode::NO_CONTENT, "{}", moved.body);
    let made = send(
        &peeker,
        Method::POST,
        &format!("{}/entries", collection(&project, NOTES)),
        Some(&json!({"title": "Theirs", "note": "theirs"})),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let (status, _, exported) = download(&peeker, &export).await;
    assert_eq!(status, StatusCode::OK, "{exported}");
    let out = lines(&exported);
    assert_eq!(out.len(), 1, "only their own: {exported}");
    assert!(out[0].contains(r#""title":"Theirs""#), "{exported}");
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_dry_run_reports_each_item_and_lands_nothing() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let notes: Value = serde_json::from_str(NOTES_DEF).expect("definition");
    assert_eq!(
        write_type(&owner, &project, NOTES, &notes).await.status,
        StatusCode::CREATED
    );
    let export = format!("{}/export", collection(&project, NOTES));
    let import = format!("{}/import", collection(&project, NOTES));

    // A plan answers per-item verdicts — `would_create` where it would land, `invalid`
    // with the refusal where it wouldn't — without an `Idempotency-Key` (D-114).
    let file = [
        r#"{"title":"First","note":"one"}"#,
        r#"{"note":"no title"}"#,
        r#"{"title":"Third","bogus":true}"#,
    ]
    .join("\n");
    let (status, reply) = plan(&owner, &format!("{import}?dry_run=true"), &file).await;
    assert_eq!(
        (status, &reply["data"]["dry_run"]),
        (StatusCode::OK, &json!(true)),
        "{reply}"
    );
    let outcomes = reply["data"]["outcomes"].as_array().expect("rows");
    assert_eq!(outcomes.len(), 3, "{reply}");
    assert_eq!(outcomes[0]["status"], "would_create", "{reply}");
    assert_eq!(outcomes[0]["item"], "/line/1", "{reply}");
    assert_eq!(outcomes[1]["status"], "invalid", "{reply}");
    assert_eq!(
        outcomes[1]["refusal"]["pointer"], "/line/2/title",
        "{reply}"
    );
    assert_eq!(outcomes[2]["refusal"]["code"], "unknown_field", "{reply}");

    // The plan wrote nothing: the collection is still empty, and applying the file that
    // held an invalid item still refuses as a whole.
    let (_, _, exported) = download(&owner, &export).await;
    assert_eq!(
        lines(&exported).len(),
        0,
        "a plan writes nothing: {exported}"
    );
    let (status, _, reply) = upload(&owner, &import, &file, &key()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
    let (_, _, exported) = download(&owner, &export).await;
    assert_eq!(lines(&exported).len(), 0, "nothing landed: {exported}");

    // A clean file plans then applies, and the apply's own report says what it did.
    let file = [r#"{"title":"First"}"#, r#"{"title":"Second"}"#].join("\n");
    let (status, reply) = plan(&owner, &format!("{import}?dry_run=true"), &file).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(
        reply["data"]["outcomes"].as_array().map(Vec::len),
        Some(2),
        "{reply}"
    );
    let (status, _, reply) = upload(&owner, &import, &file, &key()).await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    assert_eq!(reply["data"]["imported"], 2, "{reply}");
    let outcomes = reply["data"]["outcomes"].as_array().expect("rows");
    assert_eq!(outcomes[0]["status"], "created", "{reply}");
    assert_eq!(outcomes[1]["status"], "created", "{reply}");
    assert!(outcomes[0]["entry"].as_str().is_some(), "{reply}");
    let (_, _, exported) = download(&owner, &export).await;
    assert_eq!(lines(&exported).len(), 2, "{exported}");
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn strapi_and_sanity_documents_import_with_their_source_kept() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let notes: Value = serde_json::from_str(NOTES_DEF).expect("definition");
    assert_eq!(
        write_type(&owner, &project, NOTES, &notes).await.status,
        StatusCode::CREATED
    );
    let export = format!("{}/export", collection(&project, NOTES));
    let import = format!("{}/import", collection(&project, NOTES));

    // Strapi v4's `{id, attributes}` entity, v5's flat `documentId` document and the
    // `{"data": […], "meta": {…}}` API envelope all land as the same documents; the
    // report keeps the source id and names the bookkeeping keys set aside.
    let strapi = [
        r#"{"id": 42, "attributes": {"title": "Fourth", "note": "v4", "createdAt": "2024-01-01", "locale": "en"}}"#,
        r#"{"documentId": "a1b2c3", "title": "Fifth", "note": "v5", "status": "published"}"#,
    ]
    .join("\n");
    let (status, _, reply) = upload(&owner, &import, &strapi, &key()).await;
    assert_eq!(
        (status, &reply["data"]["imported"]),
        (StatusCode::CREATED, &json!(2)),
        "{reply}"
    );
    let outcomes = reply["data"]["outcomes"].as_array().expect("rows");
    assert_eq!(outcomes[0]["source"], "42", "{reply}");
    assert_eq!(
        outcomes[0]["unmapped"].as_array().map(Vec::len),
        Some(1),
        "createdAt is reported, not imported: {reply}"
    );
    assert_eq!(outcomes[1]["source"], "a1b2c3", "{reply}");

    let envelope = r#"{"data": [{"id": 7, "attributes": {"title": "Sixth", "note": "envelope"}}], "meta": {"pagination": {"page": 1}}}"#;
    let (status, _, reply) = upload(&owner, &import, envelope, &key()).await;
    assert_eq!(
        (status, &reply["data"]["imported"]),
        (StatusCode::CREATED, &json!(1)),
        "{reply}"
    );
    assert_eq!(reply["data"]["outcomes"][0]["item"], "/data/0", "{reply}");

    // Sanity NDJSON: `_id`/`_type` map to `source`/`kind`, other `_`-keys are reported.
    let sanity = [
        r#"{"_id": "drafts.7f1c", "_type": "notes", "_rev": "r3", "_createdAt": "2024-05-05", "title": "Seventh", "note": "sanity"}"#,
    ]
    .join("\n");
    let (status, _, reply) = upload(&owner, &import, &sanity, &key()).await;
    assert_eq!(
        (status, &reply["data"]["imported"]),
        (StatusCode::CREATED, &json!(1)),
        "{reply}"
    );
    let outcome = &reply["data"]["outcomes"][0];
    assert_eq!(outcome["source"], "drafts.7f1c", "{reply}");
    let unmapped: Vec<&str> = outcome["unmapped"]
        .as_array()
        .expect("keys")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(unmapped, ["_createdAt", "_rev"], "{reply}");

    // A document whose `_type` isn't this collection refuses the item's import.
    let foreign = r#"{"_id": "x", "_type": "posts", "title": "Wrong"}"#;
    let (status, _, reply) = upload(&owner, &import, foreign, &key()).await;
    assert_eq!(
        (status, &reply["error"]["details"]["reason"]),
        (StatusCode::UNPROCESSABLE_ENTITY, &json!("type_mismatch")),
        "{reply}"
    );
    // `?type=` takes just that `_type` out of a mixed file; the rest are `skipped`.
    let mixed = [
        r#"{"_id": "n1", "_type": "notes", "title": "Taken"}"#,
        r#"{"_id": "p1", "_type": "posts", "title": "Skipped"}"#,
    ]
    .join("\n");
    let (status, _, reply) = upload(&owner, &format!("{import}?type=notes"), &mixed, &key()).await;
    assert_eq!(
        (status, &reply["data"]["imported"]),
        (StatusCode::CREATED, &json!(1)),
        "{reply}"
    );
    let outcomes = reply["data"]["outcomes"].as_array().expect("rows");
    assert_eq!(outcomes[0]["status"], "created", "{reply}");
    assert_eq!(outcomes[1]["status"], "skipped", "{reply}");
    let (_, _, exported) = download(&owner, &export).await;
    let out = lines(&exported);
    assert_eq!(
        out.len(),
        5,
        "two Strapi, one envelope, one Sanity, one taken: {exported}"
    );
    assert!(out[0].contains(r#""title":"Taken""#), "{exported}");
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_source_id_that_names_an_entry_updates_it() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let notes: Value = serde_json::from_str(NOTES_DEF).expect("definition");
    assert_eq!(
        write_type(&owner, &project, NOTES, &notes).await.status,
        StatusCode::CREATED
    );
    let export = format!("{}/export", collection(&project, NOTES));
    let import = format!("{}/import", collection(&project, NOTES));
    let entries = format!("{}/entries", collection(&project, NOTES));

    // An entry created through the API, then an item whose source id is its UUID:
    // a save onto that draft, not a duplicate.
    let made = send(
        &owner,
        Method::POST,
        &entries,
        Some(&json!({"title": "Original", "note": "first"})),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let entry = made.body["data"]["id"].as_str().expect("an id").to_owned();

    let file = format!(
        r#"{{"documentId": "{entry}", "title": "Renamed", "note": "second", "status": "published"}}"#
    );
    let (status, _, reply) = upload(&owner, &import, &file, &key()).await;
    assert_eq!(
        (status, &reply["data"]["imported"]),
        (StatusCode::CREATED, &json!(1)),
        "{reply}"
    );
    let outcome = &reply["data"]["outcomes"][0];
    assert_eq!(outcome["status"], "updated", "{reply}");
    assert_eq!(outcome["entry"], entry.as_str(), "{reply}");
    assert_eq!(outcome["source"], entry.as_str(), "{reply}");
    let draft = call(
        &owner,
        Method::GET,
        &format!("{entries}/{entry}"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        draft.body["data"]["content"]["title"], "Renamed",
        "{}",
        draft.body
    );
    let (_, _, exported) = download(&owner, &export).await;
    assert_eq!(lines(&exported).len(), 1, "still one entry: {exported}");

    // A dry run says `would_update` for the same item, and a source id that names nothing
    // is adopted: the item lands with the source's own UUID as its entry id.
    let (status, reply) = plan(&owner, &format!("{import}?dry_run=true"), &file).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(
        reply["data"]["outcomes"][0]["status"], "would_update",
        "{reply}"
    );

    let fresh = "0194b456-78ab-7cde-8f01-23456789abcd";
    let (status, _, reply) = upload(
        &owner,
        &import,
        &format!(r#"{{"_id": "{fresh}", "_type": "notes", "title": "Adopted"}}"#),
        &key(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    let outcome = &reply["data"]["outcomes"][0];
    assert_eq!(
        (outcome["status"].as_str(), outcome["entry"].as_str()),
        (Some("created"), Some(fresh)),
        "{reply}"
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn an_export_comes_back_as_identical_documents_and_unknown_collections_refuse() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let notes: Value = serde_json::from_str(NOTES_DEF).expect("definition");
    for slug in [NOTES, "copies"] {
        assert_eq!(
            write_type(&owner, &project, slug, &notes).await.status,
            StatusCode::CREATED
        );
    }
    let export = format!("{}/export", collection(&project, NOTES));

    // Three drafts exported feed straight back into a same-definition collection: the
    // documents come out byte-identical (ids are the new entries' own, not carried).
    let file = [
        r#"{"title":"Alpha","note":"a"}"#,
        r#"{"title":"Beta","note":"b"}"#,
        r#"{"title":"Gamma"}"#,
    ]
    .join("\n");
    let (status, _, reply) = upload(
        &owner,
        &format!("{}/import", collection(&project, NOTES)),
        &file,
        &key(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    let (_, _, first) = download(&owner, &export).await;
    let mut before = lines(&first)
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    before.sort();

    let second = format!("{}/import", collection(&project, "copies"));
    let (status, _, reply) = upload(&owner, &second, &first, &key()).await;
    assert_eq!(
        (status, &reply["data"]["imported"]),
        (StatusCode::CREATED, &json!(3)),
        "{reply}"
    );
    let (_, _, copied) = download(
        &owner,
        &format!("{}/export", collection(&project, "copies")),
    )
    .await;
    let mut after = lines(&copied)
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    after.sort();
    assert_eq!(before, after, "round-trip keeps the documents");

    // A collection that doesn't exist is 404 before a byte of the body matters.
    let (status, _, reply) = upload(
        &owner,
        &format!("{}/import", collection(&project, "ghosts")),
        r#"{"title":"Boo"}"#,
        &key(),
    )
    .await;
    assert_eq!(
        (status, &reply["error"]["code"]),
        (StatusCode::NOT_FOUND, &json!("not_found")),
        "{reply}"
    );
}
