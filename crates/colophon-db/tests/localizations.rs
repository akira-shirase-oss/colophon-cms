//! Localizations against real PostgreSQL, as the application's login: making a sibling groups
//! the source too, the new entry starts from the source's draft, a group holds one entry per
//! locale, and localizing a missing entry says so. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    PgPool,
    entries::{self, Localized, NewRevision},
    projects,
    scope::{self, ProjectTransaction},
};
use common::TestDb;
use uuid::Uuid;

const ARTICLES: &str = "articles";

struct Content {
    id: Uuid,
    text: String,
    hash: [u8; 32],
    title: String,
}

impl Content {
    fn new(title: &str) -> Self {
        let id = Uuid::now_v7();
        Self {
            id,
            text: format!(r#"{{"title":"{title}","body":{{}}}}"#),
            hash: *id.as_bytes().repeat(2).first_chunk().expect("32 bytes"),
            title: title.to_owned(),
        }
    }

    fn revision(&self) -> NewRevision<'_> {
        NewRevision {
            id: self.id,
            content: &self.text,
            content_hash: &self.hash,
            title: &self.title,
            schema_version: 1,
        }
    }
}

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member, in an environment that exists")
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn localizing_groups_the_source_copies_the_draft_and_allows_one_per_locale() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "gazette", "Gazette")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    let first = Content::new("Hello");
    let english = Uuid::now_v7();
    entries::create(
        &mut tx,
        english,
        ARTICLES,
        first.revision(),
        &[],
        Some("en"),
    )
    .await
    .expect("created");

    // Localizing to French: a sibling is made, and the English entry joins the group.
    let french = Uuid::now_v7();
    let made = entries::create_localization(&mut tx, french, english, ARTICLES, "fr")
        .await
        .expect("localized");
    let Localized::Made(draft) = made else {
        panic!("expected a new entry, got {made:?}")
    };
    assert_eq!(draft.entry_id, french);
    assert_eq!(draft.locale.as_deref(), Some("fr"));
    assert_eq!(draft.content, first.text, "starts from the source's draft");
    assert_eq!(draft.revision.title, "Hello");

    // Both entries are in the group, by locale.
    let group = entries::localizations(&mut tx, ARTICLES, french)
        .await
        .expect("siblings");
    assert_eq!(
        group.iter().map(|s| s.locale.as_str()).collect::<Vec<_>>(),
        ["en", "fr"]
    );
    assert!(group.iter().any(|s| s.id == english));

    // A second French version is refused with the sibling that holds it; a missing source
    // says so.
    assert_eq!(
        entries::create_localization(&mut tx, Uuid::now_v7(), english, ARTICLES, "fr")
            .await
            .expect("localized"),
        Localized::Occupied(french)
    );
    assert_eq!(
        entries::create_localization(&mut tx, Uuid::now_v7(), Uuid::now_v7(), ARTICLES, "de")
            .await
            .expect("localized"),
        Localized::Missing
    );
    tx.commit().await.expect("commit");
    db.drop().await;
}
