//! The Studio, served as static files on the API's own origin (docs/modules/11-studio.md), so
//! the session cookie, the CSRF cookie and the trusted `Origin` work with no CORS at all.
//!
//! `/studio/...` serves the built single-page app: a file if it exists, otherwise the app's
//! `200.html`, which routes in the browser. Content-hashed files under `_app/immutable/` are
//! cached for a year; everything else is revalidated on every load, so a new release reaches
//! editors at once. Every response says `nosniff`, may not be framed, and sends the referrer
//! only to this origin.

use std::path::{Path, PathBuf};

use axum::{
    Router,
    extract::Request,
    http::{HeaderValue, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use tower_http::services::{ServeDir, ServeFile};

/// Where the Studio is mounted.
pub const PREFIX: &str = "/studio";

/// The single-page app's fallback page, written by SvelteKit's static adapter.
pub const FALLBACK: &str = "200.html";

/// Why a Studio build can't be served.
#[derive(Debug, thiserror::Error)]
#[error(
    "{0} is not a Studio build: it has no {FALLBACK} (run `npm run build -w @colophon/studio`)"
)]
pub struct NotABuild(PathBuf);

/// Checks that `dir` holds a Studio build, at startup rather than on the first request.
pub fn check(dir: &Path) -> Result<(), NotABuild> {
    if dir.join(FALLBACK).is_file() {
        Ok(())
    } else {
        Err(NotABuild(dir.to_owned()))
    }
}

/// Adds the Studio at [`PREFIX`] to `router`, served from `dir`.
pub fn mount<S: Clone + Send + Sync + 'static>(router: Router<S>, dir: &Path) -> Router<S> {
    let files = ServeDir::new(dir)
        .append_index_html_on_directories(false)
        .fallback(ServeFile::new(dir.join(FALLBACK)));
    let studio = Router::new()
        .fallback_service(files)
        .layer(middleware::from_fn(headers));
    router
        .route(PREFIX, get(|| async { Redirect::permanent("/studio/") }))
        .nest_service(&format!("{PREFIX}/"), studio)
}

async fn headers(request: Request, next: Next) -> Response {
    let immutable = request.uri().path().contains("/_app/immutable/");
    let mut response = next.run(request).await.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(if immutable {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        }),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("frame-ancestors 'none'"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    response
}
