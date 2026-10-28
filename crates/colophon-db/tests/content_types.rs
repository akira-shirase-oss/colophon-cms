//! Content types against real PostgreSQL, as the application's login: the built-in articles
//! definition is the domain's, versions form a gapless insert-only chain, an update applies
//! only on the current version, revisions name their collection and schema version, and another
//! project sees nothing. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    PgPool,
    content_types::{self, ARTICLES_DEFINITION, Updated},
    projects,
    scope::{self, ProjectTransaction},
};
use colophon_domain::schema::ContentType;
use common::TestDb;
use uuid::Uuid;

const NOTES: &str = r#"{"name":"Notes","title_field":"title","fields":[{"id":"title","label":"Title","type":"text","required":true,"max_chars":200,"multiline":false}]}"#;
const NOTES_V2: &str = r#"{"name":"Notes","title_field":"title","fields":[{"id":"title","label":"Title","type":"text","required":true,"max_chars":200,"multiline":false},{"id":"done","label":"Done","type":"boolean","required":false}]}"#;

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

#[test]
fn the_built_in_definition_is_the_domain_s_canonical_articles() {
    assert_eq!(
        ARTICLES_DEFINITION,
        ContentType::articles().canonical_json()
    );
    let migration = include_str!("../migrations/0003_content_types.sql");
    assert!(
        migration.contains(&format!("(VALUES ('{ARTICLES_DEFINITION}'))")),
        "migration 0003 seeds the same bytes"
    );
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_new_project_starts_with_articles_hashed_as_the_api_hashes_it() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    let listed = content_types::list(&mut tx).await.expect("list");
    assert_eq!(listed.len(), 1);
    let articles = &listed[0];
    assert_eq!(
        (articles.collection.as_str(), articles.version),
        ("articles", 1)
    );
    assert_eq!(articles.definition, ARTICLES_DEFINITION);
    let (hash,): (Vec<u8>,) = sqlx::query_as("SELECT sha256(convert_to($1, 'UTF8'))")
        .bind(ARTICLES_DEFINITION)
        .fetch_one(&mut *tx.transaction)
        .await
        .expect("hash");
    assert_eq!(articles.definition_hash, hash);
    drop(tx);
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn versions_move_forward_one_at_a_time_from_the_current_one() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    let first = content_types::create(&mut tx, "notes", NOTES, &[1; 32])
        .await
        .expect("created");
    assert_eq!(first.version, 1);
    tx.commit().await.expect("commit");
    // A taken slug is a conflict (and aborts its own transaction, nothing else).
    let mut tx = enter(&db.app, alice, project).await;
    assert!(matches!(
        content_types::create(&mut tx, "notes", NOTES, &[1; 32]).await,
        Err(colophon_db::DbError::Conflict)
    ));
    drop(tx);

    let mut tx = enter(&db.app, alice, project).await;
    let Updated::Updated(second) = content_types::update(&mut tx, "notes", 1, NOTES_V2, &[2; 32])
        .await
        .expect("update")
    else {
        panic!("version 1 was current")
    };
    assert_eq!(second.version, 2);
    tx.commit().await.expect("commit");

    let mut tx = enter(&db.app, alice, project).await;
    assert_eq!(
        content_types::update(&mut tx, "notes", 1, NOTES_V2, &[3; 32])
            .await
            .expect("update"),
        Updated::Stale { current: 2 },
        "an update on an old version writes nothing"
    );
    assert_eq!(
        content_types::update(&mut tx, "pages", 1, NOTES, &[3; 32])
            .await
            .expect("update"),
        Updated::NotFound
    );
    let current = content_types::current(&mut tx, "notes")
        .await
        .expect("current")
        .expect("exists");
    assert_eq!(
        (current.version, current.definition.as_str()),
        (2, NOTES_V2)
    );
    drop(tx);
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn versions_cannot_be_rewritten_skipped_or_seen_from_another_project() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    for sql in [
        "UPDATE content_type_versions SET definition = '{}'",
        "DELETE FROM content_type_versions",
        "DELETE FROM collections",
    ] {
        let mut tx = enter(&db.app, alice, project).await;
        let error = sqlx::query(sql)
            .execute(&mut *tx.transaction)
            .await
            .expect_err("no grant");
        assert!(
            error.to_string().contains("permission denied"),
            "{sql}: {error}"
        );
    }
    // A version that skips one has no previous version to name.
    let mut tx = enter(&db.app, alice, project).await;
    let error = sqlx::query(
        "INSERT INTO content_type_versions (project_id, environment_id, collection, version, previous_version, definition, definition_hash)
         VALUES ($1, $2, 'articles', 3, 2, '{}', $3)",
    )
    .bind(tx.project)
    .bind(tx.environment)
    .bind(vec![0_u8; 32])
    .execute(&mut *tx.transaction)
    .await
    .expect_err("version 2 doesn't exist");
    assert!(error.to_string().contains("foreign key"), "{error}");
    drop(tx);

    let mut own = enter(&db.app, alice, project).await;
    content_types::create(&mut own, "notes", NOTES, &[1; 32])
        .await
        .expect("created");
    own.commit().await.expect("commit");
    let bob = db.administrator("bob").await;
    let beta = projects::create(&db.app, bob, "beta", "Beta")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, bob, beta).await;
    let visible: Vec<String> = content_types::list(&mut tx)
        .await
        .expect("list")
        .into_iter()
        .map(|version| version.collection)
        .collect();
    assert_eq!(
        visible,
        ["articles"],
        "bob sees only his own project's types"
    );
    assert_eq!(
        content_types::current(&mut tx, "notes")
            .await
            .expect("current"),
        None
    );
    drop(tx);
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_revision_names_its_entry_s_collection_and_a_version_that_exists() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    content_types::create(&mut tx, "notes", NOTES, &[1; 32])
        .await
        .expect("created");
    let entry = Uuid::now_v7();
    let first = Uuid::now_v7();
    colophon_db::entries::create(
        &mut tx,
        entry,
        "notes",
        colophon_db::entries::NewRevision {
            id: first,
            content: r#"{"title":"One"}"#,
            content_hash: &[4; 32],
            title: "One",
            slug: None,
            schema_version: 1,
            references: &[],
            unique: &[],
        },
    )
    .await
    .expect("created");
    tx.commit().await.expect("commit");

    for (collection, version, why) in [
        ("articles", 1, "another collection"),
        ("notes", 9, "no such version"),
    ] {
        let mut tx = enter(&db.app, alice, project).await;
        let error = sqlx::query(
            "INSERT INTO revisions (project_id, environment_id, entry_id, id, base_revision_id, content, content_hash, title, author_id, collection, schema_version)
             VALUES ($1, $2, $3, $4, $5, '{}', $6, 'x', $7, $8, $9)",
        )
        .bind(tx.project)
        .bind(tx.environment)
        .bind(entry)
        .bind(Uuid::now_v7())
        .bind(first)
        .bind(vec![0_u8; 32])
        .bind(alice)
        .bind(collection)
        .bind(version)
        .execute(&mut *tx.transaction)
        .await
        .expect_err(why);
        assert!(error.to_string().contains("foreign key"), "{why}: {error}");
    }
    db.drop().await;
}
