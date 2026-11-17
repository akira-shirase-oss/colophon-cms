//! Images inside rich text and the media library (D-173 to D-175), end to end against the real
//! binary and the stack: a rich-text image names a real upload or the save is refused where it
//! names it; a release can't publish an image that isn't usable, and records the version it
//! did publish; delivery hands sites that version's immutable URL with its size and
//! renditions, and a replaced image reaches them at the next publish; an image a draft names
//! can't be deleted; the library pages, searches, filters by tag, and keeps its metadata under
//! `If-Match`. Needs the stack (Garage holds the objects) and `scripts/migrate.sh`.
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

/// A real 400×4 PNG (99 bytes): wider than `w320` but not `w1280`, so exactly one rendition.
const WIDE_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 1, 144, 0, 0, 0, 4, 8, 2,
    0, 0, 0, 47, 93, 254, 122, 0, 0, 0, 42, 73, 68, 65, 84, 120, 156, 237, 206, 65, 9, 0, 48, 12,
    4, 176, 74, 159, 243, 206, 194, 253, 142, 66, 32, 2, 50, 243, 22, 224, 134, 254, 0, 32, 212,
    31, 0, 132, 250, 3, 128, 204, 7, 24, 121, 90, 72, 220, 227, 41, 34, 0, 0, 0, 0, 73, 69, 78, 68,
    174, 66, 96, 130,
];

/// A real 1x1 RGB PNG (69 bytes): a different picture, for a replacement.
const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0,
    0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 0, 0, 3, 1,
    1, 0, 201, 254, 146, 239, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

/// The environment base of `project` (an id).
fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
}

fn assets(project: &str) -> String {
    format!("{}/assets", base(project))
}

/// `POST` bytes to `path` with the upload headers.
async fn send_file(session: &Session, path: &str, bytes: &[u8], name: &str) -> common::Reply {
    let response = app()
        .post(format!("{}{path}", common::ORIGIN))
        .header(header::COOKIE, &session.cookie)
        .header("x-csrf-token", &session.csrf)
        .header(header::ORIGIN, common::ORIGIN)
        .header("idempotency-key", key())
        .header("x-colophon-file-name", name)
        .header(header::CONTENT_TYPE, "image/png")
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

/// Uploads `bytes` as `name`; the asset's id.
async fn upload(session: &Session, project: &str, bytes: &[u8], name: &str) -> String {
    let reply = send_file(session, &assets(project), bytes, name).await;
    assert_eq!(reply.status, StatusCode::ACCEPTED, "{}", reply.body);
    reply.body["data"]["id"].as_str().expect("id").to_owned()
}

/// Waits up to `seconds` for `check` to hold of the asset's record; that record.
async fn until(
    session: &Session,
    project: &str,
    id: &str,
    seconds: u64,
    check: impl Fn(&Value) -> bool,
) -> Value {
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
        if check(&reply.body["data"]) {
            return reply.body["data"].clone();
        }
        assert!(Instant::now() < deadline, "asset {id}: {}", reply.body);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn usable(session: &Session, project: &str, id: &str) -> Value {
    let record = until(session, project, id, 20, |asset| {
        asset["state"].as_str() != Some("quarantined")
    })
    .await;
    assert_eq!(record["state"], "usable", "{record}");
    record
}

/// A `posts` type: a title and a rich-text body.
async fn posts(session: &Session, project: &str) {
    let made = call(
        session,
        Method::POST,
        &format!("{}/content-types", base(project)),
        Some(&json!({
            "slug": "posts",
            "definition": {
                "name": "Posts",
                "title_field": "title",
                "fields": [
                    {"id": "title", "label": "Title", "type": "text", "required": true},
                    {"id": "body", "label": "Body", "type": "rich_text"}
                ]
            }
        })),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
}

/// A rich-text document: a paragraph, then an image of each id.
fn body(images: &[&str]) -> Value {
    let mut content =
        vec![json!({"type": "paragraph", "content": [{"type": "text", "text": "Hello."}]})];
    content.extend(images.iter().map(
        |id| json!({"type": "image", "attrs": {"assetId": id, "alt": "A wide strip of colour"}}),
    ));
    json!({"schema": "colophon/rich-text", "version": 1, "doc": {"type": "doc", "content": content}})
}

async fn create_post(session: &Session, project: &str, images: &[&str]) -> common::Reply {
    call(
        session,
        Method::POST,
        &format!("{}/collections/posts/entries", base(project)),
        Some(&json!({"title": "A post", "body": body(images)})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request")
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
    let hash = created.body["data"]["hash"]
        .as_str()
        .expect("a hash")
        .to_owned();
    let if_match = format!("\"{hash}\"");
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

/// The delivered entry, at `generation` when given.
async fn delivered(project: &str, entry: &str, generation: Option<i64>) -> Value {
    let pin = generation.map_or_else(String::new, |g| format!("?generation={g}"));
    let response = app()
        .get(format!(
            "{}/content/v1/{project}/production/posts/{entry}{pin}",
            common::ORIGIN
        ))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK);
    response.json::<Value>().await.expect("json")["data"].clone()
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_rich_text_image_names_a_real_upload_or_the_save_is_refused_where_it_names_it() {
    let _server = common::Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    posts(&owner, &project).await;

    // A made-up id — what the old placeholder dialog inserted — is refused at its image.
    let ghost = Uuid::now_v7().to_string();
    let refused = create_post(&owner, &project, &[&ghost]).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"]["reason"], "asset_not_found",
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"]["pointer"], "/body/doc/content/1/attrs/assetId",
        "{}",
        refused.body
    );

    // Another project's upload is just as unknown here (the foreign key is per environment).
    let elsewhere = common::project(&owner).await;
    let foreign = upload(&owner, &elsewhere, WIDE_PNG, "foreign.png").await;
    let refused = create_post(&owner, &project, &[&foreign]).await;
    assert_eq!(
        refused.body["error"]["details"]["reason"], "asset_not_found",
        "{}",
        refused.body
    );

    // A real upload saves, even before it is usable; the same image twice is one reference.
    let image = upload(&owner, &project, WIDE_PNG, "strip.png").await;
    let saved = create_post(&owner, &project, &[&image, &image]).await;
    assert_eq!(saved.status, StatusCode::CREATED, "{}", saved.body);
    let entry = saved.body["data"]["id"].as_str().expect("id").to_owned();

    // "Used in" lists it, under the rich-text field.
    let used = call(
        &owner,
        Method::GET,
        &format!("{}/{image}/references", assets(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    let referrers = used.body["data"].as_array().expect("referrers");
    assert_eq!(referrers.len(), 1, "{}", used.body);
    assert_eq!(referrers[0]["entry_id"], entry.as_str());
    assert_eq!(referrers[0]["field"], "body");

    // And it can't be deleted while the draft names it.
    usable(&owner, &project, &image).await;
    let deleted = call(
        &owner,
        Method::DELETE,
        &format!("{}/{image}", assets(&project)),
        None,
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(deleted.status, StatusCode::CONFLICT, "{}", deleted.body);
    assert_eq!(
        deleted.body["error"]["details"]["reason"], "still_referenced",
        "{}",
        deleted.body
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one journey: each step's state is the next step's setup"
)]
async fn a_release_records_the_image_version_and_a_replace_reaches_sites_at_the_next_publish() {
    let _server = common::Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let editor = sign_in("dev-editor").await;
    let project = project(&owner).await;
    join(&owner, &editor, &project, "editor").await;
    posts(&owner, &project).await;

    // An image that never becomes usable keeps the post from publishing.
    let fake = send_file(
        &editor,
        &assets(&project),
        b"not an image at all",
        "fake.png",
    )
    .await;
    let fake = fake.body["data"]["id"].as_str().expect("id").to_owned();
    let post = create_post(&editor, &project, &[&fake]).await;
    let held = post.body["data"]["id"].as_str().expect("id").to_owned();
    let (path, hash) = approved(&owner, &editor, &project, &held).await;
    let refused = publish(&owner, &path, &hash, 0).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
    assert_eq!(
        refused.body["error"]["details"]["reason"], "asset_not_usable",
        "{}",
        refused.body
    );

    // A usable one publishes, and delivery hands sites the version the release recorded.
    let image = upload(&editor, &project, WIDE_PNG, "strip.png").await;
    let first = usable(&owner, &project, &image).await;
    let first_sha = first["sha256"].as_str().expect("sha").to_owned();
    let post = create_post(&editor, &project, &[&image]).await;
    let entry = post.body["data"]["id"].as_str().expect("id").to_owned();
    let (path, hash) = approved(&owner, &editor, &project, &entry).await;
    let published = publish(&owner, &path, &hash, 0).await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);
    let generation = published.body["data"]["generation"]
        .as_i64()
        .expect("generation");

    let row = delivered(&project, &entry, None).await;
    let media = &row["media"][&image];
    let current = format!("{}/media/v1/{project}/production/{image}", common::ORIGIN);
    assert_eq!(media["url"], format!("{current}/{first_sha}"), "{row}");
    assert_eq!(media["current_url"], current.as_str());
    assert_eq!(media["type"], "image/png");
    assert_eq!(
        (media["width"].as_i64(), media["height"].as_i64()),
        (Some(400), Some(4))
    );
    assert_eq!(
        media["renditions"],
        json!({"w320": format!("{current}/{first_sha}/w320")})
    );
    // The recorded URL serves the bytes, immutable.
    let served = app()
        .get(media["url"].as_str().expect("url"))
        .send()
        .await
        .expect("request");
    assert_eq!(served.status(), StatusCode::OK);
    assert_eq!(
        served.headers()["cache-control"],
        "public, max-age=31536000, immutable"
    );

    // Replacing the image moves the current version, not what the release recorded.
    let replaced = send_file(
        &owner,
        &format!("{}/{image}/replace", assets(&project)),
        PNG,
        "dot.png",
    )
    .await;
    assert_eq!(replaced.status, StatusCode::ACCEPTED, "{}", replaced.body);
    let second = until(&owner, &project, &image, 20, |asset| {
        asset["sha256"].as_str().is_some_and(|sha| sha != first_sha)
    })
    .await;
    let second_sha = second["sha256"].as_str().expect("sha").to_owned();
    let row = delivered(&project, &entry, None).await;
    assert_eq!(
        row["media"][&image]["url"],
        format!("{current}/{first_sha}"),
        "{row}"
    );

    // Publishing the unchanged entry again carries the new version; the old release, read
    // pinned, still names the old one.
    let (path, hash) = approved(&owner, &editor, &project, &entry).await;
    let again = publish(&owner, &path, &hash, generation).await;
    assert_eq!(again.status, StatusCode::CREATED, "{}", again.body);
    assert_eq!(again.body["data"]["entries_changed"], 1, "{}", again.body);
    let row = delivered(&project, &entry, None).await;
    assert_eq!(
        row["media"][&image]["url"],
        format!("{current}/{second_sha}"),
        "{row}"
    );
    assert_eq!((row["media"][&image]["width"].as_i64()), Some(1));
    assert_eq!(
        row["media"][&image]["renditions"],
        json!({}),
        "a 1-pixel image has none"
    );
    let pinned = delivered(&project, &entry, Some(generation)).await;
    assert_eq!(
        pinned["media"][&image]["url"],
        format!("{current}/{first_sha}"),
        "{pinned}"
    );

    // An edit published as generation 3, then a rollback to generation 1: the rolled-back
    // revision re-serves exactly the version generation 1 published, not the current one.
    let current_draft = call(
        &owner,
        Method::GET,
        &format!("{}/collections/posts/entries/{entry}", base(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    let edited = call(
        &owner,
        Method::PUT,
        &format!("{}/collections/posts/entries/{entry}", base(&project)),
        Some(&json!({"title": "A post, edited", "body": body(&[&image])})),
        &[
            ("if-match", &current_draft.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request");
    assert_eq!(edited.status, StatusCode::OK, "{}", edited.body);
    let (path, hash) = approved(&owner, &editor, &project, &entry).await;
    let third = publish(&owner, &path, &hash, generation + 1).await;
    assert_eq!(third.status, StatusCode::CREATED, "{}", third.body);
    let rollback = call(
        &editor,
        Method::POST,
        &format!("{}/releases/{generation}/rollback", base(&project)),
        Some(&json!({"title": "Back"})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(rollback.status, StatusCode::CREATED, "{}", rollback.body);
    let path = format!(
        "{}/changesets/{}",
        base(&project),
        rollback.body["data"]["id"].as_str().expect("id")
    );
    let hash = rollback.body["data"]["hash"]
        .as_str()
        .expect("hash")
        .to_owned();
    let if_match = format!("\"{hash}\"");
    let submitted = call(
        &editor,
        Method::POST,
        &format!("{path}/submit"),
        None,
        &[("if-match", &if_match)],
    )
    .await
    .expect("request");
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);
    let review = call(
        &owner,
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
    let back = publish(&owner, &path, &hash, generation + 2).await;
    assert_eq!(back.status, StatusCode::CREATED, "{}", back.body);
    let row = delivered(&project, &entry, None).await;
    assert_eq!(row["title"], "A post");
    assert_eq!(
        row["media"][&image]["url"],
        format!("{current}/{first_sha}"),
        "{row}"
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one journey: each step's state is the next step's setup"
)]
async fn the_library_pages_searches_filters_and_keeps_metadata_under_if_match() {
    let _server = common::Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let mut ids = Vec::new();
    for n in 0..5 {
        ids.push(upload(&owner, &project, PNG, &format!("photo-{n}.png")).await);
    }
    let special = upload(&owner, &project, PNG, "Harbour_at_dawn.png").await;

    // Pages of two, newest first, never repeating, ending with no cursor.
    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let after = cursor
            .as_ref()
            .map_or_else(String::new, |c| format!("&cursor={c}"));
        let page = call(
            &owner,
            Method::GET,
            &format!("{}?limit=2{after}", assets(&project)),
            None,
            &[],
        )
        .await
        .expect("request");
        assert_eq!(page.status, StatusCode::OK, "{}", page.body);
        let data = page.body["data"].as_array().expect("data");
        assert!(data.len() <= 2);
        seen.extend(
            data.iter()
                .map(|asset| asset["id"].as_str().expect("id").to_owned()),
        );
        match page.body["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    let mut expected: Vec<String> = ids.iter().cloned().chain([special.clone()]).collect();
    expected.reverse();
    assert_eq!(seen, expected, "newest first, each once");

    // Search: case-insensitive, `_` and `%` match only themselves.
    let found = call(
        &owner,
        Method::GET,
        &format!("{}?q=harbour_AT", assets(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    let found: Vec<&str> = found.body["data"]
        .as_array()
        .expect("data")
        .iter()
        .filter_map(|a| a["id"].as_str())
        .collect();
    assert_eq!(found, [special.as_str()]);
    let wild = call(
        &owner,
        Method::GET,
        &format!("{}?q=%25", assets(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(wild.body["data"], json!([]), "{}", wild.body);
    let long = call(
        &owner,
        Method::GET,
        &format!("{}?q={}", assets(&project), "x".repeat(101)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        long.body["error"]["details"]["reason"], "invalid_query",
        "{}",
        long.body
    );
    let bad = call(
        &owner,
        Method::GET,
        &format!("{}?cursor=nonsense", assets(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        bad.body["error"]["details"]["reason"], "invalid_cursor",
        "{}",
        bad.body
    );

    // Metadata under If-Match: the read's ETag, a change, a stale second change refused.
    let read = call(
        &owner,
        Method::GET,
        &format!("{}/{special}", assets(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    let etag = read.headers[header::ETAG]
        .to_str()
        .expect("etag")
        .to_owned();
    assert_eq!(etag, "\"v1\"");
    let change_key = key();
    let changed = call(
        &owner,
        Method::PATCH,
        &format!("{}/{special}", assets(&project)),
        Some(&json!({"alt": "Boats in the harbour at dawn", "title": "Harbour", "tags": ["harbour", "boats"]})),
        &[("if-match", &etag), ("idempotency-key", &change_key)],
    )
    .await
    .expect("request");
    assert_eq!(changed.status, StatusCode::OK, "{}", changed.body);
    assert_eq!(changed.headers[header::ETAG], "\"v2\"");
    assert_eq!(changed.body["data"]["alt"], "Boats in the harbour at dawn");
    assert_eq!(changed.body["data"]["tags"], json!(["harbour", "boats"]));
    assert_eq!(changed.body["data"]["metadata_version"], 2);
    // The same request again is the recorded answer, not a second change.
    let replay = call(
        &owner,
        Method::PATCH,
        &format!("{}/{special}", assets(&project)),
        Some(&json!({"alt": "Boats in the harbour at dawn", "title": "Harbour", "tags": ["harbour", "boats"]})),
        &[("if-match", &etag), ("idempotency-key", &change_key)],
    )
    .await
    .expect("request");
    assert!(replay.replayed(), "{}", replay.body);
    assert_eq!(replay.headers[header::ETAG], "\"v2\"");
    let stale = call(
        &owner,
        Method::PATCH,
        &format!("{}/{special}", assets(&project)),
        Some(&json!({"title": "Someone else's title"})),
        &[("if-match", &etag), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(stale.status, StatusCode::CONFLICT, "{}", stale.body);
    assert_eq!(
        stale.body["error"]["details"]["reason"], "stale_version",
        "{}",
        stale.body
    );
    assert_eq!(stale.headers[header::ETAG], "\"v2\"");

    // The new words are searchable, and the tag filters.
    let by_alt = call(
        &owner,
        Method::GET,
        &format!("{}?q=boats", assets(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        by_alt.body["data"][0]["id"],
        special.as_str(),
        "{}",
        by_alt.body
    );
    let by_tag = call(
        &owner,
        Method::GET,
        &format!("{}?tag=boats", assets(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    let tagged = by_tag.body["data"].as_array().expect("data");
    assert_eq!(tagged.len(), 1, "{}", by_tag.body);
    // A refused change names its place.
    let invalid = call(
        &owner,
        Method::PATCH,
        &format!("{}/{special}", assets(&project)),
        Some(&json!({"tags": ["ok", "Not OK"]})),
        &[("if-match", "\"v2\""), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(
        invalid.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        invalid.body
    );
    assert_eq!(
        invalid.body["error"]["details"]["pointer"], "/tags/1",
        "{}",
        invalid.body
    );
}

/// `POST /content/v1/{project}/production/{collection}/query` with `document`, anonymously.
async fn query_published(project: &str, collection: &str, document: &Value) -> Value {
    let response = app()
        .post(format!(
            "{}/content/v1/{project}/production/{collection}/query",
            common::ORIGIN
        ))
        .json(document)
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK);
    response.json::<Value>().await.expect("json")
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one journey: two types, two uploads, one release, three reads"
)]
async fn a_query_row_carries_its_populated_references_images_and_only_what_it_answers() {
    let _server = common::Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let editor = sign_in("dev-editor").await;
    let project = project(&owner).await;
    join(&owner, &editor, &project, "editor").await;
    for (slug, fields) in [
        (
            "authors",
            json!([
                {"id": "name", "label": "Name", "type": "text", "required": true},
                {"id": "avatar", "label": "Avatar", "type": "asset"}
            ]),
        ),
        (
            "stories",
            json!([
                {"id": "title", "label": "Title", "type": "text", "required": true},
                {"id": "author", "label": "Author", "type": "reference", "to": "authors"},
                {"id": "body", "label": "Body", "type": "rich_text"}
            ]),
        ),
    ] {
        let made = call(
            &owner,
            Method::POST,
            &format!("{}/content-types", base(&project)),
            Some(&json!({"slug": slug, "definition": {
                "name": slug, "title_field": fields[0]["id"], "fields": fields
            }})),
            &[("idempotency-key", &key())],
        )
        .await
        .expect("request");
        assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    }
    let avatar = upload(&editor, &project, WIDE_PNG, "avatar.png").await;
    let figure = upload(&editor, &project, PNG, "figure.png").await;
    usable(&owner, &project, &avatar).await;
    usable(&owner, &project, &figure).await;
    let author = call(
        &editor,
        Method::POST,
        &format!("{}/collections/authors/entries", base(&project)),
        Some(&json!({"name": "Ada", "avatar": avatar})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(author.status, StatusCode::CREATED, "{}", author.body);
    let author = author.body["data"]["id"].as_str().expect("id").to_owned();
    let story = call(
        &editor,
        Method::POST,
        &format!("{}/collections/stories/entries", base(&project)),
        Some(&json!({"title": "A story", "author": author, "body": body(&[&figure])})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(story.status, StatusCode::CREATED, "{}", story.body);
    let story = story.body["data"]["id"].as_str().expect("id").to_owned();
    // One release of both.
    let created = call(
        &editor,
        Method::POST,
        &format!("{}/changesets", base(&project)),
        Some(&json!({"title": "Both", "entries": [author, story]})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let path = format!(
        "{}/changesets/{}",
        base(&project),
        created.body["data"]["id"].as_str().expect("id")
    );
    let hash = created.body["data"]["hash"]
        .as_str()
        .expect("hash")
        .to_owned();
    let if_match = format!("\"{hash}\"");
    call(
        &editor,
        Method::POST,
        &format!("{path}/submit"),
        None,
        &[("if-match", &if_match)],
    )
    .await
    .expect("request");
    call(
        &owner,
        Method::POST,
        &format!("{path}/reviews"),
        Some(&json!({"decision": "approve"})),
        &[("if-match", &if_match)],
    )
    .await
    .expect("request");
    let published = publish(&owner, &path, &hash, 0).await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);

    // Populated: the story's own image and its author's avatar, each at its recorded version.
    let page = query_published(&project, "stories", &json!({"depth": 1})).await;
    let media = &page["data"][0]["media"];
    assert!(media[&figure]["url"].is_string(), "{page}");
    assert!(media[&avatar]["url"].is_string(), "{page}");
    assert_eq!(media[&avatar]["width"], 400);
    // Narrowed to the title and author: the body's image is no longer named, so not listed.
    let page = query_published(
        &project,
        "stories",
        &json!({"depth": 1, "fields": ["title", "author"]}),
    )
    .await;
    let media = &page["data"][0]["media"];
    assert!(media[&avatar]["url"].is_string(), "{page}");
    assert!(media.get(&figure).is_none(), "{page}");
    // Without depth the author stays an id, and its avatar isn't this row's to list.
    let page = query_published(&project, "stories", &json!({})).await;
    assert!(page["data"][0]["media"].get(&avatar).is_none(), "{page}");
    assert!(
        page["data"][0]["media"][&figure]["url"].is_string(),
        "{page}"
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_search_never_finds_another_projects_uploads() {
    let _server = common::Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let mine = project(&owner).await;
    let theirs = common::project(&owner).await;
    let own = upload(&owner, &mine, PNG, "lighthouse-mine.png").await;
    upload(&owner, &theirs, PNG, "lighthouse-theirs.png").await;
    // The search reads through colophon_asset_search (migration 0067), which has no row
    // security of its own: it must scope to the transaction's project exactly as the policy does.
    let found = call(
        &owner,
        Method::GET,
        &format!("{}?q=lighthouse", assets(&mine)),
        None,
        &[],
    )
    .await
    .expect("request");
    let ids: Vec<&str> = found.body["data"]
        .as_array()
        .expect("data")
        .iter()
        .filter_map(|asset| asset["id"].as_str())
        .collect();
    assert_eq!(ids, [own.as_str()], "{}", found.body);
    // A state filter and a tag combine with the search inside the function.
    let none = call(
        &owner,
        Method::GET,
        &format!("{}?q=lighthouse&state=rejected", assets(&mine)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(none.body["data"], json!([]), "{}", none.body);
}
