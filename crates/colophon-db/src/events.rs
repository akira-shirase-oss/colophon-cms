//! Change hints (D-108, docs/modules/02-db.md "Change notifications"): what migration 0044's
//! triggers publish on the `colophon_events` channel and what a server's listener decodes.
//!
//! A hint is a compact pipe-format string that names a committed change and nothing more — no
//! row, title or body ever crosses the channel:
//!
//! ```text
//! p=<project uuid>|e=<environment uuid|-->|k=<kind>[|c=<collection>]
//! ```
//!
//! `e` is `--` for the project's own rows (members, roles, tokens, the audit log), which have no
//! environment. `c` exists only for the kinds that carry a collection (`entries`, `comments`,
//! `tasks`). Anything a stream receives only tells a client to refetch through the API it was
//! already authorized for, so a hint can never reveal what a read would hide.

use uuid::Uuid;

/// The `LISTEN` channel every API server holds (migration 0044).
pub const CHANNEL: &str = "colophon_events";

/// What kind of row changed. Several tables report under one kind: a review or a changeset item
/// is `Changesets`; a delivery job, an unpublish tombstone or a probe observation is `Releases`;
/// a publication is `Entries`; an invitation is `Members`; a permission row is `Roles`/`Tokens`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// `entries`, `publications` and `tombstones` — drafts and what readers get.
    Entries,
    /// `collections` and `content_type_versions`.
    Types,
    /// `components` and `component_versions`.
    Components,
    /// `changesets`, `changeset_items` and `reviews`.
    Changesets,
    /// `review_stages`.
    Stages,
    /// `releases`, `jobs` and `observations`.
    Releases,
    /// `schedules`.
    Schedules,
    /// `tasks`.
    Tasks,
    /// `comments`.
    Comments,
    /// `migration_plans` and `migration_items`.
    Migrations,
    /// `assets`, `asset_jobs` and `asset_references`.
    Assets,
    /// `memberships` and `invitations` (project scope).
    Members,
    /// `roles` and `role_permissions` (project scope).
    Roles,
    /// `api_tokens` and `api_token_permissions` (project scope).
    Tokens,
    /// `destinations`.
    Destinations,
    /// `audit_events` (project scope).
    Audit,
}

impl Kind {
    /// The wire spelling, as the trigger writes and the `change` event's `k` carries it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Entries => "entries",
            Self::Types => "types",
            Self::Components => "components",
            Self::Changesets => "changesets",
            Self::Stages => "stages",
            Self::Releases => "releases",
            Self::Schedules => "schedules",
            Self::Tasks => "tasks",
            Self::Comments => "comments",
            Self::Migrations => "migrations",
            Self::Assets => "assets",
            Self::Members => "members",
            Self::Roles => "roles",
            Self::Tokens => "tokens",
            Self::Destinations => "destinations",
            Self::Audit => "audit",
        }
    }

    /// The kind spelled like [`Self::as_str`], or `None` for anything else.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "entries" => Self::Entries,
            "types" => Self::Types,
            "components" => Self::Components,
            "changesets" => Self::Changesets,
            "stages" => Self::Stages,
            "releases" => Self::Releases,
            "schedules" => Self::Schedules,
            "tasks" => Self::Tasks,
            "comments" => Self::Comments,
            "migrations" => Self::Migrations,
            "assets" => Self::Assets,
            "members" => Self::Members,
            "roles" => Self::Roles,
            "tokens" => Self::Tokens,
            "destinations" => Self::Destinations,
            "audit" => Self::Audit,
            _ => return None,
        })
    }
}

/// A committed change, as a notification reports it: which project, which environment (`None`
/// for the project's own rows), what kind of row, and the collection for kinds that have one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    /// The project the changed row belongs to: which channel hears it.
    pub project: Uuid,
    /// The row's environment, or `None` for project-level rows (the payload's `--`).
    pub environment: Option<Uuid>,
    /// What kind of row changed.
    pub kind: Kind,
    /// The collection the change is in; only `Entries`, `Comments` and `Tasks` carry one.
    pub collection: Option<String>,
}

impl Hint {
    /// The wire form the trigger writes.
    pub fn encode(&self) -> String {
        let mut text = format!(
            "p={}|e={}|k={}",
            self.project,
            self.environment
                .map_or_else(|| "--".to_owned(), |environment| environment.to_string()),
            self.kind.as_str()
        );
        if let Some(collection) = &self.collection {
            text.push_str("|c=");
            text.push_str(collection);
        }
        text
    }

    /// The hint a payload describes: `None` for anything that isn't exactly one
    /// (`p`, `e`, `k`, optional `c`) — never parsed loosely, so a malformed payload can only
    /// ever under-deliver a hint, and clients already cover that with `resync` on reconnect.
    pub fn decode(payload: &str) -> Option<Self> {
        let mut project = None;
        let mut environment = None;
        let mut kind = None;
        let mut collection = None;
        for field in payload.split('|') {
            let (key, value) = field.split_once('=')?;
            match key {
                "p" => project = Uuid::try_parse(value).ok(),
                "e" => {
                    environment = Some(if value == "--" {
                        None
                    } else {
                        Some(Uuid::try_parse(value).ok()?)
                    });
                }
                "k" => kind = Kind::parse(value),
                "c" => collection = Some(value.to_owned()),
                _ => {}
            }
        }
        Some(Self {
            project: project?,
            environment: environment?,
            kind: kind?,
            collection,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hint_round_trips() {
        let hint = Hint {
            project: Uuid::now_v7(),
            environment: Some(Uuid::now_v7()),
            kind: Kind::Entries,
            collection: Some("articles".to_owned()),
        };
        let text = hint.encode();
        assert_eq!(text.matches('|').count(), 3, "{text}");
        assert_eq!(Hint::decode(&text), Some(hint));

        let project_wide = Hint {
            project: Uuid::now_v7(),
            environment: None,
            kind: Kind::Members,
            collection: None,
        };
        assert!(project_wide.encode().contains("|e=--|"));
        assert_eq!(Hint::decode(&project_wide.encode()), Some(project_wide));
    }

    #[test]
    fn every_kind_spells_and_parses() {
        for (kind, spelled) in [
            (Kind::Entries, "entries"),
            (Kind::Types, "types"),
            (Kind::Components, "components"),
            (Kind::Changesets, "changesets"),
            (Kind::Stages, "stages"),
            (Kind::Releases, "releases"),
            (Kind::Schedules, "schedules"),
            (Kind::Tasks, "tasks"),
            (Kind::Comments, "comments"),
            (Kind::Migrations, "migrations"),
            (Kind::Assets, "assets"),
            (Kind::Members, "members"),
            (Kind::Roles, "roles"),
            (Kind::Tokens, "tokens"),
            (Kind::Destinations, "destinations"),
            (Kind::Audit, "audit"),
        ] {
            assert_eq!(kind.as_str(), spelled);
            assert_eq!(Kind::parse(spelled), Some(kind));
        }
        assert_eq!(Kind::parse("everything"), None);
    }

    #[test]
    fn malformed_payloads_do_not_decode() {
        let project = Uuid::now_v7();
        let environment = Uuid::now_v7();
        for payload in [
            "",
            "garbage",
            &format!("p={project}|e={environment}"), // no kind
            &format!("e={environment}|k=entries"),   // no project
            &format!("p={project}|e={environment}|k=unknown"), // a kind we don't emit
            &format!("p=not-a-uuid|e={environment}|k=entries"),
            &format!("p={project}|e=not-a-uuid|k=entries"),
        ] {
            assert_eq!(Hint::decode(payload), None, "{payload}");
        }
    }

    #[test]
    fn decode_tolerates_extra_fields() {
        // A newer writer may add fields; a hint with its core intact still decodes.
        let project = Uuid::now_v7();
        let environment = Uuid::now_v7();
        let hint = Hint::decode(&format!(
            "p={project}|e={environment}|k=entries|c=articles|v=2"
        ));
        assert_eq!(
            hint,
            Some(Hint {
                project,
                environment: Some(environment),
                kind: Kind::Entries,
                collection: Some("articles".to_owned()),
            })
        );
    }
}
