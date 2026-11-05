//! The audience a published entry is for (docs/modules/06-delivery.md, "Audience visibility";
//! D-137): who may read it, as the revision that publishes it carries. `public` is anyone,
//! `authenticated` is any live site session, and `role:<name>` is a session whose site user
//! holds that audience role — the same strings the database's visibility columns and the
//! `colophon_delivery` policy spell, validated here once so the API's answers and the stored
//! values can't spell them differently.
//!
//! Audience roles are labels, not policies: they say nothing about staff permissions and are
//! kept in `site_roles`, created by a member holding `audience.manage`. Entitlements (paid
//! tiers, expirations) are a later phase.

use crate::policy::is_role_key;

/// Who may read a published entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Visibility {
    /// Anyone: the anonymous delivery read.
    Public,
    /// Any live site session, whatever role its user holds.
    Authenticated,
    /// A live site session whose user holds this audience role.
    Role(String),
}

/// The longest stored visibility value: `role:` plus the longest role key.
pub const MAX_LEN: usize = 5 + 63;

impl Visibility {
    /// The value as it is stored and as `?visibility=` takes it: `public`, `authenticated`
    /// or `role:<name>`.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "public" => Some(Self::Public),
            "authenticated" => Some(Self::Authenticated),
            _ => text
                .strip_prefix("role:")
                .filter(|name| is_role_key(name))
                .map(|name| Self::Role(name.to_owned())),
        }
    }

    /// The stored string (`role:member` included): [`Self::parse`] of it is the same value
    /// again.
    pub fn value(&self) -> String {
        match self {
            Self::Public => "public".to_owned(),
            Self::Authenticated => "authenticated".to_owned(),
            Self::Role(name) => format!("role:{name}"),
        }
    }

    /// The audience role it names, when it names one.
    pub fn role(&self) -> Option<&str> {
        match self {
            Self::Role(name) => Some(name),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_kinds_parse_and_spell_themselves() {
        assert_eq!(Visibility::parse("public"), Some(Visibility::Public));
        assert_eq!(
            Visibility::parse("authenticated"),
            Some(Visibility::Authenticated)
        );
        let member = Visibility::parse("role:member").expect("a role gate");
        assert_eq!(member, Visibility::Role("member".to_owned()));
        assert_eq!(member.value(), "role:member");
        assert_eq!(member.role(), Some("member"));
        assert_eq!(Visibility::Public.value(), "public");
        assert_eq!(Visibility::Authenticated.value(), "authenticated");
    }

    #[test]
    fn anything_else_is_refused() {
        for bad in [
            "",
            "PUBLIC",
            "members",
            "role:",
            "role:Member",
            "role:role:member",
            "role:-member",
            "role:mémber",
            "role:member ",
            " role:member",
            "private",
            "admin",
        ] {
            assert_eq!(Visibility::parse(bad), None, "{bad:?}");
        }
        let long = format!("role:{}", "a".repeat(64));
        assert_eq!(Visibility::parse(&long), None, "a role key is 63 at most");
        let fine = format!("role:{}", "a".repeat(63));
        assert!(Visibility::parse(&fine).is_some());
    }
}
