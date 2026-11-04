//! Colophon's HTTP surface: one error format, request ids, the middleware stack and the
//! health endpoints, shared by every route the server will ever have. The management API
//! (`/api/v1`) is built on it from milestone P1.
//!
//! Design: `docs/modules/04-api.md`.

pub mod admission;
mod assets;
mod audit;
mod auth;
mod body;
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
mod fault;
pub mod health;
mod members;
pub mod middleware;
mod migrations;
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

use axum::{
    Router,
    routing::{get, post},
};

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
    middleware::apply(routes(state), limits)
}

/// The routes without the middleware stack, for tests that add their own routes before
/// applying it.
pub fn routes(state: AppState) -> Router {
    let admission = axum::middleware::from_fn_with_state(state.clone(), admission::admit);
    Router::new()
        .route("/livez", get(health::livez))
        .route("/readyz", get(health::readyz))
        .route("/version", get(version::get))
        .route("/auth/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/signup", post(auth::sign_up))
        .route("/auth/signin", post(auth::sign_in))
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
            "/api/v1/projects/{project}/environments/{environment}/permissions",
            get(roles::own),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/components",
            get(components::list).post(components::create),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/components/{slug}",
            get(components::get).put(components::update),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/content-types",
            get(content_types::list).post(content_types::create),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/content-types/{slug}",
            get(content_types::get)
                .put(content_types::update)
                .delete(content_types::delete),
        )
        .merge(migrations())
        .merge(collaboration())
        .merge(collections())
        .route(
            "/api/v1/projects/{project}/environments/{environment}/assets",
            get(assets::list).post(assets::upload),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/assets/{asset}",
            get(assets::get).delete(assets::delete),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/assets/{asset}/references",
            get(assets::references),
        )
        .route("/api/v1/rich-text/validate", post(rich_text::validate))
        .merge(publishing())
        .fallback(fallback)
        // Rate limits (D-076): inside the error envelope, before any handler does work.
        .layer(admission)
        .with_state(state)
}

/// Content schema migrations: plans, their items, and their lifecycle (D-100).
fn migrations() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/projects/{project}/environments/{environment}/migrations",
            get(migrations::list).post(migrations::create),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/migrations/{migration}",
            get(migrations::get),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/migrations/{migration}/items",
            get(migrations::items),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/migrations/{migration}/items/{entry}",
            axum::routing::put(migrations::resolve),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/migrations/{migration}/approve",
            post(migrations::approve),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/migrations/{migration}/apply",
            post(migrations::apply),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/migrations/{migration}/cancel",
            post(migrations::cancel),
        )
}

/// One collection's entries: pages, drafts, their history, and the JSONL export/import
/// transfer routes (D-086).
fn collections() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/entries",
            get(entries::list).post(entries::create),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/entries/query",
            post(entries::query),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/export",
            get(entries::export),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/import",
            post(entries::import),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/entries/{entry}",
            get(entries::get).put(entries::save).delete(entries::delete),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/entries/{entry}/localizations",
            post(entries::localize),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/entries/{entry}/revisions",
            get(entries::history),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/entries/{entry}/revisions/{revision}",
            get(entries::revision),
        )
}

/// Comments on an entry's revisions, and the environment's editorial tasks (E1, D-102).
fn collaboration() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/entries/{entry}/comments",
            get(collaboration::comments).post(collaboration::create_comment),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/entries/{entry}/comments/{comment}",
            get(collaboration::comment),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/entries/{entry}/comments/{comment}/resolve",
            post(collaboration::resolve),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/entries/{entry}/comments/{comment}/reopen",
            post(collaboration::reopen),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/tasks",
            get(collaboration::tasks).post(collaboration::create_task),
        )
        .route(
            "/api/v1/projects/{project}/environments/{environment}/tasks/{task}",
            get(collaboration::task).patch(collaboration::update_task),
        )
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
            "/api/v1/projects/{project}/environments/{environment}/releases/{generation}/rollback",
            post(changesets::rollback),
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
        .route(
            "/media/v1/{project}/{environment}/{asset}",
            get(assets::serve),
        )
        .route(
            "/media/v1/{project}/{environment}/{asset}/{name}",
            get(assets::serve_rendition),
        )
}

async fn fallback() -> ApiError {
    ApiError::RouteNotFound
}
