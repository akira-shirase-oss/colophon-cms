//! IDOR on assets (docs/security/attack-surface.md): media is private until it is published (D-229).
//! Before, anyone who knew a project, an environment and an asset id got any usable upload; asset ids are
//! `UUIDv7`, whose first 48 bits are a timestamp. Now an unpublished upload is a 404 to the public (the same
//! as one that never existed, and kept by no cache), a member sees it privately, a signed address works
//! for its path until it expires, and publishing makes it public.

use std::time::{Duration, Instant};

use aws_lc_rs::hmac;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};
use uuid::Uuid;

use super::send;
use crate::common::{
    ORIGIN, Server, Session, app, call, join, key, owner_database, project, sign_in,
};

/// A real 1x1 RGB PNG (69 bytes).
const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0,
    0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 0, 0, 3, 1,
    1, 0, 201, 254, 146, 239, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
}

/// Uploads [`PNG`] and waits until the worker verified it; its id.
async fn usable(owner: &Session, project: &str) -> String {
    let response = app()
        .post(format!("{ORIGIN}{}/assets", base(project)))
        .header(header::COOKIE, &owner.cookie)
        .header("x-csrf-token", &owner.csrf)
        .header(header::ORIGIN, ORIGIN)
        .header("idempotency-key", key())
        .header("x-colophon-file-name", "dot.png")
        .header(header::CONTENT_TYPE, "image/png")
        .body(PNG.to_vec())
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body: Value = response.json().await.expect("json");
    let asset = body["data"]["id"].as_str().expect("an id").to_owned();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let read = send(
            owner,
            Method::GET,
            &format!("{}/assets/{asset}", base(project)),
            None,
        )
        .await;
        if read.body["data"]["state"] == "usable" {
            return asset;
        }
        assert!(Instant::now() < deadline, "never usable: {}", read.body);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn get(path: &str, viewer: Option<&Session>) -> reqwest::Response {
    let mut request = app().get(format!("{ORIGIN}{path}"));
    if let Some(viewer) = viewer {
        request = request.header(header::COOKIE, &viewer.cookie);
    }
    request.send().await.expect("request")
}

/// `path` signed until `expires` with the installation's key, as a preview answer signs it.
async fn signed(path: &str, expires: i64) -> String {
    let database = owner_database().await;
    let bytes: Vec<u8> = sqlx::query_scalar("SELECT key FROM media_signing_keys WHERE id = 1")
        .fetch_one(&database)
        .await
        .expect("the signing key");
    database.close().await;
    let key = hmac::Key::new(hmac::HMAC_SHA256, &bytes);
    let tag = hmac::sign(
        &key,
        format!("colophon-media-v1\n{path}\n{expires}").as_bytes(),
    );
    format!(
        "{path}?expires={expires}&signature={}",
        URL_SAFE_NO_PAD.encode(tag.as_ref())
    )
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after 1970")
        .as_secs()
        .try_into()
        .expect("seconds fit")
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn an_unpublished_upload_is_private_until_a_release_names_it() {
    let _server = Server::start_with(&[("COLOPHON_WORKER_POLL_MS", "50")]).await;
    let owner = sign_in("dev-owner").await;
    let stranger = sign_in("dev-editor").await;
    let project = project(&owner).await;
    let asset = usable(&owner, &project).await;
    let path = format!("/media/v1/{project}/production/{asset}");

    // The public, and a person signed in to another project: the same 404 as no asset at all,
    // kept by no cache.
    let unknown = format!("/media/v1/{project}/production/{}", Uuid::now_v7());
    for (what, viewer) in [("the public", None), ("a stranger", Some(&stranger))] {
        let response = get(&path, viewer).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{what}");
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "private, no-store",
            "{what}"
        );
        let nothing = get(&unknown, viewer).await;
        assert_eq!(nothing.status(), StatusCode::NOT_FOUND, "{what}");
    }
    // A member: privately.
    let member = get(&path, Some(&owner)).await;
    assert_eq!(member.status(), StatusCode::OK);
    assert_eq!(member.headers()[header::CACHE_CONTROL], "private, no-store");
    assert!(!member.bytes().await.expect("bytes").is_empty());

    // A signed address: for its path, until it expires; any change to it is a 404.
    let good = signed(&path, now() + 300).await;
    let served = get(&good, None).await;
    assert_eq!(served.status(), StatusCode::OK);
    assert_eq!(served.headers()[header::CACHE_CONTROL], "private, no-store");
    for bad in [
        signed(&path, now() - 1).await,
        signed(&path, now() + 365 * 24 * 3600).await,
        good.replace("expires=", "expires=1"),
        format!("{}A", &good[..good.len() - 1]),
        // Another address's signature on this one.
        {
            let other = format!("/media/v1/{project}/production/{}", Uuid::now_v7());
            signed(&other, now() + 300).await.replace(&other, &path)
        },
        // A rendition's path under this address's signature.
        good.replacen(&path, &format!("{path}/w320"), 1),
    ] {
        let refused = get(&bad, None).await;
        assert_eq!(refused.status(), StatusCode::NOT_FOUND, "{bad}");
    }

    // Published: the public gets it, and caches may keep it.
    join(&owner, &stranger, &project, "publisher").await;
    let made = send(
        &owner,
        Method::POST,
        &format!("{}/content-types", base(&project)),
        Some(&json!({"slug": "covers", "definition": {"name": "Covers", "title_field": "title", "fields": [
            {"id": "title", "label": "Title", "type": "text", "required": true},
            {"id": "photo", "label": "Photo", "type": "asset"}
        ]}})),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let entry = send(
        &owner,
        Method::POST,
        &format!("{}/collections/covers/entries", base(&project)),
        Some(&json!({"title": "Cover", "photo": asset})),
    )
    .await;
    assert_eq!(entry.status, StatusCode::CREATED, "{}", entry.body);
    let id = entry.body["data"]["id"].as_str().expect("an id");
    publish(&owner, &stranger, &project, id).await;
    let public = get(&path, None).await;
    assert_eq!(public.status(), StatusCode::OK);
    assert_eq!(
        public.headers()[header::CACHE_CONTROL],
        "public, max-age=60, must-revalidate"
    );
}

/// Puts `entry` in a changeset, has `reviewer` approve it and `owner` publish it.
async fn publish(owner: &Session, reviewer: &Session, project: &str, entry: &str) {
    let created = send(
        owner,
        Method::POST,
        &format!("{}/changesets", base(project)),
        Some(&json!({"title": "Release", "entries": [entry]})),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let path = format!(
        "{}/changesets/{}",
        base(project),
        created.body["data"]["id"].as_str().expect("an id")
    );
    let hash = created.body["data"]["hash"]
        .as_str()
        .expect("a hash")
        .to_owned();
    let if_match = format!("\"{hash}\"");
    let submitted = call(
        owner,
        Method::POST,
        &format!("{path}/submit"),
        None,
        &[("if-match", &if_match)],
    )
    .await
    .expect("request");
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);
    let review = call(
        reviewer,
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
    let published = call(
        owner,
        Method::POST,
        &format!("{path}/publish"),
        Some(&json!({"base_generation": 0})),
        &[("if-match", &if_match), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);
}
