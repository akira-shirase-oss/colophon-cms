//! Colophon's HTTP surface: one error format, request ids, the middleware stack and the
//! health endpoints, shared by every route the server will ever have. The management API
//! (`/api/v1`) is built on it from milestone P1.
//!
//! Design: `docs/modules/04-api.md`.

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
pub mod events;
mod fault;
mod graphql;
pub mod health;
pub mod mcp;
mod members;
pub mod middleware;
mod migrations;
mod openapi;
mod populate;
mod projects;
mod receipts;
mod rich_text;
mod roles;
mod schedules;
pub mod session;
mod setup;
mod shape;
pub mod state;
pub mod studio;
mod tokens;
mod type_cache;
mod valkey;
pub mod version;
mod visibility;

use axum::{Router, http::HeaderMap, routing::get};

pub use cache::{DeliveryCache, InvalidCacheUrl};
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
        // Rate limits (D-076): inside the error envelope, before any handler does work.
        .layer(admission)
        .with_state(state)
}

async fn fallback() -> ApiError {
    ApiError::RouteNotFound
}
