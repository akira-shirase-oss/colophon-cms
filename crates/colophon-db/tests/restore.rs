//! The restore procedure's database half against real PostgreSQL (migration 0049, D-126;
//! module 18 "Restore procedure"; O2, O3; F13): a template-cloned "restored backup" checks
//! its schema against this binary's migrations, registers a recovery epoch on every
//! environment, freezes the unfinished outbox as `held` — a state no claim, retry or expiry
//! query reads — and only the operator's explicit resume returns those jobs to `pending`.
//! F14's replay-onto-a-clone half lives in tests/ledger.rs. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::time::Duration;

use colophon_db::{
    PgPool, changesets, destinations,
    entries::{self, NewRevision},
    jobs, projects, releases, restore,
    scope::{self, ProjectTransaction},
};
use common::{TestDb, host};
use sqlx::{AssertSqlSafe, postgres::PgPoolOptions};
use uuid::Uuid;

const HOUR: Duration = Duration::from_secs(3600);

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

/// A project with `destination_count` webhooks and one committed release; alice, the project
/// and its 'production' environment.
async fn released(db: &TestDb, destination_count: usize) -> (Uuid, Uuid, Uuid) {
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    for n in 0..destination_count {
        destinations::create(
            &mut tx,
            "webhook",
            &format!("hook {n}"),
            &format!("https://hooks.example.com/{n}"),
            None,
        )
        .await
        .expect("destination");
    }
    let entry = Uuid::now_v7();
    entries::create(
        &mut tx,
        entry,
        "articles",
        NewRevision {
            id: Uuid::now_v7(),
            content: r#"{"title":"Hi"}"#,
            content_hash: &[1; 32],
            title: "Hi",
            slug: None,
            schema_version: 1,
            references: &[],
            assets: &[],
            unique: &[],
        },
        None,
    )
    .await
    .expect("entry");
    let pins = changesets::pins(&mut tx, &[entry]).await.expect("pins");
    let changeset = Uuid::now_v7();
    changesets::create(&mut tx, changeset, "One", &pins, &[2; 32], None)
        .await
        .expect("changeset");
    changesets::submit(&mut tx, changeset)
        .await
        .expect("submitted");
    changesets::lock(&mut tx, changeset).await.expect("lock");
    changesets::review(&mut tx, changeset, &[2; 32], "approve", "", None)
        .await
        .expect("approved");
    let review = changesets::reviews(&mut tx, changeset, None)
        .await
        .expect("reviews")[0]
        .id;
    releases::commit(&mut tx, 0, changeset, &[2; 32], review, HOUR)
        .await
        .expect("released");
    let environment = tx.environment;
    tx.commit().await.expect("commit");
    (alice, project, environment)
}

/// Every environment's `(project, environment, epoch, generation)`.
async fn epochs(pool: &PgPool) -> Vec<(Uuid, Uuid, i64, i64)> {
    sqlx::query_as("SELECT project_id, id, epoch, generation FROM environments ORDER BY id")
        .fetch_all(pool)
        .await
        .expect("environments")
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_restored_backup_gets_a_new_epoch_and_a_held_outbox() {
    let db = TestDb::new().await;
    let (_alice, project, environment) = released(&db, 2).await;

    // Both jobs sit in the outbox; one is claimed and in flight when the "backup" is taken —
    // its delivery may already have landed, and nobody can tell (F13).
    let in_flight = jobs::claim(&db.app, "worker-gone", "test", HOUR, 1)
        .await
        .expect("claim");
    assert_eq!(in_flight.len(), 1, "one job under lease at backup time");
    let owner = db.owner().await;
    let before = epochs(&owner).await;
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].2, 1, "the pre-restore epoch");

    // The "backup": a template clone as it is now. Pools close so no connection hangs on it.
    db.app.close().await;
    owner.close().await;
    let backup = format!("colophon_test_{}", Uuid::now_v7().simple());
    sqlx::query(AssertSqlSafe(format!(
        "CREATE DATABASE {backup} TEMPLATE {}",
        db.name
    )))
    .execute(&db.admin)
    .await
    .expect("the backup clone");
    let restored = PgPoolOptions::new()
        .max_connections(2)
        .connect(&format!(
            "postgres://colophon:colophon-dev@{}/{backup}",
            host()
        ))
        .await
        .expect("connect to the restored backup");

    // Step 2: the clone applied exactly what this binary knows — nothing newer, nothing pending.
    let applied = restore::applied_migrations(&restored)
        .await
        .expect("applied migrations");
    assert_eq!(
        restore::check_schema(&applied),
        restore::SchemaCheck::Current
    );

    // Step 3: the recovery epoch — every environment advances exactly one past the backup's.
    let bumped = restore::advance_epoch(&restored).await.expect("new epoch");
    assert_eq!(bumped.len(), 1);
    assert_eq!(bumped[0].project_id, project);
    assert_eq!(bumped[0].environment_id, environment);
    assert_eq!(
        bumped[0].epoch, 2,
        "epoch advanced once past the backup's 1"
    );
    let now = epochs(&restored).await;
    assert_eq!(now[0].2, 2);

    // Step 5: every unfinished job freezes as 'held' — the in-flight one included.
    let held = restore::hold_jobs(&restored).await.expect("hold");
    assert_eq!(held.len(), 2);
    let mut was: Vec<&str> = held.iter().map(|job| job.was.as_str()).collect();
    was.sort_unstable();
    assert_eq!(
        was,
        ["pending", "running"],
        "one queued job and the in-flight one"
    );
    let (held_count, leased): (i64, i64) =
        sqlx::query_as("SELECT count(*), count(lease_owner) FROM jobs WHERE status = 'held'")
            .fetch_one(&restored)
            .await
            .expect("held rows");
    assert_eq!(
        (held_count, leased),
        (2, 0),
        "held, and the dead worker's lease is gone"
    );

    // O3: nothing may dispatch before reconciliation — no worker can claim a held job,
    // and the expiry sweep leaves held jobs alone.
    let claimable = jobs::claim(&restored, "worker", "test", HOUR, 10)
        .await
        .expect("claim on the restored backup");
    assert!(claimable.is_empty(), "a held job can never be claimed");
    assert_eq!(jobs::expire(&restored).await.expect("expire"), 0);
    let again = restore::hold_jobs(&restored).await.expect("hold again");
    assert!(again.is_empty(), "holding is idempotent");

    // The reconciliation list the operator works through.
    let outbox = restore::outbox(&restored).await.expect("outbox");
    assert_eq!(outbox.held.len(), 2);
    assert_eq!(outbox.jobs, [("held".to_owned(), 2)]);

    // The explicit enable: jobs return to pending, due now — then a claim succeeds.
    let resumed = restore::resume_jobs(&restored, 3600).await.expect("resume");
    assert_eq!(resumed.len(), 2);
    let claimable = jobs::claim(&restored, "worker", "test", HOUR, 10)
        .await
        .expect("claim after resume");
    assert_eq!(claimable.len(), 2, "resumed jobs dispatch as usual");

    sqlx::query(AssertSqlSafe(format!(
        "DROP DATABASE {backup} WITH (FORCE)"
    )))
    .execute(&db.admin)
    .await
    .expect("drop the backup");
    db.drop().await;
}
