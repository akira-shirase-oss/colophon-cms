//! IDOR on assets (docs/security/attack-surface.md): media is private until it is published (D-229).
//! Before, anyone who knew a project, an environment and an asset id got any usable upload; asset ids are
//! `UUIDv7`, whose first 48 bits are a timestamp. Now an unpublished upload is a 404 to the public (the same
//! as one that never existed, and kept by no cache), a member sees it privately, a signed address works
//! for its path until it expires, and publishing makes it public.

use std::{
    fmt::Write as _,
    time::{Duration, Instant},
};

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

/// The installation's current media signing key (the highest id, D-233): its id and bytes.
async fn current_key() -> (i16, Vec<u8>) {
    let database = owner_database().await;
    let key: (i16, Vec<u8>) =
        sqlx::query_as("SELECT id, key FROM media_signing_keys ORDER BY id DESC LIMIT 1")
            .fetch_one(&database)
            .await
            .expect("the signing key");
    database.close().await;
    key
}

/// `path` signed until `expires` with key `kid` for `purpose`, as the server signs (D-229, D-233).
fn sign_with(kid: i16, key: &[u8], purpose: &str, path: &str, expires: i64) -> String {
    let tag = hmac::sign(
        &hmac::Key::new(hmac::HMAC_SHA256, key),
        format!("colophon-media-v2\n{kid}\n{purpose}\n{path}\n{expires}").as_bytes(),
    );
    format!(
        "{path}?kid={kid}&expires={expires}&signature={}",
        URL_SAFE_NO_PAD.encode(tag.as_ref())
    )
}

/// `path` signed until `expires` with the current key for `purpose`: `preview` as a preview answer signs it,
/// `member` as a site member's delivery answer does (D-229).
async fn signed_for(purpose: &str, path: &str, expires: i64) -> String {
    let (kid, key) = current_key().await;
    sign_with(kid, &key, purpose, path, expires)
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
    assert_eq!(
        as_member.status(),
        StatusCode::OK,
        "a member's versioned address"
    );
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
        // The message without a purpose or a key id, as addresses were signed before D-229's fix and D-233.
        {
            let (_, bytes) = current_key().await;
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
        // The same signature without its key id: never read as some default key.
        {
            let (kid, _) = current_key().await;
            good.replacen(&format!("kid={kid}&"), "", 1)
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
        .get(format!(
            "{ORIGIN}/content/v1/{project}/production/covers/{id}"
        ))
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
    // Each version's bytes as the server stored them (it re-encodes an upload, so not `PNG` itself),
    // read by the owner. They differ, so a served body says which version it is.
    let stored = |sha: &str| {
        let owner = &owner;
        let address = format!("{ORIGIN}/media/v1/{project}/production/{asset}/{sha}");
        async move {
            app()
                .get(address)
                .header(header::COOKIE, &owner.cookie)
                .send()
                .await
                .expect("request")
                .bytes()
                .await
                .expect("bytes")
        }
    };
    let (published, replacement) = (stored(&sha1).await, stored(&sha2).await);
    assert_ne!(published, replacement, "the two versions' bytes differ");
    for address in addresses {
        assert!(!address.contains(&sha2), "{address} names the replacement");
        let served = app().get(&address).send().await.expect("request");
        let status = served.status();
        let bytes = served.bytes().await.expect("bytes");
        assert_eq!(status, StatusCode::OK, "{address}");
        assert_ne!(
            bytes, replacement,
            "{address} served the unpublished replacement"
        );
        if !address.contains("/w320") {
            assert_eq!(bytes, published, "{address} serves the published version");
        }
    }
}

/// D-233 (the review's S2-2): the signing key rotates. A link signed with the previous key still works, one with a
/// key the server hasn't read yet works at once (another server rotated), and after `--retire-previous` the old
/// key's links are refused once the server reads the keys again (here: a server started afresh; the minute's
/// bound is the unit tests').
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_rotated_key_s_links_live_one_rotation_and_a_retired_key_s_die() {
    let server = Server::start_with(&[("COLOPHON_WORKER_POLL_MS", "50")]).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let asset = usable(&owner, &project).await;
    let path = format!("/media/v1/{project}/production/{asset}");
    let rotate = |retire: bool| async move {
        let database = owner_database().await;
        let kid: i16 = sqlx::query_scalar("SELECT colophon_rotate_media_key($1, 'admin')")
            .bind(retire)
            .fetch_one(&database)
            .await
            .expect("rotated");
        database.close().await;
        kid
    };
    let status = |address: String| async move { get(&address, None).await.status() };

    let (old_kid, old_key) = current_key().await;
    let old = sign_with(old_kid, &old_key, "preview", &path, now() + 300);
    assert_eq!(
        status(old.clone()).await,
        StatusCode::OK,
        "the current key's link"
    );

    // Rotated (by an operator, as another server would see it): the server hasn't read the new key, but a link
    // naming it makes it read the keys once, and the old key still verifies.
    let new_kid = rotate(false).await;
    assert_eq!(new_kid, old_kid + 1);
    let (current, new_key) = current_key().await;
    assert_eq!(current, new_kid);
    let new = sign_with(new_kid, &new_key, "preview", &path, now() + 300);
    assert_eq!(
        status(new.clone()).await,
        StatusCode::OK,
        "a just-made key's link"
    );
    assert_eq!(
        status(old.clone()).await,
        StatusCode::OK,
        "the previous key still verifies"
    );
    // A key id nobody holds, and one from the old key under the new id: refused.
    assert_eq!(
        status(sign_with(
            new_kid + 5,
            &new_key,
            "preview",
            &path,
            now() + 300
        ))
        .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        status(sign_with(new_kid, &old_key, "preview", &path, now() + 300)).await,
        StatusCode::NOT_FOUND
    );

    // Retired: a leaked key. Once the keys are read again, its links are refused and the new one's work.
    let newest = rotate(true).await;
    drop(server);
    let _server = Server::start_with(&[("COLOPHON_WORKER_POLL_MS", "50")]).await;
    assert_eq!(
        status(old).await,
        StatusCode::NOT_FOUND,
        "a retired key's link"
    );
    assert_eq!(status(new).await, StatusCode::NOT_FOUND, "retired with it");
    let (current, newest_key) = current_key().await;
    assert_eq!(current, newest);
    assert_eq!(
        status(sign_with(
            newest,
            &newest_key,
            "preview",
            &path,
            now() + 300
        ))
        .await,
        StatusCode::OK
    );
}

/// Every media signing key the installation holds now, current first.
async fn all_keys() -> Vec<(i16, Vec<u8>)> {
    let database = owner_database().await;
    let keys = sqlx::query_as("SELECT id, key FROM media_signing_keys ORDER BY id DESC")
        .fetch_all(&database)
        .await
        .expect("the keys");
    database.close().await;
    keys
}

/// A rotation as `colophon admin media-key rotate [--retire-previous]` runs it; the new key's id.
async fn rotate(retire: bool) -> i16 {
    let database = owner_database().await;
    let kid = sqlx::query_scalar("SELECT colophon_rotate_media_key($1, 'admin')")
        .bind(retire)
        .fetch_one(&database)
        .await
        .expect("rotated");
    database.close().await;
    kid
}

/// The ways a key's bytes could be printed, whole or its first part: hex either case, base64 (standard, padded or
/// not, and URL-safe), and Rust's `Debug` of the bytes.
fn printed_as(key: &[u8]) -> Vec<String> {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
    let hex = |bytes: &[u8]| {
        bytes.iter().fold(String::new(), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
    };
    let mut forms = Vec::new();
    for part in [key, &key[..12]] {
        forms.push(hex(part));
        forms.push(hex(part).to_uppercase());
        forms.push(STANDARD.encode(part));
        forms.push(STANDARD_NO_PAD.encode(part));
        forms.push(URL_SAFE_NO_PAD.encode(part));
    }
    forms.push(
        key[..8]
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(", "),
    );
    forms
}

/// White-hat (D-233): a key id is read strictly over HTTP too. Junk, an overlong id, a sign, a leading zero, a
/// decimal point, an encoded NUL, a non-ASCII digit, an id past a `u32`, a second `kid`, none at all, and a
/// well-formed id no key has: each the same private 404 as a forgery, around a real signature that works.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn junk_overlong_and_missing_key_ids_are_refused_like_a_forgery() {
    let _server = Server::start_with(&[("COLOPHON_WORKER_POLL_MS", "50")]).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let asset = usable(&owner, &project).await;
    let path = format!("/media/v1/{project}/production/{asset}");
    let (kid, key) = current_key().await;
    let expires = now() + 300;
    let good = sign_with(kid, &key, "preview", &path, expires);
    assert_eq!(
        get(&good, None).await.status(),
        StatusCode::OK,
        "the real link"
    );

    let id = format!("kid={kid}&");
    let with_id = |replacement: &str| good.replacen(&id, &format!("kid={replacement}&"), 1);
    let unheld = i64::from(i16::MAX) + 1;
    for bad in [
        with_id(&format!("0{kid}")),
        with_id(&format!("+{kid}")),
        with_id(&format!("%2B{kid}")),
        with_id(&format!("-{kid}")),
        with_id(&format!("{kid}.0")),
        with_id(&format!("{kid}%00")),
        with_id("%00"),
        with_id("%D9%A1"),
        with_id(""),
        with_id("abc"),
        with_id(&format!("{kid} ")),
        with_id(&"9".repeat(1000)),
        with_id("4294967296"),
        good.replacen(&id, &format!("kid={kid}&kid={kid}&"), 1),
        good.replacen(&id, "", 1),
        // Well formed, signed with the real key over that id, but no key has it: one read of the keys, then 404.
        sign_with(i16::MAX, &key, "preview", &path, expires).replacen(
            &format!("kid={}&", i16::MAX),
            &format!("kid={unheld}&"),
            1,
        ),
        sign_with(kid + 1, &key, "preview", &path, expires),
    ] {
        let refused = get(&bad, None).await;
        assert_eq!(refused.status(), StatusCode::NOT_FOUND, "{bad}");
        assert_eq!(
            refused.headers()[header::CACHE_CONTROL],
            "private, no-store",
            "{bad}"
        );
    }
    assert_eq!(
        get(&good, None).await.status(),
        StatusCode::OK,
        "still works"
    );
}

/// White-hat (D-233), a rotation racing signed requests: once a server has read a `--retire-previous` rotation
/// (here a link with the new key's id makes it read at once), the retired key's links are refused at once, the
/// links it signed in the minute before included, and no burst of forged ids brings it back.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_retired_key_dies_as_soon_as_the_server_reads_the_rotation() {
    let _server = Server::start_with(&[("COLOPHON_WORKER_POLL_MS", "50")]).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let asset = usable(&owner, &project).await;
    let path = format!("/media/v1/{project}/production/{asset}");
    let (old_kid, old_key) = current_key().await;
    let old = sign_with(old_kid, &old_key, "preview", &path, now() + 300);
    assert_eq!(
        get(&old, None).await.status(),
        StatusCode::OK,
        "read and kept"
    );

    let newest = rotate(true).await;
    let (current, newest_key) = current_key().await;
    assert_eq!(current, newest);
    let new = sign_with(newest, &newest_key, "preview", &path, now() + 300);
    assert_eq!(
        get(&new, None).await.status(),
        StatusCode::OK,
        "the new key's link"
    );
    assert_eq!(
        get(&old, None).await.status(),
        StatusCode::NOT_FOUND,
        "the retired key's link, within its minute"
    );
    for forged in 1..=5 {
        let forged = sign_with(newest + forged, &newest_key, "preview", &path, now() + 300);
        assert_eq!(get(&forged, None).await.status(), StatusCode::NOT_FOUND);
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert_eq!(
        get(&old, None).await.status(),
        StatusCode::NOT_FOUND,
        "still dead"
    );
    assert_eq!(get(&new, None).await.status(), StatusCode::OK);
}

/// White-hat (D-233): no key, whole or in part, in any form, reaches the server's log at its most verbose level,
/// or `/version`, `/livez` and `/readyz` (body or headers), through signing, verifying, unknown and junk ids, and
/// two rotations, every key that lived during the test checked.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn no_media_key_reaches_the_log_or_the_status_answers() {
    let log_path =
        std::env::temp_dir().join(format!("colophon-media-key-log-{}.jsonl", Uuid::now_v7()));
    let file = std::fs::File::create(&log_path).expect("a log file");
    let server = Server::start_logged_to(
        file,
        &[("COLOPHON_WORKER_POLL_MS", "50"), ("COLOPHON_LOG", "trace")],
    )
    .await;
    let mut seen: Vec<Vec<u8>> = all_keys().await.into_iter().map(|(_, key)| key).collect();
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let asset = usable(&owner, &project).await;
    let path = format!("/media/v1/{project}/production/{asset}");
    let mut answers = String::new();
    let mut fetch = async |address: String| {
        let response = get(&address, None).await;
        let status = response.status();
        let _ = writeln!(answers, "{:?}", response.headers());
        if !address.starts_with("/media/") {
            answers.push_str(&response.text().await.expect("text"));
        }
        status
    };

    for round in [None, Some(false), Some(true)] {
        if let Some(retire) = round {
            rotate(retire).await;
            // The last round's forged id spent this second's read of the keys (D-233: one a second per server).
            tokio::time::sleep(Duration::from_millis(1100)).await;
            seen.extend(all_keys().await.into_iter().map(|(_, key)| key));
        }
        let (kid, key) = current_key().await;
        assert_eq!(
            fetch(sign_with(kid, &key, "preview", &path, now() + 300)).await,
            StatusCode::OK
        );
        fetch(sign_with(kid + 3, &key, "preview", &path, now() + 300)).await;
        fetch(format!("{path}?kid=junk&expires=1&signature=AAAA")).await;
        for status_path in ["/version", "/livez", "/readyz"] {
            fetch(status_path.to_owned()).await;
        }
    }
    drop(server);

    let log = std::fs::read_to_string(&log_path).expect("the log");
    let _ = std::fs::remove_file(&log_path);
    assert!(
        log.lines().count() > 100 && log.contains("/media/v1/"),
        "the trace log recorded the requests ({} lines)",
        log.lines().count()
    );
    seen.sort();
    seen.dedup();
    assert!(seen.len() >= 3, "three keys lived: {}", seen.len());
    for key in &seen {
        // Which form, by its place in `printed_as`, never the form itself: a failure mustn't print the key.
        for (form, printed) in printed_as(key).iter().enumerate() {
            assert!(
                !log.contains(printed),
                "a media key reached the log (form {form})"
            );
            assert!(
                !answers.contains(printed),
                "a media key reached a status answer (form {form})"
            );
        }
    }
}

/// White-hat (D-233): a rotation is the migrator's alone, run from the CLI against the database; nothing over
/// HTTP rotates or reads a key. The route table's own document names no such path, and the likeliest ones, asked
/// by the installation's administrator, by a full-access project token and by no one, change nothing.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn no_session_or_token_rotates_or_reads_a_key_over_http() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let secret = super::token(
        &owner,
        &project,
        &json!({"name": "Rotator", "kind": "full-access", "expires_in_days": 1}),
    )
    .await;
    let before = all_keys().await;

    let document: Value = app()
        .get(format!("{ORIGIN}/openapi.json"))
        .send()
        .await
        .expect("request")
        .json()
        .await
        .expect("the document");
    let paths = document["paths"].as_object().expect("paths");
    assert!(
        paths.len() > 20,
        "the real route table ({} paths)",
        paths.len()
    );
    for path in paths.keys() {
        let lower = path.to_lowercase();
        assert!(
            !["media-key", "media_key", "signing", "rotate", "kid"]
                .iter()
                .any(|word| lower.contains(word)),
            "a key path is served: {path}"
        );
    }

    let base = base(&project);
    for path in [
        "/api/v1/admin/media-key/rotate".to_owned(),
        "/api/v1/admin/media-keys".to_owned(),
        "/admin/media-key/rotate".to_owned(),
        format!("/api/v1/projects/{project}/media-key/rotate"),
        format!("{base}/media-keys"),
        format!("{base}/media-key/rotate"),
    ] {
        for method in [Method::GET, Method::POST] {
            let body = json!({"retire_previous": true});
            let tries = [
                (
                    "the administrator",
                    send(&owner, method.clone(), &path, Some(&body)).await,
                ),
                (
                    "a full-access token",
                    super::as_token(&secret, method.clone(), &path, Some(&body)).await,
                ),
                (
                    "no one",
                    super::anonymous(method.clone(), &path, Some(&body)).await,
                ),
            ];
            for (who, reply) in tries {
                super::refused(&reply, &format!("{who}: {method} {path}"));
            }
        }
    }
    assert_eq!(all_keys().await, before, "the keys are as they were");
}
