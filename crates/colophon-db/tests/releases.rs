//! Releases and delivery against real PostgreSQL (D-045): a release commits only from its base
//! generation, publication history can't overlap even when written by hand, and the delivery
//! role reads published content and nothing else. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    PgPool, changesets, delivery,
    entries::{self, NewRevision},
    projects,
    releases::{self, Committed},
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

/// A project with one article pinned into an approved changeset; (project, entry, changeset, review).
async fn ready(db: &TestDb) -> (Uuid, Uuid, Uuid, Uuid, Uuid) {
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    let (entry, revision) = (Uuid::now_v7(), Uuid::now_v7());
    entries::create(
        &mut tx,
        entry,
        "articles",
        NewRevision {
            id: revision,
            content: r#"{"title":"Hello"}"#,
            content_hash: &[3; 32],
            title: "Hello",
            schema_version: 1,
        },
    )
    .await
    .expect("created");
    let pins = changesets::pins(&mut tx, &[entry]).await.expect("pins");
    let changeset = Uuid::now_v7();
    changesets::create(&mut tx, changeset, "One", &pins, &[5; 32])
        .await
        .expect("created");
    changesets::submit(&mut tx, changeset)
        .await
        .expect("submitted");
    changesets::lock(&mut tx, changeset).await.expect("lock");
    changesets::review(&mut tx, changeset, &[5; 32], "approve", "")
        .await
        .expect("approved");
    let review = changesets::reviews(&mut tx, changeset, None)
        .await
        .expect("reviews")[0]
        .id;
    tx.commit().await.expect("commit");
    (alice, project, entry, changeset, review)
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_release_commits_once_from_its_base_and_history_never_overlaps() {
    let db = TestDb::new().await;
    let (alice, project, entry, changeset, review) = ready(&db).await;
    let mut tx = enter(&db.app, alice, project).await;
    assert_eq!(
        releases::commit(&mut tx, 3, changeset, &[5; 32], review)
            .await
            .expect("commit"),
        Committed::Moved { current: 0 },
        "a base the environment never had"
    );
    assert_eq!(
        releases::commit(&mut tx, 0, changeset, &[5; 32], review)
            .await
            .expect("commit"),
        Committed::Done {
            generation: 1,
            epoch: 1,
            changed: 1
        }
    );
    tx.commit().await.expect("commit");

    // Below the API: an overlapping interval is refused by the database itself.
    let owner = db.owner().await;
    let error = sqlx::query(
        "INSERT INTO publications (project_id, environment_id, entry_id, revision_id, collection, title, content, content_hash, valid_from)
         SELECT project_id, environment_id, entry_id, revision_id, collection, title, content, content_hash, valid_from
         FROM publications WHERE entry_id = $1",
    )
    .bind(entry)
    .execute(&owner)
    .await
    .expect_err("a second open interval for one entry");
    assert!(
        format!("{error:?}").contains("23P01")
            || format!("{error:?}").contains("exclusion")
            || format!("{error:?}").contains("duplicate"),
        "{error:?}"
    );
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn delivery_reads_published_content_and_nothing_else() {
    let db = TestDb::new().await;
    let (alice, project, entry, changeset, review) = ready(&db).await;
    let mut tx = enter(&db.app, alice, project).await;
    releases::commit(&mut tx, 0, changeset, &[5; 32], review)
        .await
        .expect("commit");
    tx.commit().await.expect("commit");

    let mut delivered = delivery::begin(&db.app, project).await.expect("begin");
    let environment = delivered
        .environment("production")
        .await
        .expect("environment")
        .expect("found");
    assert_eq!(environment.generation, 1);
    let published = delivered
        .list(environment.id, "articles", 1, None, 10)
        .await
        .expect("list");
    assert_eq!(
        published.iter().map(|row| row.entry_id).collect::<Vec<_>>(),
        [entry]
    );
    delivered.finish().await.expect("finish");

    for statement in [
        "SELECT count(*) FROM revisions",
        "SELECT count(*) FROM entries",
        "SELECT count(*) FROM changesets",
        "SELECT count(*) FROM memberships",
        "SELECT count(*) FROM sessions",
        "SELECT count(*) FROM identities",
        "UPDATE publications SET title = 'defaced'",
        "UPDATE environments SET generation = 99",
    ] {
        let mut delivered = delivery::begin(&db.app, project).await.expect("begin");
        let error = sqlx::query(statement)
            .execute(&mut **delivered.raw())
            .await
            .expect_err(statement);
        assert!(
            format!("{error:?}").contains("permission denied"),
            "{statement}: {error:?}"
        );
    }

    // Another project's id sees nothing of this one.
    let mut elsewhere = delivery::begin(&db.app, Uuid::now_v7())
        .await
        .expect("begin");
    assert_eq!(
        elsewhere
            .environment("production")
            .await
            .expect("environment"),
        None
    );
    elsewhere.finish().await.expect("finish");
    db.drop().await;
}
