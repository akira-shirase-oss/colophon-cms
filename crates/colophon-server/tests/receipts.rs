//! Delivery receipts end to end (step 3.3, D-051): a probe destination looks for each release
//! at a fake site whose answer names the release it serves, retries while it sees an older one,
//! and leaves an observation for every look. A release is verified only once probes saw it, and
//! only at the locations they looked from: a probe pinned to a location waits for a worker there.
//! A page marker counts like the header; an error page needs attention. Needs the stack and
//! `scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use std::{
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicI64, AtomicU16, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    extract::State,
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use common::{Server, Session, call, create_article, local_env, project, release, sign_in};
use reqwest::Method;
use serde_json::{Value, json};

/// Workers that look for work often, so the tests don't wait.
const FAST: [(&str, &str); 1] = [("COLOPHON_WORKER_POLL_MS", "50")];

/// How the fake site answers.
#[derive(Default)]
struct Site {
    /// The release it serves.
    generation: AtomicI64,
    /// 0 for 200, else this status.
    status: AtomicU16,
}

/// A site at `/` whose answer names `generation`, in the header at `/api` and as a page marker at
/// `/page`.
struct FakeSite {
    url: String,
    site: Arc<Site>,
    task: tokio::task::JoinHandle<()>,
}

fn answered(site: &Site) -> StatusCode {
    match site.status.load(Ordering::SeqCst) {
        0 => StatusCode::OK,
        status => StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api(State(site): State<Arc<Site>>) -> Response {
    let mut response = (answered(&site), "{}").into_response();
    response.headers_mut().insert(
        "colophon-generation",
        HeaderValue::from(site.generation.load(Ordering::SeqCst)),
    );
    response
}

async fn page(State(site): State<Arc<Site>>) -> Response {
    let generation = site.generation.load(Ordering::SeqCst);
    let html = format!(
        r#"<!doctype html><html><head><meta name="colophon-generation" content="{generation}"></head><body>Hello</body></html>"#
    );
    (answered(&site), html).into_response()
}

impl FakeSite {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a free port");
        let address = listener.local_addr().expect("an address");
        let site = Arc::new(Site::default());
        let app = axum::Router::new()
            .route("/api", axum::routing::get(api))
            .route("/page", axum::routing::get(page))
            .with_state(Arc::clone(&site));
        let task = tokio::spawn(async move {
            let _stopped = axum::serve(listener, app).await;
        });
        Self {
            url: format!("http://{address}"),
            site,
            task,
        }
    }

    fn serve(&self, generation: i64) {
        self.site.generation.store(generation, Ordering::SeqCst);
    }

    fn fail_with(&self, status: u16) {
        self.site.status.store(status, Ordering::SeqCst);
    }
}

impl Drop for FakeSite {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// A second worker, at `location`, killed when dropped.
struct Worker(Child);

impl Worker {
    fn start(location: &str) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_colophon-server"))
            .args(["--role", "worker"])
            .env_clear()
            .envs(local_env())
            .env("COLOPHON_LISTEN_ADDR", "127.0.0.1:58098")
            .env("COLOPHON_LOG", "warn")
            .env("COLOPHON_WORKER_POLL_MS", "50")
            .env("COLOPHON_WORKER_LOCATION", location)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the worker starts");
        Self(child)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _already_gone = self.0.kill();
        let _reaped = self.0.wait();
    }
}

fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
}

/// Adds a probe destination of `project` looking at `url`, pinned to `location` if given.
async fn probe(owner: &Session, project: &str, name: &str, url: &str, location: Option<&str>) {
    let reply = call(
        owner,
        Method::POST,
        &format!("{}/destinations", base(project)),
        Some(&json!({"kind": "probe", "name": name, "url": url, "location": location})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
}

/// The receipt of release `generation`.
async fn receipt(owner: &Session, project: &str, generation: i64) -> Value {
    let reply = call(
        owner,
        Method::GET,
        &format!("{}/releases/{generation}", base(project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    reply.body["data"].clone()
}

/// Waits up to `seconds` for release `generation`'s label to become `label`; its receipt.
async fn labelled(
    owner: &Session,
    project: &str,
    generation: i64,
    label: &str,
    seconds: u64,
) -> Value {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        let shown = receipt(owner, project, generation).await;
        if shown["label"] == label {
            return shown;
        }
        assert!(
            Instant::now() < deadline,
            "release {generation} never became {label}: {shown}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn publish_one(owner: &Session, project: &str, title: &str) -> i64 {
    let entry = create_article(owner, project, title).await;
    release(owner, project, &json!({"title": title, "entries": [entry]})).await
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_release_is_verified_once_a_probe_sees_it_and_every_look_is_kept() {
    let _server = Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let id = project(&owner).await;

    // Nothing checks the site yet: published in the CMS, and no more.
    assert_eq!(publish_one(&owner, &id, "Unchecked").await, 1);
    let shown = receipt(&owner, &id, 1).await;
    assert_eq!(
        (shown["label"].as_str(), shown["locations"].as_i64()),
        (Some("published"), Some(0))
    );

    let site = FakeSite::start().await;
    site.serve(1);
    probe(&owner, &id, "Site API", &format!("{}/api", site.url), None).await;
    assert_eq!(publish_one(&owner, &id, "Checked").await, 2);
    // The site still serves release 1: the probe keeps looking, and the release is pending.
    let pending = labelled(&owner, &id, 2, "pending", 15).await;
    assert_eq!(pending["deliveries"][0]["kind"], "probe");
    let deadline = Instant::now() + Duration::from_secs(15);
    while receipt(&owner, &id, 2).await["observations"]
        .as_array()
        .is_none_or(Vec::is_empty)
    {
        assert!(Instant::now() < deadline, "the probe never looked");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    site.serve(2);
    let verified = labelled(&owner, &id, 2, "verified", 30).await;
    assert_eq!(
        verified["locations"], 1,
        "verified at 1 location, never just verified"
    );
    let seen = verified["observations"].as_array().expect("observations");
    assert!(seen.len() >= 2, "{seen:?}");
    let first = &seen[0];
    assert_eq!(first["observed_generation"], 1);
    assert_eq!(first["location"], "local");
    assert!(
        first["error"]
            .as_str()
            .is_some_and(|error| error.contains("not yet 2")),
        "{first}"
    );
    let last = seen.last().expect("a last look");
    assert_eq!(
        (
            last["observed_generation"].as_i64(),
            last["http_status"].as_i64()
        ),
        (Some(2), Some(200))
    );
    assert!(
        last["body_hash"]
            .as_str()
            .is_some_and(|hash| hash.starts_with("sha256:")),
        "{last}"
    );
    assert_eq!(verified["deliveries"][0]["state"], "done");
    // Release 1 had no probe when it went out, but release 2's look covers it.
    assert_eq!(receipt(&owner, &id, 1).await["label"], "verified");
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_probe_pinned_elsewhere_waits_for_a_worker_there() {
    let _server = Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let id = project(&owner).await;
    let site = FakeSite::start().await;
    site.serve(1);
    probe(&owner, &id, "From here", &format!("{}/api", site.url), None).await;
    probe(
        &owner,
        &id,
        "From Frankfurt",
        &format!("{}/page", site.url),
        Some("eu-central"),
    )
    .await;

    assert_eq!(publish_one(&owner, &id, "Far and near").await, 1);
    // The local worker sees it; nobody runs in eu-central yet.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let shown = receipt(&owner, &id, 1).await;
        let states: Vec<&str> = shown["deliveries"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|delivery| delivery["state"].as_str())
            .collect();
        if states == ["done", "pending"] {
            assert_eq!(shown["label"], "pending");
            assert_eq!(shown["locations"], 1);
            break;
        }
        assert!(Instant::now() < deadline, "{shown}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let _frankfurt = Worker::start("eu-central");
    let verified = labelled(&owner, &id, 1, "verified", 20).await;
    assert_eq!(verified["locations"], 2, "{verified}");
    let mut places: Vec<&str> = verified["observations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|seen| seen["location"].as_str())
        .collect();
    places.sort_unstable();
    places.dedup();
    assert_eq!(places, ["eu-central", "local"]);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn an_error_page_needs_attention_even_with_a_marker() {
    let _server = Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let id = project(&owner).await;
    let site = FakeSite::start().await;
    site.serve(1);
    site.fail_with(404);
    probe(
        &owner,
        &id,
        "Broken page",
        &format!("{}/page", site.url),
        None,
    )
    .await;
    assert_eq!(publish_one(&owner, &id, "Lost").await, 1);
    let shown = labelled(&owner, &id, 1, "needs_attention", 15).await;
    assert_eq!(shown["deliveries"][0]["state"], "failed");
    assert_eq!(shown["deliveries"][0]["attempts"], 1, "a 404 is final");
    assert_eq!(shown["observations"][0]["http_status"], 404);
    assert_eq!(
        shown["observations"][0]["observed_generation"], 1,
        "the marker was there"
    );

    // Fixed and published again: the new release is verified, and it covers the old one.
    site.fail_with(0);
    site.serve(2);
    assert_eq!(publish_one(&owner, &id, "Found").await, 2);
    labelled(&owner, &id, 2, "verified", 15).await;
    assert_eq!(receipt(&owner, &id, 1).await["label"], "verified");
}
