//! Colophon's HTTP surface: one error format, request ids, the middleware stack and the
//! health endpoints, shared by every route the server will ever have. The management API
//! (`/api/v1`) is built on it from milestone P1.
//!
//! Design: `docs/modules/04-api.md`.

mod accounts;
pub mod admission;
mod assets;
mod audience;
mod audit;
mod auth;
mod body;
mod bulk;
mod cache;
mod changesets;
mod collaboration;
mod components;
mod content_types;
mod delivery;
mod destinations;
mod device;
mod entries;
pub mod error;
mod etag_encoding;
pub mod events;
mod fault;
mod graphql;
pub mod health;
mod hint;
mod legacy_cookie;
pub mod mail;
pub mod mcp;
mod me_usage;
pub mod media_sign;
mod members;
pub mod middleware;
mod migrations;
mod openapi;
pub mod outages;
mod populate;
mod preview;
mod projects;
mod receipts;
mod rich_text;
mod roles;
mod schedules;
pub mod selfserve;
pub mod session;
mod setup;
mod shape;
pub mod state;
pub mod studio;
mod tokens;
mod type_cache;
mod undo;
pub mod uploads;
mod valkey;
pub mod version;
mod visibility;

use axum::{Router, http::HeaderMap, routing::get};

pub use cache::{DeliveryCache, InvalidCacheUrl, REFILLS, Refills};
pub use colophon_version::BuildInfo;
pub use error::{ApiError, ErrorCode, ErrorDetails, PublicError};
pub use health::{CheckState, Dependency, Readiness, ReadinessReport, Status};
pub use middleware::{HttpLimits, REQUEST_ID_HEADER};
pub use state::{AppState, Services, SessionSettings};
pub use type_cache::ContentTypeCache;
pub use version::API_VERSIONS;

/// The complete router: the routes, a fallback that answers with the error envelope, and the
/// middleware stack around all of them.
pub fn app(state: AppState, limits: HttpLimits) -> Router {
    let build = state.build;
    middleware::apply(routes(state), limits, build)
}

/// The routes without the middleware stack, for tests that add their own routes before
/// applying it.
///
/// The whole router is `openapi::TABLE`: the same table emits the `OpenAPI` document, so
/// the served routes and the published contract cannot drift (D-110).
pub fn routes(state: AppState) -> Router {
    let admission = axum::middleware::from_fn_with_state(state.clone(), admission::admit);
    // CORS for `/preview/v1` (D-172): inside admission since D-231 (G), so a preflight flood is
    // limited before its origin lookup reaches Postgres; preflights are answered here, before the
    // router. Admission's own 429s therefore carry no CORS headers (tracked, review 2026-11-21).
    let preview_cors = axum::middleware::from_fn_with_state(state.clone(), preview::cors);
    // The session cookies' old names, for one release (D-235): inside admission, so a flood of
    // old cookies is rate-limited before it reaches Postgres.
    let legacy_cookie = axum::middleware::from_fn_with_state(state.clone(), legacy_cookie::upgrade);
    // The signed-in hint (D-235): outside the legacy swap, so a swapped session counts as a new one.
    let hint = axum::middleware::from_fn_with_state(state.clone(), hint::signed_in);
    let served = std::sync::Arc::new(openapi::Served::new(&state.build));
    let mut router = Router::new();
    for route in openapi::TABLE {
        router = router.route(&route.path(), route.router());
    }
    router
        .route(
            openapi::SELF_PATH,
            get(move |headers: HeaderMap| {
                let served = served.clone();
                async move { served.respond(&headers) }
            }),
        )
        .fallback(fallback)
        // The preview CORS layer reads Postgres for a preflight's origin (D-172), so it sits inside
        // the rate limits like every other read: a flood of preflights is limited per address
        // before any reaches the database (D-231, gap G).
        .layer(preview_cors)
        .layer(legacy_cookie)
        .layer(hint)
        // A compressing proxy's ETag suffix (D-243): stripped before any handler compares a tag.
        .layer(axum::middleware::from_fn(etag_encoding::strip))
        // Rate limits (D-076): inside the error envelope, before any handler or lookup does work.
        .layer(admission)
        .with_state(state)
}

async fn fallback() -> ApiError {
    ApiError::RouteNotFound
}
