//! The Studio's static files, served on the API's origin: single-page routing, caching, the
//! security headers, and no way out of the build directory.
#![expect(
    clippy::expect_used,
    reason = "test fixtures: failing to write a temporary file is a test failure"
)]

use std::{path::PathBuf, time::Duration};

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use colophon_api::{AppState, HttpLimits, Readiness, middleware, studio};
use http_body_util::BodyExt as _;
use tower::ServiceExt as _;

const INDEX: &str = "<!doctype html><title>Studio</title>";

/// A throwaway Studio build next to a file that must never be served.
struct Build {
    root: PathBuf,
}

impl Build {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("colophon-studio-{}", uuid::Uuid::now_v7()));
        let dir = root.join("build");
        std::fs::create_dir_all(dir.join("_app/immutable")).expect("directories");
        std::fs::write(dir.join(studio::FALLBACK), INDEX).expect("index");
        std::fs::write(dir.join("_app/immutable/start.js"), "export {};").expect("script");
        std::fs::write(root.join("secret.txt"), "TOP SECRET").expect("secret");
        Self { root }
    }

    fn dir(&self) -> PathBuf {
        self.root.join("build")
    }

    fn app(&self) -> Router {
        let routes = colophon_api::routes(AppState::without_services(Readiness::new("all", false)));
        middleware::apply(
            studio::mount(routes, &self.dir()),
            HttpLimits {
                request_timeout: Duration::from_secs(5),
                max_body_bytes: 1024,
            },
        )
    }
}

impl Drop for Build {
    fn drop(&mut self) {
        let _best_effort = std::fs::remove_dir_all(&self.root);
    }
}

async fn get(app: &Router, uri: &str) -> (StatusCode, axum::http::HeaderMap, String) {
    let response = app
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).expect("request"))
        .await
        .expect("a response");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

#[tokio::test]
async fn pages_route_in_the_browser_and_assets_are_cached_by_hash() {
    let build = Build::new();
    let app = build.app();

    let (status, headers, _) = get(&app, "/studio").await;
    assert_eq!(status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(headers[header::LOCATION], "/studio/");

    for page in ["/studio/", "/studio/projects/abc/articles/def"] {
        let (status, headers, body) = get(&app, page).await;
        assert_eq!((status, body.as_str()), (StatusCode::OK, INDEX), "{page}");
        assert_eq!(headers[header::CACHE_CONTROL], "no-cache", "{page}");
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(headers[header::X_FRAME_OPTIONS], "DENY");
        assert_eq!(
            headers[header::CONTENT_SECURITY_POLICY],
            "frame-ancestors 'none'"
        );
        assert_eq!(headers[header::REFERRER_POLICY], "same-origin");
        assert!(
            headers.contains_key("x-request-id"),
            "the API's middleware applies"
        );
    }

    let (status, headers, body) = get(&app, "/studio/_app/immutable/start.js").await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, "export {};"));
    assert_eq!(
        headers[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    assert!(
        headers[header::CONTENT_TYPE]
            .to_str()
            .is_ok_and(|kind| kind.contains("javascript")),
        "{:?}",
        headers[header::CONTENT_TYPE]
    );

    // The API is unaffected.
    let (status, _, _) = get(&app, "/livez").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = get(&app, "/elsewhere").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn nothing_outside_the_build_is_ever_served() {
    let build = Build::new();
    let app = build.app();
    for attempt in [
        "/studio/../secret.txt",
        "/studio/%2e%2e/secret.txt",
        "/studio/%2E%2E%2Fsecret.txt",
        "/studio/..%2fsecret.txt",
        "/studio/_app/../../secret.txt",
    ] {
        let (_, _, body) = get(&app, attempt).await;
        assert!(
            !body.contains("TOP SECRET"),
            "{attempt} escaped the build directory"
        );
    }
}

#[test]
fn a_directory_without_the_fallback_page_is_not_a_build() {
    let build = Build::new();
    assert!(studio::check(&build.dir()).is_ok());
    assert!(
        studio::check(&build.root).is_err(),
        "no 200.html at the root"
    );
    assert!(studio::check(&build.root.join("missing")).is_err());
}
