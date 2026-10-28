//! Discovery: which tools to list, from what the token may do (docs/modules/09-automation.md,
//! "Typed tools": tool discovery is filtered by the caller's current authorization).
//!
//! `GET {P}/permissions` answers the caller's own permissions, a token's included (D-068). A tool
//! is listed when the token holds the action it needs in some collection. This is discovery, not
//! authorization: a listed tool can still be refused for one entry (field lists, `own`
//! conditions), and every call is decided by the API alone (AU1).

use serde_json::Value;

use crate::{
    api::{self, Api, ApiError, Request},
    tools::{Needs, Tool},
};

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

    /// What a `GET {P}/permissions` answer allows.
    pub fn from_permissions(permissions: &[Value]) -> Self {
        let holds = |action: &str| {
            permissions
                .iter()
                .any(|permission| permission["action"] == action)
        };
        Self {
            read: holds("content.read"),
            create: holds("content.create"),
            update: holds("content.update"),
        }
    }
}

/// Asks the API what the token may do. An error means the token or the server couldn't be
/// used at all (revoked, wrong project, unreachable); the caller reports it rather than listing
/// nothing.
pub async fn discover(api: &Api) -> Result<Allowed, ApiError> {
    let answer = api.send(&Request::get("/permissions")).await?;
    let permissions = api::data(&answer)?["permissions"]
        .as_array()
        .ok_or(ApiError::BadAnswer)?;
    Ok(Allowed::from_permissions(permissions))
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
    fn the_permissions_route_decides_what_is_listed() {
        let permissions = serde_json::json!([
            {"action": "content.read", "subject": "articles"},
            {"action": "content.update", "subject": "articles", "fields": ["title"]}
        ]);
        let allowed = Allowed::from_permissions(permissions.as_array().expect("a list"));
        assert_eq!(
            allowed,
            Allowed {
                read: true,
                create: false,
                update: true
            },
            "a narrowed permission still lists its tool; the API decides each call"
        );
        assert_eq!(Allowed::from_permissions(&[]), Allowed::default());
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
