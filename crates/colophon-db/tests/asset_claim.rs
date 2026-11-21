//! A claim's cost is its limit, not the table (migration 0080): with a long history of finished
//! asset jobs behind a due backlog, claiming four reads a handful of rows by index and never the
//! whole table. It failed first on the claim's one `WHERE … OR …` over waiting jobs and lapsed
//! leases, which no index serves in order: every claim read every open job and sorted them to
//! take four (1,005 tuples here; in the assets smoke, with dead row versions piling up, a scan
//! of the whole table at 1-3.5 s a claim).
//! A lapsed lease, made waiting again by the sweep's `release_lapsed`, is claimed first, in its
//! old place. And the claim never reads the lapsed-lease index: when it searched it too, every
//! claim walked the dead entry each finished job leaves there, for as long as an open snapshot
//! kept them from being marked dead (train-3's assets smoke, claim tails climbing with the
//! run). Measured with the tables' own statistics, each backend's flushed before it is read.
//! Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::time::Duration;

use colophon_db::{PgPool, assets, projects, scope};
use common::TestDb;
use sqlx::AssertSqlSafe;
use time::OffsetDateTime;
use uuid::Uuid;

const HOUR: Duration = Duration::from_secs(3600);
/// Finished jobs behind the backlog: each was pending once, so `asset_jobs_due` held it.
const HISTORY: i64 = 20_000;
/// Waiting jobs, all due.
const BACKLOG: i64 = 1_000;
/// What one claim may read of `asset_jobs`: its four rows, each found by an index and fetched
/// again by the update, plus slack. The table-wide read it replaced reads everything.
const MAX_TUPLES: i64 = 50;

/// `asset_jobs`'s sequential scans and the rows read by them and by index, as the stats now say.
async fn reads(pool: &PgPool) -> (i64, i64) {
    sqlx::query_as(
        "SELECT seq_scan, coalesce(seq_tup_read, 0) + coalesce(idx_tup_fetch, 0)
         FROM pg_stat_user_tables WHERE relname = 'asset_jobs'",
    )
    .fetch_one(pool)
    .await
    .expect("pg_stat_user_tables")
}

/// Makes `connection`'s backend publish what it counted: the flush happens as it goes idle
/// after the request, so the next statement on it runs after the flush.
async fn flush(connection: &mut sqlx::PgConnection) {
    sqlx::query("SELECT pg_stat_force_next_flush()")
        .execute(&mut *connection)
        .await
        .expect("flush requested");
    sqlx::query("SELECT 1")
        .execute(&mut *connection)
        .await
        .expect("flushed");
}

/// The same for the application pool's one connection (the claim's backend).
async fn flush_app(db: &TestDb) {
    let mut connection = db.app.acquire().await.expect("the app connection");
    flush(&mut connection).await;
}

/// Reads of `index` (an index on `asset_jobs`) by scans, as the stats now say.
async fn index_reads(pool: &PgPool, index: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT coalesce(idx_tup_read, 0) FROM pg_stat_user_indexes WHERE indexrelname = $1",
    )
    .bind(index)
    .fetch_one(pool)
    .await
    .expect("pg_stat_user_indexes")
}

/// A project with one upload, its verify job `verify:a`: the asset every seeded job names.
async fn one_upload(db: &TestDb) {
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = scope::begin_in_project(&db.app, alice, project, "production")
        .await
        .expect("begin")
        .expect("a member");
    assets::insert(
        &mut tx,
        Uuid::now_v7(),
        "a.png",
        "image/png",
        1_024,
        "quarantine/a",
        alice,
        Uuid::now_v7(),
        "verify:a",
        OffsetDateTime::now_utc() + HOUR,
    )
    .await
    .expect("uploaded");
    tx.commit().await.expect("commit");
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_claim_reads_its_limit_not_the_table() {
    let db = TestDb::new().await;
    one_upload(&db).await;

    // The history and the backlog, all on the one asset, on one owner connection.
    let owner = db.owner().await;
    let mut seed = owner.acquire().await.expect("an owner connection");
    let fill = |prefix: &str, since: &str| {
        format!(
            "INSERT INTO asset_jobs (project_id, environment_id, id, asset_id, kind, idempotency_key,
                                     next_attempt_at, deadline_at)
             SELECT project_id, environment_id, gen_random_uuid(), asset_id, kind, '{prefix}' || n,
                    now() - interval '{since}' + n * interval '1 ms', now() + interval '1 hour'
             FROM asset_jobs, generate_series(1, $1) n WHERE idempotency_key = 'verify:a'"
        )
    };
    // History: queued, then finished, long before anything due now.
    sqlx::query(AssertSqlSafe(fill("old:", "2 hours")))
        .bind(HISTORY)
        .execute(&mut *seed)
        .await
        .expect("history queued");
    sqlx::query(
        "UPDATE asset_jobs SET status = 'succeeded', finished_at = now()
         WHERE idempotency_key LIKE 'old:%'",
    )
    .execute(&mut *seed)
    .await
    .expect("history finished");
    // The backlog: waiting, due since an hour ago.
    sqlx::query(AssertSqlSafe(fill("due:", "1 hour")))
        .bind(BACKLOG)
        .execute(&mut *seed)
        .await
        .expect("backlog queued");
    // A worker died holding the upload's own job: its lease ran out, and it is the oldest due.
    sqlx::query(
        "UPDATE asset_jobs SET status = 'running', lease_owner = 'gone',
                               lease_expires_at = now() - interval '1 minute',
                               next_attempt_at = now() - interval '3 hours'
         WHERE idempotency_key = 'verify:a'",
    )
    .execute(&mut *seed)
    .await
    .expect("a lapsed lease");
    // The sweep makes it waiting again, keeping its place.
    assert_eq!(
        assets::release_lapsed(&db.app).await.expect("released"),
        1,
        "the lapsed lease, and only it"
    );
    sqlx::query("ANALYZE asset_jobs")
        .execute(&mut *seed)
        .await
        .expect("analyze");
    flush(&mut seed).await;
    drop(seed);
    flush_app(&db).await;

    let (scans_before, tuples_before) = reads(&owner).await;
    let claimed = assets::claim(&db.app, "worker-a", HOUR, 4)
        .await
        .expect("claim");
    flush_app(&db).await;
    let (scans_after, tuples_after) = reads(&owner).await;

    assert_eq!(claimed.len(), 4, "four due jobs claimed");
    assert!(
        claimed.iter().any(|job| job.idempotency_key == "verify:a"),
        "the lapsed lease, oldest due, was claimed: {:?}",
        claimed
            .iter()
            .map(|job| &job.idempotency_key)
            .collect::<Vec<_>>()
    );
    let (scans, tuples) = (scans_after - scans_before, tuples_after - tuples_before);
    eprintln!("one claim: {scans} sequential scan(s), {tuples} tuples read");
    assert_eq!(
        scans, 0,
        "a claim scanned all of asset_jobs ({tuples} tuples)"
    );
    assert!(
        tuples <= MAX_TUPLES,
        "a claim read {tuples} tuples of asset_jobs for four jobs"
    );
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_claim_never_walks_finished_jobs_leases() {
    let db = TestDb::new().await;
    one_upload(&db).await;
    let owner = db.owner().await;
    let mut seed = owner.acquire().await.expect("an owner connection");
    // History: jobs that ran, under leases long run out, and finished. Each left a dead entry in
    // the lapsed-lease index, inside `lease_expires_at < now()`; never one in `asset_jobs_due`.
    sqlx::query(
        "INSERT INTO asset_jobs (project_id, environment_id, id, asset_id, kind, idempotency_key,
                                 status, lease_owner, lease_expires_at, next_attempt_at, deadline_at)
         SELECT project_id, environment_id, gen_random_uuid(), asset_id, kind, 'ran:' || n,
                'running', 'gone', now() - interval '1 hour', now() - interval '2 hours',
                now() + interval '1 hour'
         FROM asset_jobs, generate_series(1, $1) n WHERE idempotency_key = 'verify:a'",
    )
    .bind(HISTORY)
    .execute(&mut *seed)
    .await
    .expect("history ran");
    // A snapshot older than their finishing, as a long admission holds one: no scan may mark
    // the entries dead while it is open.
    let mut snapshot = owner.begin().await.expect("begin");
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *snapshot)
        .await
        .expect("repeatable read");
    sqlx::query("SELECT count(*) FROM asset_jobs")
        .execute(&mut *snapshot)
        .await
        .expect("the snapshot is taken");
    sqlx::query(
        "UPDATE asset_jobs SET status = 'succeeded', finished_at = now()
         WHERE idempotency_key LIKE 'ran:%'",
    )
    .execute(&mut *seed)
    .await
    .expect("history finished");
    sqlx::query("ANALYZE asset_jobs")
        .execute(&mut *seed)
        .await
        .expect("analyze");
    flush(&mut seed).await;
    drop(seed);
    flush_app(&db).await;

    let before = index_reads(&owner, "asset_jobs_leased").await;
    let claimed = assets::claim(&db.app, "worker-a", HOUR, 4)
        .await
        .expect("claim");
    flush_app(&db).await;
    let walked = index_reads(&owner, "asset_jobs_leased").await - before;

    assert_eq!(claimed.len(), 1, "the upload's own job, waiting");
    eprintln!("one claim walked {walked} lapsed-lease index entries");
    assert_eq!(
        walked, 0,
        "a claim walked {walked} entries of finished jobs' leases"
    );
    snapshot.rollback().await.expect("release the snapshot");
    owner.close().await;
    db.drop().await;
}

/// A pass is bounded, a sweep is not: 250 lapsed leases are all released in one sweep, a
/// pass of 200 then one of 50, and a live lease is never touched; a stop asked for between
/// passes ends the sweep without starting another.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn one_sweep_releases_every_lapsed_lease_a_bounded_pass_at_a_time() {
    let db = TestDb::new().await;
    one_upload(&db).await;
    let owner = db.owner().await;
    let strand = async || {
        sqlx::query(
            "INSERT INTO asset_jobs (project_id, environment_id, id, asset_id, kind,
                                     idempotency_key, status, lease_owner, lease_expires_at,
                                     next_attempt_at, deadline_at)
             SELECT project_id, environment_id, gen_random_uuid(), asset_id, kind,
                    'lapsed:' || gen_random_uuid(), 'running', 'gone',
                    now() - interval '1 minute', now() - interval '2 minutes',
                    now() + interval '1 hour'
             FROM asset_jobs, generate_series(1, 250) n WHERE idempotency_key = 'verify:a'",
        )
        .execute(&owner)
        .await
        .expect("250 lapsed leases");
    };
    strand().await;
    sqlx::query(
        "UPDATE asset_jobs SET status = 'running', lease_owner = 'alive',
                               lease_expires_at = now() + interval '1 minute'
         WHERE idempotency_key = 'verify:a'",
    )
    .execute(&owner)
    .await
    .expect("a live lease");
    assert_eq!(
        assets::release_lapsed_all(&db.app, assets::RELEASE_BUDGET, colophon_db::Stop::NEVER)
            .await
            .expect("a sweep"),
        250,
        "every lapsed lease, in one sweep"
    );
    let live: String =
        sqlx::query_scalar("SELECT status FROM asset_jobs WHERE idempotency_key = 'verify:a'")
            .fetch_one(&owner)
            .await
            .expect("the live job");
    assert_eq!(live, "running", "a live lease is never released");

    // Another 250, and a stop asked for during the first pass: the sweep ends after it.
    strand().await;
    let asked = std::sync::atomic::AtomicUsize::new(0);
    let stopped = || {
        asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        true
    };
    assert_eq!(
        assets::release_lapsed_all(
            &db.app,
            assets::RELEASE_BUDGET,
            colophon_db::Stop::new(&stopped),
        )
        .await
        .expect("a stopped sweep"),
        u64::try_from(assets::RELEASE_BATCH).expect("a count"),
        "one pass, then the stop"
    );
    assert_eq!(
        asked.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "asked once, between passes"
    );
    owner.close().await;
    db.drop().await;
}
