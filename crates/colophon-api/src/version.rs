//! `GET /version`: which release this server is, and which API versions it serves (D-062).
//!
//! It is unauthenticated, like the health endpoints, because a client checks it before it signs
//! in: the CLI reads it to say in plain words that it is too old or too new for this server,
//! instead of failing later on a missing field. It names versions only, never hosts or
//! configuration. Its shape never changes: fields may be added, none removed or renamed, so a
//! client of any version can read it.

use axum::{
    Json,
    extract::State,
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::{AppState, health::no_store};

/// The API versions this server serves, oldest first. `/api/v1` is the contract (D-062).
pub const API_VERSIONS: &[&str] = &["v1"];

#[derive(Serialize)]
struct Report {
    version: &'static str,
    commit: &'static str,
    build_date: &'static str,
    api: &'static [&'static str],
}

/// `GET /version`.
pub(crate) async fn get(State(state): State<AppState>) -> Response {
    let build = state.build;
    no_store(
        Json(Report {
            version: build.version,
            commit: build.commit,
            build_date: build.build_date,
            api: API_VERSIONS,
        })
        .into_response(),
    )
}
