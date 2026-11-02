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
