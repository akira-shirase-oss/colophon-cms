//! Criterion benchmarks for the CPU work Colophon does per request (milestone S, D-074).
//!
//! The hot paths were picked from the code, not guessed: each benchmark below says, in one line,
//! which request runs it and how often. Inputs come from `colophon_bench` (see its docs). These
//! are micro-benchmarks: they time one function on one machine, never what a server sustains.
//!
//! Run them all with `cargo bench -p colophon-bench`, one group with a filter
//! (`cargo bench -p colophon-bench -- policy/`), and compare two commits on one machine with
//! `scripts/bench.sh compare <base>`.

#![expect(
    clippy::expect_used,
    reason = "the inputs are fixed; one that stops being valid must stop the run loudly"
)]

use std::{collections::BTreeMap, hint::black_box};

use colophon_auth::tokens;
use colophon_bench::{
    BLOG_POST_FIXTURE, CSRF_TOKEN, SESSION_TOKEN, article, big_type, changeset_items, custom_role,
    long_rich_text, page, page_type, seo_component,
};
use colophon_domain::{
    cache::Scope,
    changeset,
    policy::{Action, Policy, Role},
    rich_text,
    schema::{Component, ContentType, Unlisted, expand_components},
};
use criterion::{BatchSize, Criterion, SamplingMode, Throughput, criterion_group, criterion_main};
use serde_json::Value;

/// Paragraphs in the near-limit body: 238 KiB of canonical JSON (the limit is 256 KiB).
const NEAR_LIMIT_PARAGRAPHS: usize = 500;

fn blog_post() -> Value {
    let fixture: Value = serde_json::from_str(BLOG_POST_FIXTURE).expect("the fixture is JSON");
    fixture["value"].clone()
}

/// The page type as the server stores it: components expanded, parsed, canonical.
fn page_definition() -> ContentType {
    let seo = Component::parse(&seo_component()).expect("the SEO component is valid");
    let components = BTreeMap::from([("seo".to_owned(), seo.fields_json())]);
    let expanded = expand_components(&page_type(), &components, Unlisted::Refuse)
        .expect("the page type uses only the SEO component");
    ContentType::parse(&expanded).expect("the page type is valid")
}

fn bytes(text: &str) -> Throughput {
    Throughput::Bytes(u64::try_from(text.len()).expect("a length fits in u64"))
}

fn rich_text(c: &mut Criterion) {
    let mut group = c.benchmark_group("rich_text");
    let document = blog_post();
    let canonical = rich_text::validate(&document).expect("the blog post is valid");
    group.throughput(bytes(canonical.canonical_json()));
    // POST /api/v1/rich-text/validate (perf/k6/rich-text.js), and inside every save of a
    // rich-text field: a 2 KB article body.
    group.bench_function("validate_blog_post", |b| {
        b.iter(|| rich_text::validate(black_box(&document)));
    });
    group.finish();
}

fn entry(c: &mut Criterion) {
    let mut group = c.benchmark_group("entry");
    let articles = ContentType::articles();
    let article = article(blog_post());
    // Every create and save of an article (perf/k6/entries.js): title plus rich-text body.
    group.bench_function("validate_article", |b| {
        b.iter(|| black_box(&articles).validate(black_box(&article)));
    });

    let pages = page_definition();
    let page = page();
    let content = pages.validate(&page).expect("the page is valid");
    // Every save of a page in perf/k6/modelling.js: an object, lists, a pattern, blocks and
    // references, the deepest validation path there is.
    group.bench_function("validate_page", |b| {
        b.iter(|| black_box(&pages).validate(black_box(&page)));
    });

    let body = serde_json::to_vec(&page).expect("a page serializes");
    // axum's Json extractor on that same save: the request body into a serde_json::Value.
    group.bench_function("parse_body_page", |b| {
        b.iter(|| serde_json::from_slice::<Value>(black_box(&body)));
    });

    // The SHA-256 every save takes of its canonical content (colophon-api's `entries::sha256`
    // makes the same aws-lc-rs digest call as `tokens::hash`).
    group.bench_function("sha256_page", |b| {
        b.iter(|| tokens::hash(black_box(content.canonical_json())));
    });
    group.finish();
}

fn content_type(c: &mut Criterion) {
    let mut group = c.benchmark_group("content_type");
    // Every entry request reads its collection's definition from Postgres as text and parses it
    // (colophon-api `entries::content_type`): JSON text to Value, then ContentType::parse, which
    // also compiles each text pattern.
    for (name, definition) in [
        ("load_articles", ContentType::articles()),
        ("load_page", page_definition()),
        (
            "load_big",
            ContentType::parse(&big_type()).expect("the big type is valid"),
        ),
    ] {
        let stored = definition.canonical_json();
        group.bench_function(name, |b| {
            b.iter(|| {
                let value: Value =
                    serde_json::from_str(black_box(&stored)).expect("stored text is JSON");
                ContentType::parse(&value)
            });
        });
    }

    let big = ContentType::parse(&big_type()).expect("the big type is valid");
    // Creating or changing a content type writes and hashes its canonical form; changing a
    // component rewrites every type that uses it.
    group.bench_function("canonical_json_big", |b| {
        b.iter(|| black_box(&big).canonical_json());
    });

    let seo = Component::parse(&seo_component()).expect("the SEO component is valid");
    let components = BTreeMap::from([("seo".to_owned(), seo.fields_json())]);
    let sent = page_type();
    // Creating or changing a type that uses components (D-073), and every user of a component
    // when the component changes.
    group.bench_function("expand_components_page", |b| {
        b.iter(|| expand_components(black_box(&sent), black_box(&components), Unlisted::Refuse));
    });
    group.finish();
}

fn policy(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy");
    let owner = Policy::builtin(Role::Owner).permissions().to_vec();
    // Every signed-in request rebuilds the caller's role from its rows (colophon-db
    // `scope::policy` calls Policy::new): a built-in role, 12 permissions.
    group.bench_function("new_builtin_owner", |b| {
        b.iter_batched(
            || owner.clone(),
            |permissions| Policy::new(black_box(permissions)),
            BatchSize::SmallInput,
        );
    });
    let custom = custom_role();
    // The same for a custom role at the 500-permission limit (R.2): 96 µs before d0b0af7.
    group.bench_function("new_custom_500", |b| {
        b.iter_batched(
            || custom.clone(),
            |permissions| Policy::new(black_box(permissions)),
            BatchSize::SmallInput,
        );
    });
    let policy = Policy::new(custom);
    // Every entry read and save asks which fields the role may read and change (R.3).
    group.bench_function("fields_custom_500", |b| {
        b.iter(|| {
            black_box(&policy).fields(Action::ContentRead, black_box("collection-36"), false)
        });
    });
    // Every other permission check (`members::require` and the like).
    group.bench_function("allows_custom_500", |b| {
        b.iter(|| {
            black_box(&policy).allows(Action::ContentPublish, Some(black_box("collection-69")))
        });
    });
    group.finish();
}

fn changeset(c: &mut Criterion) {
    let mut group = c.benchmark_group("changeset");
    let items = changeset_items(100);
    // Every changeset read, review and publish recomputes the hash's input from its pinned items
    // (D-044); 100 is the most a changeset holds.
    group.bench_function("canonical_100", |b| {
        b.iter(|| changeset::canonical(black_box(&items)));
    });
    group.finish();
}

fn delivery(c: &mut Criterion) {
    let mut group = c.benchmark_group("delivery");
    let scope = Scope {
        project: 0x0193_5e1a_9b2c_7d00_8e4f_0a1b_2c3d_4e5f,
        environment: 0x0193_5e1a_9b2c_7d01_8e4f_0a1b_2c3d_4e60,
        epoch: 1,
        generation: 4_812,
        unpublished: 4_790,
    };
    let entry = 0x0193_5e1b_0000_7000_8000_0000_0000_002a;
    // Every /content/v1 entry read builds its cache key and tags before the cache lookup
    // (perf/k6/delivery.js, the busiest public path).
    group.bench_function("cache_key_and_tags", |b| {
        b.iter(|| {
            let scope = black_box(&scope);
            (
                scope.entry_key(black_box("articles"), black_box(entry)),
                scope.entry_tags(black_box("articles"), black_box(entry)),
            )
        });
    });
    group.finish();
}

fn auth(c: &mut Criterion) {
    let mut group = c.benchmark_group("auth");
    // Every signed-in request hashes its session cookie to look the session up (session.rs).
    group.bench_function("session_token_hash", |b| {
        b.iter(|| tokens::hash(black_box(SESSION_TOKEN)));
    });
    let stored = tokens::hash(CSRF_TOKEN);
    // Every unsafe request (POST, PUT, DELETE) compares its CSRF header with the session's.
    group.bench_function("csrf_check", |b| {
        b.iter(|| tokens::same_secret(&tokens::hash(black_box(CSRF_TOKEN)), black_box(&stored)));
    });
    group.finish();
}

/// The biggest inputs a request may carry: each iteration takes about a millisecond, so flat
/// sampling with fewer samples keeps the run short.
fn near_limit(c: &mut Criterion) {
    let mut group = c.benchmark_group("near_limit");
    group.sampling_mode(SamplingMode::Flat).sample_size(50);
    let document = long_rich_text(NEAR_LIMIT_PARAGRAPHS);
    let canonical = rich_text::validate(&document).expect("the long body is valid");
    group.throughput(bytes(canonical.canonical_json()));
    // Validating a 238 KiB body: the rich-text endpoint's worst case.
    group.bench_function("rich_text_validate", |b| {
        b.iter(|| rich_text::validate(black_box(&document)));
    });
    let articles = ContentType::articles();
    let article = article(document);
    // Saving an article whose body is that size.
    group.bench_function("validate_article", |b| {
        b.iter(|| black_box(&articles).validate(black_box(&article)));
    });
    let body = serde_json::to_vec(&article).expect("an article serializes");
    // Parsing that save's request body.
    group.bench_function("parse_body", |b| {
        b.iter(|| serde_json::from_slice::<Value>(black_box(&body)));
    });
    let content = articles
        .validate(&article)
        .expect("the long article is valid");
    // Hashing its canonical content.
    group.bench_function("sha256", |b| {
        b.iter(|| tokens::hash(black_box(content.canonical_json())));
    });
    group.finish();
}

// Criterion's defaults (3 s of warm-up, 5 s of measurement, 100 samples) for a developer's run;
// scripts/bench.sh passes shorter times for its rounds.
criterion_group!(
    benches,
    rich_text,
    entry,
    content_type,
    policy,
    changeset,
    delivery,
    auth,
    near_limit
);
criterion_main!(benches);
