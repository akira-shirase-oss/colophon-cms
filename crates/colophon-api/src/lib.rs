//! Colophon's HTTP surface: one error format, request ids, the middleware stack and the
//! health endpoints, shared by every route the server will ever have. The management API
//! (`/api/v1`) is built on it from milestone P1.
//!
//! Design: `docs/modules/04-api.md`.

mod audit;
mod auth;
mod body;
mod cache;
mod changesets;
mod content_types;
mod delivery;
mod destinations;
mod device;
mod entries;
pub mod error;
mod fault;
pub mod health;
mod members;
pub mod middleware;
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
mod visibility;

use axum::{
    Router,
    routing::{get, post},
};

pub use cache::{DeliveryCache, InvalidCacheUrl};
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
        .route("/api/v1/setup", post(setup::claim))
        .route(
            "/api/v1/projects",
            get(projects::list).post(projects::create),
        )
        .route("/api/v1/projects/{project}", get(projects::get))
        .route("/api/v1/projects/{project}/members", get(members::list))
        .route(
            "/api/v1/projects/{project}/members/{identity}",
            axum::routing::put(members::change).delete(members::remove),
        )
        .route(
            "/api/v1/projects/{project}/invitations",
            get(members::invitations).post(members::invite),
        )
        .route(
            "/api/v1/projects/{project}/invitations/{invitation}",
            axum::routing::delete(members::revoke),
        )
        .route(
            "/api/v1/projects/{project}/roles",
            get(roles::list).post(roles::create),
        )
        .route(
            "/api/v1/projects/{project}/roles/{role}",
            get(roles::get).put(roles::update).delete(roles::delete),
        )
        .route(
            "/api/v1/projects/{project}/tokens",
            get(tokens::list).post(tokens::create),
        )
        .route(
            "/api/v1/projects/{project}/tokens/{token}",
            axum::routing::delete(tokens::revoke),
        )
        .route("/api/v1/projects/{project}/audit", get(audit::list))
        .route("/api/v1/invitations/lookup", post(members::lookup))
        .route("/api/v1/invitations/accept", post(members::accept))
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
        .route(
            "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/entries/{entry}/revisions/{revision}",
            get(entries::revision),
        )
        .route("/api/v1/rich-text/validate", post(rich_text::validate))
        .merge(publishing())
        .fallback(fallback)
        .with_state(state)
}

/// Changesets, releases, their deliveries and public delivery (P2).
fn publishing() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/projects/{project}/environments/{environment}/changesets",
            get(changesets::list).post(changesets::create),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/changesets/{changeset}",
            get(changesets::get),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/changesets/{changeset}/submit",
            post(changesets::submit),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/changesets/{changeset}/refresh",
            post(changesets::refresh),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/changesets/{changeset}/reviews",
            post(changesets::review),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/changesets/{changeset}/schedule",
            post(schedules::create),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/schedules",
            get(schedules::list),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/schedules/{schedule}",
            axum::routing::delete(schedules::cancel),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/changesets/{changeset}/publish",
            post(changesets::publish),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/releases",
            get(changesets::releases),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/releases/{generation}",
            get(receipts::get),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/destinations",
            get(destinations::list).post(destinations::create),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/destinations/{destination}",
            axum::routing::put(destinations::set_enabled),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/jobs",
            get(destinations::jobs),
        )
        .route(
            "/content/v1/{project}/{environment}/{collection}",
            get(delivery::list),
        )
        .route(
            "/content/v1/{project}/{environment}/{collection}/{entry}",
            get(delivery::get),
        )
}

async fn fallback() -> ApiError {
    ApiError::RouteNotFound
}
