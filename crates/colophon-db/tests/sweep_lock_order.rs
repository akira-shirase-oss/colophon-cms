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

/// A project of alice's with `destinations` webhook destinations: (alice, the project).
async fn project_with(db: &TestDb, destinations: usize) -> (Uuid, Uuid) {
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    for n in 0..destinations {
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
    tx.commit().await.expect("commit");
    (alice, project)
}

/// Publishes one more release of a new entry in alice's project: one pending job per
/// destination, and every older job that hasn't started superseded (the release's outbox, which
/// waits for each older job it supersedes, in id order).
async fn publish(pool: &PgPool, alice: Uuid, project: Uuid) {
    let mut tx = enter(pool, alice, project).await;
    let (_, base) = releases::generation(&mut tx).await.expect("the generation");
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
    releases::commit(&mut tx, base, changeset, &[2; 32], review, HOUR)
        .await
        .expect("released");
    tx.commit().await.expect("commit");
}

/// A project of alice's with two webhook destinations and one release: two pending delivery
/// jobs, in id order (the release inserts them in one statement, so heap order agrees).
async fn two_jobs(db: &TestDb) -> (Uuid, Uuid) {
    let (alice, project) = project_with(db, 2).await;
    publish(&db.app, alice, project).await;
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
        move || async move { jobs::expire(&pool, colophon_db::Stop::NEVER).await },
        "WITH due AS",
    )
    .await;
    assert_eq!(swept, 1, "the held job is left for the next pass");
    assert_eq!(
        jobs::expire(&db.app, colophon_db::Stop::NEVER)
            .await
            .expect("next pass"),
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
        move || async move { assets::expire(&pool, colophon_db::Stop::NEVER).await },
        "WITH due AS",
    )
    .await;
    assert_eq!(swept, 1, "the held job is left for the next pass");
    assert_eq!(
        assets::expire(&db.app, colophon_db::Stop::NEVER)
            .await
            .expect("next pass"),
        1
    );
    owner.close().await;
    db.drop().await;
}

/// The lapsed-lease release (D-237) never waits either: it takes lapsed jobs in id order and skips
/// one another transaction holds, which is the next pass's.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_lapsed_release_never_waits_on_a_held_job() {
    let db = TestDb::new().await;
    let (earlier, later) = two_jobs(&db).await;
    let owner = db.owner().await;
    sqlx::query(
        "UPDATE jobs SET status = 'running', lease_owner = 'gone', attempts = 1,
                         lease_expires_at = now() - interval '1 second'",
    )
    .execute(&owner)
    .await
    .expect("both lapsed");
    let pool = db.app.clone();
    let released = sweep_beside_a_holder(
        &owner,
        "jobs",
        earlier,
        later,
        move || async move { jobs::release_lapsed(&pool, colophon_db::Stop::NEVER).await },
        "WITH lapsed AS",
    )
    .await;
    assert_eq!(released, 1, "the held job is left for the next pass");
    owner.close().await;
    db.drop().await;
}

/// Eight at once, round after round: three lapsed-lease releases, two deadline sweeps and three
/// releases (on eight connections) superseding every older job that hasn't started (each waiting for each such job, in
/// id order), over jobs that are lapsed, out of time or waiting. No deadlock (40P01), no error at
/// all, and no transition lost: afterwards no job is left running on a lease that ran out, none
/// open past its deadline, and every job of an older release that was waiting is superseded.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn releases_sweeps_and_supersedes_racing_never_deadlock_or_lose_a_transition() {
    const DESTINATIONS: usize = 12;
    const ROUNDS: usize = 6;
    let db = TestDb::new().await;
    let (alice, project) = project_with(&db, DESTINATIONS).await;
    publish(&db.app, alice, project).await;
    let owner = db.owner().await;
    let pool = db.app_pool(8).await;
    for round in 0..ROUNDS {
        // The jobs as a crash and a slow destination leave them: a third lapsed, a third out of
        // time (some lapsed too), the rest waiting.
        sqlx::query(
            "UPDATE jobs SET status = 'running', lease_owner = 'gone', attempts = attempts + 1,
                             lease_expires_at = now() - interval '1 second'
             WHERE status IN ('pending', 'retrying') AND hashtext(id::text) % 3 = 0",
        )
        .execute(&owner)
        .await
        .expect("lapsed");
        sqlx::query(
            "UPDATE jobs SET deadline_at = now() - interval '1 second'
             WHERE status IN ('pending', 'retrying', 'running') AND hashtext(id::text) % 3 = 1",
        )
        .execute(&owner)
        .await
        .expect("out of time");
        let mut racers = tokio::task::JoinSet::new();
        for racer in 0..5 {
            let pool = pool.clone();
            racers.spawn(async move {
                if racer < 3 {
                    jobs::release_lapsed(&pool, colophon_db::Stop::NEVER)
                        .await
                        .map(|_| ())
                } else {
                    jobs::expire(&pool, colophon_db::Stop::NEVER)
                        .await
                        .map(|_| ())
                }
            });
        }
        // Three superseding releases beside them, each on a connection of its own.
        futures_join_all((0..3).map(|_| publish(&pool, alice, project))).await;
        while let Some(joined) = racers.join_next().await {
            let result = joined.expect("a racer's task");
            assert!(result.is_ok(), "round {round}: {result:?}");
        }
    }
    // The sweeps' next passes leave nothing behind.
    jobs::release_lapsed_all(&db.app, jobs::RELEASE_BUDGET, colophon_db::Stop::NEVER)
        .await
        .expect("release");
    jobs::expire(&db.app, colophon_db::Stop::NEVER)
        .await
        .expect("expire");
    let (stuck, overdue, stale): (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE status = 'running' AND lease_expires_at < now()),
                count(*) FILTER (WHERE status IN ('pending', 'retrying', 'running')
                                   AND deadline_at <= now()),
                count(*) FILTER (WHERE status IN ('pending', 'retrying')
                                   AND generation < (SELECT max(generation) FROM jobs))
         FROM jobs",
    )
    .fetch_one(&owner)
    .await
    .expect("the jobs");
    assert_eq!(stuck, 0, "a lapsed job left running");
    assert_eq!(overdue, 0, "an open job left past its deadline");
    // A job released to waiting after the last release's supersede is fine (the attempt's own
    // check supersedes it); one waiting from before it would be a lost supersede.
    let latest_supersede_missed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM jobs j
         WHERE j.status IN ('pending', 'retrying') AND j.generation < (SELECT max(generation) FROM jobs)
           AND j.updated_at < (SELECT max(published_at) FROM releases WHERE project_id = j.project_id)",
    )
    .fetch_one(&owner)
    .await
    .expect("missed supersedes");
    assert_eq!(
        latest_supersede_missed, 0,
        "an older waiting job the last release should have superseded ({stale} older waiting in all)"
    );
    pool.close().await;
    owner.close().await;
    db.drop().await;
}

/// Runs every future to its end, concurrently, on this task.
async fn futures_join_all<F: std::future::Future<Output = ()>>(futures: impl Iterator<Item = F>) {
    let mut set: Vec<std::pin::Pin<Box<F>>> = futures.map(Box::pin).collect();
    std::future::poll_fn(|cx| {
        set.retain_mut(|future| future.as_mut().poll(cx).is_pending());
        if set.is_empty() {
            std::task::Poll::Ready(())
        } else {
            std::task::Poll::Pending
        }
    })
    .await;
}
