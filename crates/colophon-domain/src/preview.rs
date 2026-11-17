//! Draft preview on the editor's own site (D-170, D-171): the rules that need no I/O — the
//! site address a preview opens, how long a grant lives, and what a preview shows.
//!
//! **The address.** A project sets one template per environment, such as
//! `https://www.example.com/api/preview?code={code}&path=/blog/{slug}`. It must be `https`
//! (plain `http` only for `localhost` and loopback on a development server, D-149's switch),
//! at most 2,048 characters, hold `{code}`, and use no placeholder but `{code}`,
//! `{collection}`, `{id}`, `{slug}` and `{locale}` — and none in the scheme, host or port, so
//! the address's origin is fixed: the Studio posts messages to exactly that origin.
//!
//! **The overlay.** A preview reads the latest release with its scope laid over it:
//! every published entry the scope doesn't name, plus each scope row that shows a revision. A
//! scope row without one (a publish request's unpublish item) takes the entry out. [`overlay`]
//! is that rule as a pure function, the model the stack tests check the SQL against.

use std::collections::BTreeMap;

/// The longest template accepted.
pub const MAX_TEMPLATE: usize = 2048;

/// The placeholders a template may use; `{code}` is required.
pub const PLACEHOLDERS: [&str; 5] = ["code", "collection", "id", "slug", "locale"];

/// A grant's default time to live, in seconds (15 minutes).
pub const DEFAULT_TTL: i32 = 900;
/// The shortest and longest time to live a grant may ask for, in seconds.
pub const TTL_RANGE: std::ops::RangeInclusive<i32> = 60..=3600;

/// Why a template was refused: a stable code and a message safe to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct TemplateError {
    /// Always `invalid_preview_url`.
    pub reason: &'static str,
    /// What is wrong, for people.
    pub message: &'static str,
}

const fn refuse(message: &'static str) -> TemplateError {
    TemplateError {
        reason: "invalid_preview_url",
        message,
    }
}

/// A checked preview address template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    text: String,
    origin: String,
}

/// The values a template is filled with. Empty ones fill as empty text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fill<'a> {
    /// The one-time code.
    pub code: &'a str,
    /// The entry's collection.
    pub collection: &'a str,
    /// The entry's id.
    pub id: &'a str,
    /// The entry's slug, if it has one.
    pub slug: &'a str,
    /// The entry's locale, if it names one.
    pub locale: &'a str,
}

impl Template {
    /// Checks `text`. `allow_http_local` is the development server's switch (D-149): plain
    /// `http` to `localhost` or a loopback address only.
    pub fn parse(text: &str, allow_http_local: bool) -> Result<Self, TemplateError> {
        let text = text.trim();
        if text.chars().count() > MAX_TEMPLATE {
            return Err(refuse("the address is longer than 2,048 characters"));
        }
        if text.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(refuse(
                "the address can't hold spaces or control characters",
            ));
        }
        let (scheme, rest) = if let Some(rest) = text.strip_prefix("https://") {
            ("https", rest)
        } else if let Some(rest) = text.strip_prefix("http://") {
            ("http", rest)
        } else {
            return Err(refuse("the address must start with https://"));
        };
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..end];
        if authority.is_empty() {
            return Err(refuse("the address needs a host"));
        }
        if authority.contains(['{', '}']) {
            return Err(refuse(
                "placeholders belong in the path or query, never in the host",
            ));
        }
        if authority.contains('@') {
            return Err(refuse("the address can't carry a user name or password"));
        }
        // The origin as browsers serialize it, through the URL parser: the Studio posts
        // messages to exactly this string.
        let parsed = url::Url::parse(&format!("{scheme}://{authority}/"))
            .map_err(|_| refuse("the host isn't a valid host name"))?;
        let host = parsed.host_str().unwrap_or_default();
        if scheme == "http" {
            let local = matches!(host, "localhost" | "127.0.0.1" | "[::1]");
            if !(allow_http_local && local) {
                return Err(refuse("the address must start with https://"));
            }
        }
        check_placeholders(&rest[end..])?;
        if !text.contains("{code}") {
            return Err(refuse(
                "the address must hold {code}, where the one-time code goes",
            ));
        }
        Ok(Self {
            text: text.to_owned(),
            origin: parsed.origin().ascii_serialization(),
        })
    }

    /// The template as set.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The origin every address it fills shares: `https://www.example.com`.
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// The address for one preview: each placeholder replaced by its value, percent-encoded
    /// so a value can't add a path segment, a query parameter or a fragment.
    pub fn fill(&self, values: &Fill<'_>) -> String {
        let mut out = String::with_capacity(self.text.len() + values.code.len());
        let mut rest = self.text.as_str();
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            // `parse` proved every `{` closes on a known name.
            let close = after.find('}').unwrap_or(after.len());
            let value = match &after[..close] {
                "code" => values.code,
                "collection" => values.collection,
                "id" => values.id,
                "slug" => values.slug,
                "locale" => values.locale,
                _ => "",
            };
            encode_into(&mut out, value);
            rest = after.get(close + 1..).unwrap_or("");
        }
        out.push_str(rest);
        out
    }
}

/// Every `{…}` in `tail` names a known placeholder, and braces pair up.
fn check_placeholders(tail: &str) -> Result<(), TemplateError> {
    let mut rest = tail;
    loop {
        match (rest.find('{'), rest.find('}')) {
            (None, None) => return Ok(()),
            (Some(open), Some(close)) if open < close => {
                let name = &rest[open + 1..close];
                if !PLACEHOLDERS.contains(&name) {
                    return Err(refuse(
                        "the only placeholders are {code}, {collection}, {id}, {slug} and {locale}",
                    ));
                }
                rest = &rest[close + 1..];
            }
            _ => return Err(refuse("a { in the address isn't closed by a }")),
        }
    }
}

/// Appends `value` percent-encoded: everything but RFC 3986's unreserved characters.
fn encode_into(out: &mut String, value: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0xf)]));
        }
    }
}

/// The time to live a grant asked for, checked: [`DEFAULT_TTL`] when absent.
pub fn ttl(asked: Option<i64>) -> Option<i32> {
    match asked {
        None => Some(DEFAULT_TTL),
        Some(seconds) => i32::try_from(seconds)
            .ok()
            .filter(|seconds| TTL_RANGE.contains(seconds)),
    }
}

/// Where a preview row came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// The latest release, untouched by the scope.
    Published,
    /// The scope's own row: a draft or a publish request's pinned revision.
    Scope,
}

/// The overlay rule (D-171) over plain values: `published` maps each entry delivered at the
/// latest generation to its revision; `scope` maps each entry the scope names to the revision
/// it shows, or `None` to take it out. The answer is every entry either side shows, keyed by
/// entry, with its revision and its source — what `/preview/v1` must list, before the issuer's
/// field policy and the page bound.
pub fn overlay<E: Ord + Clone, R: Clone>(
    published: &BTreeMap<E, R>,
    scope: &BTreeMap<E, Option<R>>,
) -> BTreeMap<E, (R, Source)> {
    let mut shown: BTreeMap<E, (R, Source)> = published
        .iter()
        .filter(|(entry, _)| !scope.contains_key(*entry))
        .map(|(entry, revision)| (entry.clone(), (revision.clone(), Source::Published)))
        .collect();
    for (entry, revision) in scope {
        if let Some(revision) = revision {
            shown.insert(entry.clone(), (revision.clone(), Source::Scope));
        }
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_must_be_https_hold_code_and_keep_its_host_fixed() {
        let ok = Template::parse(
            "https://www.example.com/api/preview?code={code}&path=/blog/{slug}",
            false,
        )
        .expect("a good template");
        assert_eq!(ok.origin(), "https://www.example.com");
        for (bad, why) in [
            ("http://www.example.com/p?code={code}", "plain http"),
            ("https://www.example.com/p", "no {code}"),
            (
                "https://{slug}.example.com/p?code={code}",
                "placeholder in host",
            ),
            ("https://user:pw@example.com/p?code={code}", "credentials"),
            (
                "https://example.com/p?code={code}&x={secret}",
                "unknown placeholder",
            ),
            ("https://example.com/p?code={code", "unclosed brace"),
            ("https://example.com/p?code=}{code}", "stray close"),
            ("ftp://example.com/p?code={code}", "another scheme"),
            ("https:///p?code={code}", "no host"),
            ("https://example.com/p?code={code} x", "a space"),
        ] {
            assert!(Template::parse(bad, true).is_err(), "{why}: {bad}");
        }
        let long = format!("https://example.com/{}?code={{code}}", "a".repeat(2048));
        assert!(Template::parse(&long, false).is_err(), "too long");
    }

    #[test]
    fn plain_http_is_for_a_development_servers_own_machine_only() {
        let local = "http://localhost:58090/preview/api?code={code}";
        assert!(
            Template::parse(local, false).is_err(),
            "not without the switch"
        );
        let parsed = Template::parse(local, true).expect("with the switch");
        assert_eq!(parsed.origin(), "http://localhost:58090");
        assert!(Template::parse("http://127.0.0.1:3000/p?code={code}", true).is_ok());
        assert!(
            Template::parse("http://example.com/p?code={code}", true).is_err(),
            "the switch never opens plain http to the internet"
        );
        assert!(Template::parse("http://10.0.0.5/p?code={code}", true).is_err());
    }

    #[test]
    fn filling_encodes_every_value_so_none_can_reshape_the_address() {
        let template = Template::parse(
            "https://example.com/api/preview?code={code}&path=/{collection}/{slug}&l={locale}#{id}",
            false,
        )
        .expect("good");
        let filled = template.fill(&Fill {
            code: "cpc_AbC-_9",
            collection: "articles",
            id: "0190",
            slug: "a b/../c?x=1&y#z",
            locale: "fr-CA",
        });
        assert_eq!(
            filled,
            "https://example.com/api/preview?code=cpc_AbC-_9&path=/articles/a%20b%2F..%2Fc%3Fx%3D1%26y%23z&l=fr-CA#0190"
        );
        assert!(filled.starts_with(template.origin()));
        let empty = template.fill(&Fill {
            code: "c",
            ..Fill::default()
        });
        assert_eq!(empty, "https://example.com/api/preview?code=c&path=//&l=#");
    }

    #[test]
    fn a_time_to_live_is_a_minute_to_an_hour() {
        assert_eq!(ttl(None), Some(900));
        assert_eq!(ttl(Some(60)), Some(60));
        assert_eq!(ttl(Some(3600)), Some(3600));
        assert_eq!(ttl(Some(59)), None);
        assert_eq!(ttl(Some(3601)), None);
        assert_eq!(ttl(Some(i64::MAX)), None);
    }

    /// A small deterministic generator, so the property runs without a dependency and a
    /// failure names the seed that found it.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    #[test]
    fn the_overlay_shows_exactly_the_scope_over_what_it_leaves_published() {
        for seed in 1..=500_u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let published: BTreeMap<u8, u32> = (0..rng.below(12))
                .map(|_| {
                    (
                        u8::try_from(rng.below(16)).unwrap_or(0),
                        1000 + u32::try_from(rng.below(1000)).unwrap_or(0),
                    )
                })
                .collect();
            let scope: BTreeMap<u8, Option<u32>> = (0..rng.below(6))
                .map(|_| {
                    let entry = u8::try_from(rng.below(16)).unwrap_or(0);
                    let shows = (rng.below(4) != 0)
                        .then(|| 5000 + u32::try_from(rng.below(1000)).unwrap_or(0));
                    (entry, shows)
                })
                .collect();
            let shown = overlay(&published, &scope);
            for entry in 0..16_u8 {
                let expected = match scope.get(&entry) {
                    Some(Some(revision)) => Some((*revision, Source::Scope)),
                    Some(None) => None,
                    None => published.get(&entry).map(|r| (*r, Source::Published)),
                };
                assert_eq!(
                    shown.get(&entry).copied(),
                    expected,
                    "seed {seed}, entry {entry}"
                );
            }
            // Nothing outside either side ever appears.
            assert!(
                shown
                    .keys()
                    .all(|e| published.contains_key(e) || scope.contains_key(e)),
                "seed {seed}"
            );
        }
    }
}
