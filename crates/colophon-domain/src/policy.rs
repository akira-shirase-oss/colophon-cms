//! Who may do what in a project (docs/modules/03-auth.md, "The policy function"; D-042).
//!
//! One deny-by-default table from role to action. Every handler that acts in a project asks
//! [`allows`] with the caller's role; nothing else grants anything. Roles are a project's
//! memberships; actions are what the API offers so far.

/// A member's role in a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// Configures the project and its members.
    Owner,
    /// Manages content types (and, later, integrations).
    Developer,
    /// Writes and edits content.
    Editor,
    /// Writes content, reviews changesets and publishes them.
    Publisher,
}

impl Role {
    /// Every role, for listings and tests.
    pub const ALL: [Self; 4] = [Self::Owner, Self::Developer, Self::Editor, Self::Publisher];

    /// The name stored in the database and used in the API.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Developer => "developer",
            Self::Editor => "editor",
            Self::Publisher => "publisher",
        }
    }

    /// The role with this name.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|role| role.as_str() == name)
    }
}

/// Something a member may try to do in a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Read entries, content types and the member list.
    Read,
    /// Create and save entries.
    WriteContent,
    /// Create and change content types.
    ChangeContentTypes,
    /// Invite people, change roles, remove members.
    ManageMembers,
    /// Approve a changeset, or ask for changes to it (D-044).
    Review,
    /// Publish an approved changeset as a release (D-045).
    Publish,
    /// Configure where releases are delivered (D-046).
    ManageDestinations,
}

/// Whether `role` may perform `action`. Deny by default: anything not listed is refused.
pub const fn allows(role: Role, action: Action) -> bool {
    matches!(
        (role, action),
        (_, Action::Read | Action::WriteContent)
            | (
                Role::Owner | Role::Developer,
                Action::ChangeContentTypes | Action::ManageDestinations
            )
            | (Role::Owner, Action::ManageMembers)
            | (
                Role::Owner | Role::Publisher,
                Action::Review | Action::Publish
            )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_as_written_in_module_03() {
        use Action::{
            ChangeContentTypes, ManageDestinations, ManageMembers, Publish, Read, Review,
            WriteContent,
        };
        let table = [
            (Role::Owner, [true, true, true, true, true, true, true]),
            (
                Role::Developer,
                [true, true, true, false, false, false, true],
            ),
            (
                Role::Editor,
                [true, true, false, false, false, false, false],
            ),
            (
                Role::Publisher,
                [true, true, false, false, true, true, false],
            ),
        ];
        for (role, expected) in table {
            let actual = [
                Read,
                WriteContent,
                ChangeContentTypes,
                ManageMembers,
                Review,
                Publish,
                ManageDestinations,
            ]
            .map(|action| allows(role, action));
            assert_eq!(actual, expected, "{role:?}");
        }
    }

    #[test]
    fn role_names_round_trip_and_unknown_names_are_none() {
        for role in Role::ALL {
            assert_eq!(Role::parse(role.as_str()), Some(role));
        }
        assert_eq!(Role::parse("admin"), None);
        assert_eq!(Role::parse("Owner"), None);
    }
}
