//! The work tables' sweeps never wait on a row another transaction holds, so they can never be
//! part of a lock cycle (CI 44719897863: two `jobs::expire` sweeps and a release's supersede,
//! each a multi-row UPDATE locking rows in its own scan order, deadlocked on `jobs` tuples). The
//! cycle is reproduced deterministically: a transaction holds the later of two expired jobs, the
//! sweep runs, and the holder then takes the earlier job, as a supersede does. A sweep that waits
//! has locked the earlier job and waits on the later: the holder's next lock closes the cycle,
//! and one of them is aborted with 40P01. A sweep that skips held rows has expired the earlier
//! job, committed and gone. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::time::{Duration, Instant};

use colophon_db::{
    PgPool, assets, changesets, destinations,
    entries::{self, NewRevision},
    jobs, projects, releases,
    scope::{self, ProjectTransaction},
};
use common::TestDb;
use time::OffsetDateTime;
use uuid::Uuid;

const HOUR: Duration = Duration::from_secs(3600);

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

/// A project of alice's with two webhook destinations and one release: two pending delivery
/// jobs, in id order (the release inserts them in one statement, so heap order agrees).
async fn two_jobs(db: &TestDb) -> (Uuid, Uuid) {
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    for n in 0..2 {
        destinations::create(
            &mut tx,
            "webhook",
            &format!("hook {n}"),
            &format!("https://hooks.example.com/alpha/{n}"),
            None,
        )
        .await
        .expect("destination");
    }
    let (entry, revision) = (Uuid::now_v7(), Uuid::now_v7());
    entries::create(
        &mut tx,
        entry,
        "articles",
        NewRevision {
            id: revision,
            content: r#"{"title":"Hi"}"#,
            content_hash: &[1; 32],
            title: "Hi",
            slug: None,
            schema_version: 1,
            references: &[],
            assets: &[],
            unique: &[],
            visibility: None,
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
    tx.commit().await.expect("commit");
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM jobs ORDER BY id")
        .fetch_all(&db.owner().await)
        .await
        .expect("jobs");
    assert_eq!(ids.len(), 2, "one job per destination");
    (ids[0], ids[1])
}

/// Whether a backend runs a statement starting with `prefix` and waits on a lock.
async fn waiting(owner: &PgPool, prefix: &str) -> bool {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_stat_activity
                        WHERE wait_event_type = 'Lock' AND ltrim(query) LIKE $1 || '%')",
    )
    .bind(prefix)
    .fetch_one(owner)
    .await
    .expect("pg_stat_activity")
}

/// Holds `later` in a transaction of its own, runs `sweep`, and once the sweep has either
/// finished or been seen waiting, takes `earlier` in the holder (a supersede's next row). Both
/// must succeed: neither side may be aborted for a deadlock, and the sweep must not have waited.
async fn sweep_beside_a_holder<F, Fut>(
    owner: &PgPool,
    table: &str,
    earlier: Uuid,
    later: Uuid,
    sweep: F,
    statement: &str,
) -> u64
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<u64, colophon_db::DbError>> + Send + 'static,
{
    let mut holder = owner.begin().await.expect("begin");
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT 1 FROM {table} WHERE id = $1 FOR UPDATE"
    )))
    .bind(later)
    .execute(&mut *holder)
    .await
    .expect("hold the later row");
    let swept = tokio::spawn(sweep());
    let started = Instant::now();
    let mut waited = false;
    while !swept.is_finished() && started.elapsed() < Duration::from_secs(5) {
        if waiting(owner, statement).await {
            waited = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let took = sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE {table} SET updated_at = now() WHERE id = $1"
    )))
    .bind(earlier)
    .execute(&mut *holder)
    .await;
    let holder_result = match took {
        Ok(_) => holder.commit().await,
        Err(error) => Err(error),
    };
    let sweep_result = tokio::time::timeout(Duration::from_secs(10), swept)
        .await
        .expect("the sweep ends")
        .expect("the sweep's task");
    assert!(
        holder_result.is_ok() && sweep_result.is_ok(),
        "a lock cycle: the holder {holder_result:?}, the sweep {sweep_result:?}"
    );
    assert!(!waited, "the sweep waited on a row the holder holds");
    sweep_result.expect("checked")
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_delivery_sweep_never_waits_on_a_held_job() {
    let db = TestDb::new().await;
    let (earlier, later) = two_jobs(&db).await;
    let owner = db.owner().await;
    // Both ran out of time, the earlier first, so every plan the sweep can take visits it first.
    sqlx::query(
        "UPDATE jobs SET deadline_at = now() - CASE WHEN id = $1 THEN interval '2 seconds'
                                                   ELSE interval '1 second' END",
    )
    .bind(earlier)
    .execute(&owner)
    .await
    .expect("expired");
    let pool = db.app.clone();
    let swept = sweep_beside_a_holder(
        &owner,
        "jobs",
        earlier,
        later,
        move || async move { jobs::expire(&pool).await },
        "UPDATE jobs SET status = 'failed'",
    )
    .await;
    assert_eq!(swept, 1, "the held job is left for the next pass");
    assert_eq!(
        jobs::expire(&db.app).await.expect("next pass"),
        1,
        "the next pass takes it"
    );
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_asset_sweep_never_waits_on_a_held_job() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    for n in 0..2 {
        let mut tx = enter(&db.app, alice, project).await;
        assets::insert(
            &mut tx,
            Uuid::now_v7(),
            &format!("a{n}.png"),
            "image/png",
            1_024,
            &format!("quarantine/{n}"),
            alice,
            Uuid::now_v7(),
            &format!("verify:{n}"),
            OffsetDateTime::now_utc() + HOUR,
        )
        .await
        .expect("uploaded");
        tx.commit().await.expect("commit");
    }
    let owner = db.owner().await;
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM asset_jobs ORDER BY id")
        .fetch_all(&owner)
        .await
        .expect("asset jobs");
    let (earlier, later) = (ids[0], ids[1]);
    sqlx::query(
        "UPDATE asset_jobs SET deadline_at = now() - CASE WHEN id = $1 THEN interval '2 seconds'
                                                         ELSE interval '1 second' END",
    )
    .bind(earlier)
    .execute(&owner)
    .await
    .expect("expired");
    let pool = db.app.clone();
    let swept = sweep_beside_a_holder(
        &owner,
        "asset_jobs",
        earlier,
        later,
        move || async move { assets::expire(&pool).await },
        "UPDATE asset_jobs SET status = 'failed'",
    )
    .await;
    assert_eq!(swept, 1, "the held job is left for the next pass");
    assert_eq!(assets::expire(&db.app).await.expect("next pass"), 1);
    owner.close().await;
    db.drop().await;
}
