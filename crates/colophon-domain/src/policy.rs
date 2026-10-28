//! Who may do what in a project (docs/modules/03-auth.md, "The policy function"; D-042, D-058).
//!
//! A role is data: a project's roles each hold a list of [`Permission`]s, and [`Policy::allows`]
//! is the one deny-by-default evaluator every handler asks. A permission names an [`Action`] and
//! what it applies to (one collection or all of them); field lists and the `own` condition are
//! stored with it for milestone R's later steps and don't narrow anything yet. Every project
//! starts with the four built-in roles of D-042, whose permissions ([`Policy::builtin`]) give
//! exactly the powers that table gave.

/// A built-in role, seeded in every project (D-042).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// Configures the project, its members and its roles.
    Owner,
    /// Manages content types and integrations.
    Developer,
    /// Writes and edits content.
    Editor,
    /// Writes content, reviews changesets and publishes them.
    Publisher,
}

impl Role {
    /// Every built-in role, for listings and tests.
    pub const ALL: [Self; 4] = [Self::Owner, Self::Developer, Self::Editor, Self::Publisher];

    /// The role's key, as memberships and invitations store it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Developer => "developer",
            Self::Editor => "editor",
            Self::Publisher => "publisher",
        }
    }

    /// The built-in role with this key.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|role| role.as_str() == name)
    }
}

/// Something a member may try to do in a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    /// Read entries and their versions.
    ContentRead,
    /// Create entries.
    ContentCreate,
    /// Save new versions of entries, and ask for them to be published.
    ContentUpdate,
    /// Delete entries (no route offers it yet).
    ContentDelete,
    /// Approve a changeset, or ask for changes to it (D-044).
    ContentReview,
    /// Publish an approved changeset as a release (D-045).
    ContentPublish,
    /// Create and change content types.
    SchemaChange,
    /// Invite people, change their roles, remove them.
    MembersManage,
    /// Create, change and delete roles.
    RolesManage,
    /// Configure where releases are delivered (D-046).
    DestinationsManage,
    /// Create and revoke API tokens.
    TokensManage,
}

impl Action {
    /// Every action, in the order the Studio lists them.
    pub const ALL: [Self; 11] = [
        Self::ContentRead,
        Self::ContentCreate,
        Self::ContentUpdate,
        Self::ContentDelete,
        Self::ContentReview,
        Self::ContentPublish,
        Self::SchemaChange,
        Self::MembersManage,
        Self::RolesManage,
        Self::DestinationsManage,
        Self::TokensManage,
    ];

    /// The name stored with permissions and used in the API.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ContentRead => "content.read",
            Self::ContentCreate => "content.create",
            Self::ContentUpdate => "content.update",
            Self::ContentDelete => "content.delete",
            Self::ContentReview => "content.review",
            Self::ContentPublish => "content.publish",
            Self::SchemaChange => "schema.change",
            Self::MembersManage => "members.manage",
            Self::RolesManage => "roles.manage",
            Self::DestinationsManage => "destinations.manage",
            Self::TokensManage => "tokens.manage",
        }
    }

    /// The action with this name.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.as_str() == name)
    }

    /// Whether the action is about a collection's content or content type, so a permission for
    /// it may name one collection. The others are about the project as a whole.
    pub const fn per_collection(self) -> bool {
        matches!(
            self,
            Self::ContentRead
                | Self::ContentCreate
                | Self::ContentUpdate
                | Self::ContentDelete
                | Self::ContentReview
                | Self::ContentPublish
                | Self::SchemaChange
        )
    }
}

/// What a permission applies to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Subject {
    /// Every collection, and the project itself.
    All,
    /// One collection, by its slug.
    Collection(String),
}

impl Subject {
    /// `*`, or the collection's slug, as stored.
    pub fn as_str(&self) -> &str {
        match self {
            Self::All => "*",
            Self::Collection(slug) => slug,
        }
    }

    /// The subject stored as `text`.
    pub fn parse(text: &str) -> Self {
        if text == "*" {
            Self::All
        } else {
            Self::Collection(text.to_owned())
        }
    }
}

/// A condition on a permission (D-058; enforced from step R.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Condition {
    /// Only entries the caller created.
    Own,
}

impl Condition {
    /// The name stored with permissions.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Own => "own",
        }
    }

    /// The condition with this name.
    pub fn parse(name: &str) -> Option<Self> {
        (name == "own").then_some(Self::Own)
    }
}

/// One thing a role may do.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Permission {
    /// What.
    pub action: Action,
    /// Where.
    pub subject: Subject,
    /// Only these fields, for reads and writes (enforced from step R.3); `None` is every field.
    pub fields: Option<Vec<String>>,
    /// Only where the condition holds (enforced from step R.3).
    pub condition: Option<Condition>,
}

impl Permission {
    /// `action` everywhere, on every field, without a condition.
    pub const fn everywhere(action: Action) -> Self {
        Self {
            action,
            subject: Subject::All,
            fields: None,
            condition: None,
        }
    }
}

/// A role's permissions, as the evaluator reads them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policy {
    permissions: Vec<Permission>,
}

impl Policy {
    /// A policy of exactly these permissions.
    pub fn new(mut permissions: Vec<Permission>) -> Self {
        permissions.sort();
        permissions.dedup();
        Self { permissions }
    }

    /// The permissions, sorted.
    pub fn permissions(&self) -> &[Permission] {
        &self.permissions
    }

    /// The permissions of a built-in role: D-042's table, unchanged.
    ///
    /// | | read, create, update | schema, destinations | members, roles, tokens | review, publish |
    /// |---|---|---|---|---|
    /// | owner | ✓ | ✓ | ✓ | ✓ |
    /// | developer | ✓ | ✓ | | |
    /// | editor | ✓ | | | |
    /// | publisher | ✓ | | | ✓ |
    pub fn builtin(role: Role) -> Self {
        use Action::{
            ContentCreate, ContentPublish, ContentRead, ContentReview, ContentUpdate,
            DestinationsManage, MembersManage, RolesManage, SchemaChange, TokensManage,
        };
        let mut actions = vec![ContentRead, ContentCreate, ContentUpdate];
        if matches!(role, Role::Owner | Role::Developer) {
            actions.extend([SchemaChange, DestinationsManage]);
        }
        if role == Role::Owner {
            actions.extend([MembersManage, RolesManage, TokensManage]);
        }
        if matches!(role, Role::Owner | Role::Publisher) {
            actions.extend([ContentReview, ContentPublish]);
        }
        Self::new(actions.into_iter().map(Permission::everywhere).collect())
    }

    /// Whether this policy lets its holder perform `action`, in `collection` when the action is
    /// about one (`None` for project-wide actions, and for a collection-wide check that must
    /// hold everywhere). Deny by default: only a permission for the action on [`Subject::All`],
    /// or on that very collection, allows it.
    pub fn allows(&self, action: Action, collection: Option<&str>) -> bool {
        self.permissions.iter().any(|permission| {
            permission.action == action
                && match (&permission.subject, collection) {
                    (Subject::All, _) => true,
                    (Subject::Collection(slug), Some(asked)) => {
                        action.per_collection() && slug == asked
                    }
                    (Subject::Collection(_), None) => false,
                }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D-042's table, as module 03 writes it: read, write content, change content types, manage
    /// members, review, publish, manage destinations.
    #[test]
    fn the_built_in_roles_keep_d_042_s_powers() {
        use Action::{
            ContentCreate, ContentPublish, ContentRead, ContentReview, ContentUpdate,
            DestinationsManage, MembersManage, SchemaChange,
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
            let policy = Policy::builtin(role);
            for collection in [None, Some("articles"), Some("pages")] {
                let write = policy.allows(ContentCreate, collection)
                    && policy.allows(ContentUpdate, collection);
                let actual = [
                    policy.allows(ContentRead, collection),
                    write,
                    policy.allows(SchemaChange, collection),
                    policy.allows(MembersManage, None),
                    policy.allows(ContentReview, collection),
                    policy.allows(ContentPublish, collection),
                    policy.allows(DestinationsManage, None),
                ];
                assert_eq!(actual, expected, "{role:?} in {collection:?}");
            }
        }
        // New actions: only owners manage roles and tokens; nobody deletes yet.
        for role in Role::ALL {
            let policy = Policy::builtin(role);
            assert_eq!(
                policy.allows(Action::RolesManage, None),
                role == Role::Owner
            );
            assert_eq!(
                policy.allows(Action::TokensManage, None),
                role == Role::Owner
            );
            assert!(!policy.allows(Action::ContentDelete, Some("articles")));
        }
    }

    #[test]
    fn a_collection_permission_allows_that_collection_only() {
        let policy = Policy::new(vec![
            Permission {
                subject: Subject::Collection("pages".to_owned()),
                ..Permission::everywhere(Action::ContentUpdate)
            },
            Permission {
                subject: Subject::Collection("pages".to_owned()),
                ..Permission::everywhere(Action::MembersManage)
            },
        ]);
        assert!(policy.allows(Action::ContentUpdate, Some("pages")));
        assert!(!policy.allows(Action::ContentUpdate, Some("articles")));
        assert!(
            !policy.allows(Action::ContentUpdate, None),
            "a check that must hold everywhere"
        );
        assert!(
            !policy.allows(Action::ContentRead, Some("pages")),
            "another action"
        );
        // A project-wide action can't be granted for one collection.
        assert!(!policy.allows(Action::MembersManage, Some("pages")));
        assert!(!policy.allows(Action::MembersManage, None));
        assert!(!Policy::default().allows(Action::ContentRead, Some("pages")));
    }

    #[test]
    fn names_round_trip_and_unknown_names_are_none() {
        for role in Role::ALL {
            assert_eq!(Role::parse(role.as_str()), Some(role));
        }
        assert_eq!(Role::parse("admin"), None);
        assert_eq!(Role::parse("Owner"), None);
        for action in Action::ALL {
            assert_eq!(Action::parse(action.as_str()), Some(action));
        }
        assert_eq!(Action::parse("content.everything"), None);
        assert_eq!(Condition::parse("own"), Some(Condition::Own));
        assert_eq!(Condition::parse("mine"), None);
        assert_eq!(Subject::parse("*"), Subject::All);
        assert_eq!(
            Subject::parse("pages"),
            Subject::Collection("pages".to_owned())
        );
    }
}
