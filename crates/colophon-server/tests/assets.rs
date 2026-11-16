//! Uploads and verification end to end (07-media): a real image is quarantined, decoded and
//! marked usable; a file that is not what it claims is rejected; bad uploads never reach the
//! store; a replayed idempotency key returns the same asset. Needs the stack (Garage holds the
//! objects) and `scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Session, app, call, join, key, project, sign_in};
use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Workers that look for work often, so the tests don't wait.
const FAST: [(&str, &str); 1] = [("COLOPHON_WORKER_POLL_MS", "50")];

/// A real 1x1 RGB PNG (69 bytes), generated for this suite.
const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0,
    0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 0, 0, 3, 1,
    1, 0, 201, 254, 146, 239, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

/// A real 400×4 PNG (99 bytes): wider than `w320` but not `w1280`, so exactly one rendition.
const WIDE_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 1, 144, 0, 0, 0, 4, 8, 2,
    0, 0, 0, 47, 93, 254, 122, 0, 0, 0, 42, 73, 68, 65, 84, 120, 156, 237, 206, 65, 9, 0, 48, 12,
    4, 176, 74, 159, 243, 206, 194, 253, 142, 66, 32, 2, 50, 243, 22, 224, 134, 254, 0, 32, 212,
    31, 0, 132, 250, 3, 128, 204, 7, 24, 121, 90, 72, 220, 227, 41, 34, 0, 0, 0, 0, 73, 69, 78, 68,
    174, 66, 96, 130,
];

/// The assets path of `project`'s production environment.
fn assets(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production/assets")
}

/// `POST` bytes (not JSON) with the upload headers.
async fn upload(
    session: &Session,
    project: &str,
    bytes: &[u8],
    name: &str,
    kind: &str,
    key: &str,
) -> (StatusCode, Value) {
    let response = app()
        .post(format!(
            "{}{assets}",
            common::ORIGIN,
            assets = assets(project)
        ))
        .header(header::COOKIE, &session.cookie)
        .header("x-csrf-token", &session.csrf)
        .header(header::ORIGIN, common::ORIGIN)
        .header("idempotency-key", key)
        .header("x-colophon-file-name", name)
        .header(header::CONTENT_TYPE, kind)
        .body(bytes.to_vec())
        .send()
        .await
        .expect("request");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

/// Waits up to `seconds` for the asset to leave `quarantined`; its final record.
async fn settled(session: &Session, project: &str, id: &str, seconds: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        let reply = call(
            session,
            Method::GET,
            &format!("{}/{id}", assets(project)),
            None,
            &[],
        )
        .await
        .expect("request");
        if reply.body["data"]["state"].as_str() != Some("quarantined") {
            return reply.body["data"].clone();
        }
        assert!(
            Instant::now() < deadline,
            "asset {id} never left quarantine: {}",
            reply.body
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Waits up to `seconds` for the asset to reach `state`; its final record.
async fn reached(session: &Session, project: &str, id: &str, state: &str, seconds: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        let reply = call(
            session,
            Method::GET,
            &format!("{}/{id}", assets(project)),
            None,
            &[],
        )
        .await
        .expect("request");
        if reply.body["data"]["state"].as_str() == Some(state) {
            return reply.body["data"].clone();
        }
        assert!(
            Instant::now() < deadline,
            "asset {id} never reached {state}: {}",
            reply.body
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// `DELETE {assets}/{id}` under `key`; the reply.
async fn delete_asset(session: &Session, project: &str, id: &str, key: &str) -> common::Reply {
    call(
        session,
        Method::DELETE,
        &format!("{}/{id}", assets(project)),
        None,
        &[("idempotency-key", key)],
    )
    .await
    .expect("request")
}

/// `POST {assets}/{id}/replace` with `bytes`, the upload headers and `key`; the reply.
async fn replace(
    session: &Session,
    project: &str,
    id: &str,
    bytes: &[u8],
    name: &str,
    kind: &str,
    key: &str,
) -> common::Reply {
    let response = app()
        .post(format!(
            "{}{assets}/{id}/replace",
            common::ORIGIN,
            assets = assets(project)
        ))
        .header(header::COOKIE, &session.cookie)
        .header("x-csrf-token", &session.csrf)
        .header(header::ORIGIN, common::ORIGIN)
        .header("idempotency-key", key)
        .header("x-colophon-file-name", name)
        .header(header::CONTENT_TYPE, kind)
        .body(bytes.to_vec())
        .send()
        .await
        .expect("request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json().await.unwrap_or(Value::Null);
    common::Reply {
        status,
        headers,
        body,
    }
}

/// Waits up to `seconds` for the asset's current sha to differ from `was` (a replacement
/// landed); the asset's final record.
async fn swung(session: &Session, project: &str, id: &str, was: &str, seconds: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        let reply = call(
            session,
            Method::GET,
            &format!("{}/{id}", assets(project)),
            None,
            &[],
        )
        .await
        .expect("request");
        if reply.body["data"]["sha256"].as_str() != Some(was) {
            return reply.body["data"].clone();
        }
        assert!(
            Instant::now() < deadline,
            "asset {id} never swung off {was}: {}",
            reply.body
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The asset's versions, oldest first (`GET`'s `versions` sibling).
async fn versions_of(session: &Session, project: &str, id: &str) -> Vec<Value> {
    let reply = call(
        session,
        Method::GET,
        &format!("{}/{id}", assets(project)),
        None,
        &[],
    )
    .await
    .expect("request");
    reply.body["versions"].as_array().expect("versions").clone()
}

/// The media origin's bytes for `path`, asserting 200.
async fn media_bytes(path: &str) -> Vec<u8> {
    let response = app()
        .get(format!("{}{}", common::ORIGIN, path))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    response.bytes().await.expect("bytes").to_vec()
}

/// The media origin's status for `path`, following nothing.
async fn media_status(path: &str) -> StatusCode {
    app()
        .get(format!("{}{}", common::ORIGIN, path))
        .send()
        .await
        .expect("request")
        .status()
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_valid_upload_is_verified_and_becomes_usable() {
    let _server = common::Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;

    let key = Uuid::now_v7().to_string();
    let (status, body) = upload(&owner, &project, PNG, "dot.png", "image/png", &key).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let id = body["data"]["id"].as_str().expect("id").to_owned();
    assert_eq!(body["data"]["state"], "quarantined", "{body}");
    assert_eq!(body["data"]["file_name"], "dot.png");

    let asset = settled(&owner, &project, &id, 15).await;
    assert_eq!(asset["state"], "usable", "{asset}");
    assert_eq!(asset["detected_type"], "image/png", "{asset}");
    assert_eq!(
        (asset["width"].as_i64(), asset["height"].as_i64()),
        (Some(1), Some(1)),
        "{asset}"
    );

    let list = call(&owner, Method::GET, &assets(&project), None, &[])
        .await
        .expect("request");
    let ids: Vec<_> = list.body["data"]
        .as_array()
        .expect("data")
        .iter()
        .filter_map(|asset| asset["id"].as_str())
        .collect();
    assert!(ids.contains(&id.as_str()), "{ids:?}");

    // The same key with the same upload answers the same asset, stored once.
    let (again, replay) = upload(&owner, &project, PNG, "dot.png", "image/png", &key).await;
    assert_eq!(again, StatusCode::ACCEPTED, "{replay}");
    assert_eq!(replay["data"]["id"], id, "{replay}");

    // The same key with a different upload is a replay mismatch, refused.
    let (status, body) = upload(&owner, &project, PNG, "other.png", "image/png", &key).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_file_that_is_not_an_image_is_rejected() {
    let _server = common::Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;

    // HTML with a .png name and an image Content-Type: the signature proves otherwise.
    let (status, body) = upload(
        &owner,
        &project,
        b"<html><body>not an image</body></html>",
        "photo.png",
        "image/png",
        &Uuid::now_v7().to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let id = body["data"]["id"].as_str().expect("id").to_owned();

    let asset = settled(&owner, &project, &id, 15).await;
    assert_eq!(asset["state"], "rejected", "{asset}");
    assert!(
        asset["rejected_reason"]
            .as_str()
            .is_some_and(|r| !r.is_empty()),
        "{asset}"
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn bad_uploads_are_refused_before_the_store() {
    let _server = common::Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;

    // No Idempotency-Key.
    let response = app()
        .post(format!("{}{}", common::ORIGIN, assets(&project)))
        .header(header::COOKIE, &owner.cookie)
        .header("x-csrf-token", &owner.csrf)
        .header(header::ORIGIN, common::ORIGIN)
        .header("x-colophon-file-name", "dot.png")
        .header(header::CONTENT_TYPE, "image/png")
        .body(PNG.to_vec())
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::PRECONDITION_REQUIRED);

    // No file name.
    let response = app()
        .post(format!("{}{}", common::ORIGIN, assets(&project)))
        .header(header::COOKIE, &owner.cookie)
        .header("x-csrf-token", &owner.csrf)
        .header(header::ORIGIN, common::ORIGIN)
        .header("idempotency-key", Uuid::now_v7().to_string())
        .header(header::CONTENT_TYPE, "image/png")
        .body(PNG.to_vec())
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // A type the intent does not allow.
    let (status, body) = upload(
        &owner,
        &project,
        PNG,
        "notes.txt",
        "text/plain",
        &Uuid::now_v7().to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // Over the 20 MiB bound: refused by the intent, never stored.
    let big = vec![0u8; 20 * 1024 * 1024 + 1];
    let (status, body) = upload(
        &owner,
        &project,
        &big,
        "huge.png",
        "image/png",
        &Uuid::now_v7().to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

/// The environment base of `project` (an id) for types, entries and changesets.
fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
}

/// A `covers` type with one `photo` asset field, made in `project`.
async fn covers(session: &Session, project: &str) {
    let made = call(
        session,
        Method::POST,
        &format!("{}/content-types", base(project)),
        Some(&json!({
            "slug": "covers",
            "definition": {
                "name": "Covers",
                "title_field": "title",
                "fields": [
                    {"id": "title", "label": "Title", "type": "text", "required": true},
                    {"id": "photo", "label": "Photo", "type": "asset", "many": false}
                ]
            }
        })),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
}

/// A `covers` entry naming `photo` (an asset id); its entry id.
async fn cover(session: &Session, project: &str, title: &str, photo: &str) -> String {
    let reply = call(
        session,
        Method::POST,
        &format!("{}/collections/covers/entries", base(project)),
        Some(&json!({"title": title, "photo": photo})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    reply.body["data"]["id"].as_str().expect("an id").to_owned()
}

/// A changeset of `entry`, submitted by `editor` and approved by `owner`; its path and hash.
async fn approved(
    owner: &Session,
    editor: &Session,
    project: &str,
    entry: &str,
) -> (String, String) {
    let created = call(
        editor,
        Method::POST,
        &format!("{}/changesets", base(project)),
        Some(&json!({"title": "Release", "entries": [entry]})),
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
    let if_match = format!(
        "\"{}\"",
        created.body["data"]["hash"].as_str().expect("a hash")
    );
    let hash = created.body["data"]["hash"]
        .as_str()
        .expect("a hash")
        .to_owned();
    let submitted = call(
        editor,
        Method::POST,
        &format!("{path}/submit"),
        None,
        &[("if-match", &if_match)],
    )
    .await
    .expect("request");
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);
    let review = call(
        owner,
        Method::POST,
        &format!("{path}/reviews"),
        Some(&json!({"decision": "approve"})),
        &[("if-match", &if_match)],
    )
    .await
    .expect("request");
    assert_eq!(
        review.body["data"]["status"]["state"], "approved",
        "{}",
        review.body
    );
    (path, hash)
}

/// `POST {changeset}/publish` on `base`; the reply.
async fn publish(session: &Session, path: &str, hash: &str, base: i64) -> common::Reply {
    call(
        session,
        Method::POST,
        &format!("{path}/publish"),
        Some(&json!({"base_generation": base})),
        &[
            ("if-match", &format!("\"{hash}\"")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request")
}

/// The media origin path for `asset` in `project`'s production environment.
fn media(project: &str, asset: &str) -> String {
    format!("/media/v1/{project}/production/{asset}")
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_release_waits_for_usable_assets_and_the_origin_serves_them() {
    let _server = common::Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let editor = sign_in("dev-editor").await;
    let project = project(&owner).await;
    join(&owner, &editor, &project, "editor").await;
    covers(&owner, &project).await;

    // A draft may name an upload that isn't usable yet; a release may not publish it. The
    // upload here is a fake: whatever state the publish lands in — still quarantined or
    // already rejected — it is not usable, so the refusal is not a race.
    let (status, body) = upload(
        &editor,
        &project,
        b"not an image at all",
        "fake.png",
        "image/png",
        &Uuid::now_v7().to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let fake = body["data"]["id"].as_str().expect("id").to_owned();
    let entry = cover(&editor, &project, "First cover", &fake).await;
    let (path, hash) = approved(&owner, &editor, &project, &entry).await;
    let refused = publish(&owner, &path, &hash, 0).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
    assert_eq!(
        refused.body["error"]["details"]["reason"], "asset_not_usable",
        "{}",
        refused.body
    );

    // It stays refused once the upload is rejected for good, and the origin has no object.
    assert_eq!(
        settled(&owner, &project, &fake, 15).await["state"],
        "rejected"
    );
    let refused = publish(&owner, &path, &hash, 0).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
    let response = app()
        .get(format!("{}{}", common::ORIGIN, media(&project, &fake)))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // A usable upload publishes, and the public origin serves the verified bytes to
    // anyone, cached forever.
    let (status, body) = upload(
        &editor,
        &project,
        PNG,
        "dot.png",
        "image/png",
        &Uuid::now_v7().to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let asset = body["data"]["id"].as_str().expect("id").to_owned();
    assert_eq!(
        settled(&owner, &project, &asset, 15).await["state"],
        "usable"
    );
    let second = cover(&editor, &project, "Second cover", &asset).await;
    let (path, hash) = approved(&owner, &editor, &project, &second).await;
    let published = publish(&owner, &path, &hash, 0).await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);

    let response = app()
        .get(format!("{}{}", common::ORIGIN, media(&project, &asset)))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(
        response.headers()["cache-control"],
        "public, max-age=31536000, immutable"
    );
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    let bytes = response.bytes().await.expect("bytes");
    assert!(
        bytes.starts_with(&[137, 80, 78, 71]),
        "not a PNG: {bytes:?}"
    );

    // A draft that names an asset that does not exist is refused at save.
    let missing = Uuid::now_v7().to_string();
    let reply = call(
        &editor,
        Method::POST,
        &format!("{}/collections/covers/entries", base(&project)),
        Some(&json!({"title": "Ghost", "photo": missing})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        reply.body
    );
    assert_eq!(
        reply.body["error"]["details"]["reason"], "asset_not_found",
        "{}",
        reply.body
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn renditions_are_served_under_their_names() {
    let _server = common::Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;

    // A 400px-wide upload: `w320` exists, `w1280` doesn't (nothing upscales).
    let (status, body) = upload(
        &owner,
        &project,
        WIDE_PNG,
        "wide.png",
        "image/png",
        &Uuid::now_v7().to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let asset = body["data"]["id"].as_str().expect("id").to_owned();
    let settled = settled(&owner, &project, &asset, 15).await;
    assert_eq!(settled["state"], "usable", "{settled}");
    assert_eq!(settled["derivatives"], json!(["w320"]), "{settled}");

    // The original, its rendition, and nothing else answers.
    for path in [
        media(&project, &asset),
        format!("{}/w320", media(&project, &asset)),
    ] {
        let response = app()
            .get(format!("{}{}", common::ORIGIN, path))
            .send()
            .await
            .expect("request");
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(response.headers()["content-type"], "image/png");
        assert_eq!(
            response.headers()["cache-control"],
            "public, max-age=31536000, immutable"
        );
    }
    for name in ["w1280", "nope", "W320", "../../quarantine"] {
        let response = app()
            .get(format!(
                "{}{}/{}",
                common::ORIGIN,
                media(&project, &asset),
                name
            ))
            .send()
            .await
            .expect("request");
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{name}");
    }
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_usable_asset_is_deleted_and_its_objects_go() {
    let _server = common::Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;

    // A 400px-wide upload, so a rendition exists beside the original.
    let (status, body) = upload(
        &owner,
        &project,
        WIDE_PNG,
        "wide.png",
        "image/png",
        &Uuid::now_v7().to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let asset = body["data"]["id"].as_str().expect("id").to_owned();
    assert_eq!(
        settled(&owner, &project, &asset, 15).await["state"],
        "usable"
    );
    for path in [
        media(&project, &asset),
        format!("{}/w320", media(&project, &asset)),
    ] {
        assert_eq!(media_status(&path).await, StatusCode::OK, "{path}");
    }
    covers(&owner, &project).await;

    // 202 is "requested", not gone: the delete job decides the asset from here.
    let deleted = delete_asset(&owner, &project, &asset, &key()).await;
    assert_eq!(deleted.status, StatusCode::ACCEPTED, "{}", deleted.body);
    assert_eq!(deleted.body["data"]["id"], asset);

    // From the moment the request commits nothing new may name it — the pending job, then the
    // tombstone, refuse it alike, so this is not a race against the worker: a save that comes
    // in now answers `asset_not_found`, like an id that never was.
    let late = call(
        &owner,
        Method::POST,
        &format!("{}/collections/covers/entries", base(&project)),
        Some(&json!({"title": "Late", "photo": asset})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(
        late.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        late.body
    );
    assert_eq!(
        late.body["error"]["details"]["reason"], "asset_not_found",
        "{}",
        late.body
    );

    // The tombstone lands: the row stays and still reads, its objects are gone.
    let tombstone = reached(&owner, &project, &asset, "deleted", 15).await;
    assert_eq!(tombstone["state"], "deleted", "{tombstone}");
    for path in [
        media(&project, &asset),
        format!("{}/w320", media(&project, &asset)),
    ] {
        assert_eq!(media_status(&path).await, StatusCode::NOT_FOUND, "{path}");
    }

    // Gone means gone: another request under a new key gets `already_deleted`.
    let again = delete_asset(&owner, &project, &asset, &key()).await;
    assert_eq!(again.status, StatusCode::CONFLICT, "{}", again.body);
    assert_eq!(
        again.body["error"]["details"]["reason"], "already_deleted",
        "{}",
        again.body
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn an_asset_a_draft_names_cannot_be_deleted() {
    let _server = common::Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    covers(&owner, &project).await;

    let (status, body) = upload(
        &owner,
        &project,
        PNG,
        "dot.png",
        "image/png",
        &Uuid::now_v7().to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let asset = body["data"]["id"].as_str().expect("id").to_owned();
    assert_eq!(
        settled(&owner, &project, &asset, 15).await["state"],
        "usable"
    );
    let entry = cover(&owner, &project, "First cover", &asset).await;

    let refused = delete_asset(&owner, &project, &asset, &key()).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
    assert_eq!(
        refused.body["error"]["details"]["reason"], "still_referenced",
        "{}",
        refused.body
    );
    let referrers = refused.body["error"]["details"]["referenced_by"]
        .as_array()
        .expect("referenced_by");
    assert!(
        referrers.iter().any(|referrer| {
            referrer["entry_id"].as_str() == Some(entry.as_str())
                && referrer["in_draft"].as_bool() == Some(true)
        }),
        "{referrers:?}"
    );

    // The references endpoint names the same hold-up.
    let listed = call(
        &owner,
        Method::GET,
        &format!("{}/{asset}/references", assets(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    let data = listed.body["data"].as_array().expect("data");
    assert!(
        data.iter().any(|referrer| {
            referrer["entry_id"].as_str() == Some(entry.as_str())
                && referrer["field"].as_str() == Some("photo")
                && referrer["in_draft"].as_bool() == Some(true)
                && referrer["published"].as_bool() == Some(false)
        }),
        "{data:?}"
    );

    // And nothing was touched: it is still there, still usable.
    let reply = call(
        &owner,
        Method::GET,
        &format!("{}/{asset}", assets(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(reply.body["data"]["state"], "usable", "{}", reply.body);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn an_asset_a_publication_names_cannot_be_deleted() {
    let _server = common::Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let editor = sign_in("dev-editor").await;
    let project = project(&owner).await;
    join(&owner, &editor, &project, "editor").await;
    covers(&owner, &project).await;

    let (status, body) = upload(
        &editor,
        &project,
        PNG,
        "dot.png",
        "image/png",
        &Uuid::now_v7().to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let asset = body["data"]["id"].as_str().expect("id").to_owned();
    assert_eq!(
        settled(&owner, &project, &asset, 15).await["state"],
        "usable"
    );
    let entry = cover(&editor, &project, "Published cover", &asset).await;
    let (path, hash) = approved(&owner, &editor, &project, &entry).await;
    let published = publish(&owner, &path, &hash, 0).await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);

    // A publication pins its assets forever — history included (D-098).
    let refused = delete_asset(&owner, &project, &asset, &key()).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
    assert_eq!(
        refused.body["error"]["details"]["reason"], "still_referenced",
        "{}",
        refused.body
    );
    let referrers = refused.body["error"]["details"]["referenced_by"]
        .as_array()
        .expect("referenced_by");
    assert!(
        referrers.iter().any(|referrer| {
            referrer["entry_id"].as_str() == Some(entry.as_str())
                && referrer["published"].as_bool() == Some(true)
        }),
        "{referrers:?}"
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_replayed_delete_returns_the_recorded_answer() {
    let _server = common::Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;

    let (status, body) = upload(
        &owner,
        &project,
        PNG,
        "dot.png",
        "image/png",
        &Uuid::now_v7().to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let asset = body["data"]["id"].as_str().expect("id").to_owned();
    assert_eq!(
        settled(&owner, &project, &asset, 15).await["state"],
        "usable"
    );

    let key = key();
    let first = delete_asset(&owner, &project, &asset, &key).await;
    assert_eq!(first.status, StatusCode::ACCEPTED, "{}", first.body);
    let replay = delete_asset(&owner, &project, &asset, &key).await;
    assert_eq!(replay.status, StatusCode::ACCEPTED, "{}", replay.body);
    assert!(replay.replayed(), "expected an idempotent replay");
    assert_eq!(replay.body, first.body);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_quarantined_asset_cannot_be_deleted() {
    // No worker in this server: the upload waits in quarantine forever, so the state the
    // refusal meets is deterministic (elsewhere it would be a coin toss against verification).
    let _server =
        common::Server::start_with(&[("COLOPHON_ROLE", "api"), ("COLOPHON_WORKER_POLL_MS", "50")])
            .await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;

    let (status, body) = upload(
        &owner,
        &project,
        PNG,
        "dot.png",
        "image/png",
        &Uuid::now_v7().to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let asset = body["data"]["id"].as_str().expect("id").to_owned();
    let reply = call(
        &owner,
        Method::GET,
        &format!("{}/{asset}", assets(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(reply.body["data"]["state"], "quarantined", "{}", reply.body);

    // A delete is a write like any other: no Idempotency-Key, no request.
    let no_key = call(
        &owner,
        Method::DELETE,
        &format!("{}/{asset}", assets(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(no_key.status, StatusCode::PRECONDITION_REQUIRED);

    // Verification may still land it usable, so deleting it is refused, not queued.
    let refused = delete_asset(&owner, &project, &asset, &key()).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
    assert_eq!(
        refused.body["error"]["details"]["reason"], "asset_quarantined",
        "{}",
        refused.body
    );
}

/// The retention sweep's own knobs: a pass every 50 ms, collectable after the spec's 24 h —
/// the tests backdate what must be old, so the default expiry still applies.
const SWEEP: [(&str, &str); 2] = [
    ("COLOPHON_WORKER_POLL_MS", "50"),
    ("COLOPHON_ASSET_SWEEP_MS", "50"),
];

/// Records `asset`'s upload two days ago, so the 24-hour expiry applies to it now.
async fn age_asset(database: &sqlx::PgPool, project: &str, asset: &str) {
    sqlx::query(
        "UPDATE assets SET created_at = now() - interval '2 days',
                           verified_at = COALESCE(verified_at, now() - interval '2 days')
         WHERE project_id = $1::uuid AND id = $2::uuid",
    )
    .bind(project)
    .bind(asset)
    .execute(database)
    .await
    .expect("age the asset");
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn an_old_unreferenced_asset_expires_and_its_objects_go() {
    let _server = common::Server::start_with(&SWEEP).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;

    let (status, body) = upload(
        &owner,
        &project,
        PNG,
        "old.png",
        "image/png",
        &Uuid::now_v7().to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let asset = body["data"]["id"].as_str().expect("id").to_owned();
    assert_eq!(
        settled(&owner, &project, &asset, 15).await["state"],
        "usable"
    );
    assert_eq!(media_status(&media(&project, &asset)).await, StatusCode::OK);

    // Nothing ever names it: two days on, the sweep expires it through the same attempt a
    // delete request would ride — the tombstone lands, the object goes.
    let database = common::owner_database().await;
    age_asset(&database, &project, &asset).await;
    let tombstone = reached(&owner, &project, &asset, "deleted", 15).await;
    assert_eq!(tombstone["state"], "deleted", "{tombstone}");
    assert_eq!(
        media_status(&media(&project, &asset)).await,
        StatusCode::NOT_FOUND
    );
    database.close().await;
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_referenced_asset_survives_the_sweep() {
    let _server = common::Server::start_with(&SWEEP).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    covers(&owner, &project).await;

    let (status, body) = upload(
        &owner,
        &project,
        PNG,
        "kept.png",
        "image/png",
        &Uuid::now_v7().to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let asset = body["data"]["id"].as_str().expect("id").to_owned();
    assert_eq!(
        settled(&owner, &project, &asset, 15).await["state"],
        "usable"
    );
    let _entry = cover(&owner, &project, "Keeping cover", &asset).await;

    // Two days old but named by a live draft: however often the sweep passes, it never earns
    // an expire job — a live draft pins the asset like a delete request's keep-rule (D-099).
    let database = common::owner_database().await;
    age_asset(&database, &project, &asset).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let reply = call(
        &owner,
        Method::GET,
        &format!("{}/{asset}", assets(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(reply.body["data"]["state"], "usable", "{}", reply.body);
    let jobs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM asset_jobs
         WHERE project_id = $1::uuid AND asset_id = $2::uuid AND kind = 'expire'",
    )
    .bind(&project)
    .bind(&asset)
    .fetch_one(&database)
    .await
    .expect("expire jobs");
    assert_eq!(jobs, 0, "a named asset must not earn an expire job");
    database.close().await;
}

/// The local stack's object store, for objects a test writes or looks for directly.
fn store() -> object_store::aws::AmazonS3 {
    let env = common::local_env();
    let value = |name: &str| {
        env.iter()
            .find_map(|(key, value)| (key == name).then(|| value.clone()))
            .expect(name)
    };
    object_store::aws::AmazonS3Builder::new()
        .with_endpoint(value("COLOPHON_S3_ENDPOINT"))
        .with_region(value("COLOPHON_S3_REGION"))
        .with_bucket_name(value("COLOPHON_S3_BUCKET"))
        .with_access_key_id(value("COLOPHON_S3_ACCESS_KEY"))
        .with_secret_access_key(value("COLOPHON_S3_SECRET_KEY"))
        .with_allow_http(true)
        .build()
        .expect("object store")
}

/// Waits up to `seconds` for `path` to leave the store.
async fn object_gone(store: &object_store::aws::AmazonS3, path: &str, seconds: u64) {
    use object_store::ObjectStoreExt;
    let path = object_store::path::Path::from(path);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        match store.head(&path).await {
            Err(object_store::Error::NotFound { .. }) => return,
            other => {
                assert!(other.is_ok(), "head of {path}: {other:?}");
                assert!(Instant::now() < deadline, "{path} was never removed");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn an_orphaned_quarantine_object_is_swept() {
    // A two-second expiry, so an object written by hand is old moments later; the check
    // against `assets.quarantine_key` uses the object key, so no upload is needed. The keys
    // sort first — "0000" is below every real quarantine name (uuid v7s lead with "01...") —
    // because a pass lists at most 500 objects from the prefix's start, and the shared
    // bucket may hold a backlog the first passes still drain.
    use object_store::ObjectStoreExt;
    let _server = common::Server::start_with(&[
        ("COLOPHON_WORKER_POLL_MS", "50"),
        ("COLOPHON_ASSET_SWEEP_MS", "50"),
        ("COLOPHON_ASSET_EXPIRY_MS", "2000"),
    ])
    .await;
    let _owner = sign_in("dev-owner").await;
    let store = store();

    let orphan =
        object_store::path::Path::from(format!("quarantine/0000/production/{}", Uuid::now_v7()));
    store
        .put(&orphan, PNG.into())
        .await
        .expect("write the orphan");
    tokio::time::sleep(Duration::from_millis(2300)).await;
    object_gone(&store, orphan.as_ref(), 15).await;

    // An object younger than the expiry is never touched, whatever names nothing names it.
    let young =
        object_store::path::Path::from(format!("quarantine/0000/production/{}", Uuid::now_v7()));
    store
        .put(&young, PNG.into())
        .await
        .expect("write the young object");
    tokio::time::sleep(Duration::from_millis(300)).await;
    store.head(&young).await.expect("the young object stays");
    store.delete(&young).await.expect("cleanup");
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_quarantined_asset_whose_object_is_gone_still_expires() {
    let _server = common::Server::start_with(&SWEEP).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;

    // A real upload first, to stand the row beside: same project, environment and uploader.
    let (status, body) = upload(
        &owner,
        &project,
        PNG,
        "seed.png",
        "image/png",
        &Uuid::now_v7().to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let seed = body["data"]["id"].as_str().expect("id").to_owned();
    assert_eq!(
        settled(&owner, &project, &seed, 15).await["state"],
        "usable"
    );

    // A row exactly like an upload whose object was lost before verification: quarantined,
    // two days old, naming a quarantine key nothing ever wrote. No delete request may touch
    // a quarantined asset, but expiry can — and a missing object is not a reason to stay.
    let database = common::owner_database().await;
    let ghost: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO assets (project_id, environment_id, id, state, file_name, declared_type,
                             size_bytes, quarantine_key, created_by, created_at)
         SELECT project_id, environment_id, gen_random_uuid(), 'quarantined', 'ghost.png',
                'image/png', 69, $2, created_by, now() - interval '2 days'
         FROM assets WHERE project_id = $1::uuid AND id = $3::uuid
         LIMIT 1
         RETURNING id",
    )
    .bind(&project)
    .bind(format!(
        "quarantine/{project}/production/{}",
        Uuid::now_v7()
    ))
    .bind(&seed)
    .fetch_one(&database)
    .await
    .expect("the ghost asset");
    database.close().await;

    let tombstone = reached(&owner, &project, &ghost.to_string(), "deleted", 15).await;
    assert_eq!(tombstone["state"], "deleted", "{tombstone}");
}

// ---------------------------------------------------------------------------
// Byte versioning (D-136): a replacement is a new content-addressed object, never an
// overwrite — what a release or a pinned URL named keeps serving.

/// A usable asset on a fresh stack, with the server guard for the caller to hold: its id, its
/// first version's sha, and the bytes the current URL served as that version.
async fn usable_asset() -> (common::Server, Session, String, String, String, Vec<u8>) {
    let server = common::Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let (status, body) = upload(
        &owner,
        &project,
        PNG,
        "dot.png",
        "image/png",
        &Uuid::now_v7().to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let asset = body["data"]["id"].as_str().expect("id").to_owned();
    let first = settled(&owner, &project, &asset, 15).await;
    assert_eq!(first["state"], "usable", "{first}");
    let sha1 = first["sha256"].as_str().expect("a sha256").to_owned();
    assert_eq!(sha1.len(), 64, "a hex sha256: {sha1}");
    let one = versions_of(&owner, &project, &asset).await;
    assert_eq!(one.len(), 1, "{one:?}");
    assert_eq!(one[0]["seq"], 1, "{one:?}");
    assert_eq!(one[0]["sha256"], sha1, "{one:?}");
    let v1_bytes = media_bytes(&media(&project, &asset)).await;
    (server, owner, project, asset, sha1, v1_bytes)
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_replace_lands_a_new_version_and_the_old_keeps_serving() {
    let (_server, owner, project, asset, sha1, v1_bytes) = usable_asset().await;

    // A replace is a write like any other: same headers, own idempotency key, 202 queued.
    let asked = replace(
        &owner,
        &project,
        &asset,
        WIDE_PNG,
        "wide.png",
        "image/png",
        &key(),
    )
    .await;
    assert_eq!(asked.status, StatusCode::ACCEPTED, "{}", asked.body);

    // The swing: current version moves to the wide upload; the asset's columns describe it.
    let second = swung(&owner, &project, &asset, &sha1, 15).await;
    let sha2 = second["sha256"].as_str().expect("a new sha256").to_owned();
    assert_ne!(sha2, sha1, "{second}");
    assert_eq!(second["state"], "usable", "{second}");
    assert_eq!(
        (second["width"].as_i64(), second["height"].as_i64()),
        (Some(400), Some(4)),
        "{second}"
    );
    assert_eq!(second["derivatives"], json!(["w320"]), "{second}");

    // Both landings are on record: seq 1 the first, seq 2 the distinct replacement.
    let versions = versions_of(&owner, &project, &asset).await;
    assert_eq!(versions.len(), 2, "{versions:?}");
    assert_eq!(versions[0]["seq"], 1, "{versions:?}");
    assert_eq!(versions[0]["sha256"], sha1, "{versions:?}");
    assert_eq!(versions[1]["seq"], 2, "{versions:?}");
    assert_eq!(versions[1]["sha256"], sha2, "{versions:?}");

    // The bare path serves the current version — different bytes than version 1's, with its
    // rendition under it — and the pinned path serves version 1's exact bytes forever.
    let current = media_bytes(&media(&project, &asset)).await;
    assert_ne!(current, v1_bytes, "the current version moved on");
    assert_eq!(
        media_status(&format!("{}/w320", media(&project, &asset))).await,
        StatusCode::OK
    );
    assert_eq!(
        media_bytes(&format!("{}/{sha1}", media(&project, &asset))).await,
        v1_bytes,
        "the pinned version serves the bytes the first upload verified"
    );
    // A rendition of a pinned version answers under its own address — and only when the
    // version actually wrote one (the 1x1 first upload made no `w320`).
    assert_eq!(
        media_status(&format!("{}/{sha2}/w320", media(&project, &asset))).await,
        StatusCode::OK
    );
    assert_eq!(
        media_status(&format!("{}/{sha1}/w320", media(&project, &asset))).await,
        StatusCode::NOT_FOUND
    );
    // A 64-hex segment that names no version of this asset is a 404, like anything that
    // never was.
    assert_eq!(
        media_status(&format!("{}/{:0>64}", media(&project, &asset), "deadbeef")).await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_replace_replays_its_key_coalesces_identical_bytes_and_survives_a_refusal() {
    let (_server, owner, project, asset, sha1, _v1) = usable_asset().await;
    let second = replace(
        &owner,
        &project,
        &asset,
        WIDE_PNG,
        "wide.png",
        "image/png",
        &key(),
    )
    .await;
    assert_eq!(second.status, StatusCode::ACCEPTED, "{}", second.body);
    let sha2 = swung(&owner, &project, &asset, &sha1, 15).await["sha256"]
        .as_str()
        .expect("a new sha256")
        .to_owned();

    // The replace idempotency key replays its recorded 202 — it never enqueues a second
    // reverify.
    let again_key = key();
    let asked = replace(
        &owner,
        &project,
        &asset,
        PNG,
        "dot.png",
        "image/png",
        &again_key,
    )
    .await;
    assert_eq!(asked.status, StatusCode::ACCEPTED, "{}", asked.body);
    let replay = replace(
        &owner,
        &project,
        &asset,
        PNG,
        "dot.png",
        "image/png",
        &again_key,
    )
    .await;
    assert_eq!(replay.status, StatusCode::ACCEPTED, "{}", replay.body);
    assert!(replay.replayed(), "expected an idempotent replay");
    // Bytes already on record land the same content key: the current version swings back to
    // version 1 and `asset_versions` records nothing new.
    let swung_back = swung(&owner, &project, &asset, &sha2, 15).await;
    assert_eq!(swung_back["sha256"], sha1, "{swung_back}");
    let versions = versions_of(&owner, &project, &asset).await;
    assert_eq!(versions.len(), 2, "{versions:?}");

    // A refused replacement leaves the current version serving: not-an-image is rejected by
    // the worker, and the asset answers exactly as before.
    let refused = replace(
        &owner,
        &project,
        &asset,
        b"definitely not an image",
        "bad.png",
        "image/png",
        &key(),
    )
    .await;
    assert_eq!(refused.status, StatusCode::ACCEPTED, "{}", refused.body);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let after = call(
        &owner,
        Method::GET,
        &format!("{}/{asset}", assets(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(after.body["data"]["state"], "usable", "{}", after.body);
    assert_eq!(after.body["data"]["sha256"], sha1, "{}", after.body);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_replaced_asset_a_release_names_keeps_every_version_serving() {
    let _server = common::Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let editor = sign_in("dev-editor").await;
    let project = project(&owner).await;
    join(&owner, &editor, &project, "editor").await;
    covers(&owner, &project).await;

    // Release 1 publishes the asset at version 1.
    let (status, body) = upload(
        &editor,
        &project,
        PNG,
        "dot.png",
        "image/png",
        &Uuid::now_v7().to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let asset = body["data"]["id"].as_str().expect("id").to_owned();
    let first = settled(&owner, &project, &asset, 15).await;
    let sha1 = first["sha256"].as_str().expect("a sha256").to_owned();
    let v1_bytes = media_bytes(&media(&project, &asset)).await;
    let entry = cover(&editor, &project, "Released cover", &asset).await;
    let (path, hash) = approved(&owner, &editor, &project, &entry).await;
    let published = publish(&owner, &path, &hash, 0).await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);

    // The asset is replaced afterwards: the current URL moves on, but a publication pins the
    // asset — and so every version's objects — forever (D-098, D-136).
    let asked = replace(
        &owner,
        &project,
        &asset,
        WIDE_PNG,
        "wide.png",
        "image/png",
        &key(),
    )
    .await;
    assert_eq!(asked.status, StatusCode::ACCEPTED, "{}", asked.body);
    let second = swung(&owner, &project, &asset, &sha1, 15).await;
    let sha2 = second["sha256"].as_str().expect("a new sha256").to_owned();

    // What the release named still serves, byte for byte, at its pinned address.
    assert_eq!(
        media_bytes(&format!("{}/{sha1}", media(&project, &asset))).await,
        v1_bytes,
        "the version release 1 saw is still there and still served"
    );
    assert_eq!(
        media_status(&format!("{}/{sha2}", media(&project, &asset))).await,
        StatusCode::OK
    );

    // And the delete request's keep-rule never let go: a publication still naming the asset
    // refuses the delete outright, so neither version's objects can be collected.
    let refused = delete_asset(&owner, &project, &asset, &key()).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
    assert_eq!(
        refused.body["error"]["details"]["reason"], "still_referenced",
        "{}",
        refused.body
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_deleted_asset_collects_every_version_and_rendition() {
    let _server = common::Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;

    // Version 1 wide (with w320), replaced by the 1x1: two versions, three objects.
    let (status, body) = upload(
        &owner,
        &project,
        WIDE_PNG,
        "wide.png",
        "image/png",
        &Uuid::now_v7().to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let asset = body["data"]["id"].as_str().expect("id").to_owned();
    let first = settled(&owner, &project, &asset, 15).await;
    let sha1 = first["sha256"].as_str().expect("a sha256").to_owned();
    let asked = replace(
        &owner,
        &project,
        &asset,
        PNG,
        "dot.png",
        "image/png",
        &key(),
    )
    .await;
    assert_eq!(asked.status, StatusCode::ACCEPTED, "{}", asked.body);
    let second = swung(&owner, &project, &asset, &sha1, 15).await;
    let sha2 = second["sha256"].as_str().expect("a new sha256").to_owned();

    // The environment's id, for the literal object keys the worker wrote.
    let database = common::owner_database().await;
    let environment: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM environments WHERE project_id = $1::uuid AND slug = 'production'",
    )
    .bind(&project)
    .fetch_one(&database)
    .await
    .expect("the production environment");

    // Both versions' objects — originals and the rendition — exist before the delete.
    let keys = [
        format!("public/{project}/{environment}/{asset}/{sha1}"),
        format!("public/{project}/{environment}/{asset}/{sha1}/w320"),
        format!("public/{project}/{environment}/{asset}/{sha2}"),
    ];
    {
        use object_store::ObjectStoreExt;
        let store = store();
        for key in &keys {
            store
                .head(&object_store::path::Path::from(key.as_str()))
                .await
                .unwrap_or_else(|_| panic!("{key} should exist before the delete"));
        }
    }

    let deleted = delete_asset(&owner, &project, &asset, &key()).await;
    assert_eq!(deleted.status, StatusCode::ACCEPTED, "{}", deleted.body);
    let tombstone = reached(&owner, &project, &asset, "deleted", 15).await;
    assert_eq!(tombstone["state"], "deleted", "{tombstone}");

    // The whole prefix went — not only the recorded current key: every version, every
    // rendition, is collected by the listing delete.
    let store = store();
    for key in &keys {
        object_gone(&store, key, 15).await;
    }
    for path in [
        media(&project, &asset),
        format!("{}/{sha1}", media(&project, &asset)),
        format!("{}/{sha2}", media(&project, &asset)),
    ] {
        assert_eq!(media_status(&path).await, StatusCode::NOT_FOUND, "{path}");
    }
    database.close().await;
}
