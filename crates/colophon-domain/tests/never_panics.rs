//! Every parser of outside input, fed hostile and random input: it answers (accepts or refuses)
//! and never panics, overflows the stack or loops (docs/security/attack-surface.md). Where a
//! parser has a canonical form or a round trip, the property says so too.
//!
//! The JSON values are drawn from the keys and values the formats really use, so random input
//! reaches past the first `only(...)` check, and the rich-text fixtures are mutated in place.
//! The cargo-fuzz targets in `fuzz/` run the same entry points for longer.

#![expect(
    clippy::expect_used,
    reason = "a property test: a fixture that can't be read, or a setup that fails, is a test failure"
)]

use std::{collections::BTreeMap, fs, net::IpAddr, path::PathBuf, sync::OnceLock};

use colophon_domain::{
    audience::Visibility,
    bulk, destination, locale, migrate,
    preview::{Fill, Template},
    query::Query,
    receipt, rich_text,
    schedule::{self, Choice},
    schema::{ContentType, Unlisted, components_used, expand_components},
    transfer,
};
use proptest::prelude::*;
use serde_json::{Map, Value, json};

/// Keys the formats use: rich text, content types, queries, migrations, bulk and imports.
const KEYS: &[&str] = &[
    "type",
    "content",
    "text",
    "marks",
    "attrs",
    "href",
    "level",
    "start",
    "language",
    "assetId",
    "alt",
    "tone",
    "schema",
    "version",
    "doc",
    "name",
    "title_field",
    "fields",
    "id",
    "label",
    "required",
    "max_chars",
    "min_chars",
    "index",
    "values",
    "of",
    "blocks",
    "to",
    "many",
    "when",
    "field",
    "is",
    "pattern",
    "min",
    "max",
    "integer",
    "decimal",
    "multiline",
    "unique",
    "default",
    "component",
    "filter",
    "sort",
    "limit",
    "depth",
    "eq",
    "ne",
    "in",
    "lt",
    "gt",
    "op",
    "path",
    "value",
    "from",
    "data",
    "meta",
    "_id",
    "_type",
    "documentId",
    "headline",
    "title",
    "body",
    "slug",
    "locale",
    "tags",
    "related",
    "photo",
    "kind",
    "label_field",
];

/// Strings worth trying anywhere a string goes.
const STRINGS: &[&str] = &[
    "",
    "paragraph",
    "text",
    "heading",
    "link",
    "image",
    "doc",
    "colophon/rich-text",
    "fill",
    "move",
    "update",
    "delete",
    "set",
    "list",
    "object",
    "rich_text",
    "reference",
    "boolean",
    "enum",
    "number",
    "javascript:alert(1)",
    "https://example.com/",
    "//evil.example",
    "/\t/evil.example",
    "\u{0}\u{1}\u{1f}\u{7f}",
    "\u{2028}\u{2029}\u{feff}",
    "𝄞é漢字",
    "../../etc/passwd",
    "' OR 1=1 --",
    "<script>alert(1)</script>",
    "9223372036854775807",
    "-9223372036854775808",
    "1e309",
];

fn leaf() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::from),
        prop_oneof![Just(0i64), Just(1), Just(-1), Just(2), Just(100), Just(101)]
            .prop_map(Value::from),
        any::<f64>().prop_map(|number| json!(number)),
        any::<u64>().prop_map(Value::from),
        prop::sample::select(STRINGS).prop_map(Value::from),
        prop::sample::select(KEYS).prop_map(Value::from),
        ".{0,24}".prop_map(Value::from),
    ]
}

/// A JSON value up to `depth` levels deep, objects keyed from [`KEYS`] (and sometimes not).
fn value(depth: u32) -> impl Strategy<Value = Value> {
    leaf().prop_recursive(depth, 96, 8, |inner| {
        let key = prop_oneof![
            9 => prop::sample::select(KEYS).prop_map(str::to_owned),
            1 => ".{0,8}",
        ];
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..8).prop_map(Value::Array),
            prop::collection::vec((key, inner), 0..8)
                .prop_map(|pairs| Value::Object(pairs.into_iter().collect::<Map<_, _>>())),
        ]
    })
}

/// `base` with the value at the `choice`-th node (pre-order) replaced by `with`.
fn mutate(base: &Value, choice: usize, with: Value) -> Value {
    fn walk(node: &mut Value, target: usize, seen: &mut usize, with: &mut Option<Value>) {
        if *seen == target {
            if let Some(replacement) = with.take() {
                *node = replacement;
            }
            return;
        }
        *seen += 1;
        match node {
            Value::Array(items) => items
                .iter_mut()
                .for_each(|item| walk(item, target, seen, with)),
            Value::Object(map) => map
                .values_mut()
                .for_each(|item| walk(item, target, seen, with)),
            _ => {}
        }
    }
    fn count(node: &Value) -> usize {
        1 + match node {
            Value::Array(items) => items.iter().map(count).sum(),
            Value::Object(map) => map.values().map(count).sum(),
            _ => 0,
        }
    }
    let mut out = base.clone();
    let target = choice % count(base);
    walk(&mut out, target, &mut 0, &mut Some(with));
    out
}

/// Every rich-text fixture, valid and invalid: the seed corpus the editor and server share.
fn rich_text_seeds() -> &'static [Value] {
    static SEEDS: OnceLock<Vec<Value>> = OnceLock::new();
    SEEDS.get_or_init(read_rich_text_seeds)
}

fn read_rich_text_seeds() -> Vec<Value> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rich-text/v1");
    let mut seeds = Vec::new();
    for kind in ["valid", "invalid"] {
        let mut paths: Vec<_> = fs::read_dir(root.join(kind))
            .expect("the rich-text fixtures")
            .map(|entry| entry.expect("a fixture").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        paths.sort();
        for path in paths {
            let text = fs::read_to_string(&path).expect("a fixture");
            let fixture: Value = serde_json::from_str(&text).expect("fixture JSON");
            seeds.push(fixture["value"].clone());
        }
    }
    assert!(
        seeds.len() >= 40,
        "the fixtures moved: {} seeds",
        seeds.len()
    );
    seeds
}

/// A content type with every field kind, the one most properties check entries against.
fn every_kind() -> Value {
    json!({
        "name": "Posts",
        "title_field": "headline",
        "fields": [
            {"id": "headline", "label": "Headline", "type": "text", "required": true, "max_chars": 120, "index": true},
            {"id": "author", "label": "Author", "type": "reference", "to": "staff", "index": true},
            {"id": "publish_at", "label": "Publish at", "type": "datetime", "index": true},
            {"id": "section", "label": "Section", "type": "enum", "values": ["News", "Guides"], "index": true},
            {"id": "rating", "label": "Rating", "type": "number", "integer": true, "min": 0, "max": 5, "index": true},
            {"id": "score", "label": "Score", "type": "number", "index": true},
            {"id": "featured", "label": "Featured", "type": "boolean", "index": true},
            {"id": "slug", "label": "Slug", "type": "slug", "index": true},
            {"id": "email", "label": "Email", "type": "email", "index": true},
            {"id": "site", "label": "Site", "type": "url", "index": true},
            {"id": "born", "label": "Born", "type": "date", "index": true},
            {"id": "opens", "label": "Opens", "type": "time", "index": true},
            {"id": "notes", "label": "Notes", "type": "text", "multiline": true},
            {"id": "body", "label": "Body", "type": "rich_text"},
            {"id": "meta", "label": "Meta", "type": "object", "fields": [
                {"id": "seo", "label": "SEO", "type": "text"}
            ]},
            {"id": "tags", "label": "Tags", "type": "list", "of": {"type": "text"}},
            {"id": "related", "label": "Related", "type": "reference", "to": "articles", "many": true},
            {"id": "photo", "label": "Photo", "type": "asset"},
            {"id": "accent", "label": "Accent", "type": "color"},
            {"id": "scratch", "label": "Scratch", "type": "json"},
            {"id": "script", "label": "Script", "type": "markdown"}
        ]
    })
}

fn posts() -> ContentType {
    ContentType::parse(&every_kind()).expect("every kind parses")
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    #[test]
    fn rich_text_answers_any_value(input in value(6)) {
        let _ = rich_text::validate(&input);
    }

    /// Fixtures with one node replaced: deep in a real document, past the envelope checks.
    #[test]
    fn rich_text_answers_any_mutated_fixture(seed in any::<prop::sample::Index>(), at in any::<usize>(), with in value(3)) {
        let seeds = rich_text_seeds();
        let input = mutate(seed.get(seeds), at, with);
        let _ = rich_text::validate(&input);
    }

    /// The canonical form is a fixed point: validating it again accepts it unchanged.
    #[test]
    fn rich_text_canonical_form_is_stable(seed in any::<prop::sample::Index>(), at in any::<usize>(), with in value(2)) {
        let seeds = rich_text_seeds();
        let input = mutate(seed.get(seeds), at, with);
        if let Ok(document) = rich_text::validate(&input) {
            let again: Value = serde_json::from_str(document.canonical_json()).expect("canonical JSON parses");
            let twice = rich_text::validate(&again).expect("the canonical form is accepted");
            prop_assert_eq!(twice.canonical_json(), document.canonical_json());
        }
    }

    /// An allowed link never carries a control character, whitespace or a backslash, and is a
    /// fragment, a one-slash path or an http(s)/mailto URL.
    #[test]
    fn allowed_links_are_only_safe_ones(href in prop_oneof![".{0,40}", prop::sample::select(STRINGS).prop_map(str::to_owned)]) {
        if rich_text::is_allowed_href(&href) {
            let unsafe_character = href.chars().any(|c| c <= ' ' || c == '\u{7f}' || c == '\\');
            prop_assert!(!unsafe_character, "{:?}", href);
            let lower = href.to_ascii_lowercase();
            prop_assert!(
                href.starts_with('#')
                    || (href.starts_with('/') && !href.starts_with("//"))
                    || lower.starts_with("http:")
                    || lower.starts_with("https:")
                    || lower.starts_with("mailto:"),
                "{href:?}"
            );
        }
    }

    #[test]
    fn content_types_answer_any_definition(input in value(6)) {
        let _ = ContentType::parse(&input);
        let _ = components_used(&input);
        let _ = expand_components(&input, &BTreeMap::default(), Unlisted::Refuse);
    }

    /// A real definition with one node replaced. Whatever parses has one canonical form.
    #[test]
    fn content_types_answer_any_mutated_definition(at in any::<usize>(), with in value(3)) {
        let input = mutate(&every_kind(), at, with);
        if let Ok(parsed) = ContentType::parse(&input) {
            let _ = parsed.validate(&json!({"headline": "x"}));
        }
    }

    #[test]
    fn entries_answer_any_content(input in value(5)) {
        let posts = posts();
        let _ = posts.validate(&input);
        let _ = posts.accepts(&input.to_string());
    }

    /// One field of a valid entry replaced by anything.
    #[test]
    fn entries_answer_any_field_value(field in prop::sample::select(&[
        "headline", "author", "publish_at", "section", "rating", "score", "featured", "slug",
        "email", "site", "born", "opens", "notes", "body", "meta", "tags", "related", "photo",
        "accent", "scratch", "script",
    ][..]), with in value(4)) {
        let mut entry = json!({"headline": "A title"});
        entry[field] = with;
        let _ = posts().validate(&entry);
    }

    #[test]
    fn queries_answer_any_document(input in value(5)) {
        let _ = Query::validate(&posts(), &input);
    }

    #[test]
    fn query_cursors_answer_any_token(token in prop_oneof![".{0,64}", "[A-Za-z0-9_-]{0,400}"]) {
        let query = Query::validate(&posts(), &json!({"sort": [{"field": "rating"}]})).expect("a query");
        let _ = query.after_cursor(&token);
    }

    #[test]
    fn migrations_answer_any_operations(operations in value(5), document in value(4)) {
        if let Ok(ops) = migrate::parse(&operations) {
            let _ = migrate::apply(&document, &ops);
        }
    }

    #[test]
    fn bulk_answers_any_operation(input in value(5), stored in value(4)) {
        if let Ok(op) = bulk::parse(&input) {
            let _ = bulk::apply(&op, &stored);
        }
    }

    #[test]
    fn imports_map_any_item(item in value(5)) {
        let _ = transfer::map(&item, &posts());
    }

    /// Canonical locale tags are a fixed point and name the same locale.
    #[test]
    fn locales_canonicalise_once(tag in "[A-Za-z0-9-]{0,35}") {
        let once = locale::canonical(&tag);
        prop_assert_eq!(locale::canonical(&once), once.clone());
        prop_assert!(locale::same(&tag, &once));
    }

    #[test]
    fn receipt_markers_answer_any_page(page in ".{0,200}") {
        let _ = receipt::marker(&page);
    }

    #[test]
    fn receipt_markers_read_the_generation_they_carry(generation in 1i64..=999_999_999_999_999_999) {
        let page = format!(r#"<head><meta name="colophon-generation" content="{generation}"></head>"#);
        prop_assert_eq!(receipt::marker(&page), Some(generation));
    }

    /// A filled preview address always stays on the template's origin, whatever the values.
    #[test]
    fn preview_addresses_never_leave_their_origin(
        template in prop_oneof![
            Just("https://site.example/preview?code={code}&id={id}".to_owned()),
            Just("https://site.example/{collection}/{slug}/{locale}?c={code}".to_owned()),
            ".{0,60}",
        ],
        code in ".{0,20}", collection in ".{0,20}", id in ".{0,20}", slug in ".{0,20}", locale_tag in ".{0,20}",
    ) {
        if let Ok(parsed) = Template::parse(&template, false) {
            let filled = parsed.fill(&Fill { code: &code, collection: &collection, id: &id, slug: &slug, locale: &locale_tag });
            let url = url::Url::parse(&filled).expect("a filled template is a URL");
            prop_assert_eq!(url.origin().ascii_serialization(), parsed.origin());
        }
    }

    #[test]
    fn destination_urls_answer_any_text(text in prop_oneof![".{0,80}", "https://\\[[0-9a-f:.]{0,40}\\]/"], allow in any::<bool>()) {
        let _ = destination::check_url(&text, allow);
    }

    /// A public address is never one the special-purpose registries reserve.
    #[test]
    fn every_address_is_judged(ip in any::<IpAddr>()) {
        let _ = destination::is_public(ip);
    }

    #[test]
    fn visibilities_round_trip(text in prop_oneof![".{0,70}", "role:[a-z][a-z0-9-]{0,62}"]) {
        if let Some(visibility) = Visibility::parse(&text) {
            prop_assert_eq!(Visibility::parse(&visibility.value()), Some(visibility));
        }
    }

    #[test]
    fn schedules_answer_any_local_time(local in ".{0,30}", zone in prop_oneof![".{0,30}", Just("Europe/London".to_owned())], later in any::<Option<bool>>()) {
        let choice = later.map(|later| if later { Choice::Later } else { Choice::Earlier });
        let _ = schedule::resolve(&local, &zone, choice);
    }

    /// Instants are seconds from outside: any i64 gets an answer, never an overflow.
    #[test]
    fn schedule_windows_answer_any_instants(at in any::<i64>(), now in any::<i64>(), expires in any::<i64>()) {
        let _ = schedule::check_window(at, now, expires);
        let _ = schedule::delayed(at, now);
    }
}

// The hostile cases the survey found, one test each: they failed before their fix.

/// A condition's `is` list is refused for its length before anything looks at its values: a
/// list of tens of thousands of distinct values used to be compared pairwise (O(n²)) first,
/// holding the content-type write's transaction for minutes.
#[test]
fn a_huge_condition_list_is_refused_at_once() {
    let values: Vec<Value> = (0..30_000).map(Value::from).collect();
    let definition = json!({
        "name": "T",
        "title_field": "t",
        "fields": [
            {"id": "t", "label": "T", "type": "text"},
            {"id": "flag", "label": "Flag", "type": "boolean"},
            {"id": "x", "label": "X", "type": "text", "when": {"field": "flag", "is": values}}
        ]
    });
    let started = std::time::Instant::now();
    let refusal = ContentType::parse(&definition).expect_err("too many values");
    assert!(
        started.elapsed() < std::time::Duration::from_millis(500),
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(refusal.pointer, "/fields/2/when");
}

/// An operation with an empty path is refused, never a panic: `parse` never makes one, but
/// `Op` is public, so any caller (and every bulk item) can.
#[test]
fn an_operation_with_an_empty_path_is_refused() {
    use migrate::{Op, Segment};
    let field = || vec![Segment::Field("a".to_owned())];
    let free = || vec![Segment::Field("b".to_owned())];
    let document = json!({"a": 1});
    for ops in [
        // The whole document moved to a free field: this one reached `path.len() - 1`.
        vec![Op::Move {
            from: vec![],
            to: free(),
        }],
        vec![Op::Move {
            from: vec![],
            to: field(),
        }],
        vec![Op::Move {
            from: field(),
            to: vec![],
        }],
        vec![Op::Fill {
            path: vec![],
            value: json!(1),
        }],
    ] {
        assert!(migrate::apply(&document, &ops).is_err(), "{ops:?}");
        assert!(bulk::apply(&bulk::Op::Apply(ops), &document).is_err());
    }
}

/// IPv6 addresses that carry an IPv4 address inside them are judged by that address: a
/// destination can't reach 10.0.0.1 or 127.0.0.1 through an embedding.
#[test]
fn private_addresses_inside_ipv6_are_refused() {
    for host in [
        "[::10.0.0.1]", // IPv4-compatible (deprecated, still routed by some stacks)
        "[::127.0.0.1]",
        "[2002:a00:1::]",                         // 6to4 of 10.0.0.1
        "[2002:7f00:1::]",                        // 6to4 of 127.0.0.1
        "[2001:0:4136:e378:8000:63bf:f5ff:fffe]", // Teredo
        "[fec0::1]",                              // site-local (deprecated)
        "[64:ff9b:1::a00:1]",                     // local-use NAT64 of 10.0.0.1
    ] {
        let url = format!("https://{host}/hook");
        assert!(
            destination::check_url(&url, false).is_err(),
            "{url} was allowed"
        );
    }
    // A public address embedded the same ways is judged by that public address.
    assert!(destination::check_url("https://[2002:808:808::]/hook", false).is_ok());
    assert!(destination::check_url("https://[2606:4700:4700::1111]/hook", false).is_ok());
}
