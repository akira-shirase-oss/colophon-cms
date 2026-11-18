//! Draft preview on the editor's own site (D-171, D-172), against the real binary and stack.
//!
//! Leaking a draft is the worst failure here, so the first test puts a caching proxy — a
//! "cache everything" CDN that keys on the URL alone and ignores `Authorization` and `Vary`,
//! keeping whatever `Cache-Control` doesn't forbid — in front of the server, and proves a
//! preview answer is never stored or served to anyone else. The scope property test lays random
//! grants over random content in two projects and checks every read against the pure overlay
//! model (`colophon_domain::preview::overlay`). The rest walk the threat model's rows: single
//! use, revocation, a removed issuer, a hidden field, a moved or published publish request, API
//! tokens, and the exchange race. Needs the stack and `scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
};

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, HeaderValue, Method as AxumMethod, StatusCode as AxumStatus, Uri},
    response::{IntoResponse, Response},
};
use colophon_domain::preview::{Source, overlay};
use common::{
    ORIGIN, Server, Session, article, call, create_article, entries, join, key, project, release,
    sign_in,
};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The site address the tests' projects preview on: never fetched, only filled.
const TARGET: &str = "http://localhost:58099/preview?code={code}&path=/blog/{id}";

fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
}

async fn set_target(owner: &Session, project: &str) {
    let reply = call(
        owner,
        Method::PUT,
        &format!("{}/preview-target", base(project)),
        Some(&json!({ "url_template": TARGET })),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["data"]["origin"], "http://localhost:58099");
}

/// Issues a preview of `scope` (`{"entry": …}` or `{"changeset": …}`): the grant's view.
async fn issue(session: &Session, project: &str, scope: &Value) -> Value {
    let reply = call(
        session,
        Method::POST,
        &format!("{}/previews", base(project)),
        Some(scope),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    assert_eq!(reply.headers["cache-control"], "no-store");
    reply.body["data"].clone()
}

/// Exchanges `code` on `project`: the status and body.
async fn exchange(project: &str, code: &str) -> (StatusCode, HeaderMap, Value) {
    let response = common::app()
        .post(format!("{ORIGIN}/preview/v1/{project}/production/exchange"))
        .json(&json!({ "code": code }))
        .send()
        .await
        .expect("request");
    let (status, headers) = (response.status(), convert(response.headers()));
    (
        status,
        headers,
        response.json().await.unwrap_or(Value::Null),
    )
}

/// Issues and exchanges a preview of `scope`: the token.
async fn token(session: &Session, project: &str, scope: &Value) -> String {
    let issued = issue(session, project, scope).await;
    let (status, _, body) = exchange(project, issued["code"].as_str().expect("a code")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["data"]["token"].as_str().expect("a token").to_owned()
}

fn convert(headers: &reqwest::header::HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (name, value) in headers {
        if let (Ok(name), Ok(value)) = (
            axum::http::HeaderName::from_bytes(name.as_str().as_bytes()),
            HeaderValue::from_bytes(value.as_bytes()),
        ) {
            out.append(name, value);
        }
    }
    out
}

/// A preview read of `path` with `token`: status, headers, body.
async fn read(token: Option<&str>, path: &str) -> (StatusCode, HeaderMap, Value) {
    let mut request = common::app().get(format!("{ORIGIN}{path}"));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.expect("request");
    let (status, headers) = (response.status(), convert(response.headers()));
    (
        status,
        headers,
        response.json().await.unwrap_or(Value::Null),
    )
}

fn reason(body: &Value) -> &str {
    body["error"]["details"]["reason"].as_str().unwrap_or("?")
}

/// Saves `title` as the entry's new draft.
async fn save(owner: &Session, project: &str, entry: &str, title: &str) {
    let path = format!("{}/{entry}", entries(project));
    let current = call(owner, Method::GET, &path, None, &[])
        .await
        .expect("request");
    let etag = current.etag().expect("an ETag");
    let saved = call(
        owner,
        Method::PUT,
        &path,
        Some(&article(title, "draft text")),
        &[("if-match", &etag), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
}

fn titles(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| row["title"].as_str().map(str::to_owned))
        .collect()
}

fn assert_sealed(headers: &HeaderMap, what: &str) {
    assert_eq!(headers["cache-control"], "private, no-store", "{what}");
    assert_eq!(
        headers["x-robots-tag"], "noindex, nofollow, noarchive",
        "{what}"
    );
    assert_eq!(headers["referrer-policy"], "no-referrer", "{what}");
    assert!(headers.get("etag").is_none(), "{what}: no ETag");
    assert!(headers.get("cache-tag").is_none(), "{what}: no Cache-Tag");
}

// ---------------------------------------------------------------------------
// A caching proxy that keeps everything `Cache-Control` doesn't forbid.

#[derive(Default)]
struct Store {
    /// Answers held, by path and query alone — `Authorization` and `Vary` ignored.
    copies: HashMap<String, (u16, Bytes)>,
    /// Every path whose answer the proxy chose to store.
    stored: Vec<String>,
}

type StoreState = Arc<Mutex<Store>>;

/// Whether an aggressive shared cache may keep this answer: anything but `no-store` and
/// `private` — stricter origins than this are what the test proves Colophon is.
fn storable(headers: &reqwest::header::HeaderMap) -> bool {
    let value = headers
        .get("cache-control")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    !value
        .split(',')
        .map(str::trim)
        .any(|part| part == "no-store" || part == "private")
}

async fn proxy(
    State(store): State<StoreState>,
    method: AxumMethod,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let path = uri
        .path_and_query()
        .map_or_else(|| uri.path().to_owned(), |p| p.as_str().to_owned());
    if method == AxumMethod::GET {
        let held = store
            .lock()
            .expect("not poisoned")
            .copies
            .get(&path)
            .cloned();
        if let Some((status, body)) = held {
            let mut response =
                (AxumStatus::from_u16(status).unwrap_or(AxumStatus::OK), body).into_response();
            response
                .headers_mut()
                .insert("x-cache", HeaderValue::from_static("hit"));
            return response;
        }
    }
    let mut request = common::app().request(
        Method::from_bytes(method.as_str().as_bytes()).expect("a method"),
        format!("{ORIGIN}{path}"),
    );
    for name in ["authorization", "content-type"] {
        if let Some(value) = headers.get(name) {
            request = request.header(name, value.as_bytes());
        }
    }
    let answer = request.body(body).send().await.expect("the origin answers");
    let status = answer.status().as_u16();
    let keep = method == AxumMethod::GET && status == 200 && storable(answer.headers());
    let origin_headers = convert(answer.headers());
    let bytes = answer.bytes().await.expect("a body");
    if keep {
        let mut store = store.lock().expect("not poisoned");
        store.copies.insert(path.clone(), (status, bytes.clone()));
        store.stored.push(path);
    }
    let mut response = (
        AxumStatus::from_u16(status).unwrap_or(AxumStatus::BAD_GATEWAY),
        bytes,
    )
        .into_response();
    for (name, value) in &origin_headers {
        response.headers_mut().insert(name.clone(), value.clone());
    }
    response
        .headers_mut()
        .insert("x-cache", HeaderValue::from_static("miss"));
    response
}

struct CachingProxy {
    url: String,
    store: StoreState,
    task: tokio::task::JoinHandle<()>,
}

impl CachingProxy {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a free port");
        let address = listener.local_addr().expect("an address");
        let store: StoreState = Arc::default();
        let app = axum::Router::new()
            .fallback(proxy)
            .with_state(Arc::clone(&store));
        let task = tokio::spawn(async move {
            let _stopped = axum::serve(listener, app).await;
        });
        Self {
            url: format!("http://{address}"),
            store,
            task,
        }
    }

    async fn get(&self, token: Option<&str>, path: &str) -> (u16, bool, Value) {
        let mut request = common::app().get(format!("{}{path}", self.url));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let answer = request.send().await.expect("the proxy answers");
        let status = answer.status().as_u16();
        let hit = answer
            .headers()
            .get("x-cache")
            .is_some_and(|value| value == "hit");
        (status, hit, answer.json().await.unwrap_or(Value::Null))
    }

    fn stored(&self) -> Vec<String> {
        self.store.lock().expect("not poisoned").stored.clone()
    }
}

impl Drop for CachingProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: publish, draft, preview, through a cache, end"
)]
async fn a_draft_is_shown_only_through_its_preview_and_no_cache_ever_keeps_it() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let id = project(&owner).await;

    // Not set up yet: issuing is refused in plain words.
    let published = create_article(&owner, &id, "Published title").await;
    let refused = call(
        &owner,
        Method::POST,
        &format!("{}/previews", base(&id)),
        Some(&json!({ "entry": published })),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reason(&refused.body), "preview_not_configured");
    set_target(&owner, &id).await;
    let bad = call(
        &owner,
        Method::PUT,
        &format!("{}/preview-target", base(&id)),
        Some(&json!({ "url_template": "https://example.com/{secret}?code={code}" })),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reason(&bad.body), "invalid_preview_url");

    assert_eq!(
        release(
            &owner,
            &id,
            &json!({"title": "One", "entries": [published]})
        )
        .await,
        1
    );
    save(&owner, &id, &published, "Draft title").await;
    let unpublished = create_article(&owner, &id, "Never published").await;

    // Issue: one code, in the site's address, never stored in the clear.
    let issued = issue(&owner, &id, &json!({ "entry": published })).await;
    let code = issued["code"].as_str().expect("a code");
    assert!(code.starts_with("cpc_") && code.len() == 47, "{code}");
    assert_eq!(
        issued["url"],
        format!("http://localhost:58099/preview?code={code}&path=/blog/{published}")
    );
    let (status, headers, body) = exchange(&id, code).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_sealed(&headers, "exchange");
    let token = body["data"]["token"].as_str().expect("a token").to_owned();
    assert!(token.starts_with("cpv_") && token.len() == 47);
    assert_eq!(
        body["data"]["scope"],
        json!({"kind": "entry", "entry": published})
    );
    // A code works once.
    let (again, headers, body) = exchange(&id, code).await;
    assert_eq!(
        (again, reason(&body)),
        (StatusCode::NOT_FOUND, "preview_not_found")
    );
    assert_sealed(&headers, "a spent code");

    // The preview shows the draft, in delivery's shape, sealed against caches.
    let list = format!("/preview/v1/{id}/production/articles");
    let entry = format!("{list}/{published}");
    let (status, headers, body) = read(Some(&token), &entry).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_sealed(&headers, "a preview read");
    assert_eq!(headers["colophon-preview"], "entry");
    assert_eq!(headers["colophon-generation"], "1");
    assert_eq!(headers["vary"], "Authorization");
    assert_eq!(body["data"]["title"], "Draft title");
    assert_eq!(body["data"]["preview"]["source"], "draft");
    let (status, _, body) = read(Some(&token), &list).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        titles(&body),
        ["Draft title"],
        "out of scope and never published: absent"
    );
    let (status, _, _) = read(Some(&token), &format!("{list}/{unpublished}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Published rows read byte for byte as delivery reads them, `preview` aside.
    let other = create_article(&owner, &id, "Second").await;
    release(&owner, &id, &json!({"title": "Two", "entries": [other]})).await;
    let (_, _, previewed) = read(Some(&token), &format!("{list}/{other}")).await;
    let (_, delivered) = common::get(
        None,
        &format!("/content/v1/{id}/production/articles/{other}"),
    )
    .await;
    let mut row = previewed["data"].as_object().expect("a row").clone();
    assert_eq!(row.remove("preview"), Some(json!({"source": "published"})));
    // Delivery's entry read also lists the entry's published translations (`alternates`,
    // D-182); a preview read doesn't yet, so that one field is set aside here.
    let mut delivered = delivered["data"].as_object().expect("a row").clone();
    assert_eq!(delivered.remove("alternates"), Some(json!([])));
    assert_eq!(Value::Object(row), Value::Object(delivered));

    // Through a cache that keeps whatever it's allowed to: the preview is never stored, and
    // nobody else ever gets it — not the anonymous reader of the same address, not delivery.
    let cdn = CachingProxy::start().await;
    for path in [&entry, &list] {
        for _ in 0..2 {
            let (status, hit, body) = cdn.get(Some(&token), path).await;
            assert_eq!((status, hit), (200, false), "{path}: {body}");
        }
        let (status, hit, body) = cdn.get(None, path).await;
        assert_eq!((status, hit), (401, false), "{path}: {body}");
        assert_eq!(reason(&body), "preview_invalid");
    }
    let delivery = format!("/content/v1/{id}/production/articles/{published}");
    for _ in 0..2 {
        let (status, _, body) = cdn.get(None, &delivery).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["title"], "Published title", "never the draft");
    }
    let stored = cdn.stored();
    assert!(
        stored.iter().all(|path| !path.starts_with("/preview/")),
        "a preview answer was stored: {stored:?}"
    );
    assert!(
        stored.contains(&delivery),
        "the cache does keep what it may: {stored:?}"
    );

    // `?generation=` is refused; another environment is not found.
    let (status, _, body) = read(Some(&token), &format!("{entry}?generation=1")).await;
    assert_eq!(
        (status, reason(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "preview_reads_latest")
    );
    let (status, _, _) = read(Some(&token), &format!("/preview/v1/{id}/staging/articles")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Revoked: the next read is 401 at once, with the bearer challenge.
    let grant = issued["id"].as_str().expect("an id");
    let revoked = call(
        &owner,
        Method::DELETE,
        &format!("{}/previews/{grant}", base(&id)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(revoked.status, StatusCode::NO_CONTENT, "{}", revoked.body);
    let (status, headers, body) = read(Some(&token), &entry).await;
    assert_eq!(
        (status, reason(&body)),
        (StatusCode::UNAUTHORIZED, "preview_revoked")
    );
    assert_eq!(
        headers["www-authenticate"],
        "Bearer error=\"invalid_token\""
    );
    assert_sealed(&headers, "a refusal");
}

/// A small deterministic generator: the property runs without a dependency, and a failure
/// names its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        let len = u64::try_from(items.len()).expect("a length");
        &items[usize::try_from(self.next() % len).expect("an index")]
    }
}

/// One project's content for the property: each entry's published title (if any) and its
/// current draft's title.
struct World {
    project: String,
    published: BTreeMap<String, String>,
    drafts: BTreeMap<String, String>,
}

async fn world(owner: &Session, label: &str, rng: &mut Rng) -> World {
    let project = project(owner).await;
    set_target(owner, &project).await;
    let mut published = BTreeMap::new();
    let mut drafts = BTreeMap::new();
    let mut to_publish = Vec::new();
    for n in 0..6 {
        let title = format!("{label} {n} v1");
        let entry = create_article(owner, &project, &title).await;
        drafts.insert(entry.clone(), title.clone());
        if !rng.next().is_multiple_of(3) {
            published.insert(entry.clone(), title);
            to_publish.push(entry);
        }
    }
    if !to_publish.is_empty() {
        release(
            owner,
            &project,
            &json!({"title": label, "entries": to_publish}),
        )
        .await;
    }
    for entry in drafts.keys().cloned().collect::<Vec<_>>() {
        if rng.next().is_multiple_of(2) {
            let title = format!("{label} draft of {}", &entry[30..]);
            save(owner, &project, &entry, &title).await;
            drafts.insert(entry, title);
        }
    }
    World {
        project,
        published,
        drafts,
    }
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn every_read_shows_exactly_its_own_scope_over_the_release() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let seed = std::env::var("COLOPHON_PREVIEW_SEED")
        .ok()
        .and_then(|seed| seed.parse().ok())
        .unwrap_or_else(|| u64::from(std::process::id()) | 1);
    let mut rng = Rng(seed);
    let worlds = [
        world(&owner, "A", &mut rng).await,
        world(&owner, "B", &mut rng).await,
    ];
    // A handful of grants, each on a random entry of a random project.
    let mut grants = Vec::new();
    for _ in 0..6 {
        let at = usize::try_from(rng.next() % 2).expect("an index");
        let entries: Vec<&String> = worlds[at].drafts.keys().collect();
        let entry = (*rng.pick(&entries)).clone();
        let token = token(&owner, &worlds[at].project, &json!({ "entry": entry })).await;
        grants.push((at, entry, token));
    }
    for round in 0..60 {
        let (at, entry, token) = rng.pick(&grants);
        let asked = usize::try_from(rng.next() % 2).expect("an index");
        let world = &worlds[asked];
        let model = overlay(
            &world.published,
            &if asked == *at {
                BTreeMap::from([(entry.clone(), Some(worlds[*at].drafts[entry].clone()))])
            } else {
                BTreeMap::new()
            },
        );
        let list = format!("/preview/v1/{}/production/articles", world.project);
        let context = format!("seed {seed}, round {round}, grant on {at}:{entry}, asked {asked}");
        if asked != *at {
            // Another project's address: the token is unknown there.
            let (status, _, body) = read(Some(token), &list).await;
            assert_eq!(
                (status, reason(&body)),
                (StatusCode::UNAUTHORIZED, "preview_invalid"),
                "{context}"
            );
            continue;
        }
        if rng.next().is_multiple_of(2) {
            let (status, _, body) = read(Some(token), &list).await;
            assert_eq!(status, StatusCode::OK, "{context}: {body}");
            let mut shown = titles(&body);
            shown.sort();
            let mut expected: Vec<String> =
                model.values().map(|(title, _)| title.clone()).collect();
            expected.sort();
            assert_eq!(shown, expected, "{context}");
            for row in body["data"].as_array().into_iter().flatten() {
                let row_entry = row["id"].as_str().expect("an id");
                let source = if model.get(row_entry).map(|(_, s)| *s) == Some(Source::Scope) {
                    "draft"
                } else {
                    "published"
                };
                assert_eq!(row["preview"]["source"], source, "{context}");
            }
        } else {
            let entries: Vec<&String> = world.drafts.keys().collect();
            let target = *rng.pick(&entries);
            let (status, _, body) = read(Some(token), &format!("{list}/{target}")).await;
            match model.get(target) {
                Some((title, _)) => {
                    assert_eq!(status, StatusCode::OK, "{context}: {target}");
                    assert_eq!(body["data"]["title"], title.as_str(), "{context}: {target}");
                }
                None => assert_eq!(status, StatusCode::NOT_FOUND, "{context}: {target}"),
            }
        }
    }
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "the threat model's rows, one after another, on one project"
)]
async fn a_preview_ends_with_its_issuer_and_follows_its_publish_request() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let editor = sign_in("dev-editor").await;
    let id = project(&owner).await;
    set_target(&owner, &id).await;
    join(&owner, &editor, &id, "editor").await;
    let list = format!("/preview/v1/{id}/production/articles");

    // An editor's preview works while they're a member, and ends the moment they aren't.
    let theirs = create_article(&editor, &id, "Editor's piece").await;
    let token_of_editor = token(&editor, &id, &json!({ "entry": theirs })).await;
    let (status, _, body) = read(Some(&token_of_editor), &format!("{list}/{theirs}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // Only the issuer manages their preview.
    let mine = call(
        &owner,
        Method::GET,
        &format!("{}/previews", base(&id)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        mine.body["data"],
        json!([]),
        "the owner sees only their own"
    );
    let members = call(
        &owner,
        Method::GET,
        &format!("/api/v1/projects/{id}/members"),
        None,
        &[],
    )
    .await
    .expect("request");
    let editor_id = members.body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .find_map(|m| {
            (m["role"] == "editor").then(|| m["identity_id"].as_str().expect("an id").to_owned())
        })
        .expect("the editor");
    let removed = call(
        &owner,
        Method::DELETE,
        &format!("/api/v1/projects/{id}/members/{editor_id}"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(removed.status, StatusCode::NO_CONTENT, "{}", removed.body);
    let (status, _, body) = read(Some(&token_of_editor), &format!("{list}/{theirs}")).await;
    assert_eq!(
        (status, reason(&body)),
        (StatusCode::UNAUTHORIZED, "preview_revoked"),
        "{body}"
    );

    // An expired grant: 401 preview_expired.
    let first = create_article(&owner, &id, "First").await;
    let short = issue(&owner, &id, &json!({ "entry": first, "ttl_seconds": 60 })).await;
    let (_, _, exchanged) = exchange(&id, short["code"].as_str().expect("a code")).await;
    let short_token = exchanged["data"]["token"]
        .as_str()
        .expect("a token")
        .to_owned();
    let database = common::owner_database().await;
    sqlx::query(
        "UPDATE preview_grants SET created_at = now() - interval '2 hours',
                expires_at = now() - interval '1 second' WHERE id = $1",
    )
    .bind(uuid::Uuid::try_parse(short["id"].as_str().expect("an id")).expect("a uuid"))
    .execute(&database)
    .await
    .expect("expire it");
    let (status, _, body) = read(Some(&short_token), &list).await;
    assert_eq!(
        (status, reason(&body)),
        (StatusCode::UNAUTHORIZED, "preview_expired")
    );

    // A publish request's preview shows its pinned revisions and its unpublish items as gone.
    let down = create_article(&owner, &id, "Coming down").await;
    release(
        &owner,
        &id,
        &json!({"title": "Base", "entries": [first, down]}),
    )
    .await;
    save(&owner, &id, &first, "First, edited").await;
    let set = call(
        &owner,
        Method::POST,
        &format!("{}/changesets", base(&id)),
        Some(&json!({"title": "Next", "entries": [first], "unpublish": [down]})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(set.status, StatusCode::CREATED, "{}", set.body);
    let changeset = set.body["data"]["id"].as_str().expect("an id").to_owned();
    let changeset_token = token(&owner, &id, &json!({ "changeset": changeset })).await;
    // A later draft isn't what the request publishes: the pinned revision is.
    save(&owner, &id, &first, "First, edited again").await;
    let (status, headers, body) = read(Some(&changeset_token), &list).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(headers["colophon-preview"], "changeset");
    assert_eq!(titles(&body), ["First, edited"]);
    assert_eq!(body["data"][0]["preview"]["source"], "changeset");
    let (status, _, _) = read(Some(&changeset_token), &format!("{list}/{down}")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "an unpublish item takes the entry out"
    );

    // The request moves: 409 preview_stale.
    let path = format!("{}/changesets/{changeset}", base(&id));
    let hash = format!("\"{}\"", set.body["data"]["hash"].as_str().expect("a hash"));
    let refreshed = call(
        &owner,
        Method::POST,
        &format!("{path}/refresh"),
        None,
        &[("if-match", &hash)],
    )
    .await
    .expect("request");
    assert_eq!(refreshed.status, StatusCode::OK, "{}", refreshed.body);
    let (status, _, body) = read(Some(&changeset_token), &list).await;
    assert_eq!(
        (status, reason(&body)),
        (StatusCode::CONFLICT, "preview_stale")
    );

    // Preview the refreshed request, then publish it: 410 preview_published, naming the release.
    let fresh_token = token(&owner, &id, &json!({ "changeset": changeset })).await;
    let hash = refreshed.etag().expect("an ETag");
    for step in ["submit", "reviews"] {
        let body = (step == "reviews").then(|| json!({"decision": "approve"}));
        let reply = call(
            &owner,
            Method::POST,
            &format!("{path}/{step}"),
            body.as_ref(),
            &[("if-match", &hash)],
        )
        .await
        .expect("request");
        assert!(reply.status.is_success(), "{step}: {}", reply.body);
    }
    let published = call(
        &owner,
        Method::POST,
        &format!("{path}/publish"),
        Some(&json!({"base_generation": 1})),
        &[("if-match", &hash), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);
    let (status, headers, body) = read(Some(&fresh_token), &list).await;
    assert_eq!(
        (status, reason(&body)),
        (StatusCode::GONE, "preview_published")
    );
    assert_eq!(headers["colophon-generation"], "2");
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_code_is_spent_once_even_when_two_sites_race_for_it_and_tokens_cant_issue() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let id = project(&owner).await;
    set_target(&owner, &id).await;
    let entry = create_article(&owner, &id, "Raced").await;
    for _ in 0..5 {
        let issued = issue(&owner, &id, &json!({ "entry": entry })).await;
        let code = issued["code"].as_str().expect("a code").to_owned();
        let racers: Vec<_> = (0..4)
            .map(|_| {
                let (id, code) = (id.clone(), code.clone());
                tokio::spawn(async move { exchange(&id, &code).await.0 })
            })
            .collect();
        let mut won = 0;
        for racer in racers {
            match racer.await.expect("joined") {
                StatusCode::OK => won += 1,
                StatusCode::NOT_FOUND => {}
                other => panic!("an exchange answered {other}"),
            }
        }
        assert_eq!(won, 1, "exactly one exchange wins a code");
    }
    // A fresh code for the same grant replaces its token.
    let issued = issue(&owner, &id, &json!({ "entry": entry })).await;
    let (_, _, first) = exchange(&id, issued["code"].as_str().expect("a code")).await;
    let old = first["data"]["token"].as_str().expect("a token").to_owned();
    let again = call(
        &owner,
        Method::POST,
        &format!(
            "{}/previews/{}/code",
            base(&id),
            issued["id"].as_str().expect("an id")
        ),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(again.status, StatusCode::CREATED, "{}", again.body);
    let (_, _, second) = exchange(&id, again.body["data"]["code"].as_str().expect("a code")).await;
    let new = second["data"]["token"]
        .as_str()
        .expect("a token")
        .to_owned();
    let list = format!("/preview/v1/{id}/production/articles");
    assert_eq!(read(Some(&new), &list).await.0, StatusCode::OK);
    let (status, _, body) = read(Some(&old), &list).await;
    assert_eq!(
        (status, reason(&body)),
        (StatusCode::UNAUTHORIZED, "preview_invalid")
    );

    // A project API token can't open a preview: agents propose, people preview.
    let made = call(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{id}/tokens"),
        Some(&json!({"name": "Agent", "kind": "full-access", "expires_in_days": 1})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let secret = made.body["data"]["secret"].as_str().expect("a secret");
    let refused = common::app()
        .post(format!("{ORIGIN}{}/previews", base(&id)))
        .bearer_auth(secret)
        .json(&json!({ "entry": entry }))
        .send()
        .await
        .expect("request");
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    let body: Value = refused.json().await.unwrap_or(Value::Null);
    assert_eq!(reason(&body), "api_token_not_allowed");
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_preview_never_shows_a_field_its_issuer_cant_read() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let reader = sign_in("dev-editor").await;
    let id = project(&owner).await;
    set_target(&owner, &id).await;
    let made = call(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{id}/roles"),
        Some(&json!({
            "key": "titles",
            "name": "Titles only",
            "permissions": [{"action": "content.read", "subject": "articles", "fields": ["title"]}]
        })),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    join(&owner, &reader, &id, "titles").await;
    let entry = create_article(&owner, &id, "Owner's piece").await;
    release(&owner, &id, &json!({"title": "One", "entries": [entry]})).await;
    save(&owner, &id, &entry, "Owner's piece, edited").await;

    // The reader may open a preview of what they can read some of — and it shows only that.
    let token = token(&reader, &id, &json!({ "entry": entry })).await;
    let list = format!("/preview/v1/{id}/production/articles");
    let (status, _, body) = read(Some(&token), &format!("{list}/{entry}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["data"]["content"],
        json!({"title": "Owner's piece, edited"})
    );
    assert_eq!(body["data"]["preview"]["hidden_fields"], json!(["body"]));
    // The owner's own preview of the same entry shows everything.
    let full = token_for_owner(&owner, &id, &entry).await;
    let (_, _, body) = read(Some(&full), &format!("{list}/{entry}")).await;
    assert!(body["data"]["content"]["body"].is_object(), "{body}");
    assert!(
        body["data"]["preview"].get("hidden_fields").is_none(),
        "{body}"
    );
}

async fn token_for_owner(owner: &Session, project: &str, entry: &str) -> String {
    token(owner, project, &json!({ "entry": entry })).await
}
