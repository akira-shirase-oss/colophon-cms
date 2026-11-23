//! A stop never drops a database transaction under way. Every background loop (the payload
//! fold, the table sweep, the worker's sweeps, the asset retention pass, the change-hint pump)
//! hears a stop only between its units: the unit in flight runs to its end, so its work either
//! commits or fails with an error and is rolled back, and never stays unknown. They failed
//! first on the fold and on the table sweep, each of which raced its unit against the stop and
//! dropped it on the spot (a COMMIT dropped in flight queues a ROLLBACK the server answers with
//! "there is no transaction in progress": CI 44759556991's assets smoke, at shutdown). A unit
//! that can never finish is still bounded: its statements end server side
//! (`colophon_db::bound_background`), well inside the server's 5 s cleanup deadline.
//!
//! And the whole process: SIGTERM while a save waits on a row lock lets the save finish, the
//! server exits 0, and it logs no dropped transaction. Needs the stack and scripts/migrate.sh.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: a failed request or setup step is a test failure"
)]

mod common;

use std::{
    io::{BufRead as _, BufReader},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use common::{
    ORIGIN, PORT, article, call, create_article, entries, key, local_env, owner_database, project,
    sign_in,
};
use reqwest::Method;
use tokio_util::sync::CancellationToken;

/// How long a stopped loop is given to show it is still finishing its unit.
const STILL_RUNNING: Duration = Duration::from_millis(300);
/// The server's cleanup deadline: everything after the grace shares it.
const CLEANUP_DEADLINE: Duration = Duration::from_secs(5);

/// The application login's pool, as the server's worker uses it.
async fn app_database() -> sqlx::PgPool {
    let url = local_env()
        .into_iter()
        .find_map(|(key, value)| (key == "COLOPHON_DATABASE_URL").then_some(value))
        .expect("deploy/local.env names the application database");
    sqlx::PgPool::connect(&url)
        .await
        .expect("the stack's database")
}

/// Waits until a backend running a statement that contains `fragment` waits on a lock.
async fn waits_on_a_lock(owner: &sqlx::PgPool, fragment: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND wait_event_type = 'Lock'
               AND query LIKE '%' || $1 || '%' AND pid <> pg_backend_pid()",
        )
        .bind(fragment)
        .fetch_one(owner)
        .await
        .expect("pg_stat_activity");
        if waiting > 0 {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "nothing ever waited on {fragment}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn deltas(owner: &sqlx::PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM payload_deltas")
        .fetch_one(owner)
        .await
        .expect("count")
}

/// Two deltas that cancel out (the counter stays exact), and a holder keeping one of them
/// locked: the fold's DELETE of the log waits on it.
async fn a_held_delta(
    owner: &sqlx::PgPool,
    app: &sqlx::PgPool,
) -> sqlx::Transaction<'static, sqlx::Postgres> {
    colophon_db::work::fold_payload(app, colophon_db::Stop::NEVER)
        .await
        .expect("the log starts empty");
    sqlx::query("INSERT INTO payload_deltas (bytes) VALUES (7), (-7)")
        .execute(owner)
        .await
        .expect("two deltas");
    let mut holder = owner.begin().await.expect("begin");
    sqlx::query("SELECT 1 FROM payload_deltas LIMIT 1 FOR UPDATE")
        .execute(&mut *holder)
        .await
        .expect("hold a delta");
    holder
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_stop_lets_the_fold_under_way_commit() {
    let (owner, app) = (owner_database().await, app_database().await);
    let holder = a_held_delta(&owner, &app).await;
    let cancel = CancellationToken::new();
    let folder =
        tokio::spawn(colophon_worker::payload::Folder::new(app.clone()).run(cancel.clone()));
    waits_on_a_lock(&owner, "colophon_payload_tick").await;

    cancel.cancel();
    tokio::time::sleep(STILL_RUNNING).await;
    assert!(!folder.is_finished(), "the stop dropped the fold under way");
    holder.rollback().await.expect("release");
    tokio::time::timeout(CLEANUP_DEADLINE, folder)
        .await
        .expect("the fold finishes once the delta is free")
        .expect("joined");
    assert_eq!(deltas(&owner).await, 0, "the fold under way committed");
    app.close().await;
    owner.close().await;
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_stuck_fold_ends_on_its_own_inside_the_cleanup_deadline() {
    let (owner, app) = (owner_database().await, app_database().await);
    let holder = a_held_delta(&owner, &app).await;
    let cancel = CancellationToken::new();
    let folder =
        tokio::spawn(colophon_worker::payload::Folder::new(app.clone()).run(cancel.clone()));
    waits_on_a_lock(&owner, "colophon_payload_tick").await;

    // The delta is never released: the fold's lock wait ends server side, the fold fails and
    // rolls back, and the loop stops — before the server's cleanup deadline would give up.
    let stopped = Instant::now();
    cancel.cancel();
    tokio::time::timeout(CLEANUP_DEADLINE, folder)
        .await
        .expect("a stuck fold ends inside the cleanup deadline")
        .expect("joined");
    let took = stopped.elapsed();
    assert!(
        took <= colophon_db::BACKGROUND_LOCK_TIMEOUT + Duration::from_secs(1),
        "the stuck fold took {took:?}"
    );
    assert_eq!(deltas(&owner).await, 2, "the failed fold rolled back");
    holder.rollback().await.expect("release");
    colophon_db::work::fold_payload(&app, colophon_db::Stop::NEVER)
        .await
        .expect("fold the test's deltas away");
    app.close().await;
    owner.close().await;
}

/// The table sweep, its first table held, beside a stop: the pass doesn't return while its
/// statement under way waits. With `last` held too (the pass's last table), it must also run
/// nothing after the statement it was in once that one finishes.
async fn sweep_beside_a_stop(last: bool) -> Duration {
    let (owner, app) = (owner_database().await, app_database().await);
    // The pass's first delete is the settled budget ledger's, its last the preview grants':
    // a share lock on a table holds the delete.
    let mut first = owner.begin().await.expect("begin");
    sqlx::query("LOCK TABLE budget_reservations IN SHARE MODE")
        .execute(&mut *first)
        .await
        .expect("hold the ledger");
    let mut final_table = owner.begin().await.expect("begin");
    if last {
        sqlx::query("LOCK TABLE preview_grants IN SHARE MODE")
            .execute(&mut *final_table)
            .await
            .expect("hold the preview grants");
    }
    let cancel = CancellationToken::new();
    let sweeper = tokio::spawn(
        colophon_worker::tables::Sweeper::new(app.clone(), Duration::from_secs(3600))
            .run(cancel.clone()),
    );
    waits_on_a_lock(&owner, "budget_reservations").await;

    cancel.cancel();
    tokio::time::sleep(STILL_RUNNING).await;
    assert!(
        !sweeper.is_finished(),
        "the stop dropped the sweep pass under way"
    );
    let released = Instant::now();
    first.rollback().await.expect("release");
    tokio::time::timeout(CLEANUP_DEADLINE, sweeper)
        .await
        .expect("the pass ends once its statement does, then the sweep stops")
        .expect("joined");
    let took = released.elapsed();
    final_table.rollback().await.expect("release");
    app.close().await;
    owner.close().await;
    took
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_stop_lets_the_table_sweep_statement_under_way_finish() {
    sweep_beside_a_stop(false).await;
}

/// Statement and lock timeouts bound one statement, not a pass of twenty: a stopped pass runs
/// no statement after the one it was in, and rolls back. It failed first with the pass running
/// on to its last table and waiting there for the lock timeout.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_stopped_sweep_pass_runs_nothing_after_its_statement_under_way() {
    let took = sweep_beside_a_stop(true).await;
    assert!(
        took < Duration::from_secs(1),
        "the stopped pass ran on to its last table: {took:?} after its first was free"
    );
}

/// `colophon_db::begin_background` asks for the stop before anything else and once more after
/// the `BEGIN`: a stop already requested sends nothing to the server (no `BEGIN`, so no
/// `ROLLBACK` either), and one that comes while the `BEGIN` runs ends the unit before its first
/// statement, rolled back, never handed back as a transaction to run on.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn begin_background_begins_nothing_once_stopped() {
    let (pool, held) = an_exhausted_pool().await;
    let owner = owner_database().await;
    let mut held = held;
    let backend: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *held)
        .await
        .expect("the pool's one backend");
    drop(held);

    // Already stopped: nothing reaches the server. Any statement on the backend (a BEGIN, its
    // ROLLBACK, the pool's release check after them) would move its state_change.
    tokio::time::sleep(STILL_RUNNING).await;
    let changed = async || -> String {
        sqlx::query_scalar("SELECT state_change::text FROM pg_stat_activity WHERE pid = $1")
            .bind(backend)
            .fetch_one(&owner)
            .await
            .expect("the backend is still connected")
    };
    let before = changed().await;
    let stopped = || true;
    let begun = colophon_db::begin_background(&pool, colophon_db::Stop::new(&stopped)).await;
    assert!(
        matches!(begun, Err(colophon_db::DbError::Stopped)),
        "{begun:?}"
    );
    tokio::time::sleep(STILL_RUNNING).await;
    assert_eq!(
        changed().await,
        before,
        "a stopped unit still sent statements"
    );

    // Stopped once the connection is in use (during the BEGIN, at the latest right after it):
    // the unit ends before its first statement.
    let in_use = || pool.size() == 1 && pool.num_idle() == 0;
    let begun = colophon_db::begin_background(&pool, colophon_db::Stop::new(&in_use)).await;
    assert!(
        matches!(begun, Err(colophon_db::DbError::Stopped)),
        "a stop during the BEGIN handed the transaction back: {begun:?}"
    );
    tokio::time::sleep(STILL_RUNNING).await;
    let open: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_stat_activity WHERE pid = $1 AND state LIKE 'idle in transaction%'",
    )
    .bind(backend)
    .fetch_one(&owner)
    .await
    .expect("pg_stat_activity");
    assert_eq!(open, 0, "the stopped unit's transaction was rolled back");
    pool.close().await;
    owner.close().await;
}

/// The tables one table-sweep pass deletes from, in its order (`colophon_db::retention::sweep`).
const SWEEP_ORDER: [&str; 20] = [
    "budget_reservations",
    "observations",
    "jobs",
    "asset_jobs",
    "bulk_items",
    "bulk_jobs",
    "migration_items",
    "migration_plans",
    "schedules",
    "idempotency_records",
    "sessions",
    "access_tokens",
    "invitations",
    "device_logins",
    "sign_in_attempts",
    "setup_attempts",
    "setup_codes",
    "audience_magic_links",
    "sign_in_throttle",
    "preview_grants",
];

/// Every stop check of the sweep, each one: for each pair of neighbouring statements, both
/// tables held, a stop while the first statement waits, then its table released. The pass
/// must answer `Stopped` at once: a check missing after that statement would run the next,
/// which waits on its held table for the lock timeout and fails as a lock wait instead. (A
/// test holding only the first and last tables let any one check go missing: the next one
/// caught the stop.)
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn every_check_of_the_sweep_stops_it_after_the_statement_under_way() {
    let (owner, app) = (owner_database().await, app_database().await);
    for pair in SWEEP_ORDER.windows(2) {
        let (now, next) = (pair[0], pair[1]);
        let mut held = Vec::new();
        for table in pair {
            let mut holder = owner.begin().await.expect("begin");
            // Audited: `table` is one of SWEEP_ORDER's literals.
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "LOCK TABLE {table} IN SHARE MODE"
            )))
            .execute(&mut *holder)
            .await
            .expect("hold the table");
            let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *holder)
                .await
                .expect("its backend");
            held.push((holder, pid));
        }
        let stop_now = std::sync::atomic::AtomicBool::new(false);
        let stopping = || stop_now.load(std::sync::atomic::Ordering::SeqCst);
        let first_holder = held[0].1;
        let driver = async {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let waiting: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM pg_stat_activity WHERE $1 = ANY(pg_blocking_pids(pid))",
                )
                .bind(first_holder)
                .fetch_one(&owner)
                .await
                .expect("pg_stat_activity");
                if waiting > 0 {
                    break;
                }
                assert!(Instant::now() < deadline, "the pass never reached {now}");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            stop_now.store(true, std::sync::atomic::Ordering::SeqCst);
            let (first, _) = held.remove(0);
            first.rollback().await.expect("release");
            Instant::now()
        };
        let (swept, released) = tokio::join!(
            colophon_db::retention::sweep(
                &app,
                &colophon_db::retention::Retention::DEFAULT,
                colophon_db::retention::BATCH,
                colophon_db::Stop::new(&stopping),
            ),
            driver
        );
        let took = released.elapsed();
        assert!(
            matches!(swept, Err(colophon_db::DbError::Stopped)),
            "stopped at {now}, the pass ran on to {next}: {swept:?}"
        );
        assert!(
            took < Duration::from_secs(1),
            "stopped at {now}, the pass took {took:?} after it"
        );
        for (holder, _) in held {
            holder.rollback().await.expect("release");
        }
    }
    app.close().await;
    owner.close().await;
}

/// A stop that comes while a unit's COMMIT is itself under way: the COMMIT lands (the work is
/// saved, never left unknown), and the loop stops after it. The fold's COMMIT is made to take
/// 2 s by a deferred constraint trigger that fires only on this test's sentinel delta (7777),
/// so a trigger left behind by a failed run slows nothing else; it is dropped at the end.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_stop_mid_commit_lets_the_commit_land() {
    let (owner, app) = (owner_database().await, app_database().await);
    colophon_db::work::fold_payload(&app, colophon_db::Stop::NEVER)
        .await
        .expect("the log starts empty");
    sqlx::raw_sql(
        "CREATE OR REPLACE FUNCTION colophon_test_slow_commit() RETURNS trigger
             LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_sleep(2); RETURN NULL; END $$;
         DROP TRIGGER IF EXISTS colophon_test_slow_commit ON payload_deltas;
         CREATE CONSTRAINT TRIGGER colophon_test_slow_commit AFTER DELETE ON payload_deltas
             DEFERRABLE INITIALLY DEFERRED FOR EACH ROW WHEN (OLD.bytes = 7777)
             EXECUTE FUNCTION colophon_test_slow_commit();",
    )
    .execute(&owner)
    .await
    .expect("a slow commit for the sentinel");
    sqlx::query("INSERT INTO payload_deltas (bytes) VALUES (7777), (-7777)")
        .execute(&owner)
        .await
        .expect("the sentinel deltas");
    let cancel = CancellationToken::new();
    let folder =
        tokio::spawn(colophon_worker::payload::Folder::new(app.clone()).run(cancel.clone()));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let committing: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND state = 'active'
               AND upper(query) LIKE 'COMMIT%' AND pid <> pg_backend_pid()",
        )
        .fetch_one(&owner)
        .await
        .expect("pg_stat_activity");
        if committing > 0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the fold never reached its COMMIT"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    cancel.cancel();
    tokio::time::sleep(STILL_RUNNING).await;
    assert!(
        !folder.is_finished(),
        "the stop dropped the COMMIT under way"
    );
    tokio::time::timeout(CLEANUP_DEADLINE, folder)
        .await
        .expect("the COMMIT lands inside the cleanup deadline")
        .expect("joined");
    let left = deltas(&owner).await;
    sqlx::raw_sql(
        "DROP TRIGGER IF EXISTS colophon_test_slow_commit ON payload_deltas;
         DROP FUNCTION IF EXISTS colophon_test_slow_commit();",
    )
    .execute(&owner)
    .await
    .expect("drop the slow commit");
    assert_eq!(left, 0, "the fold under way at the stop committed");
    app.close().await;
    owner.close().await;
}

/// A one-connection pool on the application login, the worker pool's settings otherwise (its
/// 2 s acquire timeout included), and that connection held: a unit started on it waits in
/// pool acquisition, as a sweep does on a worker pool its deliveries have exhausted.
async fn an_exhausted_pool() -> (sqlx::PgPool, sqlx::pool::PoolConnection<sqlx::Postgres>) {
    let url = local_env()
        .into_iter()
        .find_map(|(key, value)| (key == "COLOPHON_DATABASE_URL").then_some(value))
        .expect("deploy/local.env names the application database");
    let pool = colophon_db::connect_lazy(
        &url,
        colophon_db::PoolSettings {
            max_connections: 1,
            ..colophon_db::PoolSettings::default()
        },
    )
    .expect("a pool");
    let held = pool.acquire().await.expect("the pool's one connection");
    (pool, held)
}

/// The pre-review's item 1: a stop that finds a unit still waiting for a connection ends the
/// loop at once, before the unit begins (dropping a begin is safe, D-217), so shutdown never
/// waits on the acquire timeout plus a statement. Once the connection is free, nothing began:
/// the log the fold would have emptied is untouched. It failed first: the loop waited for the
/// connection, then folded after the stop.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_stop_while_the_fold_waits_for_a_connection_begins_nothing() {
    let (owner, app) = (owner_database().await, app_database().await);
    colophon_db::work::fold_payload(&app, colophon_db::Stop::NEVER)
        .await
        .expect("the log starts empty");
    sqlx::query("INSERT INTO payload_deltas (bytes) VALUES (7), (-7)")
        .execute(&owner)
        .await
        .expect("two deltas");
    let (pool, held) = an_exhausted_pool().await;
    let cancel = CancellationToken::new();
    let folder =
        tokio::spawn(colophon_worker::payload::Folder::new(pool.clone()).run(cancel.clone()));
    tokio::time::sleep(STILL_RUNNING).await;
    assert!(
        !folder.is_finished(),
        "the fold waits for the held connection"
    );

    cancel.cancel();
    tokio::time::sleep(STILL_RUNNING).await;
    assert!(
        folder.is_finished(),
        "a fold still waiting for a connection held up the stop"
    );
    drop(held);
    tokio::time::sleep(STILL_RUNNING).await;
    assert_eq!(deltas(&owner).await, 2, "nothing began after the stop");
    folder.await.expect("joined");
    colophon_db::work::fold_payload(&app, colophon_db::Stop::NEVER)
        .await
        .expect("fold the test's deltas away");
    pool.close().await;
    app.close().await;
    owner.close().await;
}

/// The same for a unit that runs as `colophon_worker` (the table sweep): queued for a
/// connection at the stop, it never begins.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_stop_while_the_table_sweep_waits_for_a_connection_begins_nothing() {
    let (pool, mut held) = an_exhausted_pool().await;
    let owner = owner_database().await;
    let backend: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *held)
        .await
        .expect("the held connection's backend");
    let cancel = CancellationToken::new();
    let sweeper = tokio::spawn(
        colophon_worker::tables::Sweeper::new(pool.clone(), Duration::from_secs(3600))
            .run(cancel.clone()),
    );
    tokio::time::sleep(STILL_RUNNING).await;
    assert!(
        !sweeper.is_finished(),
        "the sweep waits for the held connection"
    );

    cancel.cancel();
    tokio::time::sleep(STILL_RUNNING).await;
    assert!(
        sweeper.is_finished(),
        "a sweep still waiting for a connection held up the stop"
    );
    drop(held);
    sweeper.await.expect("joined");
    // The pool's one backend, once free, ran none of a unit's statements.
    tokio::time::sleep(STILL_RUNNING).await;
    let last: String = sqlx::query_scalar("SELECT query FROM pg_stat_activity WHERE pid = $1")
        .bind(backend)
        .fetch_one(&owner)
        .await
        .expect("the backend is still connected");
    let upper = last.to_uppercase();
    assert!(
        ![
            "BEGIN",
            "COMMIT",
            "ROLLBACK",
            "COLOPHON_WORKER",
            "COLOPHON_SWEEP",
            "SET_CONFIG"
        ]
        .iter()
        .any(|word| upper.contains(word)),
        "a unit began after the stop; the backend last ran: {last}"
    );
    pool.close().await;
    owner.close().await;
}

/// An approved publish request for a new article in `project`, scheduled `seconds` from now in
/// UTC (the HTTP path an editor takes): the changeset's path.
async fn scheduled(owner: &common::Session, project: &str, seconds: i64) -> String {
    let base = format!("/api/v1/projects/{project}/environments/production");
    let entry = create_article(owner, project, "Scheduled at a stop").await;
    let set = call(
        owner,
        Method::POST,
        &format!("{base}/changesets"),
        Some(&serde_json::json!({"title": "Scheduled at a stop", "entries": [entry]})),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert!(set.status.is_success(), "{}", set.body);
    let path = format!(
        "{base}/changesets/{}",
        set.body["data"]["id"].as_str().expect("an id")
    );
    let if_match = format!("\"{}\"", set.body["data"]["hash"].as_str().expect("a hash"));
    let at = (jiff::Timestamp::now() + jiff::SignedDuration::from_secs(seconds))
        .strftime("%Y-%m-%dT%H:%M:%S")
        .to_string();
    for (step, body) in [
        ("submit", None),
        ("reviews", Some(serde_json::json!({"decision": "approve"}))),
        (
            "schedule",
            Some(serde_json::json!({"local_time": at, "time_zone": "UTC"})),
        ),
    ] {
        let done = call(
            owner,
            Method::POST,
            &format!("{path}/{step}"),
            body.as_ref(),
            &[("if-match", &if_match)],
        )
        .await
        .expect("request");
        assert!(done.status.is_success(), "{step}: {}", done.body);
    }
    path
}

/// The coordinator's scheduler ruling (D-197): a scheduled publish is an editorial publish, not
/// a background unit, so a stop neither bounds it to 4 s nor rolls it back between statements;
/// it keeps the HTTP publish's own bound, every pool's 5 s statement timeout. Blocked on a held
/// lock when the stop comes, it fails at that bound and rolls back, and the loop ends inside the
/// 5 s cleanup deadline, never waiting on systemd's kill. The next start publishes the schedule
/// exactly once (one release, generation 1), and another pass after that publishes nothing more:
/// the schedule row is consumed in the publish's own transaction.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one schedule told in order: blocked at a stop, rolled back, published once"
)]
async fn a_scheduled_publish_blocked_at_a_stop_ends_by_the_deadline_and_publishes_once() {
    // The API alone: the test runs the scheduler itself, on the worker pool's settings.
    let _server = common::Server::start_with(&[("COLOPHON_ROLE", "api")]).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    scheduled(&owner, &project, 2).await;
    let url = local_env()
        .into_iter()
        .find_map(|(key, value)| (key == "COLOPHON_DATABASE_URL").then_some(value))
        .expect("deploy/local.env names the application database");
    let worker_pool =
        colophon_db::connect_lazy(&url, colophon_db::PoolSettings::default()).expect("a pool");
    let database = owner_database().await;
    let releases = async || -> (i64, Option<i64>) {
        sqlx::query_as("SELECT count(*), max(generation) FROM releases WHERE project_id = $1::uuid")
            .bind(&project)
            .fetch_one(&database)
            .await
            .expect("the releases")
    };
    let state = async || -> String {
        sqlx::query_scalar("SELECT state FROM schedules WHERE project_id = $1::uuid")
            .bind(&project)
            .fetch_one(&database)
            .await
            .expect("the schedule")
    };

    // The publish's release insert waits on the table, held.
    let mut holder = database.begin().await.expect("begin");
    sqlx::query("LOCK TABLE releases IN SHARE MODE")
        .execute(&mut *holder)
        .await
        .expect("hold the releases");
    let holder_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *holder)
        .await
        .expect("its backend");
    let cancel = CancellationToken::new();
    let scheduler = tokio::spawn(
        colophon_worker::schedules::Scheduler::new(worker_pool.clone(), Duration::from_millis(200))
            .run(cancel.clone()),
    );
    // The blocked statement's own start: the bound under test, every pool's 5 s statement
    // timeout, runs from there, not from the stop (measured from the stop, the margin was only
    // how soon this loop saw the wait, and a loaded run tipped over it once).
    let deadline = Instant::now() + Duration::from_secs(15);
    let started_ago: f64 = loop {
        let started: Option<f64> = sqlx::query_scalar(
            "SELECT extract(epoch FROM clock_timestamp() - min(query_start))::float8
             FROM pg_stat_activity WHERE $1 = ANY(pg_blocking_pids(pid))",
        )
        .bind(holder_pid)
        .fetch_one(&database)
        .await
        .expect("pg_stat_activity");
        if let Some(started) = started {
            break started;
        }
        assert!(
            Instant::now() < deadline,
            "the scheduled publish never reached the releases"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let statement_started = Instant::now()
        .checked_sub(Duration::from_secs_f64(started_ago.max(0.0)))
        .expect("the statement started after the process");

    cancel.cancel();
    // At most the statement timeout from the statement's start, then its rollback: a second
    // for the rollback and the loop's exit.
    let bound = Duration::from_secs(5) + Duration::from_secs(1);
    tokio::time::timeout(bound, scheduler)
        .await
        .expect("the scheduler ends within its statement's bound")
        .expect("joined");
    let took = statement_started.elapsed();
    holder.rollback().await.expect("release");
    assert!(
        took < bound,
        "the blocked publish ran {took:?} from its statement's start"
    );
    assert_eq!(
        releases().await,
        (0, None),
        "the blocked publish rolled back"
    );
    assert_eq!(state().await, "scheduled", "and left its schedule due");

    // The next start: published once, then nothing more on another pass.
    for pass in 0..2 {
        let cancel = CancellationToken::new();
        let scheduler = tokio::spawn(
            colophon_worker::schedules::Scheduler::new(
                worker_pool.clone(),
                Duration::from_millis(200),
            )
            .run(cancel.clone()),
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        while state().await != "published" {
            assert!(Instant::now() < deadline, "the schedule never published");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        tokio::time::sleep(Duration::from_millis(600)).await;
        cancel.cancel();
        scheduler.await.expect("joined");
        assert_eq!(
            releases().await,
            (1, Some(1)),
            "published exactly once (pass {pass})"
        );
    }
    worker_pool.close().await;
    database.close().await;
}

/// The real binary on the test origin, its JSON log kept.
struct Logged {
    child: Child,
    lines: Arc<Mutex<Vec<String>>>,
}

impl Logged {
    async fn start() -> Self {
        common::assert_no_foreign_server();
        let mut child = Command::new(env!("CARGO_BIN_EXE_colophon-server"))
            .env_clear()
            .envs(local_env())
            .env("COLOPHON_LISTEN_ADDR", format!("127.0.0.1:{PORT}"))
            .env("COLOPHON_PUBLIC_URL", ORIGIN)
            .env("COLOPHON_LOG_FORMAT", "json")
            .env("COLOPHON_RATE_LIMIT_PASSWORD", "1000/2000")
            .env("COLOPHON_DRAIN_DELAY_MS", "200")
            .env("COLOPHON_SHUTDOWN_GRACE_MS", "10000")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the server starts");
        let stdout = child.stdout.take().expect("stdout is piped");
        let lines = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&lines);
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                kept.lock().expect("the log").push(line);
            }
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while reqwest::get(format!("http://127.0.0.1:{PORT}/livez"))
            .await
            .is_err()
        {
            assert!(Instant::now() < deadline, "the server never answered");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Self { child, lines }
    }

    fn terminate(&self) {
        let status = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status()
            .expect("kill runs");
        assert!(status.success());
    }

    async fn exit_code(&mut self, within: Duration) -> Option<i32> {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().expect("wait") {
                return status.code();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        None
    }

    fn log(&self) -> Vec<String> {
        self.lines.lock().expect("the log").clone()
    }
}

impl Drop for Logged {
    fn drop(&mut self) {
        let _already_gone = self.child.kill();
        let _reaped = self.child.wait();
    }
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn sigterm_mid_save_lets_the_save_finish_and_drops_no_transaction() {
    stopped_mid_save(1).await;
}

/// A second SIGTERM while the first's drain is under way changes nothing: no forced exit, the
/// save under way still finishes, the server still exits 0 and drops no transaction.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_second_sigterm_mid_save_changes_nothing() {
    stopped_mid_save(2).await;
}

/// A save waiting on a held row lock, `signals` SIGTERMs (100 ms apart), then the row freed.
async fn stopped_mid_save(signals: usize) {
    let mut server = Logged::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let id = create_article(&owner, &project, "Held").await;
    let path = format!("{}/{id}", entries(&project));
    let read = call(&owner, Method::GET, &path, None, &[])
        .await
        .expect("read");
    let etag = read.etag().expect("an etag");

    // The entry's row is held, so the save waits on it inside its transaction.
    let database = owner_database().await;
    let mut holder = database.begin().await.expect("begin");
    sqlx::query("SELECT 1 FROM entries WHERE id = $1::uuid FOR UPDATE")
        .bind(&id)
        .execute(&mut *holder)
        .await
        .expect("hold the entry");
    let save = tokio::spawn({
        let (owner, path) = (owner.clone(), path.clone());
        async move {
            call(
                &owner,
                Method::PUT,
                &path,
                Some(&article("Saved", "text")),
                &[("idempotency-key", &key()), ("if-match", &etag)],
            )
            .await
        }
    });
    waits_on_a_lock(&database, "entries").await;

    for _ in 0..signals {
        server.terminate();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tokio::time::sleep(STILL_RUNNING).await;
    holder.rollback().await.expect("release");
    let saved = tokio::time::timeout(Duration::from_secs(10), save)
        .await
        .expect("the save answers")
        .expect("joined")
        .expect("a response, not a dropped connection");
    assert!(
        saved.status.is_success(),
        "the save under way at SIGTERM: {} {}",
        saved.status,
        saved.body
    );
    assert_eq!(
        server.exit_code(Duration::from_secs(15)).await,
        Some(0),
        "a clean exit"
    );
    let dropped: Vec<String> = server
        .log()
        .into_iter()
        .filter(|line| line.contains("transaction in progress"))
        .collect();
    assert!(dropped.is_empty(), "a dropped transaction: {dropped:?}");
    database.close().await;
}
