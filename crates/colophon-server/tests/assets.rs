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

use common::{Session, app, call, project, sign_in};
use reqwest::{Method, StatusCode, header};
use serde_json::Value;
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
