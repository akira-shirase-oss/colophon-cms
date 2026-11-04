//! The query language over the published projection, end to end (DX.4, D-103): `POST
//! /content/v1/{project}/{environment}/{collection}/query` answers anyone — no sign-in —
//! from `publications` at the generation `?generation=` pins, filters, sorts and pages
//! like the draft route does, echoes `applied`, seals `q1:<generation>:<v1 token>`
//! cursors, narrows `fields`, and never shows an unpublished, revoked or tombstoned
//! entry. Needs the stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{ORIGIN, Reply, Server, Session, app, call, key, project, release, sign_in};
use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};

fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
}

/// A request that may write: content types and entries carry an idempotency key.
async fn send(session: &Session, method: Method, path: &str, body: Option<&Value>) -> Reply {
    call(session, method, path, body, &[("idempotency-key", &key())])
        .await
        .expect("request")
}

/// `POST /content/v1/{project}/production/{collection}/query` with `document` —
/// anonymously unless `bearer` gives a credential, `?cursor=` and `?generation=` as
/// named. Delivery reads sign nobody in.
async fn ask(
    project: &str,
    collection: &str,
    document: &Value,
    cursor: Option<&str>,
    generation: Option<i64>,
    bearer: Option<&str>,
) -> Reply {
    let mut path = format!("/content/v1/{project}/production/{collection}/query");
    let mut parameters: Vec<String> = Vec::new();
    if let Some(cursor) = cursor {
        parameters.push(format!("cursor={cursor}"));
    }
    if let Some(generation) = generation {
        parameters.push(format!("generation={generation}"));
    }
    if !parameters.is_empty() {
        path.push('?');
        path.push_str(&parameters.join("&"));
    }
    let mut request = app().post(format!("{ORIGIN}{path}")).json(document);
    if let Some(bearer) = bearer {
        request = request.header(header::AUTHORIZATION, format!("Bearer {bearer}"));
    }
    let response = request.send().await.expect("request");
    Reply {
        status: response.status(),
        headers: response.headers().clone(),
        body: response.json().await.unwrap_or(Value::Null),
    }
}

/// The collection's type: `headline` the title, five indexed scalars of the casts the
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

/// The project with `posts` and the five drafts of [`contents`]; the drafts' ids, in
/// creation order.
async fn seed(owner: &Session) -> (String, Vec<String>) {
    let project = project(owner).await;
    let made = send(
        owner,
        Method::POST,
        &format!("{}/content-types", base(&project)),
        Some(&json!({"slug": "posts", "definition": posts()})),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let mut ids = Vec::new();
    for content in contents() {
        let made = send(
            owner,
            Method::POST,
            &format!("{}/collections/posts/entries", base(&project)),
            Some(&content),
        )
        .await;
        assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
        ids.push(made.body["data"]["id"].as_str().expect("an id").to_owned());
    }
    (project, ids)
}

/// The drafts and the release publishing them; the generation.
async fn seeded(owner: &Session) -> (String, Vec<String>, i64) {
    let (project, ids) = seed(owner).await;
    let generation = release(
        owner,
        &project,
        &json!({"title": "All five", "entries": ids}),
    )
    .await;
    assert!(generation > 0);
    (project, ids, generation)
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

/// The generation a reply says it was read at.
fn read_at(reply: &Reply) -> Option<i64> {
    reply
        .headers
        .get("colophon-generation")
        .and_then(|value| value.to_str().ok()?.parse().ok())
}

/// The `data` rows of a refused reply stay out of the answer; `reason` says why.
fn reason(reply: &Reply) -> &str {
    reply.body["error"]["details"]["reason"]
        .as_str()
        .expect("a refusal reason")
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn the_published_set_filters_sorts_and_pages_as_drafts_do() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, _, generation) = seeded(&owner).await;

    // The same cases the draft route answers (tests/query.rs) — the published copies.
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
        let reply = ask(&project, "posts", document, None, None, None).await;
        assert_eq!(headlines(&reply), *expected, "{name}: {}", reply.body);
        assert_eq!(read_at(&reply), Some(generation));
    }

    // Paging: Guides before News ascending, the higher rating first inside a section,
    // the entry id breaking the tie in the last key's direction — three pages of two
    // cover the five posts once, each page read at the same generation.
    let document = json!({
        "sort": [{"field": "section"}, {"field": "rating", "dir": "desc"}],
        "limit": 2
    });
    let mut seen: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;
    for page in 0..3 {
        let reply = ask(&project, "posts", &document, cursor.as_deref(), None, None).await;
        seen.extend(headlines(&reply));
        assert_eq!(read_at(&reply), Some(generation));
        let next = reply.body["next_cursor"].as_str().map(str::to_owned);
        if page < 2 {
            let next = next.as_deref().expect("a page boundary");
            assert!(
                next.starts_with(&format!("q1:{generation}:")),
                "the cursor pins its generation: {next}"
            );
            cursor = Some(next.to_owned());
        } else {
            assert_eq!(next, None, "last page: {}", reply.body);
        }
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
async fn a_release_pinned_query_reads_it_as_it_was() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, ids, first) = seeded(&owner).await;

    let document = json!({"sort": [{"field": "headline"}], "limit": 100});
    let at_first = ask(&project, "posts", &document, None, Some(first), None).await;
    assert_eq!(
        headlines(&at_first),
        [
            "Alpha news",
            "Beta guide",
            "Delta guide",
            "Epsilon news",
            "Gamma news"
        ]
    );

    // A second release changes one post — the pinned read stays release 1's.
    let path = format!("{}/collections/posts/entries/{}", base(&project), ids[0]);
    let read = send(&owner, Method::GET, &path, None).await;
    let saved = call(
        &owner,
        Method::PUT,
        &path,
        Some(
            &json!({"headline": "Alpha retitled", "rating": 5, "score": 1.5,
                     "section": "News", "featured": true,
                     "publish_at": "2026-01-01T09:00:00Z"}),
        ),
        &[
            ("if-match", &read.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request");
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    let second = release(
        &owner,
        &project,
        &json!({"title": "Retitle", "entries": [ids[0].clone()]}),
    )
    .await;
    assert_eq!(second, first + 1);

    // Pinned at the first release, the old title still answers; unpinned, the new one.
    let pinned = ask(&project, "posts", &document, None, Some(first), None).await;
    assert!(headlines(&pinned).contains(&"Alpha news".to_owned()));
    assert!(!headlines(&pinned).contains(&"Alpha retitled".to_owned()));
    assert_eq!(read_at(&pinned), Some(first));
    let latest = ask(&project, "posts", &document, None, None, None).await;
    assert!(headlines(&latest).contains(&"Alpha retitled".to_owned()));
    assert_eq!(read_at(&latest), Some(second));

    // And a generation that hasn't committed is refused, like the list's.
    let future = ask(&project, "posts", &document, None, Some(second + 1), None).await;
    assert_eq!(
        future.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        future.body
    );
    assert_eq!(reason(&future), "invalid_generation");
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn unpublished_and_tombstoned_entries_never_answer() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, ids, first) = seeded(&owner).await;

    let document = json!({"limit": 100});
    let all = ask(&project, "posts", &document, None, None, None).await;
    assert_eq!(headlines(&all).len(), 5);

    // Release 2 takes Delta down: it leaves the latest read — and release 1's too,
    // since a revoked publication never delivers at any generation (DL5).
    let second = release(
        &owner,
        &project,
        &json!({"title": "Take Delta down", "unpublish": [ids[3].clone()]}),
    )
    .await;
    assert_eq!(second, first + 1);
    for generation in [None, Some(first), Some(second)] {
        let reply = ask(&project, "posts", &document, None, generation, None).await;
        assert!(
            !headlines(&reply).contains(&"Delta guide".to_owned()),
            "generation {generation:?}: {}",
            reply.body
        );
    }

    // Once unpublished it can be deleted; the tombstone answers nothing either.
    let path = format!("{}/collections/posts/entries/{}", base(&project), ids[3]);
    let read = send(&owner, Method::GET, &path, None).await;
    let deleted = call(
        &owner,
        Method::DELETE,
        &path,
        None,
        &[
            ("if-match", &read.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request");
    assert_eq!(deleted.status, StatusCode::OK, "{}", deleted.body);
    for generation in [None, Some(first)] {
        let reply = ask(&project, "posts", &document, None, generation, None).await;
        assert!(
            !headlines(&reply).contains(&"Delta guide".to_owned()),
            "tombstoned, generation {generation:?}: {}",
            reply.body
        );
        assert_eq!(headlines(&reply).len(), 4);
    }
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn anyone_answers_whether_they_present_a_credential_or_not() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, _, _) = seeded(&owner).await;

    let document = json!({"filters": [{"field": "featured", "op": "eq", "value": true}]});
    let anonymous = ask(&project, "posts", &document, None, None, None).await;
    assert_eq!(anonymous.status, StatusCode::OK, "{}", anonymous.body);

    // A credential doesn't change a public answer: the same rows, the same applied
    // echo — delivery signs nobody in.
    let credentialed = ask(
        &project,
        "posts",
        &document,
        None,
        None,
        Some("a-delivery-credential"),
    )
    .await;
    assert_eq!(credentialed.status, StatusCode::OK, "{}", credentialed.body);
    assert_eq!(credentialed.body, anonymous.body);

    // And the answer revalidates like the rest of delivery: an `If-None-Match` naming
    // its ETag gets 304.
    let etag = anonymous.etag().expect("an ETag");
    let unchanged = app()
        .post(format!(
            "{ORIGIN}/content/v1/{project}/production/posts/query"
        ))
        .header(header::IF_NONE_MATCH, &etag)
        .json(&document)
        .send()
        .await
        .expect("request");
    assert_eq!(unchanged.status(), StatusCode::NOT_MODIFIED);
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_cursor_that_isnt_this_querys_is_refused() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, _, generation) = seeded(&owner).await;

    // A real `q1:` cursor for one query…
    let ordered = json!({"sort": [{"field": "publish_at"}], "limit": 2});
    let first = ask(&project, "posts", &ordered, None, None, None).await;
    let cursor = first.body["next_cursor"]
        .as_str()
        .expect("a first page boundary")
        .to_owned();
    // …refused verbatim by another query: the inner token's hash binds them apart.
    let other = json!({"sort": [{"field": "rating"}], "limit": 2});
    let refused = ask(&project, "posts", &other, Some(&cursor), None, None).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(reason(&refused), "foreign_cursor");

    // What isn't a `q1:` envelope at all — a list's `d1:` cursor, a bare `v1:` token,
    // junk — is refused before the domain's own check.
    let list = {
        let page = app()
            .get(format!(
                "{ORIGIN}/content/v1/{project}/production/posts?limit=1"
            ))
            .send()
            .await
            .expect("request")
            .json::<Value>()
            .await
            .unwrap_or_default();
        page["next_cursor"]
            .as_str()
            .expect("a list cursor")
            .to_owned()
    };
    for token in [
        list,
        "not-a-cursor".to_owned(),
        format!("q1:x:{cursor}"),
        "q1:2".to_owned(),
    ] {
        let refused = ask(&project, "posts", &ordered, Some(&token), None, None).await;
        assert_eq!(
            refused.status,
            StatusCode::BAD_REQUEST,
            "{token}: {}",
            refused.body
        );
    }

    // And an envelope whose generation hasn't committed yet is refused like
    // `?generation=` would be.
    let ahead = cursor.replacen(
        &format!("q1:{generation}:"),
        &format!("q1:{}:", generation + 10),
        1,
    );
    let refused = ask(&project, "posts", &ordered, Some(&ahead), None, None).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(reason(&refused), "invalid_generation");
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn the_fields_the_query_names_are_the_content_it_keeps() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, _, _) = seeded(&owner).await;

    let document = json!({"fields": ["headline", "notes"], "sort": [{"field": "headline"}]});
    let reply = ask(&project, "posts", &document, None, None, None).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let data = reply.body["data"].as_array().expect("a page");
    for entry in data {
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
        // The projection's own columns still answer — only `content` narrows.
        assert!(entry["id"].is_string());
        assert!(entry["published_in"].is_number());
        assert!(entry["content_hash"].is_string());
    }
    assert_eq!(
        data[0]["content"],
        json!({"headline": "Alpha news", "notes": "first"})
    );
    assert_eq!(data[1]["content"], json!({"headline": "Beta guide"}));

    // The applied echo names the query that ran — canonical document, sha256 hash —
    // like the draft route's.
    assert_eq!(
        reply.body["applied"]["query"],
        json!({
            "depth": 0, "fields": ["headline", "notes"], "filters": [],
            "limit": 20, "sort": [{"dir": "asc", "field": "headline"}]
        })
    );
    let hash = reply.body["applied"]["query_hash"]
        .as_str()
        .expect("a query hash");
    assert!(hash.starts_with("sha256:") && hash.len() == 71, "{hash}");
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn the_document_is_validated_against_the_collection_or_the_collection_isnt_there() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, _, _) = seeded(&owner).await;

    // A field that isn't declared, a sort on a field that isn't indexed, an operator
    // its kind doesn't take: the same refusals the draft route gives.
    for (document, code) in [
        (
            json!({"filters": [{"field": "missing", "op": "eq", "value": 1}]}),
            "unknown_field",
        ),
        (json!({"sort": [{"field": "notes"}]}), "field_not_indexed"),
        (
            json!({"filters": [{"field": "featured", "op": "contains", "value": "x"}]}),
            "invalid_op",
        ),
    ] {
        let refused = ask(&project, "posts", &document, None, None, None).await;
        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{code}: {}",
            refused.body
        );
        assert_eq!(reason(&refused), code);
    }

    // And a collection that doesn't exist is 404, like the list's.
    let missing = ask(&project, "stories", &json!({}), None, None, None).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND, "{}", missing.body);
}
