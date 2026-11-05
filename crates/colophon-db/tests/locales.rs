//! Entry locales and localization groups against real PostgreSQL, as the application's login
//! (D-087): `?locale=` storage and filtering, the group's one-entry-per-locale rule, a sibling
//! that copies its source's draft, and tombstoned siblings freeing their locale. Needs the
//! local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    PgPool,
    entries::{self, Deleted, Localized, NewRevision, SetLocale},
    projects,
    scope::{self, ProjectTransaction},
};
use common::TestDb;
use uuid::Uuid;

const ARTICLES: &str = "articles";

/// A revision with made-up content: these tests are about storage, not the article format.
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
            slug: None,
            schema_version: 1,
            references: &[],
            assets: &[],
            unique: &[],
            visibility: None,
        }
    }
}

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member, in an environment that exists")
}

/// Alice's project, ready for entries.
async fn alice_with_a_project(db: &TestDb) -> (Uuid, Uuid) {
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    (alice, project)
}

/// One committed-style entry in `tx`; the entry id and its first revision's.
async fn create(tx: &mut ProjectTransaction, title: &str, locale: Option<&str>) -> (Uuid, Uuid) {
    let entry = Uuid::now_v7();
    let draft = Content::new(title);
    entries::create(tx, entry, ARTICLES, draft.revision(), locale)
        .await
        .expect("created");
    (entry, draft.id)
}

/// A group's members as `(id, locale)` pairs.
async fn group(tx: &mut ProjectTransaction, entry: Uuid) -> Vec<(Uuid, Option<String>)> {
    entries::localizations(tx, ARTICLES, entry)
        .await
        .expect("localizations")
        .into_iter()
        .map(|sibling| (sibling.id, sibling.locale))
        .collect()
}

#[tokio::test]
async fn entry_locales() {
    let db = TestDb::new().await;
    let (alice, project) = alice_with_a_project(&db).await;
    let mut tx = enter(&db.app, alice, project).await;

    let (english, _) = create(&mut tx, "Hello", Some("en")).await;
    let (_, _) = create(&mut tx, "Bonjour", Some("fr")).await;
    let (plain, _) = create(&mut tx, "Untagged", None).await;

    // The tag reads back on the draft and on list summaries.
    let draft = entries::get_with_live(&mut tx, ARTICLES, english)
        .await
        .expect("get")
        .expect("exists")
        .0;
    assert_eq!(draft.locale.as_deref(), Some("en"));
    let summaries = entries::list(&mut tx, ARTICLES, None, 20, None, None, None)
        .await
        .expect("list");
    assert_eq!(summaries.len(), 3);
    assert_eq!(
        summaries
            .iter()
            .find(|row| row.entry_id == english)
            .and_then(|row| row.locale.as_deref()),
        Some("en")
    );

    // A list filters on the tag.
    let only_english = entries::list(&mut tx, ARTICLES, None, 20, None, None, Some("en"))
        .await
        .expect("list");
    assert_eq!(only_english.len(), 1);
    assert_eq!(only_english[0].entry_id, english);

    // The label changes and clears while the entry is ungrouped; a missing entry is Missing.
    assert_eq!(
        entries::set_locale(&mut tx, ARTICLES, english, Some("en-US"))
            .await
            .expect("set"),
        SetLocale::Set
    );
    assert_eq!(
        entries::set_locale(&mut tx, ARTICLES, plain, Some("pt-BR"))
            .await
            .expect("set"),
        SetLocale::Set
    );
    assert_eq!(
        entries::set_locale(&mut tx, ARTICLES, english, None)
            .await
            .expect("clear"),
        SetLocale::Set
    );
    assert_eq!(
        entries::set_locale(&mut tx, ARTICLES, Uuid::now_v7(), Some("en"))
            .await
            .expect("missing"),
        SetLocale::Missing
    );
    tx.commit().await.expect("commit");
}

#[tokio::test]
async fn localizations() {
    let db = TestDb::new().await;
    let (alice, project) = alice_with_a_project(&db).await;
    let mut tx = enter(&db.app, alice, project).await;

    let (source, _) = create(&mut tx, "Welcome", Some("en")).await;

    // Ungrouped, an entry lists no siblings; a missing source reads as absent.
    assert_eq!(group(&mut tx, source).await.len(), 0);
    assert!(
        entries::source_for_localization(&mut tx, ARTICLES, Uuid::now_v7())
            .await
            .expect("read")
            .is_none()
    );

    // The first sibling groups the source with it and copies the draft's content.
    let source_row = entries::source_for_localization(&mut tx, ARTICLES, source)
        .await
        .expect("read")
        .expect("exists");
    assert_eq!(source_row.localization_id, None);
    assert_eq!(source_row.locale.as_deref(), Some("en"));
    let copy = Content::new("Bienvenue");
    let french = Uuid::now_v7();
    let made =
        entries::create_localization(&mut tx, french, source, ARTICLES, "fr", copy.revision())
            .await
            .expect("localize");
    let Localized::Made(draft) = made else {
        panic!("expected a sibling, got {made:?}")
    };
    assert_eq!(draft.locale.as_deref(), Some("fr"));

    // The source joined the group; the list holds both, ordered by locale, self included.
    assert_eq!(
        group(&mut tx, source).await,
        [
            (source, Some("en".to_owned())),
            (french, Some("fr".to_owned()))
        ]
    );
    // The sibling sees the same group.
    assert_eq!(group(&mut tx, french).await.len(), 2);

    // A second version in a held locale names its holder; a missing source is Missing.
    let another = Uuid::now_v7();
    let occupied = entries::create_localization(
        &mut tx,
        another,
        source,
        ARTICLES,
        "fr",
        Content::new("Encore").revision(),
    )
    .await
    .expect("localize");
    assert!(
        matches!(occupied, Localized::Occupied(id) if id == french),
        "occupied answers the holding sibling, got {occupied:?}"
    );
    let missing = entries::create_localization(
        &mut tx,
        Uuid::now_v7(),
        Uuid::now_v7(),
        ARTICLES,
        "de",
        Content::new("Hallo").revision(),
    )
    .await
    .expect("localize");
    assert!(matches!(missing, Localized::Missing));

    // Grouped members keep a locale, and a free tag can move to a sibling's vacant one only.
    assert_eq!(
        entries::set_locale(&mut tx, ARTICLES, source, None)
            .await
            .expect("clear"),
        SetLocale::Grouped
    );
    assert_eq!(
        entries::set_locale(&mut tx, ARTICLES, source, Some("fr"))
            .await
            .expect("move"),
        SetLocale::Occupied(french)
    );
    assert_eq!(
        entries::set_locale(&mut tx, ARTICLES, source, Some("de"))
            .await
            .expect("move"),
        SetLocale::Set
    );
    tx.commit().await.expect("commit");
}

#[tokio::test]
async fn tombstoned_sibling_frees_its_locale() {
    let db = TestDb::new().await;
    let (alice, project) = alice_with_a_project(&db).await;
    let mut tx = enter(&db.app, alice, project).await;

    let (source, _) = create(&mut tx, "Welcome", Some("en")).await;
    let french = Uuid::now_v7();
    let made = entries::create_localization(
        &mut tx,
        french,
        source,
        ARTICLES,
        "fr",
        Content::new("Bienvenue").revision(),
    )
    .await
    .expect("localize");
    let Localized::Made(draft) = made else {
        panic!("expected a sibling, got {made:?}")
    };
    tx.commit().await.expect("commit");

    // Tombstoning the sibling frees its tag for a fresh version.
    let mut tx = enter(&db.app, alice, project).await;
    let deleted = entries::delete(&mut tx, ARTICLES, french, draft.revision.id)
        .await
        .expect("delete");
    assert!(matches!(deleted, Deleted::Deleted { .. }), "{deleted:?}");
    let again = Uuid::now_v7();
    let remade = entries::create_localization(
        &mut tx,
        again,
        source,
        ARTICLES,
        "fr",
        Content::new("Encore").revision(),
    )
    .await
    .expect("localize");
    assert!(matches!(remade, Localized::Made(_)), "{remade:?}");
    // The group lists the live members only.
    assert_eq!(
        group(&mut tx, source).await,
        [
            (source, Some("en".to_owned())),
            (again, Some("fr".to_owned()))
        ]
    );
    tx.commit().await.expect("commit");
}
