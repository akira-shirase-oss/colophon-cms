//! The query language over real HTTP (step DX.3, D-103): `POST
//! {P}/collections/{collection}/entries/query` runs a validated document against each
//! entry's current draft, answers the page in the declared order, echoes the canonical
//! document and its hash, seals a page boundary into `next_cursor`, and refuses a cursor
//! that isn't this query's own — malformed, oversized, foreign or mismatched. Needs the
//! stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Reply, Server, Session, call, join, key, project, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
}

/// A request that may write: content types, entries, roles carry an idempotency key;
/// the query itself needs none.
async fn send(session: &Session, method: Method, path: &str, body: Option<&Value>) -> Reply {
    call(session, method, path, body, &[("idempotency-key", &key())])
        .await
        .expect("request")
}

/// `POST …/entries/query` with `document` — and `?cursor=` when a page boundary is
/// handed back.
async fn ask(
    session: &Session,
    project: &str,
    collection: &str,
    document: &Value,
    cursor: Option<&str>,
) -> Reply {
    let mut path = format!("{}/collections/{collection}/entries/query", base(project));
    if let Some(cursor) = cursor {
        path.push_str("?cursor=");
        path.push_str(cursor);
    }
    send(session, Method::POST, &path, Some(document)).await
}

/// The collection's type: `headline` the title, six indexed scalars of every cast the
/// translator makes, and `notes` a declared field a query may project but not filter on.
fn posts() -> Value {
    json!({
        "name": "Posts",
        "title_field": "headline",
        "fields": [
            {"id": "headline", "label": "Headline", "type": "text", "required": true, "index": true},
            {"id": "rating", "label": "Rating", "type": "number", "integer": true, "min": 0, "max": 5, "index": true},
            {"id": "score", "label": "Score", "type": "number", "index": true},
            {"id": "section", "label": "Section", "type": "enum", "values": ["News", "Guides"], "index": true},
            {"id": "featured", "label": "Featured", "type": "boolean", "index": true},
            {"id": "publish_at", "label": "Publish at", "type": "datetime", "index": true},
            {"id": "notes", "label": "Notes", "type": "text", "multiline": true}
        ]
    })
}

/// The five posts every test works against, in creation order.
fn contents() -> Vec<Value> {
    vec![
        json!({"headline": "Alpha news", "rating": 5, "score": 1.5, "section": "News",
               "featured": true, "publish_at": "2026-01-01T09:00:00Z", "notes": "first"}),
        json!({"headline": "Beta guide", "rating": 3, "score": 2.5, "section": "Guides",
               "featured": false, "publish_at": "2026-01-02T09:00:00Z"}),
        json!({"headline": "Gamma news", "rating": 5, "score": 3.5, "section": "News",
               "featured": true, "publish_at": "2026-01-03T09:00:00Z"}),
        json!({"headline": "Delta guide", "rating": 1, "score": 4.5, "section": "Guides",
               "featured": false, "publish_at": "2026-01-04T09:00:00Z"}),
        json!({"headline": "Epsilon news", "rating": 3, "score": 5.5, "section": "News",
               "featured": true, "publish_at": "2026-01-05T09:00:00Z", "notes": "last"}),
    ]
}

/// The project with `posts` and the five entries of [`contents`], as `owner`.
async fn seed(owner: &Session) -> String {
    let project = project(owner).await;
    let made = send(
        owner,
        Method::POST,
        &format!("{}/content-types", base(&project)),
        Some(&json!({"slug": "posts", "definition": posts()})),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    for content in contents() {
        let made = send(
            owner,
            Method::POST,
            &format!("{}/collections/posts/entries", base(&project)),
            Some(&content),
        )
        .await;
        assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    }
    project
}

/// The page's headlines, in the order the answer gave them.
fn headlines(reply: &Reply) -> Vec<String> {
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    reply.body["data"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|entry| {
            entry["content"]["headline"]
                .as_str()
                .expect("a headline")
                .to_owned()
        })
        .collect()
}

/// The `data` rows of a refused reply stay out of the answer; `reason` says why.
fn reason(reply: &Reply) -> &str {
    reply.body["error"]["details"]["reason"]
        .as_str()
        .expect("a refusal reason")
}

/// `v1:<hash>:<position>` sealed as a cursor is, without the domain's own sealing.
fn forge(text: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let bytes = text.as_bytes();
    for chunk in bytes.chunks(3) {
        let n = chunk.len();
        let first = u32::from(chunk[0]);
        let second = u32::from(*chunk.get(1).unwrap_or(&0));
        let third = u32::from(*chunk.get(2).unwrap_or(&0));
        let word = (first << 16) | (second << 8) | third;
        out.push(char::from(ALPHABET[(word >> 18) as usize & 63]));
        out.push(char::from(ALPHABET[(word >> 12) as usize & 63]));
        if n > 1 {
            out.push(char::from(ALPHABET[(word >> 6) as usize & 63]));
        }
        if n > 2 {
            out.push(char::from(ALPHABET[word as usize & 63]));
        }
    }
    out
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn the_document_filters_by_the_field_values_it_names() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = seed(&owner).await;

    // `eq` on a number, `in` on an enum, `contains` on text, ranges on a date-time and a
    // number, `eq` on a boolean, and two filters ANDed.
    let cases: &[(&str, Value, &[&str])] = &[
        (
            "eq number",
            json!({"filters": [{"field": "rating", "op": "eq", "value": 5}]}),
            &["Alpha news", "Gamma news"],
        ),
        (
            "in enum",
            json!({"filters": [{"field": "section", "op": "in", "value": ["Guides"]}]}),
            &["Beta guide", "Delta guide"],
        ),
        (
            "contains text",
            json!({"filters": [{"field": "headline", "op": "contains", "value": "news"}]}),
            &["Alpha news", "Gamma news", "Epsilon news"],
        ),
        (
            "lt datetime",
            json!({"filters": [{"field": "publish_at", "op": "lt", "value": "2026-01-03T00:00:00Z"}]}),
            &["Alpha news", "Beta guide"],
        ),
        (
            "gte number",
            json!({"filters": [{"field": "rating", "op": "gte", "value": 3}]}),
            &["Alpha news", "Beta guide", "Gamma news", "Epsilon news"],
        ),
        (
            "eq boolean",
            json!({"filters": [{"field": "featured", "op": "eq", "value": true}]}),
            &["Alpha news", "Gamma news", "Epsilon news"],
        ),
        (
            "and",
            json!({"filters": [
                {"field": "rating", "op": "gte", "value": 3},
                {"field": "section", "op": "eq", "value": "News"}
            ]}),
            &["Alpha news", "Gamma news", "Epsilon news"],
        ),
    ];
    for (name, document, expected) in cases {
        let reply = ask(&owner, &project, "posts", document, None).await;
        assert_eq!(headlines(&reply), *expected, "{name}: {}", reply.body);
    }
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn pages_follow_the_declared_order_until_every_entry_is_seen_once() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = seed(&owner).await;

    // Guides before News ascending; inside a section the higher rating first; the entry
    // id breaks the 5–5 tie in the last key's direction — descending, so the
    // later-created Gamma precedes Alpha. Three pages of two cover the five posts once.
    let document = json!({
        "sort": [{"field": "section"}, {"field": "rating", "dir": "desc"}],
        "limit": 2
    });
    let mut seen: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;
    for page in 0..3 {
        let reply = ask(&owner, &project, "posts", &document, cursor.as_deref()).await;
        seen.extend(headlines(&reply));
        let next = reply.body["next_cursor"].as_str().map(str::to_owned);
        if page < 2 {
            assert!(next.is_some(), "page {page}: {}", reply.body);
        } else {
            assert_eq!(next, None, "last page: {}", reply.body);
        }
        cursor = next;
    }
    assert_eq!(
        seen,
        [
            "Beta guide",
            "Delta guide",
            "Gamma news",
            "Alpha news",
            "Epsilon news"
        ]
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn the_answer_echoes_the_applied_query_and_its_hash() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = seed(&owner).await;

    // Two spellings of one query — keys and filter members scrambled — get one canonical
    // echo and one hash.
    let spellings = [
        json!({"limit": 2, "filters": [{"value": 5, "op": "eq", "field": "rating"}]}),
        json!({"filters": [{"field": "rating", "op": "eq", "value": 5}], "limit": 2, "sort": [], "depth": 0}),
    ];
    let canonical = json!({
        "depth": 0, "fields": null, "limit": 2, "sort": [],
        "filters": [{"field": "rating", "op": "eq", "value": 5}]
    });
    let mut hashes: Vec<String> = Vec::new();
    for document in &spellings {
        let reply = ask(&owner, &project, "posts", document, None).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        assert_eq!(reply.body["applied"]["query"], canonical);
        let hash = reply.body["applied"]["query_hash"]
            .as_str()
            .expect("a query hash");
        assert_eq!(hash.len(), 71, "{hash}");
        assert!(hash.starts_with("sha256:"), "{hash}");
        assert!(
            hash[7..]
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "{hash}"
        );
        hashes.push(hash.to_owned());
    }
    assert_eq!(hashes[0], hashes[1]);
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn the_query_narrows_the_content_to_the_fields_it_names() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = seed(&owner).await;

    let reply = ask(
        &owner,
        &project,
        "posts",
        &json!({"fields": ["headline", "notes"]}),
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    for entry in reply.body["data"].as_array().expect("a page") {
        // Only the selected fields — and only those the entry sets.
        let keys: Vec<&str> = entry["content"]
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert!(
            keys.iter().all(|key| ["headline", "notes"].contains(key)),
            "{keys:?}"
        );
        assert!(!entry["id"].is_null());
        assert!(entry["published"].is_null());
    }
    let data = reply.body["data"].as_array().expect("a page");
    assert_eq!(
        data[0]["content"],
        json!({"headline": "Alpha news", "notes": "first"})
    );
    assert_eq!(data[1]["content"], json!({"headline": "Beta guide"}));
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_cursor_that_isnt_this_querys_is_refused() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = seed(&owner).await;

    // A real cursor for one query…
    let ordered = json!({"sort": [{"field": "publish_at"}], "limit": 2});
    let first = ask(&owner, &project, "posts", &ordered, None).await;
    let cursor = first.body["next_cursor"]
        .as_str()
        .expect("a first page boundary")
        .to_owned();
    // …refused verbatim by another query: the hash binds them apart.
    let other = json!({"sort": [{"field": "rating"}], "limit": 2});
    let refused = ask(&owner, &project, "posts", &other, Some(&cursor)).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(reason(&refused), "foreign_cursor");

    // Tokens that don't decode, an empty one, and one over the byte cap.
    for (token, expected) in [
        ("not-a-cursor".to_owned(), "invalid_cursor"),
        (String::new(), "invalid_cursor"),
        ("A".repeat(12000), "too_large"),
    ] {
        let refused = ask(&owner, &project, "posts", &ordered, Some(&token)).await;
        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{expected}: {}",
            refused.body
        );
        assert_eq!(reason(&refused), expected);
    }

    // And a well-formed token of this query's own — a `v1:<hash>:<position>` seal —
    // whose position a field can't hold is refused before SQL runs, not cast into a
    // database error.
    let hash = first.body["applied"]["query_hash"]
        .as_str()
        .expect("a query hash")
        .trim_start_matches("sha256:")
        .to_owned();
    let cases = [
        // publish_at's boundary spelled as a number, not a date-time.
        forge(&format!(
            "v1:{hash}:[1700000000,\"123e4567-e89b-12d3-a456-426614174000\"]"
        )),
        // The entry-id boundary that isn't a uuid.
        forge(&format!(
            "v1:{hash}:[\"2026-01-02T09:00:00Z\",\"not-a-uuid\"]"
        )),
    ];
    for token in &cases {
        let refused = ask(&owner, &project, "posts", &ordered, Some(token)).await;
        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{}",
            refused.body
        );
        assert_eq!(reason(&refused), "position_mismatch");
    }
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_role_reading_only_its_own_entries_gets_only_its_own() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let guest = sign_in("dev-editor").await;
    let project = seed(&owner).await;

    let made = send(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/roles"),
        Some(&json!({
            "key": "stringer",
            "name": "Stringer",
            "permissions": [
                {"action": "content.read", "subject": "posts", "condition": "own"},
                {"action": "content.create", "subject": "posts"}
            ]
        })),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    join(&owner, &guest, &project, "stringer").await;

    let own = send(
        &guest,
        Method::POST,
        &format!("{}/collections/posts/entries", base(&project)),
        Some(
            &json!({"headline": "Guest post", "rating": 4, "section": "News",
                     "publish_at": "2026-01-06T09:00:00Z"}),
        ),
    )
    .await;
    assert_eq!(own.status, StatusCode::CREATED, "{}", own.body);

    // The owner's five stay out of the guest's page.
    let reply = ask(&guest, &project, "posts", &json!({}), None).await;
    assert_eq!(headlines(&reply), ["Guest post"]);

    // And a collection the role names nowhere is forbidden, not empty.
    let denied = ask(&guest, &project, "articles", &json!({}), None).await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN, "{}", denied.body);
}
