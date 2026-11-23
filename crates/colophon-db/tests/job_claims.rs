//! Delivery job claims and sweeps at production's cardinalities (D-237): many projects and
//! environments, several destinations each, a long history per environment that passed through
//! `running` (so its dead index entries are there to walk), a backlog in the thousands, some
//! destinations busy, and a snapshot held open while recent jobs finish, as the smokes hold one.
//! What a statement reads is measured on its own backend (`pg_stat_get_backend_io`), so nothing
//! another session reads is counted. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::time::Duration;

use colophon_db::{PgPool, jobs};
use common::TestDb;
use uuid::Uuid;

const HOUR: Duration = Duration::from_secs(3600);

/// Environments a project, destinations an environment.
const ENVIRONMENTS: i64 = 2;
const DESTINATIONS: i64 = 3;
/// Finished while the snapshot is open: a few seconds' worth on a fast runner.
const RECENT: i64 = 500;

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

/// Blocks backend `pid` read from relations (hits and reads), as its flushed stats say.
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

/// The installation: every project's environments, destinations, and releases `from..=to` (a
/// changeset and an approving review each), with one job per destination per release.
/// `finished` says which of them are history: claimed once (a lease that has since run out) and
/// finished, as the worker lives it, so each leaves dead entries in the due and leased indexes;
/// the rest wait, due in release order.
async fn history(owner: &PgPool, me: Uuid, from: i64, to: i64, finished: i64) {
    let mut seed = owner.acquire().await.expect("a seeding connection");
    for template in [
        "CREATE TEMP TABLE IF NOT EXISTS seed_c (project_id uuid, environment_id uuid, id uuid,
                                                 review uuid, g bigint)",
        "TRUNCATE seed_c",
        "INSERT INTO seed_c
         SELECT e.project_id, e.id, gen_random_uuid(), gen_random_uuid(), g
         FROM environments e, generate_series($2, $3) g
         WHERE e.slug LIKE 'jc-env-%'",
        "INSERT INTO changesets (project_id, environment_id, id, title, hash, created_by,
                                 submitted_by, submitted_at)
         SELECT project_id, environment_id, id, 'cs ' || g, sha256(id::text::bytea), $1, $1, now()
         FROM seed_c",
        "INSERT INTO reviews (project_id, environment_id, id, changeset_id, changeset_hash, seq,
                              reviewer_id, decision)
         SELECT project_id, environment_id, review, id, sha256(id::text::bytea), 1, $1, 'approve'
         FROM seed_c",
        "INSERT INTO releases (project_id, environment_id, generation, epoch, changeset_id,
                               changeset_hash, review_id, published_by, published_at)
         SELECT project_id, environment_id, g, 1, id, sha256(id::text::bytea), review, $1,
                now() - interval '6 hours' + g * interval '1 minute'
         FROM seed_c",
        "UPDATE environments SET generation = $3 WHERE slug LIKE 'jc-env-%'",
        "INSERT INTO jobs (project_id, environment_id, id, destination_id, generation,
                           idempotency_key, status, next_attempt_at, deadline_at)
         SELECT d.project_id, d.environment_id, gen_random_uuid(), d.id, c.g,
                'jc:' || d.id || ':' || c.g, 'pending',
                now() - interval '6 hours' + c.g * interval '1 minute'
                    + (row_number() OVER (PARTITION BY c.g ORDER BY d.id)) * interval '1 ms',
                now() + interval '1 hour'
         FROM destinations d
         JOIN seed_c c ON c.project_id = d.project_id AND c.environment_id = d.environment_id
         WHERE d.name LIKE 'jc hook %'",
        "UPDATE jobs SET status = 'running', lease_owner = 'old',
                         lease_expires_at = now() - interval '5 hours', attempts = 1
         WHERE idempotency_key LIKE 'jc:%' AND generation BETWEEN $2 AND $4
           AND status = 'pending'",
        "UPDATE jobs SET status = 'succeeded', lease_owner = NULL, lease_expires_at = NULL,
                         finished_at = now() - interval '5 hours'
         WHERE idempotency_key LIKE 'jc:%' AND status = 'running' AND lease_owner = 'old'",
    ] {
        // Audited: every value is the test's own (a uuid it made, integer constants).
        let statement = template
            .replace("$1", &format!("'{me}'::uuid"))
            .replace("$2", &from.to_string())
            .replace("$3", &to.to_string())
            .replace("$4", &finished.to_string());
        sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
            .execute(&mut *seed)
            .await
            .expect(template);
    }
}

/// `projects` projects with their environments and destinations, `generations` releases each,
/// the last `waiting` of them waiting, 30% of destinations busy (their first waiting job on a
/// live lease, the rest of their backlog behind it), then `VACUUM`ed: the history's dead entries
/// are gone, as a long-running installation's are.
async fn installation(db: &TestDb, projects: i64, generations: i64, waiting: i64) -> PgPool {
    let me = db.administrator("alice").await;
    let owner = db.owner().await;
    for template in [
        "INSERT INTO projects (id, slug, name)
         SELECT gen_random_uuid(), 'jc-' || n, 'JC ' || n FROM generate_series(1, $1) n",
        "INSERT INTO environments (project_id, id, slug)
         SELECT p.id, gen_random_uuid(), 'jc-env-' || m
         FROM projects p, generate_series(1, $2) m WHERE p.slug LIKE 'jc-%'",
        "INSERT INTO destinations (project_id, environment_id, id, kind, name, url, created_by)
         SELECT e.project_id, e.id, gen_random_uuid(), 'webhook', 'jc hook ' || k,
                'https://hook.example.com/' || e.id || '/' || k, $4
         FROM environments e, generate_series(1, $3) k WHERE e.slug LIKE 'jc-env-%'",
    ] {
        // Audited: every value is the test's own (constants, a uuid it made).
        let statement = template
            .replace("$1", &projects.to_string())
            .replace("$2", &ENVIRONMENTS.to_string())
            .replace("$3", &DESTINATIONS.to_string())
            .replace("$4", &format!("'{me}'::uuid"));
        sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
            .execute(&owner)
            .await
            .expect(template);
    }
    history(&owner, me, 1, generations, generations - waiting).await;
    sqlx::query(
        "UPDATE jobs j SET status = 'running', lease_owner = 'busy',
                           lease_expires_at = now() + interval '1 hour', attempts = 1
         WHERE j.idempotency_key LIKE 'jc:%' AND j.generation = $1
           AND hashtext(j.destination_id::text) % 10 IN (0, 1, 2)",
    )
    .bind(generations - waiting + 1)
    .execute(&owner)
    .await
    .expect("busy destinations");
    sqlx::raw_sql("VACUUM ANALYZE jobs, destinations, releases")
        .execute(&owner)
        .await
        .expect("vacuum");
    owner
}

/// What one claim of four reads on its own backend, under the plan the server runs it with
/// after five calls (a generic one), with a snapshot held open while the [`RECENT`] oldest
/// waiting jobs finish under it (their dead due entries can't be marked while it is open). The
/// snapshot is released before returning.
async fn one_claim(db: &TestDb, owner: &PgPool) -> i64 {
    let mut snapshot = owner.begin().await.expect("begin");
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *snapshot)
        .await
        .expect("repeatable read");
    sqlx::query("SELECT count(*) FROM jobs")
        .execute(&mut *snapshot)
        .await
        .expect("the snapshot is taken");
    sqlx::query(
        "UPDATE jobs SET status = 'succeeded', finished_at = now()
         WHERE id IN (SELECT id FROM jobs WHERE idempotency_key LIKE 'jc:%' AND status = 'pending'
                      ORDER BY next_attempt_at LIMIT $1)",
    )
    .bind(RECENT)
    .execute(owner)
    .await
    .expect("the recent ones finished");
    {
        let mut app = db.app.acquire().await.expect("the app connection");
        sqlx::query("SET plan_cache_mode = force_generic_plan")
            .execute(&mut *app)
            .await
            .expect("generic plans");
        flush(&mut app).await;
    }
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&db.app)
        .await
        .expect("the claim's backend");
    // One unmeasured claim first: catalog, relation and plan caches are hot for the measured
    // one, so what it reads is its own work.
    let warm = jobs::claim(&db.app, "warm", "local", HOUR, 1, colophon_db::Stop::NEVER)
        .await
        .expect("a warming claim");
    assert_eq!(warm.len(), 1, "the warming claim found work");
    {
        let mut app = db.app.acquire().await.expect("the app connection");
        flush(&mut app).await;
    }
    let before = own_reads(owner, pid).await;
    let claimed = jobs::claim(
        &db.app,
        "worker-a",
        "local",
        HOUR,
        4,
        colophon_db::Stop::NEVER,
    )
    .await
    .expect("claim");
    {
        let mut app = db.app.acquire().await.expect("the app connection");
        flush(&mut app).await;
    }
    let read = own_reads(owner, pid).await - before;
    assert_eq!(claimed.len(), 4, "four waiting jobs claimed");
    snapshot.rollback().await.expect("release the snapshot");
    read
}

/// The most one claim of four may read on its own backend: the [`RECENT`] dead due entries it
/// steps over under the snapshot (at most a block each, and far fewer in practice), and its own
/// handful of rows, never the backlog or an environment's history.
const MAX_CLAIM_BLOCKS: i64 = 400;

/// A claim reads its own work and a step over what died under the snapshot, at 6,000 waiting,
/// whatever the history: the same at 30 releases an environment and at 90. It failed first on
/// the claim as it was, two ways: the due set was every waiting job OR'd with every lapsed one,
/// read and sorted whole to keep four; and the UPDATE joined `destinations` on the environment
/// alone, so each claimed job was found by reading every job of its environment.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_claim_reads_a_handful_of_blocks_at_production_cardinalities() {
    let db = TestDb::new().await;
    // 100 projects: 600 destinations, 10 waiting releases each = 6,000 waiting jobs.
    let owner = installation(&db, 100, 30, 10).await;
    let short = one_claim(&db, &owner).await;
    // Sixty releases more of history in every environment, finished and VACUUMed.
    let me: Uuid = sqlx::query_scalar("SELECT created_by FROM destinations LIMIT 1")
        .fetch_one(&owner)
        .await
        .expect("alice");
    history(&owner, me, 31, 90, 90).await;
    sqlx::raw_sql("VACUUM ANALYZE jobs, releases")
        .execute(&owner)
        .await
        .expect("vacuum");
    let long = one_claim(&db, &owner).await;
    eprintln!("one claim of four, 6,000 waiting: {short} blocks at 30 releases, {long} at 90");
    for (history, read) in [(30, short), (90, long)] {
        assert!(
            read <= MAX_CLAIM_BLOCKS,
            "a claim read {read} blocks at {history} releases an environment"
        );
    }
    owner.close().await;
    db.drop().await;
}

/// The installation's size doesn't reach a claim either: at 2,000 projects (12,000 destinations,
/// 12,000 waiting, one each), a claim reads its own work. A generic plan read every destination
/// of the installation twice here (1,650 buffers a claim, scratch-database `EXPLAIN`), which is why
/// the claim is planned for its own limit every call.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_claim_reads_a_handful_of_blocks_across_a_large_installation() {
    let db = TestDb::new().await;
    let owner = installation(&db, 2_000, 3, 1).await;
    let read = one_claim(&db, &owner).await;
    eprintln!("one claim of four, 12,000 destinations, 12,000 waiting: {read} blocks");
    assert!(
        read <= MAX_CLAIM_BLOCKS,
        "a claim read {read} blocks across 12,000 destinations"
    );
    owner.close().await;
    db.drop().await;
}
