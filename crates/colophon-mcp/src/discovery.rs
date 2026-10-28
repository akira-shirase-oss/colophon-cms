//! Discovery: which tools to list, from what the token may do (docs/modules/09-automation.md,
//! "Typed tools": tool discovery is filtered by the caller's current authorization).
//!
//! The API has no route that tells a token its own permissions (`/me` refuses tokens by design,
//! 403 `api_token_not_allowed`; `…/tokens` needs `tokens.manage`, which no token may hold). So
//! this module asks the API questions whose answers show the permission, and that can never change
//! anything:
//!
//! - **read:** `GET {P}/collections/{c}/entries?limit=1` answers 200, or 403 without
//!   `content.read` there;
//! - **create:** `POST {P}/collections/{c}/entries` with the body `null`. The API checks the
//!   permission before the content, so it answers 403 without `content.create` there and 422
//!   with it; `null` is never valid content, so nothing can be created;
//! - **update:** `PUT {P}/collections/{c}/entries/{nil id}` with `If-Match` on the nil id and the
//!   body `null`: 403 without `content.update` there, 404 with it (no entry has the nil id), so
//!   nothing can be saved.
//!
//! A tool is listed when the token may do what it needs in at least one collection. This is
//! discovery, not authorization: a listed tool can still be refused for one entry (field lists,
//! `own` conditions), and every call is decided by the API alone (AU1). If the API ever checked
//! content before permission, a write tool would be listed and then refused, never the reverse.
//! A route that answers a token's permissions directly would replace the probes (proposed in
//! module 09).

use reqwest::Method;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    api::{self, Api, ApiError, Request, new_idempotency_key},
    tools::{Needs, Tool},
};

/// The most collections probed for each permission; collections past these are not looked at.
pub const MAX_PROBED: usize = 50;

/// What the token may do somewhere in the project.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Allowed {
    /// `content.read` in some collection.
    pub read: bool,
    /// `content.create` in some collection.
    pub create: bool,
    /// `content.update` in some collection.
    pub update: bool,
}

impl Allowed {
    /// Whether a tool that needs `needs` is listed.
    pub const fn permits(self, needs: Needs) -> bool {
        match needs {
            Needs::Token => true,
            Needs::Read => self.read,
            Needs::Create => self.create,
            Needs::Update => self.update,
        }
    }

    /// The definitions of the tools to list.
    pub fn tools(self) -> Vec<Value> {
        Tool::ALL
            .into_iter()
            .filter(|tool| self.permits(tool.needs()))
            .map(Tool::definition)
            .collect()
    }
}

/// One question about a permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Probe {
    Read,
    Create,
    Update,
}

impl Probe {
    /// Whether the token may do this in `collection`, from the API's answer.
    async fn ask(self, api: &Api, collection: &str) -> Result<bool, ApiError> {
        let entries = format!("/collections/{collection}/entries");
        let nobody = Uuid::nil().to_string();
        let key = new_idempotency_key();
        let request = match self {
            Self::Read => Request::get(entries).query("limit", "1"),
            Self::Create => Request::new(Method::POST, entries)
                .json(&Value::Null)
                .idempotency_key(&key),
            Self::Update => Request::new(Method::PUT, format!("{entries}/{nobody}"))
                .json(&Value::Null)
                .if_match(&nobody)
                .idempotency_key(&key),
        };
        match (self, api.send(&request).await) {
            (Self::Read, Ok(_)) => Ok(true),
            (_, Ok(_)) => {
                // Impossible while the API refuses `null` content; say so loudly if it ever doesn't.
                tracing::error!(probe = ?self, "a discovery probe was accepted");
                Ok(true)
            }
            (_, Err(error)) if error.refused_with(403) => Ok(false),
            (Self::Create, Err(error)) if error.refused_with(422) => Ok(true),
            (Self::Update, Err(error)) if error.refused_with(404) || error.refused_with(422) => {
                Ok(true)
            }
            // The collection went away since it was listed.
            (Self::Read | Self::Create, Err(error)) if error.refused_with(404) => Ok(false),
            (_, Err(error)) => Err(error),
        }
    }
}

/// Asks the API what the token may do. An error means the token or the server couldn't be
/// used at all (revoked, wrong project, unreachable); the caller reports it rather than listing
/// nothing.
pub async fn discover(api: &Api) -> Result<Allowed, ApiError> {
    let answer = api.send(&Request::get("/content-types")).await?;
    let collections: Vec<String> = api::data(&answer)?
        .as_array()
        .ok_or(ApiError::BadAnswer)?
        .iter()
        .filter_map(|content_type| content_type["slug"].as_str().map(str::to_owned))
        .take(MAX_PROBED)
        .collect();
    let mut allowed = Allowed::default();
    for probe in [Probe::Read, Probe::Create, Probe::Update] {
        for collection in &collections {
            if probe.ask(api, collection).await? {
                match probe {
                    Probe::Read => allowed.read = true,
                    Probe::Create => allowed.create = true,
                    Probe::Update => allowed.update = true,
                }
                break;
            }
        }
    }
    Ok(allowed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(allowed: Allowed) -> Vec<String> {
        allowed
            .tools()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    #[test]
    fn a_read_only_token_sees_no_write_tools() {
        let read_only = Allowed {
            read: true,
            ..Allowed::default()
        };
        assert_eq!(
            names(read_only),
            [
                "list_content_types",
                "list_entries",
                "get_entry",
                "get_publish_request"
            ]
        );
    }

    #[test]
    fn writing_tools_follow_the_permissions_they_need() {
        let everything = Allowed {
            read: true,
            create: true,
            update: true,
        };
        assert_eq!(names(everything).len(), Tool::ALL.len());
        let create_only = Allowed {
            create: true,
            ..Allowed::default()
        };
        assert_eq!(
            names(create_only),
            ["list_content_types", "create_draft", "get_publish_request"]
        );
        let update_only = Allowed {
            update: true,
            ..Allowed::default()
        };
        assert_eq!(
            names(update_only),
            [
                "list_content_types",
                "update_draft",
                "create_publish_request",
                "request_review",
                "get_publish_request"
            ]
        );
    }
}
