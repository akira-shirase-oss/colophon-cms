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
    colophon_db::work::fold_payload(app)
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
    colophon_db::work::fold_payload(&app)
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

    server.terminate();
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
