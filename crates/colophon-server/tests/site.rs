//! The reference site end to end (step 3.4, D-052): the real server and the site runner
//! (`apps/web/src/runner.ts`, run with Node). Each release is announced to the runner, which
//! builds that release into a static site, checks every page carries its marker, and promotes it;
//! a probe reads the live page's marker, so the release's receipt says the site shows it. An
//! article taken down is gone from the rebuilt site. Needs the stack, `scripts/migrate.sh` and
//! Node 24 with the workspace installed (`npm ci`).
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use common::{ORIGIN, Server, Session, call, create_article, project, release, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// Workers that look for work often, so the test doesn't wait.
const FAST: [(&str, &str); 1] = [("COLOPHON_WORKER_POLL_MS", "50")];
const SITE: &str = "127.0.0.1:58095";

/// The site runner, killed when dropped, its folder removed.
struct SiteRunner {
    child: Child,
    root: PathBuf,
}

impl SiteRunner {
    fn start(project: &str) -> Self {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = std::env::temp_dir().join(format!("colophon-site-{}", uuid::Uuid::now_v7().simple()));
        let child = Command::new("node")
            .arg(repo.join("apps/web/src/runner.ts"))
            .current_dir(&repo)
            .env("SITE_API", ORIGIN)
            .env("SITE_PROJECT", project)
            .env("SITE_ROOT", &root)
            .env("SITE_LISTEN", SITE)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("node runs the site runner");
        Self { child, root }
    }
}

impl Drop for SiteRunner {
    fn drop(&mut self) {
        let _already_gone = self.child.kill();
        let _reaped = self.child.wait();
        let _cleaned = std::fs::remove_dir_all(&self.root);
    }
}

/// A page of the live site: its status and HTML.
async fn page(path: &str) -> (StatusCode, String) {
    match reqwest::get(format!("http://{SITE}{path}")).await {
        Ok(response) => {
            let status = response.status();
            (status, response.text().await.unwrap_or_default())
        }
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, String::new()),
    }
}

async fn destination(owner: &Session, project: &str, body: &Value) {
    let reply = call(
        owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/environments/production/destinations"),
        Some(body),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
}

/// Waits up to `seconds` for release `generation` to be verified; its receipt.
async fn verified(owner: &Session, project: &str, generation: i64, seconds: u64) -> Value {
    let path = format!("/api/v1/projects/{project}/environments/production/releases/{generation}");
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        let reply = call(owner, Method::GET, &path, None, &[])
            .await
            .expect("request");
        if reply.body["data"]["label"] == "verified" {
            return reply.body["data"].clone();
        }
        assert!(
            Instant::now() < deadline,
            "release {generation} was never verified: {}",
            reply.body
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test]
#[ignore = "needs the stack, scripts/migrate.sh and node with npm ci"]
async fn each_release_becomes_a_checked_static_site_and_its_receipt_says_so() {
    let _server = Server::start_with(&FAST).await;
    let owner = sign_in("dev-owner").await;
    let id = project(&owner).await;
    let _site = SiteRunner::start(&id);
    let ready = Instant::now() + Duration::from_secs(20);
    while page("/").await.0 != StatusCode::OK {
        assert!(Instant::now() < ready, "the site runner never served its first build");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(page("/").await.1.contains("Nothing published yet"));
    destination(&owner, &id, &json!({"name": "Site", "url": format!("http://{SITE}/hook")})).await;
    destination(&owner, &id, &json!({"kind": "probe", "name": "Home page", "url": format!("http://{SITE}/")})).await;

    let first = create_article(&owner, &id, "Hello, site").await;
    assert_eq!(release(&owner, &id, &json!({"title": "One", "entries": [first]})).await, 1);
    let receipt = verified(&owner, &id, 1, 30).await;
    assert_eq!(receipt["locations"], 1);
    let (status, home) = page("/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(home.contains(r#"<meta name="colophon-generation" content="1">"#), "{home}");
    assert!(home.contains("Hello, site"), "{home}");
    let (status, article) = page(&format!("/articles/{first}/")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(article.contains("<h1>Hello, site</h1>"), "{article}");

    let second = create_article(&owner, &id, "Second post").await;
    assert_eq!(release(&owner, &id, &json!({"title": "Two", "entries": [second]})).await, 2);
    verified(&owner, &id, 2, 30).await;
    let (_, colophon) = page("/colophon/").await;
    assert!(colophon.contains("release <strong>2</strong>"), "{colophon}");
    assert!(colophon.contains("Hello, site: release 1"), "{colophon}");

    // Taken down: the rebuilt site has no page for it, and only the second post is listed.
    let unpublish = json!({"title": "Take it down", "unpublish": [first]});
    assert_eq!(release(&owner, &id, &unpublish).await, 3);
    verified(&owner, &id, 3, 30).await;
    assert_eq!(page(&format!("/articles/{first}/")).await.0, StatusCode::NOT_FOUND);
    let (_, home) = page("/").await;
    assert!(!home.contains("Hello, site") && home.contains("Second post"), "{home}");
}
