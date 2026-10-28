//! Who may do what in a project (docs/modules/03-auth.md, "The policy function"; D-042, D-058).
//!
//! A role is data: a project's roles each hold a list of [`Permission`]s, and [`Policy::allows`]
//! is the one deny-by-default evaluator every handler asks. A permission names an [`Action`] and
//! what it applies to (one collection or all of them), and may be narrowed to some fields or to
//! the member's own entries (since step R.3, below). Every project
//! starts with the four built-in roles of D-042, whose permissions ([`Policy::builtin`]) give
//! exactly the powers that table gave.
//!
//! Since step R.2, owners define more roles, and nobody can hand out power they don't hold
//! ([`Policy::covers`]). Since step R.3, a permission can be narrowed to some fields and to the
//! member's own entries ([`Permission::parse_narrowed`], [`Policy::fields`]).

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
    /// A field list on an action that isn't about fields.
    #[error("only content.read, content.create and content.update can be limited to some fields")]
    FieldsNotApplicable,
    /// A field list that is empty, too long, or names something that isn't a field id.
    #[error("a field list names 1 to 100 field ids, like title or body")]
    InvalidFields,
    /// The own condition on an action it can't apply to.
    #[error(
        "only content.read, content.update and content.delete can be limited to the member's own entries"
    )]
    ConditionNotApplicable,
    /// A condition that isn't `own`.
    #[error("the one condition is own: entries the member created")]
    UnknownCondition,
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
            Self::FieldsNotApplicable => "fields_not_applicable",
            Self::InvalidFields => "invalid_fields",
            Self::ConditionNotApplicable => "condition_not_applicable",
            Self::UnknownCondition => "unknown_condition",
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

    /// [`Permission::parse`], narrowed to some fields and to the member's own entries (step R.3).
    /// Field lists apply to reading, creating and updating content; `own` to reading, updating and
    /// deleting it (a member's own entries are the ones they created).
    pub fn parse_narrowed(
        action: &str,
        subject: &str,
        fields: Option<&[String]>,
        condition: Option<&str>,
    ) -> Result<Self, RoleError> {
        let mut permission = Self::parse(action, subject)?;
        if let Some(fields) = fields {
            if !matches!(
                permission.action,
                Action::ContentRead | Action::ContentCreate | Action::ContentUpdate
            ) {
                return Err(RoleError::FieldsNotApplicable);
            }
            let mut list: Vec<String> = fields.to_vec();
            list.sort();
            list.dedup();
            if !(1..=100).contains(&list.len())
                || !list.iter().all(|field| crate::schema::is_field_id(field))
            {
                return Err(RoleError::InvalidFields);
            }
            permission.fields = Some(list);
        }
        if let Some(condition) = condition {
            let condition = Condition::parse(condition).ok_or(RoleError::UnknownCondition)?;
            if !matches!(
                permission.action,
                Action::ContentRead | Action::ContentUpdate | Action::ContentDelete
            ) {
                return Err(RoleError::ConditionNotApplicable);
            }
            permission.condition = Some(condition);
        }
        Ok(permission)
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

/// The fields a permission lets its holder touch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fields {
    /// Every field.
    All,
    /// Only these, by field id.
    Only(std::collections::BTreeSet<String>),
}

impl Fields {
    /// Whether `field` is among them.
    pub fn includes(&self, field: &str) -> bool {
        match self {
            Self::All => true,
            Self::Only(fields) => fields.contains(field),
        }
    }

    /// Both together.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        match (self, other) {
            (Self::Only(mut one), Self::Only(other)) => {
                one.extend(other);
                Self::Only(one)
            }
            _ => Self::All,
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
    /// A policy of exactly these permissions. Permissions for one action, subject and condition
    /// merge into one, with the union of their fields (or every field, if any allows all): a role
    /// holds at most one such permission, as the database stores it (migration 0021).
    pub fn new(permissions: Vec<Permission>) -> Self {
        let mut merged: Vec<Permission> = Vec::with_capacity(permissions.len());
        for permission in permissions {
            match merged.iter_mut().find(|held| {
                held.action == permission.action
                    && held.subject == permission.subject
                    && held.condition == permission.condition
            }) {
                None => merged.push(permission),
                Some(held) => {
                    held.fields = match (held.fields.take(), permission.fields) {
                        (Some(mut one), Some(other)) => {
                            one.extend(other);
                            one.sort();
                            one.dedup();
                            Some(one)
                        }
                        _ => None,
                    };
                }
            }
        }
        merged.sort();
        Self {
            permissions: merged,
        }
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
            ContentCreate, ContentDelete, ContentPublish, ContentRead, ContentReview,
            ContentUpdate, DestinationsManage, MembersManage, RolesManage, SchemaChange,
            TokensManage,
        };
        let mut actions = vec![ContentRead, ContentCreate, ContentUpdate];
        if matches!(role, Role::Owner | Role::Developer) {
            actions.extend([SchemaChange, DestinationsManage]);
        }
        if role == Role::Owner {
            // Every permission (D-058), deleting included: nobody could grant it otherwise.
            actions.extend([MembersManage, RolesManage, TokensManage, ContentDelete]);
        }
        if matches!(role, Role::Owner | Role::Publisher) {
            actions.extend([ContentReview, ContentPublish]);
        }
        Self::new(actions.into_iter().map(Permission::everywhere).collect())
    }

    /// Whether a holder of this policy may hand out `other`: as a role they create or edit, or
    /// by giving someone a role. Nobody grants what they can't do themselves, so no chain of
    /// grants ends with more power than it started with: each of `other`'s permissions must be
    /// within this policy's, on its subject, for its fields, with its condition (step R.3).
    pub fn covers(&self, other: &Self) -> bool {
        other.permissions.iter().all(|permission| {
            let own = permission.condition == Some(Condition::Own);
            match self.fields_on(permission.action, &permission.subject, own) {
                None => false,
                Some(mine) => match &permission.fields {
                    None => mine == Fields::All,
                    Some(asked) => asked.iter().all(|field| mine.includes(field)),
                },
            }
        })
    }

    /// Which fields this policy lets its holder `action` on an entry of `collection`, `own` when
    /// they created it; `None` when it allows nothing there (step R.3). Every permission that
    /// applies (its action, its subject everything or this collection, its condition met) adds
    /// its fields, and one without a field list allows them all. So "titles of any article" with
    /// "every field of my own articles" allows every field of one's own and titles of the rest.
    pub fn fields(&self, action: Action, collection: &str, own: bool) -> Option<Fields> {
        self.fields_on(action, &Subject::Collection(collection.to_owned()), own)
    }

    fn fields_on(&self, action: Action, subject: &Subject, own: bool) -> Option<Fields> {
        self.permissions
            .iter()
            .filter(|permission| {
                permission.action == action
                    && match (&permission.subject, subject) {
                        (Subject::All, _) => true,
                        (Subject::Collection(granted), Subject::Collection(asked)) => {
                            action.per_collection() && granted == asked
                        }
                        (Subject::Collection(_), Subject::All) => false,
                    }
                    && match permission.condition {
                        None => true,
                        Some(Condition::Own) => own,
                    }
            })
            .map(|permission| match &permission.fields {
                None => Fields::All,
                Some(list) => Fields::Only(list.iter().cloned().collect()),
            })
            .reduce(Fields::union)
    }

    /// Whether this policy lets its holder perform `action` in `collection` (`None` for
    /// project-wide actions, and for a check that must hold in every collection) on every field
    /// of every entry. Deny by default: only a permission for the action on [`Subject::All`], or
    /// on that very collection, without a field list or a condition, allows it; narrower ones
    /// count only where [`Policy::fields`] asks about them.
    pub fn allows(&self, action: Action, collection: Option<&str>) -> bool {
        let subject = collection.map_or(Subject::All, |slug| Subject::Collection(slug.to_owned()));
        self.fields_on(action, &subject, false) == Some(Fields::All)
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
        // New actions: only owners manage roles and tokens, and only owners delete (no route
        // deletes yet), and the owner role holds every action there is.
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
            assert_eq!(
                policy.allows(Action::ContentDelete, Some("articles")),
                role == Role::Owner
            );
        }
    }

    #[test]
    fn the_owner_role_holds_every_action() {
        let owner = Policy::builtin(Role::Owner);
        for action in Action::ALL {
            assert!(owner.allows(action, None), "{action:?}");
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

    fn only(fields: &[&str]) -> Fields {
        Fields::Only(fields.iter().map(|field| (*field).to_owned()).collect())
    }

    fn narrowed(
        action: Action,
        collection: &str,
        fields: Option<&[&str]>,
        own: bool,
    ) -> Permission {
        Permission {
            action,
            subject: Subject::Collection(collection.to_owned()),
            fields: fields.map(|list| list.iter().map(|field| (*field).to_owned()).collect()),
            condition: own.then_some(Condition::Own),
        }
    }

    #[test]
    fn fields_and_ownership_narrow_what_a_permission_allows() {
        // Titles of any article, and every field of one's own articles.
        let policy = Policy::new(vec![
            narrowed(Action::ContentUpdate, "articles", Some(&["title"]), false),
            narrowed(Action::ContentUpdate, "articles", None, true),
            narrowed(Action::ContentRead, "articles", None, true),
        ]);
        assert_eq!(
            policy.fields(Action::ContentUpdate, "articles", false),
            Some(only(&["title"]))
        );
        assert_eq!(
            policy.fields(Action::ContentUpdate, "articles", true),
            Some(Fields::All)
        );
        assert_eq!(policy.fields(Action::ContentUpdate, "pages", true), None);
        assert_eq!(policy.fields(Action::ContentRead, "articles", false), None);
        assert_eq!(
            policy.fields(Action::ContentRead, "articles", true),
            Some(Fields::All)
        );
        // Narrow permissions never grant the collection-wide power the coarse check asks for.
        assert!(!policy.allows(Action::ContentUpdate, Some("articles")));
        assert!(!policy.allows(Action::ContentRead, Some("articles")));
        // Field lists add up.
        let two = Policy::new(vec![
            narrowed(Action::ContentRead, "articles", Some(&["title"]), false),
            narrowed(Action::ContentRead, "articles", Some(&["body"]), false),
        ]);
        assert_eq!(
            two.fields(Action::ContentRead, "articles", false),
            Some(only(&["body", "title"]))
        );
        assert_eq!(only(&["a"]).union(Fields::All), Fields::All);
        assert!(only(&["a"]).includes("a") && !only(&["a"]).includes("b"));
        // The built-in roles allow every field of every entry.
        assert_eq!(
            Policy::builtin(Role::Editor).fields(Action::ContentUpdate, "articles", false),
            Some(Fields::All)
        );
    }

    #[test]
    fn nobody_hands_out_more_fields_or_entries_than_they_hold() {
        let own_articles = Policy::new(vec![narrowed(
            Action::ContentUpdate,
            "articles",
            None,
            true,
        )]);
        let any_article = Policy::new(vec![narrowed(
            Action::ContentUpdate,
            "articles",
            None,
            false,
        )]);
        let titles = Policy::new(vec![narrowed(
            Action::ContentUpdate,
            "articles",
            Some(&["title"]),
            false,
        )]);
        let titles_and_slugs = Policy::new(vec![narrowed(
            Action::ContentUpdate,
            "articles",
            Some(&["title", "slug"]),
            false,
        )]);
        assert!(
            any_article.covers(&own_articles),
            "everyone's covers one's own"
        );
        assert!(
            !own_articles.covers(&any_article),
            "one's own doesn't cover everyone's"
        );
        assert!(own_articles.covers(&own_articles));
        assert!(any_article.covers(&titles), "every field covers some");
        assert!(titles_and_slugs.covers(&titles));
        assert!(!titles.covers(&titles_and_slugs), "nor fewer fields more");
        assert!(!titles.covers(&any_article));
        assert!(Policy::builtin(Role::Editor).covers(&titles_and_slugs));
        assert!(
            !own_articles.covers(&titles),
            "own entries don't cover anyone's titles"
        );
    }

    #[test]
    fn narrowed_permissions_parse_only_where_they_apply_and_merge() {
        let titles = ["title".to_owned()];
        assert_eq!(
            Permission::parse_narrowed("content.update", "articles", Some(&titles), None),
            Ok(narrowed(
                Action::ContentUpdate,
                "articles",
                Some(&["title"]),
                false
            ))
        );
        assert_eq!(
            Permission::parse_narrowed("content.update", "*", None, Some("own"))
                .map(|p| p.condition),
            Ok(Some(Condition::Own))
        );
        for (action, fields, condition, error) in [
            (
                "content.publish",
                Some(&titles[..]),
                None,
                RoleError::FieldsNotApplicable,
            ),
            (
                "content.read",
                Some(&[][..]),
                None,
                RoleError::InvalidFields,
            ),
            (
                "content.read",
                Some(&["Title".to_owned()][..]),
                None,
                RoleError::InvalidFields,
            ),
            (
                "content.publish",
                None,
                Some("own"),
                RoleError::ConditionNotApplicable,
            ),
            (
                "content.create",
                None,
                Some("own"),
                RoleError::ConditionNotApplicable,
            ),
            (
                "content.read",
                None,
                Some("mine"),
                RoleError::UnknownCondition,
            ),
        ] {
            assert_eq!(
                Permission::parse_narrowed(action, "articles", fields, condition),
                Err(error),
                "{action} {fields:?} {condition:?}"
            );
        }
        // One action, subject and condition: one permission, with the union of fields.
        let merged = Policy::new(vec![
            narrowed(Action::ContentRead, "articles", Some(&["title"]), false),
            narrowed(
                Action::ContentRead,
                "articles",
                Some(&["body", "title"]),
                false,
            ),
            narrowed(Action::ContentRead, "articles", None, true),
        ]);
        assert_eq!(merged.permissions().len(), 2);
        assert_eq!(
            merged.fields(Action::ContentRead, "articles", false),
            Some(only(&["body", "title"]))
        );
        let widened = Policy::new(vec![
            narrowed(Action::ContentRead, "articles", Some(&["title"]), false),
            narrowed(Action::ContentRead, "articles", None, false),
        ]);
        assert_eq!(widened.permissions().len(), 1);
        assert!(widened.allows(Action::ContentRead, Some("articles")));
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
