//! Changesets and reviews against real PostgreSQL, as the application's login (D-044): items pin
//! an entry's own revisions only, reviews can't be changed or removed and are numbered in the
//! order they're recorded, and another project's changesets don't exist. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    PgPool, changesets,
    entries::{self, NewRevision},
    projects,
    scope::{self, ProjectTransaction},
};
use common::TestDb;
use uuid::Uuid;

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

/// A new article in the transaction's environment; its id and revision.
async fn article(tx: &mut ProjectTransaction, title: &str) -> (Uuid, Uuid) {
    let (entry, revision) = (Uuid::now_v7(), Uuid::now_v7());
    let hash = [7_u8; 32];
    let content = format!(r#"{{"title":"{title}"}}"#);
    entries::create(
        tx,
        entry,
        "articles",
        NewRevision {
            id: revision,
            content: &content,
            content_hash: &hash,
            title,
            schema_version: 1,
        },
    )
    .await
    .expect("created");
    (entry, revision)
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn reviews_are_numbered_kept_and_private_to_their_project() {
    let db = TestDb::new().await;
    let (alice, mallory) = (
        db.administrator("alice").await,
        db.administrator("mallory").await,
    );
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let theirs = projects::create(&db.app, mallory, "beta", "Beta")
        .await
        .expect("created")
        .id;

    let mut tx = enter(&db.app, alice, project).await;
    let (first, _) = article(&mut tx, "First").await;
    let (second, second_revision) = article(&mut tx, "Second").await;
    let pins = changesets::pins(&mut tx, &[first, second, Uuid::now_v7()])
        .await
        .expect("pins");
    assert_eq!(pins.len(), 2, "an unknown entry has no pin");
    let id = Uuid::now_v7();
    changesets::create(&mut tx, id, "Launch", &pins, &[1; 32])
        .await
        .expect("created");
    changesets::submit(&mut tx, id).await.expect("submitted");
    for decision in ["request_changes", "approve", "approve"] {
        changesets::lock(&mut tx, id)
            .await
            .expect("lock")
            .expect("found");
        changesets::review(&mut tx, id, &[1; 32], decision, "")
            .await
            .expect("reviewed");
    }
    let reviews = changesets::reviews(&mut tx, id, None)
        .await
        .expect("reviews");
    assert_eq!(
        reviews
            .iter()
            .map(|review| (review.seq, review.decision.as_str()))
            .collect::<Vec<_>>(),
        [(3, "approve"), (2, "approve"), (1, "request_changes")],
        "newest first, numbered in the order recorded"
    );
    tx.commit().await.expect("commit");

    // Reviews and pins can't be rewritten by the application.
    for statement in [
        "UPDATE reviews SET decision = 'approve'",
        "DELETE FROM reviews",
        "DELETE FROM changeset_items",
    ] {
        let mut tx = enter(&db.app, alice, project).await;
        let error = sqlx::query(statement)
            .execute(&mut *tx.transaction)
            .await
            .expect_err(statement);
        assert!(
            format!("{error:?}").contains("permission denied"),
            "{statement}: {error:?}"
        );
    }

    // An item can't pin another entry's revision.
    let mut tx = enter(&db.app, alice, project).await;
    let other = Uuid::now_v7();
    changesets::create(&mut tx, other, "Mixed up", &[], &[2; 32])
        .await
        .expect("created");
    let error = sqlx::query(
        "INSERT INTO changeset_items (project_id, environment_id, changeset_id, entry_id, revision_id)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(tx.project)
    .bind(tx.environment)
    .bind(other)
    .bind(first)
    .bind(second_revision)
    .execute(&mut *tx.transaction)
    .await
    .expect_err("a revision of another entry");
    assert!(format!("{error:?}").contains("foreign key"), "{error:?}");
    drop(tx);

    // Another project's member sees no changesets here, and can't reach this one by id.
    let mut tx = enter(&db.app, mallory, theirs).await;
    assert_eq!(changesets::list(&mut tx).await.expect("list"), Vec::new());
    assert_eq!(changesets::get(&mut tx, id).await.expect("get"), None);
    drop(tx);
    db.drop().await;
}

/// Regression (CI, 2026-10-26): Postgres settled on one generic plan for the list while `reviews`
/// was nearly empty (a full scan of it for each of the page's 100 changesets), then kept that plan
/// while the table grew to 3,000 rows: the list took 200 ms. It is planned afresh on every run
/// now, with the tables as they are at that moment.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_changeset_list_is_planned_afresh_on_every_run() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    // More runs than Postgres needs before it switches a prepared statement to a generic plan.
    for _ in 0..8 {
        changesets::list(&mut tx).await.expect("listed");
    }
    let (generic, custom): (i64, i64) = sqlx::query_as(
        "SELECT generic_plans, custom_plans FROM pg_prepared_statements
         WHERE statement LIKE '%FROM changesets c%LATERAL%' AND statement NOT LIKE '%pg_prepared_statements%'",
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .expect("the list is a prepared statement");
    assert_eq!((generic, custom), (0, 8), "(generic plans, custom plans)");
    tx.commit().await.expect("commit");
    db.drop().await;
}
