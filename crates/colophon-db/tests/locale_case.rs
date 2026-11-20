//! Migration 0077 (D-212) against real PostgreSQL: a localization group the old schema let hold `fr`
//! and `FR` makes it refuse, by name and naming the group; once one is deleted it finishes, stored
//! tags are in BCP 47's case, and its trigger recases every tag written by any role, so the group
//! index refuses a tag that differs from a sibling's only in case. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    PgPool,
    entries::{self, NewRevision},
    projects,
    scope::{self, ProjectTransaction},
};
use common::TestDb;
use uuid::Uuid;

const ARTICLES: &str = "articles";

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member, in an environment that exists")
}

/// One entry, tagged `locale`.
async fn create(tx: &mut ProjectTransaction, title: &str, locale: &str) -> Uuid {
    let entry = Uuid::now_v7();
    let revision = Uuid::now_v7();
    let text = format!(r#"{{"title":"{title}","body":{{}}}}"#);
    let hash = *revision
        .as_bytes()
        .repeat(2)
        .first_chunk()
        .expect("32 bytes");
    entries::create(
        tx,
        entry,
        ARTICLES,
        NewRevision {
            id: revision,
            content: &text,
            content_hash: &hash,
            title,
            slug: None,
            schema_version: 1,
            references: &[],
            assets: &[],
            unique: &[],
            visibility: None,
        },
        Some(locale),
    )
    .await
    .expect("created");
    entry
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn case_only_duplicates_stop_the_migration_until_one_is_deleted() {
    let db = TestDb::before(77).await;
    // The old schema's rows are written through the Rust API, whose begin needs 0079 (D-227).
    db.lend_enter_functions().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    let english = create(&mut tx, "Tides", "en").await;
    let french = create(&mut tx, "Marées", "fr").await;
    let shouting = create(&mut tx, "MARÉES", "FR").await;
    let brazil = create(&mut tx, "Marés", "pt-br").await;
    tx.commit().await.expect("commit");
    // One group, as the old case-sensitive index allowed: en, fr, FR and pt-br.
    let owner = db.owner().await;
    let group = Uuid::now_v7();
    sqlx::query("UPDATE entries SET localization_id = $1 WHERE id = ANY($2)")
        .bind(group)
        .bind(vec![english, french, shouting, brazil])
        .execute(&owner)
        .await
        .expect("grouped under the old schema");

    db.take_back_enter_functions().await;

    // 0077 refuses by name, naming the group and both tags, and keeps nothing.
    let refused = colophon_db::migrate(&owner).await.expect_err("refused");
    let said = format!("{refused:?}");
    assert!(said.contains("locale_case_duplicates"), "{said}");
    assert!(said.contains(&group.to_string()), "{said}");
    assert!(said.contains("FR") && said.contains("fr"), "{said}");
    let kept: String = sqlx::query_scalar("SELECT locale FROM entries WHERE id = $1")
        .bind(brazil)
        .fetch_one(&owner)
        .await
        .expect("read");
    assert_eq!(kept, "pt-br", "a refused migration rewrites nothing");
    // sqlx keeps its migration lock on a connection when a migration fails (the CLI's process exits
    // and drops it); this test goes on in-process, so it starts again on a fresh pool.
    owner.close().await;
    let owner = db.owner().await;

    // One of the two is deleted (as the API's delete tombstones it); the migrations then finish.
    sqlx::query("UPDATE entries SET deleted_at = now(), deleted_by = $2 WHERE id = $1")
        .bind(shouting)
        .bind(alice)
        .execute(&owner)
        .await
        .expect("tombstoned");
    colophon_db::migrate(&owner)
        .await
        .expect("the migrations finish");
    let tags: Vec<String> = sqlx::query_scalar(
        "SELECT locale FROM entries WHERE localization_id = $1 AND deleted_at IS NULL ORDER BY locale",
    )
    .bind(group)
    .fetch_all(&owner)
    .await
    .expect("read");
    assert_eq!(tags, ["en", "fr", "pt-BR"], "stored in BCP 47's case");

    // The trigger recases what any role writes: the table owner here, outside the API.
    sqlx::query("UPDATE entries SET locale = 'EN-gb' WHERE id = $1")
        .bind(english)
        .execute(&owner)
        .await
        .expect("updated");
    let recased: String = sqlx::query_scalar("SELECT locale FROM entries WHERE id = $1")
        .bind(english)
        .fetch_one(&owner)
        .await
        .expect("read");
    assert_eq!(recased, "en-GB");
    // …and the application's login, through the Rust API, which passes the tag on as given.
    let mut tx = enter(&db.app, alice, project).await;
    let mexico = create(&mut tx, "Mareas", "Es-mx").await;
    tx.commit().await.expect("commit");
    let stored: String = sqlx::query_scalar("SELECT locale FROM entries WHERE id = $1")
        .bind(mexico)
        .fetch_one(&owner)
        .await
        .expect("read");
    assert_eq!(stored, "es-MX");

    // So a sibling that differs only in case meets 0034's index as it is, even from the owner.
    let again = sqlx::query("UPDATE entries SET locale = 'Fr' WHERE id = $1")
        .bind(english)
        .execute(&owner)
        .await
        .expect_err("refused by the group index");
    let constraint = again
        .as_database_error()
        .and_then(|error| error.constraint().map(str::to_owned));
    assert_eq!(constraint.as_deref(), Some("entries_one_locale_per_group"));
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_database_and_the_domain_agree_on_every_tag() {
    let db = TestDb::new().await;
    let owner = db.owner().await;
    for tag in [
        "EN",
        "pt-br",
        "ZH-HANT-tw",
        "es-419",
        "de-ch-1901",
        "EN-us-U-CA-Gregory",
        "en-X-Legal",
        "iw",
        "x-AB",
        "i-KLINGON",
    ] {
        let stored: String = sqlx::query_scalar("SELECT colophon_canonical_locale($1)")
            .bind(tag)
            .fetch_one(&owner)
            .await
            .expect("canonical");
        assert_eq!(stored, colophon_domain::locale::canonical(tag), "{tag}");
    }
    // A tag that starts with a singleton is lowercase throughout (RFC 5646 §2.1.1).
    for (tag, expected) in [("x-AB", "x-ab"), ("i-KLINGON", "i-klingon")] {
        let stored: String = sqlx::query_scalar("SELECT colophon_canonical_locale($1)")
            .bind(tag)
            .fetch_one(&owner)
            .await
            .expect("canonical");
        assert_eq!(stored, expected, "{tag}");
    }
    owner.close().await;
    db.drop().await;
}
