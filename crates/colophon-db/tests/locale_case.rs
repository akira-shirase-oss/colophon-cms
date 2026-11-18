//! Migrations 0077–0079 (D-212) against real PostgreSQL: a localization group the old schema let
//! hold `fr` and `FR` makes 0077 refuse, by name and naming the group; once one is deleted the
//! migrations finish, stored tags are in BCP 47's case, and the database itself refuses a tag that
//! differs from a sibling's only in case. Needs the local stack.
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
    let hash = *revision.as_bytes().repeat(2).first_chunk().expect("32 bytes");
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

    // One of the two is deleted (as the API's delete tombstones it); the migrations then finish.
    sqlx::query("UPDATE entries SET deleted_at = now() WHERE id = $1")
        .bind(shouting)
        .execute(&owner)
        .await
        .expect("tombstoned");
    colophon_db::migrate(&owner).await.expect("the migrations finish");
    let tags: Vec<String> = sqlx::query_scalar(
        "SELECT locale FROM entries WHERE localization_id = $1 AND deleted_at IS NULL ORDER BY locale",
    )
    .bind(group)
    .fetch_all(&owner)
    .await
    .expect("read");
    assert_eq!(tags, ["en", "fr", "pt-BR"], "stored in BCP 47's case");

    // The database itself now refuses a sibling that differs only in case, even as the table owner.
    let again = sqlx::query("UPDATE entries SET locale = 'Fr' WHERE id = $1")
        .bind(english)
        .execute(&owner)
        .await
        .expect_err("refused by the index");
    let constraint = again
        .as_database_error()
        .and_then(|error| error.constraint().map(str::to_owned));
    assert_eq!(constraint.as_deref(), Some("entries_one_locale_per_group_ci"));
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_database_and_the_domain_agree_on_every_tag() {
    let db = TestDb::new().await;
    let owner = db.owner().await;
    for tag in ["EN", "pt-br", "ZH-HANT-tw", "es-419", "de-ch-1901", "EN-us-U-CA-Gregory", "en-X-Legal", "iw"] {
        let stored: String = sqlx::query_scalar("SELECT colophon_canonical_locale($1)")
            .bind(tag)
            .fetch_one(&owner)
            .await
            .expect("canonical");
        assert_eq!(stored, colophon_domain::locale::canonical(tag), "{tag}");
    }
    owner.close().await;
    db.drop().await;
}
