//! What handlers can reach: readiness always; the database and sign-in when configured.

use std::{sync::Arc, time::Duration};

use axum::extract::FromRef;
use colophon_auth::OidcClient;
use colophon_db::PgPool;

use crate::Readiness;

/// Session lifetimes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionSettings {
    /// A session ends this long after sign-in, however busy.
    pub lifetime: Duration,
    /// A session ends after this long unused.
    pub idle: Duration,
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            lifetime: Duration::from_hours(12),
            idle: Duration::from_hours(2),
        }
    }
}

/// The database, sign-in, and the origins allowed to send cookie-authenticated writes.
pub struct Services {
    /// The application role's pool.
    pub pool: PgPool,
    /// The OpenID Connect client; `None` when sign-in isn't configured.
    pub oidc: Option<Arc<OidcClient>>,
    /// Session lifetimes.
    pub sessions: SessionSettings,
    /// Exact origins (`scheme://host[:port]`) whose pages may send cookie-authenticated writes.
    /// The first is this server's public origin.
    pub trusted_origins: Vec<String>,
    /// Whether delivery destinations may be private addresses: only for a development server
    /// bound to loopback (D-046).
    pub allow_private_destinations: bool,
}

/// The router's state. Cloning is two reference-count increments.
#[derive(Clone)]
pub struct AppState {
    /// Readiness, for `/readyz`.
    pub readiness: Readiness,
    /// Everything that needs the database; `None` in tests of the HTTP contract alone.
    pub services: Option<Arc<Services>>,
}

impl AppState {
    /// State for a server without a database (only health and stateless endpoints work).
    pub fn without_services(readiness: Readiness) -> Self {
        Self {
            readiness,
            services: None,
        }
    }
}

impl FromRef<AppState> for Readiness {
    fn from_ref(state: &AppState) -> Self {
        state.readiness.clone()
    }
}
