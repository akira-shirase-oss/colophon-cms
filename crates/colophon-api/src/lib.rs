//! Colophon's HTTP surface: one error format, request ids, the middleware stack and the
//! health endpoints, shared by every route the server will ever have. The management API
//! (`/api/v1`) is built on it from milestone P1.
//!
//! Design: `docs/modules/04-api.md`.

mod auth;
mod body;
mod content_types;
mod device;
mod entries;
pub mod error;
mod fault;
pub mod health;
pub mod middleware;
mod projects;
mod rich_text;
pub mod session;
mod shape;
pub mod state;
pub mod studio;

use axum::{
    Router,
    routing::{get, post},
};

pub use error::{ApiError, ErrorCode, ErrorDetails, PublicError};
pub use health::{CheckState, Dependency, Readiness, ReadinessReport, Status};
pub use middleware::{HttpLimits, REQUEST_ID_HEADER};
pub use state::{AppState, Services, SessionSettings};

/// The complete router: the routes, a fallback that answers with the error envelope, and the
/// middleware stack around all of them.
pub fn app(state: AppState, limits: HttpLimits) -> Router {
    middleware::apply(routes(state), limits)
}

/// The routes without the middleware stack, for tests that add their own routes before
/// applying it.
pub fn routes(state: AppState) -> Router {
    Router::new()
        .route("/livez", get(health::livez))
        .route("/readyz", get(health::readyz))
        .route("/auth/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/device", post(device::start))
        .route("/auth/device/approve", post(device::approve))
        .route("/auth/device/token", post(device::token))
        .route("/auth/device/{code}", get(device::show))
        .route("/me", get(auth::me))
        .route(
            "/api/v1/projects",
            get(projects::list).post(projects::create),
        )
        .route("/api/v1/projects/{project}", get(projects::get))
        .route(
            "/api/v1/projects/{project}/environments/{environment}/content-types",
            get(content_types::list).post(content_types::create),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/content-types/{slug}",
            get(content_types::get).put(content_types::update),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/entries",
            get(entries::list).post(entries::create),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/entries/{entry}",
            get(entries::get).put(entries::save),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/entries/{entry}/revisions",
            get(entries::history),
        )
        .route("/api/v1/rich-text/validate", post(rich_text::validate))
        .fallback(fallback)
        .with_state(state)
}

async fn fallback() -> ApiError {
    ApiError::RouteNotFound
}
