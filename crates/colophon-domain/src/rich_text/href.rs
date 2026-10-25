//! Which links a document may hold (`docs/formats/rich-text-v1.md`, "Marks").

use super::MAX_HREF_CHARS;

/// `https:`, `http:` and `mailto:` URLs, read with the WHATWG URL parser browsers use; paths
/// starting with exactly one `/`; fragments starting with `#`. Control characters, whitespace
/// and backslashes are refused anywhere: browsers strip tabs and newlines and read `\` as `/`,
/// so `/<tab>/evil.example` would lead to another site.
pub fn is_allowed_href(href: &str) -> bool {
    let length = href.chars().count();
    if length == 0 || length > MAX_HREF_CHARS {
        return false;
    }
    if href
        .chars()
        .any(|character| character <= ' ' || character == '\u{7f}' || character == '\\')
    {
        return false;
    }
    if href.starts_with('#') {
        return true;
    }
    if href.starts_with('/') {
        return !href.starts_with("//");
    }
    url::Url::parse(href).is_ok_and(|url| matches!(url.scheme(), "https" | "http" | "mailto"))
}

#[cfg(test)]
mod tests {
    use super::{MAX_HREF_CHARS, is_allowed_href};

    // The same table as packages/editor/test/schema.test.ts: the two must agree.
    #[test]
    fn allowed() {
        let long = format!("https://example.com/{}", "a".repeat(MAX_HREF_CHARS - 20));
        for href in [
            "https://example.com",
            "https://example.com/a?b=c#d",
            "HTTPS://EXAMPLE.COM",
            "http://example.com",
            "https:example.com",
            "mailto:editor@example.com",
            "mailto:",
            "/about",
            "/",
            "/a/b?c=d#e",
            "#links",
            "#",
            "https://例え.jp/パス",
            &long,
        ] {
            assert!(is_allowed_href(href), "{href:?} should be allowed");
        }
    }

    #[test]
    fn refused() {
        let long = format!("https://example.com/{}", "a".repeat(MAX_HREF_CHARS));
        for href in [
            "",
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "data:text/html,x",
            "vbscript:x",
            "file:///etc/passwd",
            "ftp://example.com",
            "//evil.example",
            "/\t/evil.example",
            "/\\evil.example",
            "\\\\evil.example",
            " https://example.com",
            "https://example.com/a b",
            "java\nscript:alert(1)",
            "https://",
            "relative/path",
            "example.com",
            &long,
        ] {
            assert!(!is_allowed_href(href), "{href:?} should be refused");
        }
    }
}
