//! Who may do what in a project (docs/modules/03-auth.md, "The policy function"; D-042, D-058).
//!
//! A role is data: a project's roles each hold a list of [`Permission`]s, and [`Policy::allows`]
//! is the one deny-by-default evaluator every handler asks. A permission names an [`Action`] and
//! what it applies to (one collection or all of them); field lists and the `own` condition are
//! stored with it for milestone R's later steps and don't narrow anything yet. Every project
//! starts with the four built-in roles of D-042, whose permissions ([`Policy::builtin`]) give
//! exactly the powers that table gave.
//!
//! Since step R.2, owners define more roles. Nobody can hand out power they don't hold
//! ([`Policy::covers`]), and a permission arrives through [`Permission::parse`], which refuses
//! field lists and conditions until step R.3 enforces them.

use crate::project::Slug;

/// The most permissions one role holds: every action on every collection of a large project.
pub const MAX_PERMISSIONS: usize = 500;

/// The most roles a project has, the built-in ones included.
pub const MAX_ROLES: usize = 100;

/// Why a role, or one of its permissions, was refused. Stable codes for the API's details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RoleError {
    /// Not 1 to 63 lowercase letters, digits and hyphens starting with a letter.
    #[error(
        "a role's key is 1 to 63 lowercase letters, digits and hyphens, starting with a letter"
    )]
    InvalidKey,
    /// Blank, too long, or with control characters.
    #[error("a role's name is 1 to 100 characters, without control characters")]
    InvalidName,
    /// Too long, or with control characters.
    #[error("a role's description is at most 500 characters, without control characters")]
    InvalidDescription,
    /// Not one of [`Action::ALL`].
    #[error(
        "an action is one of content.read, content.create, content.update, content.delete, content.review, content.publish, schema.change, members.manage, roles.manage, destinations.manage and tokens.manage"
    )]
    UnknownAction,
    /// Neither `*` nor a collection's slug.
    #[error("a permission applies to `*` or to one collection's slug")]
    InvalidSubject,
    /// A project-wide action named one collection.
    #[error(
        "members, roles, destinations and tokens belong to the whole project, so their permissions apply to `*`"
    )]
    ProjectWide,
    /// More than [`MAX_PERMISSIONS`].
    #[error("a role has at most 500 permissions")]
    TooManyPermissions,
    /// A field list or a condition, which nothing enforces before step R.3.
    #[error(
        "field lists and conditions arrive with step R.3; until they are enforced, they are refused rather than ignored"
    )]
    NotYetEnforced,
}

impl RoleError {
    /// The code clients branch on.
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidKey => "invalid_role_key",
            Self::InvalidName => "invalid_role_name",
            Self::InvalidDescription => "invalid_role_description",
            Self::UnknownAction => "unknown_action",
            Self::InvalidSubject => "invalid_subject",
            Self::ProjectWide => "project_wide_action",
            Self::TooManyPermissions => "too_many_permissions",
            Self::NotYetEnforced => "not_yet_enforced",
        }
    }
}

/// Whether `text` is a role key: `^[a-z][a-z0-9-]{0,62}$`, the rule the database checks.
pub fn is_role_key(text: &str) -> bool {
    let bytes = text.as_bytes();
    (1..=63).contains(&bytes.len())
        && bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

/// A role's name, trimmed: 1 to 100 characters without control characters.
pub fn role_name(text: &str) -> Result<&str, RoleError> {
    let name = text.trim();
    if (1..=100).contains(&name.chars().count()) && !name.chars().any(char::is_control) {
        Ok(name)
    } else {
        Err(RoleError::InvalidName)
    }
}

/// A role's description, trimmed: at most 500 characters without control characters.
pub fn role_description(text: &str) -> Result<&str, RoleError> {
    let description = text.trim();
    if description.chars().count() <= 500 && !description.chars().any(char::is_control) {
        Ok(description)
    } else {
        Err(RoleError::InvalidDescription)
    }
}

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
    /// A permission as a client sends it: an action's name and `*` or a collection's slug.
    /// Project-wide actions apply to `*` only, so a stored permission always means what it says.
    pub fn parse(action: &str, subject: &str) -> Result<Self, RoleError> {
        let action = Action::parse(action).ok_or(RoleError::UnknownAction)?;
        let subject = if subject == "*" {
            Subject::All
        } else {
            Subject::Collection(
                Slug::parse(subject)
                    .map_err(|_| RoleError::InvalidSubject)?
                    .as_str()
                    .to_owned(),
            )
        };
        if !action.per_collection() && subject != Subject::All {
            return Err(RoleError::ProjectWide);
        }
        Ok(Self {
            action,
            subject,
            fields: None,
            condition: None,
        })
    }

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

    /// Whether a holder of this policy may hand out `other`: as a role they create or edit, or
    /// by giving someone a role. Nobody grants what they can't do themselves, so no chain of
    /// grants ends with more power than it started with.
    pub fn covers(&self, other: &Self) -> bool {
        other.permissions.iter().all(|permission| {
            let collection = match &permission.subject {
                Subject::All => None,
                Subject::Collection(slug) => Some(slug.as_str()),
            };
            self.allows(permission.action, collection)
        })
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
    fn nobody_hands_out_power_they_do_not_hold() {
        let owner = Policy::builtin(Role::Owner);
        for role in Role::ALL {
            assert!(
                owner.covers(&Policy::builtin(role)),
                "owner covers {role:?}"
            );
            assert!(
                Policy::builtin(role).covers(&Policy::builtin(role)),
                "{role:?} covers itself"
            );
        }
        let editor = Policy::builtin(Role::Editor);
        assert!(!editor.covers(&Policy::builtin(Role::Publisher)));
        assert!(!editor.covers(&owner));
        assert!(!Policy::builtin(Role::Developer).covers(&Policy::builtin(Role::Publisher)));
        assert!(
            editor.covers(&Policy::default()),
            "an empty role is anyone's to give"
        );

        let pages = |action| Permission {
            subject: Subject::Collection("pages".to_owned()),
            ..Permission::everywhere(action)
        };
        let pages_editor = Policy::new(vec![
            pages(Action::ContentRead),
            pages(Action::ContentUpdate),
        ]);
        assert!(
            editor.covers(&pages_editor),
            "everywhere covers one collection"
        );
        assert!(
            !pages_editor.covers(&editor),
            "one collection doesn't cover everywhere"
        );
        assert!(
            !pages_editor.covers(&Policy::new(vec![Permission {
                subject: Subject::Collection("articles".to_owned()),
                ..Permission::everywhere(Action::ContentRead)
            }])),
            "nor another collection"
        );
    }

    #[test]
    fn permissions_parse_as_they_are_stored_or_not_at_all() {
        assert_eq!(
            Permission::parse("content.update", "pages"),
            Ok(Permission {
                subject: Subject::Collection("pages".to_owned()),
                ..Permission::everywhere(Action::ContentUpdate)
            })
        );
        assert_eq!(
            Permission::parse("members.manage", "*"),
            Ok(Permission::everywhere(Action::MembersManage))
        );
        for (action, subject, error) in [
            ("content.everything", "*", RoleError::UnknownAction),
            ("Content.Read", "*", RoleError::UnknownAction),
            ("content.read", "", RoleError::InvalidSubject),
            ("content.read", "Pages", RoleError::InvalidSubject),
            ("content.read", "-pages", RoleError::InvalidSubject),
            ("content.read", "**", RoleError::InvalidSubject),
            ("members.manage", "pages", RoleError::ProjectWide),
            ("tokens.manage", "articles", RoleError::ProjectWide),
        ] {
            assert_eq!(
                Permission::parse(action, subject),
                Err(error),
                "{action} on {subject:?}"
            );
        }
    }

    #[test]
    fn role_keys_names_and_descriptions() {
        for good in ["editor", "copy-desk", "a", "r2", &"a".repeat(63)] {
            assert!(is_role_key(good), "{good:?}");
        }
        for bad in [
            "",
            "2nd",
            "-desk",
            "Copy",
            "copy desk",
            "copy_desk",
            &"a".repeat(64),
        ] {
            assert!(!is_role_key(bad), "{bad:?}");
        }
        assert_eq!(role_name("  Copy desk "), Ok("Copy desk"));
        assert_eq!(role_name("   "), Err(RoleError::InvalidName));
        assert_eq!(role_name(&"x".repeat(101)), Err(RoleError::InvalidName));
        assert_eq!(role_name("a\u{7}b"), Err(RoleError::InvalidName));
        assert_eq!(role_description(""), Ok(""));
        assert_eq!(
            role_description(&"x".repeat(501)),
            Err(RoleError::InvalidDescription)
        );
        assert_eq!(
            role_description("line\nbreak"),
            Err(RoleError::InvalidDescription)
        );
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
