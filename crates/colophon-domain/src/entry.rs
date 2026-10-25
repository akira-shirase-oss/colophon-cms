//! Entries and their revisions' content (docs/modules/01-domain.md, "Entries and revisions").
//!
//! Until content types can be defined (D-034), every entry belongs to one built-in collection,
//! [`ARTICLES`], and every revision holds an [`Article`]: a title and a rich-text body. Parse,
//! don't validate: an `Article` that exists is valid, and its [`Article::canonical_json`] is the
//! exact text stored and hashed.

use serde_json::Value;

use crate::rich_text::{self, Rejection, RichText, json::write_string, pointer::Pointer};

/// The one collection that exists before content types do.
pub const ARTICLES: &str = "articles";

/// The longest title, in characters.
pub const MAX_TITLE_CHARS: usize = 200;

/// The longest unknown field name repeated back in an error.
const MAX_REPORTED_FIELD_CHARS: usize = 64;

/// Why an article was refused. Each has a stable code and a JSON Pointer to the problem.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArticleError {
    /// The value isn't a JSON object.
    #[error("an article is a JSON object with `title` and `body`")]
    NotAnObject,
    /// A required field is missing.
    #[error("`{0}` is required")]
    Missing(&'static str),
    /// A field the collection doesn't have. Never dropped silently (D11).
    #[error("`{0}` is not a field of an article")]
    Unknown(String),
    /// The title isn't a string of 1 to 200 characters without control characters or
    /// surrounding whitespace. It is refused rather than trimmed, so what is stored is exactly
    /// what was sent.
    #[error("a title is 1 to 200 characters, without control characters or spaces at either end")]
    InvalidTitle,
    /// The body isn't a valid `colophon/rich-text` v1 document.
    #[error("{0}")]
    Body(Rejection),
}

impl ArticleError {
    /// The code clients branch on.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotAnObject => "not_an_object",
            Self::Missing(_) => "missing_field",
            Self::Unknown(_) => "unknown_field",
            Self::InvalidTitle => "invalid_title",
            Self::Body(rejection) => rejection.code.as_str(),
        }
    }

    /// RFC 6901 JSON Pointer to the problem, from the article's root.
    pub fn pointer(&self) -> String {
        let mut pointer = Pointer::new();
        match self {
            Self::NotAnObject => {}
            Self::Missing(field) => pointer.push(field),
            Self::Unknown(field) => pointer.push(field),
            Self::InvalidTitle => pointer.push("title"),
            Self::Body(rejection) => return format!("/body{}", rejection.pointer),
        }
        pointer.as_str().to_owned()
    }
}

/// A valid article: the content of one revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Article {
    title: String,
    body: RichText,
}

impl Article {
    /// Parses `{"title": "...", "body": <colophon/rich-text v1 document>}`. Every other field is
    /// refused.
    pub fn parse(value: &Value) -> Result<Self, ArticleError> {
        let object = value.as_object().ok_or(ArticleError::NotAnObject)?;
        if let Some(unknown) = object
            .keys()
            .find(|key| !matches!(key.as_str(), "title" | "body"))
        {
            return Err(ArticleError::Unknown(
                unknown.chars().take(MAX_REPORTED_FIELD_CHARS).collect(),
            ));
        }
        let title = object.get("title").ok_or(ArticleError::Missing("title"))?;
        let body = object.get("body").ok_or(ArticleError::Missing("body"))?;
        let title = title.as_str().ok_or(ArticleError::InvalidTitle)?;
        let length = title.chars().count();
        if !(1..=MAX_TITLE_CHARS).contains(&length)
            || title.chars().any(char::is_control)
            || title.trim() != title
        {
            return Err(ArticleError::InvalidTitle);
        }
        let body = rich_text::validate(body).map_err(ArticleError::Body)?;
        Ok(Self {
            title: title.to_owned(),
            body,
        })
    }

    /// The title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The body, in canonical form.
    pub fn body(&self) -> &RichText {
        &self.body
    }

    /// `{"title":...,"body":...}` with no whitespace, the title escaped as `JSON.stringify`
    /// escapes it: the bytes stored and hashed, which the Studio can reproduce exactly.
    pub fn canonical_json(&self) -> String {
        let body = self.body.canonical_json();
        let mut out = String::with_capacity(self.title.len() + body.len() + 24);
        out.push_str(r#"{"title":"#);
        write_string(&mut out, &self.title);
        out.push_str(r#","body":"#);
        out.push_str(body);
        out.push('}');
        out
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn body() -> Value {
        json!({"schema": "colophon/rich-text", "version": 1, "doc": {"type": "doc", "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": "Hello"}]}
        ]}})
    }

    fn article(title: &Value) -> Result<Article, ArticleError> {
        Article::parse(&json!({"title": title, "body": body()}))
    }

    #[test]
    fn a_valid_article_has_one_canonical_form() {
        let parsed = article(&json!("A \"quoted\" title, é")).expect("valid");
        assert_eq!(parsed.title(), "A \"quoted\" title, é");
        let canonical = parsed.canonical_json();
        assert!(
            canonical.starts_with(r#"{"title":"A \"quoted\" title, é","body":{"#),
            "{canonical}"
        );
        let reparsed: Value = serde_json::from_str(&canonical).expect("canonical JSON parses");
        assert_eq!(
            Article::parse(&reparsed).expect("valid").canonical_json(),
            canonical,
            "parsing the canonical form gives it back unchanged"
        );
    }

    #[test]
    fn titles_are_checked_not_trimmed() {
        assert!(
            article(&json!("é".repeat(MAX_TITLE_CHARS))).is_ok(),
            "counted in characters"
        );
        for bad in [
            json!(""),
            json!(" leading"),
            json!("trailing "),
            json!("tab\tinside"),
            json!("x".repeat(MAX_TITLE_CHARS + 1)),
            json!(42),
            json!(null),
        ] {
            let error = article(&bad).expect_err("refused");
            assert_eq!(
                (error.code(), error.pointer()),
                ("invalid_title", "/title".to_owned()),
                "{bad}"
            );
        }
    }

    #[test]
    fn missing_unknown_and_malformed_fields_are_refused_with_a_pointer() {
        let cases = [
            (json!([]), "not_an_object", ""),
            (json!({"body": body()}), "missing_field", "/title"),
            (json!({"title": "T"}), "missing_field", "/body"),
            (
                json!({"title": "T", "body": body(), "slug": "t"}),
                "unknown_field",
                "/slug",
            ),
            (
                json!({"title": "T", "body": body(), "a/b~c": 1}),
                "unknown_field",
                "/a~1b~0c",
            ),
        ];
        for (value, code, pointer) in cases {
            let error = Article::parse(&value).expect_err("refused");
            assert_eq!(
                (error.code(), error.pointer().as_str()),
                (code, pointer),
                "{value}"
            );
        }
    }

    #[test]
    fn a_bad_body_is_reported_inside_the_body() {
        let mut bad = body();
        bad["doc"]["content"][0]["type"] = json!("script");
        let error = Article::parse(&json!({"title": "T", "body": bad})).expect_err("refused");
        assert!(
            error.pointer().starts_with("/body/doc/content/0"),
            "{}",
            error.pointer()
        );
    }

    #[test]
    fn long_unknown_field_names_are_cut_short_in_errors() {
        let long = "k".repeat(1_000);
        let error = Article::parse(&json!({"title": "T", "body": body(), long.clone(): 1}))
            .expect_err("refused");
        assert_eq!(error.pointer().len(), 1 + MAX_REPORTED_FIELD_CHARS);
    }
}
