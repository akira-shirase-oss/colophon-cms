//! Projects: the unit of isolation (docs/modules/01-domain.md). Parse, don't validate: a `Slug`
//! or a `ProjectName` that exists is valid.

use std::fmt;

/// Why a project field was refused. Stable codes for the API's error details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProjectFieldError {
    /// Not 1 to 63 lowercase letters, digits and inner hyphens.
    #[error(
        "a slug is 1 to 63 lowercase letters, digits and hyphens, not starting or ending with a hyphen"
    )]
    InvalidSlug,
    /// Blank, too long, or containing control characters.
    #[error("a name is 1 to 200 characters, without control characters")]
    InvalidName,
}

impl ProjectFieldError {
    /// The code clients branch on.
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidSlug => "invalid_slug",
            Self::InvalidName => "invalid_name",
        }
    }
}

/// A project's unique, URL-safe name: `^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$`, the same rule the
/// database's CHECK constraint enforces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slug(String);

impl Slug {
    /// Parses a slug.
    pub fn parse(text: &str) -> Result<Self, ProjectFieldError> {
        let bytes = text.as_bytes();
        let allowed = |byte: &u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
        let valid = (1..=63).contains(&bytes.len())
            && bytes.first().is_some_and(allowed)
            && bytes.last().is_some_and(allowed)
            && bytes.iter().all(|byte| allowed(byte) || *byte == b'-');
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(ProjectFieldError::InvalidSlug)
        }
    }

    /// The slug.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A project's display name: trimmed, 1 to 200 characters, no control characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectName(String);

impl ProjectName {
    /// Parses a name, trimming surrounding whitespace.
    pub fn parse(text: &str) -> Result<Self, ProjectFieldError> {
        let trimmed = text.trim();
        let length = trimmed.chars().count();
        if (1..=200).contains(&length) && !trimmed.chars().any(char::is_control) {
            Ok(Self(trimmed.to_owned()))
        } else {
            Err(ProjectFieldError::InvalidName)
        }
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        for good in ["a", "blog", "my-site-2", "a1", &"a".repeat(63)] {
            assert!(Slug::parse(good).is_ok(), "{good:?}");
        }
        for bad in [
            "",
            "-blog",
            "blog-",
            "Blog",
            "my_site",
            "my site",
            "é",
            "a/b",
            &"a".repeat(64),
        ] {
            assert_eq!(
                Slug::parse(bad),
                Err(ProjectFieldError::InvalidSlug),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn names() {
        assert_eq!(
            ProjectName::parse("  My blog  ").map(|name| name.0),
            Ok("My blog".to_owned())
        );
        assert!(
            ProjectName::parse(&"é".repeat(200)).is_ok(),
            "counted in characters"
        );
        for bad in ["", "   ", "tab\there", &"x".repeat(201)] {
            assert_eq!(
                ProjectName::parse(bad),
                Err(ProjectFieldError::InvalidName),
                "{bad:?}"
            );
        }
    }
}
