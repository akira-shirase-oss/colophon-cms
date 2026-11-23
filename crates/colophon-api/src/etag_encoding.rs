//! Entity tags that came back through a compressing proxy (D-243).
//!
//! Caddy's `encode` (the droplet's front door) compresses a response and, to keep the tag honest
//! about the bytes it now describes, appends the coding to a strong `ETag`: `"abc"` leaves the
//! proxy as `"abc-gzip"` (or `-zstd`, `-br`). A browser then sends that suffixed tag back in
//! `If-Match` / `If-None-Match`, and every handler that compares it with the revision it names
//! refuses it, so no draft could be saved through the proxy. The tag still names the same
//! revision: only its encoding differs. This layer strips those suffixes from the request's
//! conditional headers before any handler reads them. Colophon's own tags are ids and hashes and
//! never end in a coding's name.

use axum::{
    extract::Request, http::HeaderValue, http::header, middleware::Next, response::Response,
};

const CODINGS: [&str; 3] = ["-gzip", "-zstd", "-br"];

/// `"abc-gzip"` → `"abc"`; anything else, including `*`, unchanged. Weak tags keep their `W/`.
fn strip_one(tag: &str) -> String {
    let tag = tag.trim();
    let (weak, quoted) = tag
        .strip_prefix("W/")
        .map_or(("", tag), |rest| ("W/", rest));
    let Some(inner) = quoted.strip_prefix('"').and_then(|t| t.strip_suffix('"')) else {
        return tag.to_owned();
    };
    let inner = CODINGS
        .iter()
        .find_map(|coding| inner.strip_suffix(coding))
        .unwrap_or(inner);
    format!("{weak}\"{inner}\"")
}

/// A whole header value: a comma-separated list of tags.
fn strip_list(value: &str) -> String {
    value
        .split(',')
        .map(strip_one)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Middleware: rewrite `If-Match` and `If-None-Match` without a proxy's coding suffix.
pub(crate) async fn strip(mut request: Request, next: Next) -> Response {
    for name in [header::IF_MATCH, header::IF_NONE_MATCH] {
        let rewritten = request
            .headers()
            .get(&name)
            .and_then(|v| v.to_str().ok())
            .filter(|v| CODINGS.iter().any(|c| v.contains(&format!("{c}\""))))
            .map(strip_list)
            .and_then(|v| HeaderValue::from_str(&v).ok());
        if let Some(value) = rewritten {
            request.headers_mut().insert(name, value);
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::{strip_list, strip_one};

    #[test]
    fn a_proxys_coding_suffix_is_removed_and_nothing_else_changes() {
        assert_eq!(
            strip_one("\"01a20939-e5aa-72c5-8066-4dbba5d2a3bd-gzip\""),
            "\"01a20939-e5aa-72c5-8066-4dbba5d2a3bd\""
        );
        assert_eq!(strip_one("\"sha256:abc-zstd\""), "\"sha256:abc\"");
        assert_eq!(strip_one("\"v3-br\""), "\"v3\"");
        assert_eq!(strip_one("W/\"abc-gzip\""), "W/\"abc\"");
        assert_eq!(
            strip_one("\"01a20939-e5aa\""),
            "\"01a20939-e5aa\"",
            "no suffix"
        );
        assert_eq!(strip_one("*"), "*");
        assert_eq!(
            strip_one("unquoted-gzip"),
            "unquoted-gzip",
            "not a tag: left for the handler to refuse"
        );
        assert_eq!(strip_list("\"a-gzip\", \"b\""), "\"a\", \"b\"");
    }
}
