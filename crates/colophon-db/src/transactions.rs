//! Beginning a transaction so a cancelled caller can't leave one open on the pool (D-217).
//!
//! sqlx 0.9.0 counts a connection's transaction depth only once the server has answered
//! `BEGIN`: a `pool.begin()` dropped while waiting for that answer (a client that went away,
//! a request deadline) rolls back nothing, since its guard rolls back only at depth one or more.
//! The connection then returns to the pool inside the open transaction and still looks clean.
//! The next borrower's autocommit writes run inside that transaction, invisible to every other
//! connection (a session just written reads back as signed out), and its own `BEGIN` draws
//! "there is already a transaction in progress". Two guards:
//!
//! - [`begin_tx`] and [`begin_tx_snapshot`] run the `BEGIN` in a task of its own, so dropping the
//!   caller can't cut it short: the task finishes, and a transaction nobody is waiting for any
//!   more drops at depth one, which rolls it back before the connection is pooled again. They are
//!   the only way this workspace begins a transaction on a pool (`tests/one_begin.rs` checks).
//! - [`release_check`], the pool's `after_release` hook, closes any connection handed back while
//!   still inside a transaction, whatever left it there, and counts it ([`dirty_releases`]). It
//!   runs in sqlx's own release task, after the caller has its answer.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};

use sqlx::Transaction;
use sqlx::pool::PoolConnectionMetadata;
use sqlx::postgres::{PgConnection, PgPool, Postgres};

/// Begins a transaction on a connection from `pool`, at Postgres's default isolation.
pub async fn begin_tx(pool: &PgPool) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
    let pool = pool.clone();
    detached(async move { pool.begin().await }).await
}

/// Begins a `REPEATABLE READ` transaction: one snapshot for all its reads.
pub async fn begin_tx_snapshot(
    pool: &PgPool,
) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
    let pool = pool.clone();
    detached(async move {
        pool.begin_with("BEGIN ISOLATION LEVEL REPEATABLE READ")
            .await
    })
    .await
}

/// Runs `begin` to completion in its own task, even if the caller stops waiting. A task that
/// panicked or was cancelled (the runtime shutting down) is an error, never a panic here.
async fn detached<F>(begin: F) -> Result<Transaction<'static, Postgres>, sqlx::Error>
where
    F: Future<Output = Result<Transaction<'static, Postgres>, sqlx::Error>> + Send + 'static,
{
    tokio::spawn(begin)
        .await
        .map_err(|_| sqlx::Error::WorkerCrashed)?
}

/// Connections the pool closed because they came back inside a transaction.
static DIRTY: AtomicU64 = AtomicU64::new(0);

/// How many connections [`release_check`] has closed since the process started. Anything above
/// zero is a bug: a transaction began outside [`begin_tx`] and was left open.
pub fn dirty_releases() -> u64 {
    DIRTY.load(Ordering::Relaxed)
}

/// Outside a transaction a statement's transaction is the statement itself, so `now()` (the
/// transaction's start) equals `statement_timestamp()`; inside one they differ. The id check
/// catches a transaction that has written something, too. It goes as a simple query, one
/// message: the extended protocol's Parse, Bind and Execute each move `statement_timestamp()`
/// on, so the two would differ outside a transaction too.
const IDLE: &str =
    "SELECT now() = statement_timestamp() AND pg_current_xact_id_if_assigned() IS NULL";

/// The pool's `after_release` hook: `true` keeps the connection, `false` closes it. A
/// connection inside a transaction, or one the check can't run on (an aborted transaction
/// refuses every statement), is closed, counted and reported once, with no SQL in the report.
pub fn release_check(
    conn: &mut PgConnection,
    _meta: PoolConnectionMetadata,
) -> Pin<Box<dyn Future<Output = Result<bool, sqlx::Error>> + Send + '_>> {
    Box::pin(async move {
        let idle = sqlx::raw_sql(IDLE)
            .fetch_one(&mut *conn)
            .await
            .ok()
            .and_then(|row| sqlx::Row::try_get::<Option<bool>, _>(&row, 0).ok())
            .flatten()
            .unwrap_or(false);
        if !idle {
            let closed = DIRTY.fetch_add(1, Ordering::Relaxed) + 1;
            tracing::error!(
                closed,
                "a pooled connection came back inside a transaction; closed it (D-217)"
            );
        }
        Ok(idle)
    })
}
