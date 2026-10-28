//! Inputs for Colophon's micro-benchmarks (milestone S, D-074).
//!
//! Every input is deterministic: built here in code or read from the shared fixtures in
//! `fixtures/`, with no network and no database. They are shaped after what the load scenarios
//! in `perf/k6/` send, so a benchmark times the same work a request does: an article like the
//! blog-post fixture, the page of objects, lists and blocks that `perf/k6/modelling.js` saves,
//! a body near the rich-text size limit, a content type near its field limit, a custom role near
//! its permission limit.
//!
//! The benchmarks are in `benches/hot_paths.rs`; `scripts/bench.sh` runs them and compares two
//! builds on one machine (docs/modules/17-verification.md, "Micro-benchmarks"). The tests below
//! keep every input valid, so a benchmark can never time a refusal by accident.

use std::fmt::Write as _;

use colophon_domain::{changeset, policy::Permission};
use serde_json::{Value, json};

/// `fixtures/rich-text/v1/valid/12-blog-post.json`: "A realistic article using most of the
/// format", about 2 KB of canonical rich text. Its document is under `value`.
pub const BLOG_POST_FIXTURE: &str =
    include_str!("../../../fixtures/rich-text/v1/valid/12-blog-post.json");

/// Two people the page's quotes name: entry ids as the database writes them.
pub const PEOPLE: [&str; 2] = [
    "01a18f43-a955-755e-ab9c-1ded44066830",
    "01a18f43-a955-755e-ab9c-1ded44066831",
];

/// A session token as `colophon_auth::tokens::random_token` makes them: 43 URL-safe characters.
pub const SESSION_TOKEN: &str = "q3V9xYb0ZkP2mW7nR4tL8sH1cJ6dF5gA0eU9iO3yT2w";

/// A CSRF token of the same shape, as the `X-CSRF-Token` header carries it.
pub const CSRF_TOKEN: &str = "M8kQ2pZr7Vx4Lc1Nb6Hy9Tw3Gf5Dj0Sa2Ue8Io4Pq7R";

/// An article: a title and the blog-post fixture as its body (`ContentType::articles`).
pub fn article(body: Value) -> Value {
    let mut fields = serde_json::Map::new();
    fields.insert("title".to_owned(), Value::from("Why Colophon"));
    fields.insert("body".to_owned(), body);
    Value::Object(fields)
}

/// The `seo` component `perf/k6/modelling.js` defines.
pub fn seo_component() -> Value {
    json!({
        "name": "SEO",
        "fields": [{ "id": "meta_title", "label": "Meta title", "type": "text", "max_chars": 70 }],
    })
}

/// The `pages` content type `perf/k6/modelling.js` defines, as sent: the SEO component, a hero
/// object, tags matched against a pattern, a list of questions, and blocks whose quotes
/// reference people.
pub fn page_type() -> Value {
    json!({
        "name": "Pages",
        "title_field": "title",
        "fields": [
            { "id": "seo", "label": "SEO", "type": "component", "component": "seo" },
            { "id": "title", "label": "Title", "type": "text", "required": true, "unique": true },
            { "id": "hero", "label": "Hero", "type": "object", "fields": [
                { "id": "heading", "label": "Heading", "type": "text", "required": true },
                { "id": "accent", "label": "Accent", "type": "color" },
            ] },
            { "id": "tags", "label": "Tags", "type": "list",
              "of": { "type": "text", "max_chars": 30, "pattern": "[a-z0-9]+" }, "max": 5 },
            { "id": "faq", "label": "Questions", "type": "list", "max": 10, "of": { "type": "object", "fields": [
                { "id": "q", "label": "Question", "type": "text", "required": true },
                { "id": "a", "label": "Answer", "type": "markdown" },
            ] } },
            { "id": "sections", "label": "Sections", "type": "blocks", "max": 20, "blocks": [
                { "name": "text_block", "label": "Text", "fields": [
                    { "id": "body", "label": "Body", "type": "markdown", "required": true },
                ] },
                { "name": "quote", "label": "Quote", "fields": [
                    { "id": "text", "label": "Quote", "type": "text", "required": true },
                    { "id": "by", "label": "By", "type": "reference", "to": "people" },
                ] },
            ] },
        ],
    })
}

/// A page as `perf/k6/modelling.js` saves it (its `page()` with `n` = 42).
pub fn page() -> Value {
    json!({
        "title": "Page by VU 7",
        "seo": { "meta_title": "Edition 42" },
        "hero": { "heading": "Edition 42", "accent": "#10002a" },
        "tags": ["news", "n42"],
        "faq": [{ "q": "Free?", "a": "Yes, edition 42." }, { "q": "Open?", "a": "Yes." }],
        "sections": [
            { "_block": "text_block", "body": "# Edition 42" },
            { "_block": "quote", "text": "Ship it", "by": PEOPLE[0] },
            { "_block": "quote", "text": "Prove it", "by": PEOPLE[1] },
        ],
    })
}

/// A rich-text document of `paragraphs` paragraphs (with a heading every tenth and a bullet list
/// every twenty-fifth), each with bold, italic and linked text. 500 paragraphs come to about
/// 238 KiB of canonical JSON and 4,241 nodes: near [`colophon_domain::rich_text::MAX_BYTES`]
/// (256 KiB), well within [`colophon_domain::rich_text::MAX_NODES`].
pub fn long_rich_text(paragraphs: usize) -> Value {
    let mut content = Vec::with_capacity(paragraphs + paragraphs / 10 + paragraphs / 25);
    for n in 0..paragraphs {
        if n % 10 == 0 {
            content.push(json!({
                "type": "heading",
                "attrs": { "level": 2 },
                "content": [{ "type": "text", "text": format!("Part {}", n / 10 + 1) }],
            }));
        }
        content.push(json!({
            "type": "paragraph",
            "content": [
                { "type": "text", "text": format!("Paragraph {n} opens with plain words, then ") },
                { "type": "text", "text": "a bold phrase", "marks": [{ "type": "bold" }] },
                { "type": "text", "text": ", an " },
                { "type": "text", "text": "italic one", "marks": [{ "type": "italic" }] },
                { "type": "text", "text": " and a " },
                { "type": "text", "text": "link",
                  "marks": [{ "type": "link", "attrs": { "href": format!("https://example.com/notes/{n}") } }] },
                { "type": "text", "text": " before it closes with an ordinary sentence." },
            ],
        }));
        if n % 25 == 24 {
            content.push(json!({
                "type": "bulletList",
                "content": [
                    { "type": "listItem", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "First point" }] }] },
                    { "type": "listItem", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "Second point" }] }] },
                ],
            }));
        }
    }
    json!({
        "schema": "colophon/rich-text",
        "version": 1,
        "doc": { "type": "doc", "content": content },
    })
}

/// A content type at [`colophon_domain::schema::MAX_FIELDS`] (100) top-level fields of every
/// kind: text (every one with a pattern), numbers, choices, emails, URLs, dates, Markdown,
/// references, objects, lists, page-builder blocks, yes-or-no fields with defaults and rich text.
/// What a large site's main type might look like, and what every request on its entries parses.
pub fn big_type() -> Value {
    let mut fields = vec![
        json!({ "id": "title", "label": "Title", "type": "text", "required": true, "unique": true }),
        json!({ "id": "slug", "label": "Slug", "type": "slug", "source": "title" }),
    ];
    for n in 0..98 {
        let id = format!("f{n:02}");
        let label = format!("Field {n}");
        let field = match n % 13 {
            0 => json!({ "id": id, "label": label, "type": "text", "max_chars": 120,
                         "pattern": "[A-Z]{2}-[0-9]{4}(-[a-z]+)?" }),
            1 => {
                json!({ "id": id, "label": label, "type": "number", "integer": true, "min": 0, "max": 100_000 })
            }
            2 => json!({ "id": id, "label": label, "type": "enum",
                         "values": (0..12).map(|v| format!("option-{v}")).collect::<Vec<_>>() }),
            3 => {
                json!({ "id": id, "label": label, "type": "email", "description": "Where replies go." })
            }
            4 => json!({ "id": id, "label": label, "type": "url" }),
            5 => json!({ "id": id, "label": label, "type": "date" }),
            6 => json!({ "id": id, "label": label, "type": "markdown", "max_chars": 20_000 }),
            7 => {
                json!({ "id": id, "label": label, "type": "reference", "to": "people", "many": true, "max": 10 })
            }
            8 => json!({ "id": id, "label": label, "type": "object", "fields": [
                { "id": "heading", "label": "Heading", "type": "text", "required": true },
                { "id": "count", "label": "Count", "type": "number" },
                { "id": "shown", "label": "Shown", "type": "boolean" },
                { "id": "accent", "label": "Accent", "type": "color" },
                { "id": "at", "label": "At", "type": "datetime" },
            ] }),
            9 => json!({ "id": id, "label": label, "type": "list", "max": 20,
                         "of": { "type": "text", "max_chars": 40 } }),
            10 => json!({ "id": id, "label": label, "type": "blocks", "max": 30, "blocks": [
                { "name": "text_block", "label": "Text", "fields": [
                    { "id": "body", "label": "Body", "type": "markdown", "required": true },
                ] },
                { "name": "quote", "label": "Quote", "fields": [
                    { "id": "text", "label": "Quote", "type": "text", "required": true },
                    { "id": "by", "label": "By", "type": "reference", "to": "people" },
                ] },
                { "name": "figure", "label": "Figure", "fields": [
                    { "id": "src", "label": "Source", "type": "url", "required": true },
                    { "id": "caption", "label": "Caption", "type": "text" },
                ] },
            ] }),
            11 => json!({ "id": id, "label": label, "type": "boolean", "default": false }),
            _ => json!({ "id": id, "label": label, "type": "rich_text" }),
        };
        fields.push(field);
    }
    json!({ "name": "Big", "title_field": "title", "fields": fields })
}

/// A custom role at [`colophon_domain::policy::MAX_PERMISSIONS`] (500): the seven per-collection
/// actions on each of 70 collections (every third one's reads, creations and updates narrowed to
/// five fields), the five project-wide actions, and reading one's own entries in five more
/// collections. In the order `colophon_db::scope` reads them (by action name, then subject).
pub fn custom_role() -> Vec<Permission> {
    let fields = ["title", "summary", "body", "tags", "author"].map(str::to_owned);
    let mut permissions = Vec::with_capacity(500);
    // A permission that doesn't parse is left out; the tests count them.
    let mut grant = |action: &str, subject: &str, fields: Option<&[String]>, own: bool| {
        let condition = own.then_some("own");
        if let Ok(permission) = Permission::parse_narrowed(action, subject, fields, condition) {
            permissions.push(permission);
        }
    };
    for collection in 0..70 {
        let subject = format!("collection-{collection:02}");
        let narrowed = (collection % 3 == 0).then_some(fields.as_slice());
        for action in ["content.read", "content.create", "content.update"] {
            grant(action, &subject, narrowed, false);
        }
        for action in [
            "content.delete",
            "content.review",
            "content.publish",
            "schema.change",
        ] {
            grant(action, &subject, None, false);
        }
    }
    for action in [
        "members.manage",
        "roles.manage",
        "destinations.manage",
        "tokens.manage",
        "audit.read",
    ] {
        grant(action, "*", None, false);
    }
    for collection in 70..75 {
        grant(
            "content.read",
            &format!("collection-{collection:02}"),
            None,
            true,
        );
    }
    permissions.sort_by(|a, b| {
        (a.action.as_str(), a.subject.as_str()).cmp(&(b.action.as_str(), b.subject.as_str()))
    });
    permissions
}

/// A changeset of `count` items (the API takes at most 100), as the database hands them to
/// [`changeset::canonical`]: lowercase UUIDs and hex hashes, half publishing and half not in
/// entry order.
pub fn changeset_items(count: u32) -> Vec<changeset::Item> {
    let hex = |seed: u32| {
        let mut text = String::with_capacity(64);
        for word in 0..8u32 {
            let _infallible = write!(
                text,
                "{:08x}",
                seed.wrapping_mul(2_654_435_761).wrapping_add(word)
            );
        }
        text
    };
    (0..count)
        .map(|n| changeset::Item {
            kind: if n % 7 == 0 {
                changeset::Kind::Unpublish
            } else {
                changeset::Kind::Publish
            },
            // Reversed, so the canonical form has sorting to do.
            entry: format!("01a18f43-a955-755e-ab9c-{:012x}", u64::from(count - n)),
            revision: format!("01a18f44-0000-7000-8000-{:012x}", u64::from(n)),
            collection: format!("collection-{:02}", n % 12),
            content_hash: hex(n),
            schema_version: i32::try_from(n % 9).unwrap_or(0) + 1,
            definition_hash: hex(n + 1_000),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use colophon_domain::{
        policy::{Action, MAX_PERMISSIONS, Policy},
        rich_text,
        schema::{Component, ContentType, MAX_FIELDS, Unlisted, expand_components},
    };

    use super::*;

    fn blog_post() -> Value {
        let fixture: Value = serde_json::from_str(BLOG_POST_FIXTURE).unwrap();
        fixture["value"].clone()
    }

    fn page_definition() -> ContentType {
        let seo = Component::parse(&seo_component()).unwrap();
        let components = BTreeMap::from([("seo".to_owned(), seo.fields_json())]);
        let expanded = expand_components(&page_type(), &components, Unlisted::Refuse).unwrap();
        ContentType::parse(&expanded).unwrap()
    }

    #[test]
    fn the_article_is_valid() {
        let content = ContentType::articles()
            .validate(&article(blog_post()))
            .unwrap();
        assert!(content.canonical_json().len() > 1_500);
    }

    #[test]
    fn the_page_is_valid_and_names_both_people() {
        let content = page_definition().validate(&page()).unwrap();
        let named: Vec<&str> = content
            .references()
            .iter()
            .map(|r| r.entry.as_str())
            .collect();
        assert_eq!(named, PEOPLE);
    }

    #[test]
    fn the_long_body_is_near_the_size_limit() {
        let document = rich_text::validate(&long_rich_text(500)).unwrap();
        assert!(
            document.bytes() > rich_text::MAX_BYTES * 9 / 10
                && document.bytes() < rich_text::MAX_BYTES,
            "{} bytes",
            document.bytes()
        );
        assert!(document.nodes() < rich_text::MAX_NODES);
        ContentType::articles()
            .validate(&article(long_rich_text(500)))
            .unwrap();
    }

    #[test]
    fn the_big_type_has_every_field_it_may_and_round_trips() {
        let parsed = ContentType::parse(&big_type()).unwrap();
        assert_eq!(parsed.fields().len(), MAX_FIELDS);
        let canonical: Value = serde_json::from_str(&parsed.canonical_json()).unwrap();
        assert_eq!(ContentType::parse(&canonical).unwrap(), parsed);
    }

    #[test]
    fn the_custom_role_holds_the_most_permissions_a_role_may() {
        let permissions = custom_role();
        assert_eq!(permissions.len(), MAX_PERMISSIONS);
        let policy = Policy::new(permissions);
        assert_eq!(
            policy.permissions().len(),
            MAX_PERMISSIONS,
            "nothing merged"
        );
        assert!(policy.allows(Action::ContentPublish, Some("collection-37")));
        assert!(
            !policy.allows(Action::ContentRead, Some("collection-36")),
            "narrowed"
        );
    }

    #[test]
    fn the_changeset_has_distinct_entries() {
        let items = changeset_items(100);
        let text = changeset::canonical(&items);
        assert_eq!(text.lines().count(), 101);
    }
}
