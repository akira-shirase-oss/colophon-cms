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

/// A real 400x4 RGB PNG: wide enough for a `w320` rendition, so a rendition's address is a real one.
const WIDE_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 1, 144, 0, 0, 0, 4, 8, 2,
    0, 0, 0, 47, 93, 254, 122, 0, 0, 0, 42, 73, 68, 65, 84, 120, 156, 237, 206, 65, 9, 0, 48, 12,
    4, 176, 74, 159, 243, 206, 194, 253, 142, 66, 32, 2, 50, 243, 22, 224, 134, 254, 0, 32, 212,
    31, 0, 132, 250, 3, 128, 204, 7, 24, 121, 90, 72, 220, 227, 41, 34, 0, 0, 0, 0, 73, 69, 78, 68,
    174, 66, 96, 130,
];

/// Uploads [`WIDE_PNG`] and waits until the worker verified it (renditions made); its id.
async fn usable(owner: &Session, project: &str) -> String {
    let response = app()
        .post(format!("{ORIGIN}{}/assets", base(project)))
        .header(header::COOKIE, &owner.cookie)
        .header("x-csrf-token", &owner.csrf)
        .header(header::ORIGIN, ORIGIN)
        .header("idempotency-key", key())
        .header("x-colophon-file-name", "strip.png")
        .header(header::CONTENT_TYPE, "image/png")
        .body(WIDE_PNG.to_vec())
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

/// `path` signed until `expires` with the installation's key for `purpose`: `preview` as a preview answer
/// signs it, `member` as a site member's delivery answer does (D-229).
async fn signed_for(purpose: &str, path: &str, expires: i64) -> String {
    let database = owner_database().await;
    let bytes: Vec<u8> = sqlx::query_scalar("SELECT key FROM media_signing_keys WHERE id = 1")
        .fetch_one(&database)
        .await
        .expect("the signing key");
    database.close().await;
    let key = hmac::Key::new(hmac::HMAC_SHA256, &bytes);
    let tag = hmac::sign(
        &key,
        format!("colophon-media-v1\n{purpose}\n{path}\n{expires}").as_bytes(),
    );
    format!(
        "{path}?expires={expires}&signature={}",
        URL_SAFE_NO_PAD.encode(tag.as_ref())
    )
}

/// `path` signed as a preview answer signs it.
async fn signed(path: &str, expires: i64) -> String {
    signed_for("preview", path, expires).await
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after 1970")
        .as_secs()
        .try_into()
        .expect("seconds fit")
}

/// `address` with the first character of its signature replaced by a different one.
fn tampered(address: &str) -> String {
    let at = address.find("signature=").expect("a signed address") + "signature=".len();
    let first = &address[at..=at];
    let other = if first == "A" { "B" } else { "A" };
    format!("{}{other}{}", &address[..at], &address[at + 1..])
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[allow(
    clippy::too_many_lines,
    reason = "one asset's whole life: private, signed, each forgery refused, published"
)]
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
    // The rendition is real: signed for its own path it is served, so the 404 below is the signature's.
    let rendition = format!("{path}/w320");
    let own = get(&signed(&rendition, now() + 300).await, None).await;
    assert_eq!(own.status(), StatusCode::OK, "the w320 rendition exists");
    // A member's signature works on a version's own address.
    let record = send(
        &owner,
        Method::GET,
        &format!("{}/assets/{asset}", base(&project)),
        None,
    )
    .await;
    let sha = record.body["data"]["sha256"].as_str().expect("a sha256");
    let version = format!("{path}/{sha}");
    let as_member = get(&signed_for("member", &version, now() + 300).await, None).await;
    assert_eq!(as_member.status(), StatusCode::OK, "a member's versioned address");
    for bad in [
        signed(&path, now() - 1).await,
        signed(&path, now() + 365 * 24 * 3600).await,
        good.replace("expires=", "expires=1"),
        // One character of the signature changed. Not the last: of a 32-byte MAC's 43 characters
        // it carries 4 bits, so "set it to A" was a no-op one run in 16 (CI 44869366703).
        tampered(&good),
        // Another address's signature on this one.
        {
            let other = format!("/media/v1/{project}/production/{}", Uuid::now_v7());
            signed(&other, now() + 300).await.replace(&other, &path)
        },
        // A rendition's path under this address's signature.
        good.replacen(&path, &rendition, 1),
        // A member's signature is for a version a release named, never the bare address that follows
        // the newest upload (review S1, 2026-11-21).
        signed_for("member", &path, now() + 300).await,
        // The message without a purpose, as addresses were signed before.
        {
            let database = owner_database().await;
            let bytes: Vec<u8> =
                sqlx::query_scalar("SELECT key FROM media_signing_keys WHERE id = 1")
                    .fetch_one(&database)
                    .await
                    .expect("the signing key");
            database.close().await;
            let expires = now() + 300;
            let tag = hmac::sign(
                &hmac::Key::new(hmac::HMAC_SHA256, &bytes),
                format!("colophon-media-v1\n{path}\n{expires}").as_bytes(),
            );
            format!(
                "{path}?expires={expires}&signature={}",
                URL_SAFE_NO_PAD.encode(tag.as_ref())
            )
        },
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
pub(super) async fn publish(owner: &Session, reviewer: &Session, project: &str, entry: &str) {
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

/// The current sha of `asset`, once it differs from `was`: a replacement landed.
async fn swung(owner: &Session, project: &str, asset: &str, was: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let read = send(
            owner,
            Method::GET,
            &format!("{}/assets/{asset}", base(project)),
            None,
        )
        .await;
        if let Some(sha) = read.body["data"]["sha256"].as_str()
            && sha != was
            && read.body["data"]["state"] == "usable"
        {
            return sha.to_owned();
        }
        assert!(Instant::now() < deadline, "never replaced: {}", read.body);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Review S1 (2026-11-21): a site member's answer signed the bare media address, and the origin serves a
/// signed bare address the asset's CURRENT version, so after a replace any site account got a shareable
/// link to the unpublished replacement. Every address a member's answer names must serve the bytes the
/// release published.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[allow(
    clippy::too_many_lines,
    reason = "one journey: publish, sign a visitor up, replace, read as the visitor"
)]
async fn a_site_member_s_answer_never_links_an_unpublished_replacement() {
    let _server = Server::start_with(&[("COLOPHON_WORKER_POLL_MS", "50")]).await;
    let owner = sign_in("dev-owner").await;
    let reviewer = sign_in("dev-editor").await;
    let project = project(&owner).await;
    join(&owner, &reviewer, &project, "publisher").await;

    // A published cover, its image's first version.
    let asset = usable(&owner, &project).await;
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
    let id = entry.body["data"]["id"].as_str().expect("an id").to_owned();
    publish(&owner, &reviewer, &project, &id).await;
    let first = send(
        &owner,
        Method::GET,
        &format!("{}/assets/{asset}", base(&project)),
        None,
    )
    .await;
    let sha1 = first.body["data"]["sha256"]
        .as_str()
        .expect("a sha256")
        .to_owned();

    // A site account: signed up to the project's open audience role.
    let role = send(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/audience/roles"),
        Some(&json!({"name": "member", "self_assign": true})),
    )
    .await;
    assert_eq!(role.status, StatusCode::CREATED, "{}", role.body);
    let signup = app()
        .post(format!("{ORIGIN}/audience/v1/{project}/signup"))
        .json(&json!({
            "email": format!("visitor-{}@example.test", Uuid::now_v7().simple()),
            "password": "a-long-enough-password",
            "display_name": "Visitor"
        }))
        .send()
        .await
        .expect("request");
    assert_eq!(signup.status(), StatusCode::CREATED);
    let body: Value = signup.json().await.expect("json");
    let bearer = format!(
        "Bearer {}",
        body["data"]["session"].as_str().expect("a session token")
    );

    // The image is replaced; nothing publishes the replacement.
    let replaced = app()
        .post(format!("{ORIGIN}{}/assets/{asset}/replace", base(&project)))
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
    assert_eq!(replaced.status(), StatusCode::ACCEPTED);
    let sha2 = swung(&owner, &project, &asset, &sha1).await;

    // The member's answer: every media address it names serves the first version's bytes, or nothing.
    let answer = app()
        .get(format!("{ORIGIN}/content/v1/{project}/production/covers/{id}"))
        .header(header::AUTHORIZATION, &bearer)
        .send()
        .await
        .expect("request");
    assert_eq!(answer.status(), StatusCode::OK);
    let answer: Value = answer.json().await.expect("json");
    let view = &answer["data"]["media"][&asset];
    let mut addresses: Vec<String> = ["url", "current_url"]
        .iter()
        .map(|name| view[name].as_str().expect("an address").to_owned())
        .collect();
    addresses.extend(
        view["renditions"]
            .as_object()
            .expect("renditions")
            .values()
            .map(|address| address.as_str().expect("an address").to_owned()),
    );
    assert!(addresses.len() >= 3, "{view}");
    let replacement = app()
        .get(format!("{ORIGIN}/media/v1/{project}/production/{asset}/{sha2}"))
        .header(header::COOKIE, &owner.cookie)
        .send()
        .await
        .expect("request")
        .bytes()
        .await
        .expect("bytes");
    assert_eq!(replacement.as_ref(), PNG, "the replacement is the 1x1 image");
    for address in addresses {
        assert!(!address.contains(&sha2), "{address} names the replacement");
        let served = app().get(&address).send().await.expect("request");
        let status = served.status();
        let bytes = served.bytes().await.expect("bytes");
        assert!(
            status == StatusCode::OK && bytes.as_ref() != PNG,
            "{address}: {status}, the replacement's bytes: {}",
            bytes.as_ref() == PNG
        );
    }
}
