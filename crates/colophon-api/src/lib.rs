//! Colophon's HTTP surface: one error format, request ids, the middleware stack and the
//! health endpoints, shared by every route the server will ever have. The management API
//! (`/api/v1`) is built on it from milestone P1.
//!
//! Design: `docs/modules/04-api.md`.

mod body;
pub mod error;
pub mod health;
pub mod middleware;
mod rich_text;

use axum::{
    Router,
    routing::{get, post},
};

pub use error::{ApiError, ErrorCode, ErrorDetails, PublicError};
pub use health::{CheckState, Dependency, Readiness, ReadinessReport, Status};
pub use middleware::{HttpLimits, REQUEST_ID_HEADER};

/// The complete router: the routes, a fallback that answers with the error envelope, and the
/// middleware stack around all of them.
pub fn app(readiness: Readiness, limits: HttpLimits) -> Router {
    middleware::apply(routes(readiness), limits)
}

/// The routes without the middleware stack, for tests that add their own routes before
/// applying it.
pub fn routes(readiness: Readiness) -> Router {
    Router::new()
        .route("/livez", get(health::livez))
        .route("/readyz", get(health::readyz))
        .route("/api/v1/rich-text/validate", post(rich_text::validate))
        .fallback(fallback)
        .with_state(readiness)
}

async fn fallback() -> ApiError {
    ApiError::RouteNotFound
}
