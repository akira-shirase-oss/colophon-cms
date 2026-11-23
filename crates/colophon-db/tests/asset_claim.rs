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
        assets::release_lapsed(&db.app, colophon_db::Stop::NEVER)
            .await
            .expect("released"),
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
    let claimed = assets::claim(&db.app, "worker-a", HOUR, 4, colophon_db::Stop::NEVER)
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
    let claimed = assets::claim(&db.app, "worker-a", HOUR, 4, colophon_db::Stop::NEVER)
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
/// passes ends the sweep without starting another, and one asked for before it begins nothing.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
#[expect(
    clippy::too_many_lines,
    reason = "one sweep, a stop under way, and a stop before: one table's leases in order"
)]
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

    // Another 250, and a stop asked for while the first pass is under way (its release waits on
    // the table, held): the pass finishes, and the sweep ends after it.
    strand().await;
    let lapsed = async || -> i64 {
        sqlx::query_scalar(
            "SELECT count(*) FROM asset_jobs WHERE status = 'running' AND lease_owner = 'gone'",
        )
        .fetch_one(&owner)
        .await
        .expect("the lapsed leases")
    };
    let stop_now = std::sync::atomic::AtomicBool::new(false);
    let stopped = || stop_now.load(std::sync::atomic::Ordering::SeqCst);
    let mut holder = owner.begin().await.expect("begin");
    sqlx::query("LOCK TABLE asset_jobs IN SHARE MODE")
        .execute(&mut *holder)
        .await
        .expect("hold the table");
    let driver = async {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity
                 WHERE datname = current_database() AND wait_event_type = 'Lock'
                   AND query LIKE '%asset_jobs%' AND pid <> pg_backend_pid()",
            )
            .fetch_one(&owner)
            .await
            .expect("pg_stat_activity");
            if waiting > 0 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the pass never waited"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        stop_now.store(true, std::sync::atomic::Ordering::SeqCst);
        holder.rollback().await.expect("release the table");
    };
    let (released, ()) = tokio::join!(
        assets::release_lapsed_all(
            &db.app,
            assets::RELEASE_BUDGET,
            colophon_db::Stop::new(&stopped),
        ),
        driver
    );
    assert_eq!(
        released.expect("a stopped sweep"),
        u64::try_from(assets::RELEASE_BATCH).expect("a count"),
        "the pass under way finished, then the stop"
    );
    assert_eq!(
        lapsed().await,
        250 - assets::RELEASE_BATCH,
        "no second pass"
    );

    // A stop asked for before the sweep: it begins nothing (colophon_db::begin_background).
    assert!(matches!(
        assets::release_lapsed_all(
            &db.app,
            assets::RELEASE_BUDGET,
            colophon_db::Stop::new(&stopped),
        )
        .await,
        Err(colophon_db::DbError::Stopped)
    ));
    assert_eq!(
        lapsed().await,
        250 - assets::RELEASE_BATCH,
        "nothing released"
    );
    owner.close().await;
    db.drop().await;
}

/// Blocks the backend `pid` read or found in shared buffers for relations, by its own I/O
/// statistics: nothing another backend did (an autovacuum, another test) is counted.
async fn own_reads(pool: &PgPool, pid: i32) -> i64 {
    sqlx::query_scalar(
        "SELECT coalesce(sum(coalesce(hits, 0) + coalesce(reads, 0)), 0)::int8
         FROM pg_stat_get_backend_io($1) WHERE object = 'relation' AND context = 'normal'",
    )
    .bind(pid)
    .fetch_one(pool)
    .await
    .expect("pg_stat_get_backend_io")
}

/// A claim's reads stay a handful of blocks at the size the assets smoke reaches on a fast
/// runner (main's CI 44868206132: 70,871 blocks a claim, 4.5 s at the end), under the plan the
/// server runs it with after five calls. It failed first two ways: joining `assets` in the
/// UPDATE read every asset of the environment for each claimed job and filtered on the id after
/// (457,559 buffers measured at this size); and the generic plan, not knowing `LIMIT $3` is a
/// handful, scanned all of `asset_jobs` for the UPDATE's join (1,130 blocks here, and 1,160 with
/// the subqueries but without the claim's `force_custom_plan`: that setting carries the fix).
/// With both, a claim reads its own work and a step over what died under the snapshot the
/// smoke holds open.
///
/// Measured on the claim's backend alone (`pg_stat_get_backend_io`), after a warming claim, so
/// nothing another session reads is counted: CI 44894906554 failed this at 708 blocks when the
/// test still counted every backend's reads of the two tables (an autovacuum landing in the
/// window is the likely reading). Another backend's read, flushed inside the window, now
/// shows that: counted table-wide it made the claim 893 blocks; counted on the claim's backend
/// it changes nothing (229-234 over six runs).
///
/// The lesson for every fixture: seed the cardinalities production has. Here that's many
/// assets in one environment, each with its own jobs. The other tests in this file name a
/// single asset from every job, which made the join to `assets` one row and hid this twice.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
#[expect(
    clippy::too_many_lines,
    reason = "one fixture at production's cardinalities, then one measured claim"
)]
async fn a_claim_reads_a_handful_of_blocks_at_a_fast_runners_scale() {
    const ASSETS: i64 = 23_000;
    const WAITING: i64 = 3_000;
    /// Finished while the snapshot is open: a few seconds' worth on a fast runner.
    const RECENT: i64 = 500;
    /// The most one claim of four may read on its own backend (see the assertion).
    const MAX_CLAIM_BLOCKS: i64 = 500;
    let db = TestDb::new().await;
    one_upload(&db).await;
    let owner = db.owner().await;
    let mut seed = owner.acquire().await.expect("an owner connection");
    // Every job on an asset of its own: 20,000 finished uploads and 3,000 waiting, as the smoke
    // has them, in the upload's project and environment.
    sqlx::query(
        "INSERT INTO assets (project_id, environment_id, id, file_name, declared_type, size_bytes,
                             quarantine_key, created_by)
         SELECT a.project_id, a.environment_id, gen_random_uuid(), 'f' || n || '.png', 'image/png',
                1024, 'quarantine/f' || n, a.created_by
         FROM assets a, generate_series(1, $1) n",
    )
    .bind(ASSETS)
    .execute(&mut *seed)
    .await
    .expect("assets");
    sqlx::query(
        "INSERT INTO asset_jobs (project_id, environment_id, id, asset_id, kind, idempotency_key,
                                 next_attempt_at, deadline_at)
         SELECT a.project_id, a.environment_id, gen_random_uuid(), a.id, 'verify',
                'job:' || a.file_name, now() - interval '2 hours'
                    + row_number() OVER (ORDER BY a.file_name) * interval '1 ms',
                now() + interval '1 hour'
         FROM assets a WHERE a.file_name LIKE 'f%'",
    )
    .execute(&mut *seed)
    .await
    .expect("one job each");
    // The long history finished before any snapshot now open: a first claim marks its dead
    // entries in the due index dead, as the worker's claims do all day.
    let finish = "UPDATE asset_jobs SET status = 'succeeded', finished_at = now()
                  WHERE id IN (SELECT id FROM asset_jobs
                               WHERE idempotency_key LIKE 'job:%' AND status = 'pending'
                               ORDER BY next_attempt_at LIMIT $1)";
    sqlx::query(finish)
        .bind(ASSETS - WAITING - RECENT)
        .execute(&mut *seed)
        .await
        .expect("the history finished");
    assets::claim(&db.app, "worker-a", HOUR, 1, colophon_db::Stop::NEVER)
        .await
        .expect("a first claim");
    // A snapshot held through the claim, as the smoke's are (up to 3.7 s), and the uploads
    // that finish under it: their dead entries can't be marked while it is open.
    let mut snapshot = owner.begin().await.expect("begin");
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *snapshot)
        .await
        .expect("repeatable read");
    sqlx::query("SELECT count(*) FROM asset_jobs")
        .execute(&mut *snapshot)
        .await
        .expect("the snapshot is taken");
    sqlx::query(finish)
        .bind(RECENT)
        .execute(&mut *seed)
        .await
        .expect("the recent ones finished");
    sqlx::query("ANALYZE asset_jobs, assets")
        .execute(&mut *seed)
        .await
        .expect("analyze");
    flush(&mut seed).await;
    drop(seed);
    // The server's plan: after five calls a prepared statement settles on a generic one.
    {
        let mut app = db.app.acquire().await.expect("the app connection");
        sqlx::query("SET plan_cache_mode = force_generic_plan")
            .execute(&mut *app)
            .await
            .expect("generic plans");
    }
    flush_app(&db).await;

    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&db.app)
        .await
        .expect("the claim's backend");
    // One unmeasured claim first, on this session's plan settings, after the ANALYZE: catalog,
    // relation and plan caches are hot for the measured one, so what it reads is its own work.
    assets::claim(&db.app, "worker-a", HOUR, 1, colophon_db::Stop::NEVER)
        .await
        .expect("a warming claim");
    flush_app(&db).await;
    let (scans_before, _) = reads(&owner).await;
    let before = own_reads(&owner, pid).await;
    // Another backend reads these tables inside the window, as an autovacuum may after a
    // burst of updates, or any other session: the likely reading of CI 44894906554's 708
    // blocks, when the test counted every backend's reads of both tables as the claim's. Only
    // the claim's own backend is measured now, so this read changes nothing.
    {
        let mut other = owner.acquire().await.expect("another backend");
        sqlx::query("SELECT sum(size_bytes) FROM assets WHERE file_name LIKE 'f%'")
            .execute(&mut *other)
            .await
            .expect("another backend's read");
        flush(&mut other).await;
    }
    let claimed = assets::claim(&db.app, "worker-a", HOUR, 4, colophon_db::Stop::NEVER)
        .await
        .expect("claim");
    flush_app(&db).await;
    let (scans_after, _) = reads(&owner).await;
    let read = own_reads(&owner, pid).await - before;

    assert_eq!(claimed.len(), 4, "four waiting jobs claimed");
    // The worker's own transaction set the plan for this call: generic is the session's.
    eprintln!("one claim at {ASSETS} assets, {WAITING} waiting: {read} blocks");
    assert_eq!(
        scans_after - scans_before,
        0,
        "a claim scanned a whole table ({read} blocks)"
    );
    // The claim's own work, counted on its backend alone, its caches warm: about 214 blocks
    // with nothing dead under the snapshot (planning, the four rows' index entries, the
    // triggers), 229-234 with the 500 that finished under it (measured six times each). A
    // whole-table read or a per-asset join is thousands to hundreds of thousands; 500 sits
    // between with room on both sides.
    assert!(
        read <= MAX_CLAIM_BLOCKS,
        "a claim read {read} blocks for four jobs ({RECENT} finished under the snapshot)"
    );
    snapshot.rollback().await.expect("release the snapshot");
    owner.close().await;
    db.drop().await;
}

/// The deadline sweep (`assets::expire`, every 10 s on every worker) reads the jobs that ran out
/// of time, not the table (D-237). At the size the assets smoke reaches, 23,000 jobs (20,000
/// finished, 3,000 waiting) and five past their deadline, it reads a handful of blocks on its
/// own backend (about 120, against 1,041 before). It failed first on main: `asset_jobs` had no index on the open jobs' deadline
/// (`jobs` has `jobs_open_by_deadline`), so every sweep read the whole table.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_deadline_sweep_reads_the_expired_not_the_table() {
    const JOBS: i64 = 23_000;
    const WAITING: i64 = 3_000;
    const EXPIRED: i64 = 5;
    /// The most one sweep that fails five jobs may read on its own backend: finding them is a
    /// few blocks of the open jobs' deadline index; updating them (their keys, their index
    /// entries, the hint and payload triggers) is the rest, about 100 in all, at any table size.
    const MAX_EXPIRE_BLOCKS: i64 = 250;
    let db = TestDb::new().await;
    one_upload(&db).await;
    let owner = db.owner().await;
    sqlx::query(
        "INSERT INTO assets (project_id, environment_id, id, file_name, declared_type, size_bytes,
                             quarantine_key, created_by)
         SELECT a.project_id, a.environment_id, gen_random_uuid(), 'f' || n || '.png', 'image/png',
                1024, 'quarantine/f' || n, a.created_by
         FROM assets a, generate_series(1, $1) n",
    )
    .bind(JOBS)
    .execute(&owner)
    .await
    .expect("assets");
    sqlx::query(
        "INSERT INTO asset_jobs (project_id, environment_id, id, asset_id, kind, idempotency_key,
                                 next_attempt_at, deadline_at)
         SELECT a.project_id, a.environment_id, gen_random_uuid(), a.id, 'verify',
                'job:' || a.file_name, now() - interval '2 hours', now() + interval '1 hour'
         FROM assets a WHERE a.file_name LIKE 'f%'",
    )
    .execute(&owner)
    .await
    .expect("one job each");
    sqlx::query(
        "UPDATE asset_jobs SET status = 'succeeded', finished_at = now() - interval '1 hour'
         WHERE id IN (SELECT id FROM asset_jobs WHERE idempotency_key LIKE 'job:%'
                      ORDER BY idempotency_key LIMIT $1)",
    )
    .bind(JOBS - WAITING)
    .execute(&owner)
    .await
    .expect("the history finished");
    sqlx::raw_sql("VACUUM ANALYZE asset_jobs, assets")
        .execute(&owner)
        .await
        .expect("vacuum");
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&db.app)
        .await
        .expect("the sweep's backend");
    // An unmeasured sweep with nothing to expire: caches hot for the measured one.
    assert_eq!(
        assets::expire(&db.app, colophon_db::Stop::NEVER)
            .await
            .expect("a warming sweep"),
        0
    );
    sqlx::query(
        "UPDATE asset_jobs SET deadline_at = now() - interval '1 minute'
         WHERE id IN (SELECT id FROM asset_jobs WHERE status = 'pending'
                      AND idempotency_key LIKE 'job:%' ORDER BY idempotency_key LIMIT $1)",
    )
    .bind(EXPIRED)
    .execute(&owner)
    .await
    .expect("five out of time");
    flush_app(&db).await;
    let before = own_reads(&owner, pid).await;
    let expired = assets::expire(&db.app, colophon_db::Stop::NEVER)
        .await
        .expect("the sweep");
    flush_app(&db).await;
    let read = own_reads(&owner, pid).await - before;
    eprintln!("one deadline sweep at {JOBS} jobs, {EXPIRED} expired: {read} blocks");
    assert_eq!(expired, u64::try_from(EXPIRED).expect("a count"));
    assert!(
        read <= MAX_EXPIRE_BLOCKS,
        "a deadline sweep read {read} blocks at {JOBS} jobs"
    );
    owner.close().await;
    db.drop().await;
}

/// The expiry enqueue (`assets::enqueue_expired`, the asset retention sweep's first half) counts
/// each project's open work once a pass, not once per asset (D-237). At 23,000 collectable assets
/// in a project with 3,000 open asset jobs, the count ran for every asset before the limit
/// applied: 4.7 million buffers and 7.5 s a pass, past the background units' 4 s statement bound
/// (D-236), so the sweep could never enqueue an expiry at that size. Counted once per project, a
/// pass reads its per-asset checks and the project's open work once.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_expiry_enqueue_counts_a_project_s_open_work_once() {
    const ASSETS: i64 = 23_000;
    const WAITING: i64 = 3_000;
    /// The most one pass enqueueing 200 may read: about 3 a collectable asset and the
    /// project's open work once (77,000 measured at this size), against 4.7 million before.
    const MAX_ENQUEUE_BLOCKS: i64 = 150_000;
    let db = TestDb::new().await;
    one_upload(&db).await;
    let owner = db.owner().await;
    sqlx::query(
        "INSERT INTO assets (project_id, environment_id, id, file_name, declared_type, size_bytes,
                             quarantine_key, created_by, created_at)
         SELECT a.project_id, a.environment_id, gen_random_uuid(), 'f' || n || '.png', 'image/png',
                1024, 'quarantine/f' || n, a.created_by, now() - interval '2 days'
         FROM assets a, generate_series(1, $1) n",
    )
    .bind(ASSETS)
    .execute(&owner)
    .await
    .expect("old assets");
    sqlx::query(
        "INSERT INTO asset_jobs (project_id, environment_id, id, asset_id, kind, idempotency_key,
                                 next_attempt_at, deadline_at, status, finished_at)
         SELECT a.project_id, a.environment_id, gen_random_uuid(), a.id, 'verify',
                'job:' || a.file_name, now() - interval '2 days', now() + interval '1 hour',
                CASE WHEN row_number() OVER (ORDER BY a.file_name) <= $1 THEN 'pending'
                     ELSE 'succeeded' END,
                CASE WHEN row_number() OVER (ORDER BY a.file_name) <= $1 THEN NULL
                     ELSE now() - interval '1 day' END
         FROM assets a WHERE a.file_name LIKE 'f%'",
    )
    .bind(WAITING)
    .execute(&owner)
    .await
    .expect("one job each, 3,000 still open");
    sqlx::raw_sql("VACUUM ANALYZE assets, asset_jobs")
        .execute(&owner)
        .await
        .expect("vacuum");
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&db.app)
        .await
        .expect("the sweep's backend");
    flush_app(&db).await;
    let before = own_reads(&owner, pid).await;
    let enqueued = assets::enqueue_expired(
        &db.app,
        OffsetDateTime::now_utc() - time::Duration::days(1),
        OffsetDateTime::now_utc() + HOUR,
        200,
        i64::MAX,
        colophon_db::Stop::NEVER,
    )
    .await;
    flush_app(&db).await;
    let read = own_reads(&owner, pid).await - before;
    eprintln!("one expiry enqueue at {ASSETS} collectable assets: {read} blocks");
    assert_eq!(enqueued.expect("the enqueue, inside its bounds"), 200);
    assert!(
        read <= MAX_ENQUEUE_BLOCKS,
        "an expiry enqueue read {read} blocks at {ASSETS} assets"
    );
    owner.close().await;
    db.drop().await;
}
