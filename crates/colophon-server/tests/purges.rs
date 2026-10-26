//! Purges through a fake CDN (step 3.2, part 2; D-050): F07 end to end. The fake CDN caches the
//! origin's public answers for as long as their `s-maxage` allows, files them under their
//! `Cache-Tag`s, and drops every answer with a tag a purge names. An unpublished entry stays at
//! the CDN until its purge is accepted, and the release says so meanwhile; a purge skipped because
//! a newer release came along is covered by the newer one; a refused purge leaves the release
//! marked failed. Needs the stack and `scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use common::{
    ORIGIN, Server, Session, call, create_article, finished_job, project, release, sign_in,
};
use reqwest::Method;
use serde_json::{Value, json};

/// Workers that look for work often, so the tests don't wait.
const FAST: [(&str, &str); 1] = [("COLOPHON_WORKER_POLL_MS", "50")];

/// One answer the fake CDN holds.
struct Copy {
    status: u16,
    body: Bytes,
    tags: Vec<String>,
    expires: Instant,
}

#[derive(Default)]
struct Edge {
    copies: HashMap<String, Copy>,
    /// Every purge asked for, in order: its tags, and whether it was accepted.
    purges: Vec<(Vec<String>, bool)>,
    /// What purges are answered with while set.
    refuse: Option<u16>,
}

type EdgeState = Arc<Mutex<Edge>>;

/// A CDN in front of the test server's origin: `GET` anything to read through it, `POST /purge`
/// with `{"tags": [...]}` to purge.
struct FakeCdn {
    url: String,
    edge: EdgeState,
    task: tokio::task::JoinHandle<()>,
}

/// The `s-maxage` of a `Cache-Control` value, if it lets shared caches keep the answer.
fn shared_lifetime(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let value = headers.get("cache-control")?.to_str().ok()?;
    value
        .split(',')
        .find_map(|part| part.trim().strip_prefix("s-maxage="))
        .and_then(|seconds| seconds.parse().ok())
        .map(Duration::from_secs)
}

async fn read_through(State(edge): State<EdgeState>, uri: Uri) -> Response {
    let key = uri
        .path_and_query()
        .map_or_else(|| uri.path().to_owned(), |path| path.as_str().to_owned());
    let held = {
        let edge = edge.lock().expect("not poisoned");
        edge.copies
            .get(&key)
            .filter(|copy| copy.expires > Instant::now())
            .map(|copy| (copy.status, copy.body.clone()))
    };
    let (status, body, served) = if let Some((status, body)) = held {
        (status, body, "hit")
    } else {
        let answer = reqwest::get(format!("{ORIGIN}{key}"))
            .await
            .expect("the origin answers");
        let status = answer.status().as_u16();
        let lifetime = shared_lifetime(answer.headers());
        let tags: Vec<String> = answer
            .headers()
            .get("cache-tag")
            .and_then(|value| value.to_str().ok())
            .map(|value| value.split(',').map(str::to_owned).collect())
            .unwrap_or_default();
        let body = answer.bytes().await.expect("a body");
        if let Some(lifetime) = lifetime.filter(|_| status == 200) {
            let copy = Copy {
                status,
                body: body.clone(),
                tags,
                expires: Instant::now() + lifetime,
            };
            edge.lock().expect("not poisoned").copies.insert(key, copy);
        }
        (status, body, "miss")
    };
    let mut response = (
        StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
        body,
    )
        .into_response();
    response
        .headers_mut()
        .insert("x-cdn", HeaderValue::from_static(served));
    response
}

async fn purge(State(edge): State<EdgeState>, _headers: HeaderMap, body: Bytes) -> StatusCode {
    let asked: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let tags: Vec<String> = asked["tags"]
        .as_array()
        .map(|tags| {
            tags.iter()
                .filter_map(|tag| tag.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let mut edge = edge.lock().expect("not poisoned");
    if let Some(status) = edge.refuse {
        edge.purges.push((tags, false));
        return StatusCode::from_u16(status).unwrap_or(StatusCode::SERVICE_UNAVAILABLE);
    }
    edge.copies
        .retain(|_, copy| !copy.tags.iter().any(|tag| tags.contains(tag)));
    edge.purges.push((tags, true));
    StatusCode::OK
}

impl FakeCdn {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a free port");
        let address = listener.local_addr().expect("an address");
        let edge: EdgeState = Arc::default();
        let app = axum::Router::new()
            .route("/purge", axum::routing::post(purge))
            .fallback(read_through)
            .with_state(Arc::clone(&edge));
        let task = tokio::spawn(async move {
            let _stopped = axum::serve(listener, app).await;
        });
        Self {
            url: format!("http://{address}"),
            edge,
            task,
        }
    }

    /// Reads `path` through the CDN: the status, whether it was a hit, and the body.
    async fn get(&self, path: &str) -> (u16, bool, Value) {
        let answer = reqwest::get(format!("{}{path}", self.url))
            .await
            .expect("the CDN answers");
        let status = answer.status().as_u16();
        let hit = answer
            .headers()
            .get("x-cdn")
            .is_some_and(|value| value == "hit");
        let body = answer.json().await.unwrap_or(Value::Null);
        (status, hit, body)
    }

    /// Answer purges with `status` until called with `None`.
    fn refuse(&self, status: Option<u16>) {
        self.edge.lock().expect("not poisoned").refuse = status;
    }

    fn purges(&self) -> Vec<(Vec<String>, bool)> {
        self.edge.lock().expect("not poisoned").purges.clone()
    }
}

impl Drop for FakeCdn {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
}

/// Points a new CDN destination of `project` at `cdn`.
async fn cdn_destination(owner: &Session, project: &str, cdn: &FakeCdn) {
    let reply = call(
        owner,
        Method::POST,
        &format!("{}/destinations", base(project)),
        Some(&json!({"kind": "cdn", "name": "Fake CDN", "url": format!("{}/purge", cdn.url)})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    assert_eq!(reply.body["data"]["kind"], "cdn");
}

/// The purge state the releases list shows for `generation`.
async fn purge_state(owner: &Session, project: &str, generation: i64) -> String {
    let reply = call(
        owner,
        Method::GET,
        &format!("{}/releases", base(project)),
        None,
        &[],
    )
    .await
    .expect("request");
    reply.body["data"]
        .as_array()
        .and_then(|releases| {
            releases
                .iter()
                .find(|release| release["generation"] == generation)
        })
        .and_then(|release| release["purge"].as_str())
        .expect("the release is listed")
        .to_owned()
}

/// Whether `tags` name `entry` of `project` (the environment's id sits between the two).
fn names(tags: &[String], project: &str, entry: &str) -> bool {
    let (project, entry) = (project.replace('-', ""), entry.replace('-', ""));
    tags.iter().any(|tag| {
        tag.starts_with(&format!("colophon:p:{project}:e:"))
            && tag.ends_with(&format!(":n:{entry}"))
    })
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn an_unpublished_entry_stays_at_the_cdn_until_its_purge_is_accepted() {
    let _server = Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let id = project(&owner).await;
    let cdn = FakeCdn::start().await;
    cdn_destination(&owner, &id, &cdn).await;
    let keep = create_article(&owner, &id, "Keep").await;
    let down = create_article(&owner, &id, "Take down").await;
    let both = json!({"title": "Both", "entries": [keep, down]});
    assert_eq!(release(&owner, &id, &both).await, 1);
    assert_eq!(
        finished_job(&owner, &id, 1, 15).await["status"],
        "succeeded"
    );
    assert_eq!(purge_state(&owner, &id, 1).await, "accepted");

    // Readers fill the CDN: the entry, latest and pinned, and the list.
    let list = format!("/content/v1/{id}/production/articles");
    let entry = format!("{list}/{down}");
    let pinned = format!("{entry}?generation=1");
    for path in [&list, &entry, &pinned] {
        cdn.get(path).await;
        let (status, hit, _) = cdn.get(path).await;
        assert_eq!((status, hit), (200, true), "{path}");
    }

    // The CDN refuses purges for now: the takedown is refused at the origin at once, but the
    // CDN's copies are still served, and the release says its purge is pending.
    cdn.refuse(Some(503));
    let unpublish = json!({"title": "Take it down", "unpublish": [down]});
    assert_eq!(release(&owner, &id, &unpublish).await, 2);
    let (status, _) = common::get(None, &entry).await;
    assert_eq!(status, StatusCode::GONE, "the origin denies it at once");
    let (status, hit, body) = cdn.get(&entry).await;
    assert_eq!((status, hit), (200, true), "the CDN still has its copy");
    assert_eq!(body["data"]["title"], "Take down");
    assert_eq!(purge_state(&owner, &id, 2).await, "pending");

    cdn.refuse(None);
    let job = finished_job(&owner, &id, 2, 30).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    assert!(job["attempts"].as_i64() > Some(1), "{job}");
    assert_eq!(purge_state(&owner, &id, 2).await, "accepted");
    let tags = &cdn.purges().last().expect("a purge").0.clone();
    assert!(names(tags, &id, &down), "{tags:?}");
    assert!(
        tags.iter().any(|tag| tag.ends_with(":c:articles")),
        "{tags:?}"
    );
    for path in [&entry, &pinned] {
        let (status, hit, _) = cdn.get(path).await;
        assert_eq!(
            (status, hit),
            (410, false),
            "{path}: purged, and gone at the origin"
        );
    }
    let (_, _, listed) = cdn.get(&list).await;
    let titles: Vec<&str> = listed["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["title"].as_str())
        .collect();
    assert_eq!(titles, ["Keep"]);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_purge_skipped_for_a_newer_release_is_covered_by_it() {
    let _server = Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let id = project(&owner).await;
    let cdn = FakeCdn::start().await;
    cdn_destination(&owner, &id, &cdn).await;
    let down = create_article(&owner, &id, "Short-lived").await;
    assert_eq!(
        release(&owner, &id, &json!({"title": "One", "entries": [down]})).await,
        1
    );
    assert_eq!(
        finished_job(&owner, &id, 1, 15).await["status"],
        "succeeded"
    );

    cdn.refuse(Some(503));
    let unpublish = json!({"title": "Take it down", "unpublish": [down]});
    assert_eq!(release(&owner, &id, &unpublish).await, 2);
    let later = create_article(&owner, &id, "Later").await;
    assert_eq!(
        release(&owner, &id, &json!({"title": "Three", "entries": [later]})).await,
        3
    );
    cdn.refuse(None);

    let skipped = finished_job(&owner, &id, 2, 30).await;
    assert_eq!(skipped["status"], "superseded", "{skipped}");
    let job = finished_job(&owner, &id, 3, 30).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    let (tags, accepted) = cdn.purges().last().expect("a purge").clone();
    assert!(accepted);
    for entry in [&down, &later] {
        assert!(names(&tags, &id, entry), "{entry}: {tags:?}");
    }
    assert_eq!(
        purge_state(&owner, &id, 2).await,
        "accepted",
        "release 3's purge covered release 2"
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_refused_purge_leaves_the_release_marked_failed() {
    let _server = Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let id = project(&owner).await;
    let cdn = FakeCdn::start().await;
    cdn_destination(&owner, &id, &cdn).await;
    // 403 is final: no retry can fix a purge the CDN won't take.
    cdn.refuse(Some(403));
    let entry = create_article(&owner, &id, "Stuck").await;
    assert_eq!(
        release(&owner, &id, &json!({"title": "One", "entries": [entry]})).await,
        1
    );
    let job = finished_job(&owner, &id, 1, 15).await;
    assert_eq!(
        (job["status"].as_str(), job["attempts"].as_i64()),
        (Some("failed"), Some(1)),
        "{job}"
    );
    assert_eq!(purge_state(&owner, &id, 1).await, "failed");
}
