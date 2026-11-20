//! A claim's cost is its limit, not the table (migration 0080): with a long history of finished
//! asset jobs behind a due backlog, claiming four reads a handful of rows by index and never the
//! whole table. It failed first on the claim's one `WHERE … OR …` over waiting jobs and lapsed
//! leases, which no index serves in order: every claim read every open job and sorted them to
//! take four (1,005 tuples here; in the assets smoke, with dead row versions piling up, a scan
//! of the whole table at 1-3.5 s a claim).
//! A lapsed lease, the oldest due job, is still claimed first. Measured with the table's own
//! statistics, each backend's flushed before it is read. Needs the local stack.
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

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_claim_reads_its_limit_not_the_table() {
    let db = TestDb::new().await;
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
