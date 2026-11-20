//! D-217's guarantee, path by path, against real PostgreSQL (D-226): however a transaction ends,
//! the pooled connection it used is handed out next outside any transaction. sqlx 0.9.0 keeps this
//! itself for every path but a cancelled `BEGIN` (which `begin_tx` guards): a `Transaction` dropped
//! while open queues a `ROLLBACK`, and the pool's own release `ping` flushes it and reads every
//! pending answer before the connection is pooled again (sqlx-core `pool/connection.rs`
//! `return_to_pool`; sqlx-postgres `start_rollback`, `ping`, `wait_until_ready`).
//!
//! Each test runs its path on `db.app`, a pool of ONE connection with no release check (what sqlx
//! and `begin_tx` guarantee alone), so the next borrower provably gets the same connection. It then
//! asserts that connection is idle, and that an autocommit write made on it right after is visible
//! from a second pool: a leftover transaction would hide it. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::time::Duration;

use colophon_db::{PgPool, identity};
use common::TestDb;

/// Whether the connection `pool` hands out next is outside any transaction. The simple protocol:
/// one message, so outside a transaction the statement's time is its transaction's start.
async fn idle(pool: &PgPool) -> bool {
    let row = sqlx::raw_sql(
        "SELECT now() = statement_timestamp() AND pg_current_xact_id_if_assigned() IS NULL",
    )
    .fetch_one(pool)
    .await
    .expect("ask the connection");
    sqlx::Row::get(&row, 0)
}

/// Writes an identity through `db.app` (autocommit) and says whether a second pool sees it.
async fn next_write_is_visible(db: &TestDb, subject: &str) -> bool {
    identity::record_sign_in(&db.app, "test", subject, None)
        .await
        .expect("an autocommit write after the path");
    let owner = db.owner().await;
    let seen: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM identities WHERE issuer = 'test' AND subject = $1",
    )
    .bind(subject)
    .fetch_one(&owner)
    .await
    .expect("read from a second pool");
    owner.close().await;
    seen == 1
}

/// The path's connection is idle and the next write on it commits.
async fn assert_clean(db: &TestDb, path: &str) {
    assert!(
        idle(&db.app).await,
        "{path}: the pooled connection is still inside a transaction"
    );
    assert!(
        next_write_is_visible(db, path).await,
        "{path}: the next autocommit write ran inside a leftover transaction"
    );
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_handler_dropped_between_two_statements_leaves_its_connection_idle() {
    let db = TestDb::new().await;
    // A handler that has run one statement in its transaction and is awaiting the second when its
    // request's deadline (or a client disconnect) drops it.
    let handler = async {
        let mut tx = colophon_db::begin_tx(&db.app).await.expect("begin");
        sqlx::query("SELECT 1")
            .execute(&mut *tx)
            .await
            .expect("the first statement");
        // Two seconds: dropping the future doesn't stop the statement at the server, and the
        // release waits for it to finish before pooling the connection (bounded in production by
        // statement_timeout). A longer sleep only makes this test slower.
        sqlx::query("SELECT pg_sleep(2)")
            .execute(&mut *tx)
            .await
            .expect("the second statement");
        tx.commit().await.expect("never reached");
    };
    let dropped = tokio::time::timeout(Duration::from_millis(300), handler).await;
    assert!(
        dropped.is_err(),
        "the handler must be dropped mid-transaction, not finish"
    );
    assert_clean(&db, "dropped-between-statements").await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_cancelled_commit_leaves_its_connection_idle() {
    let db = TestDb::new().await;
    for attempt in 0..20 {
        let mut tx = colophon_db::begin_tx(&db.app).await.expect("begin");
        sqlx::query("SELECT 1")
            .execute(&mut *tx)
            .await
            .expect("a statement");
        // Boxed, so dropping it drops the future and the connection it holds (`pin!` would keep
        // the future alive in its stack slot until the end of the scope).
        let mut commit = Box::pin(tx.commit());
        // Poll the COMMIT once, then drop it: sent or only buffered, its answer unread.
        let finished = tokio::select! {
            biased;
            done = &mut commit => Some(done),
            () = std::future::ready(()) => None,
        };
        if let Some(done) = finished {
            done.expect("commit");
            continue; // finished within one poll; try again for an in-flight one
        }
        drop(commit);
        assert_clean(&db, &format!("cancelled-commit-{attempt}")).await;
        db.drop().await;
        return;
    }
    panic!("every COMMIT finished within one poll; none was caught in flight");
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_transaction_dropped_after_an_error_leaves_its_connection_idle() {
    let db = TestDb::new().await;
    {
        let mut tx = colophon_db::begin_tx(&db.app).await.expect("begin");
        // The transaction is now aborted ('E'): the server refuses everything until it ends.
        let error = sqlx::query("SELECT 1 / 0").execute(&mut *tx).await;
        assert!(error.is_err(), "division by zero must fail");
        let refused = sqlx::query("SELECT 1").execute(&mut *tx).await;
        assert!(
            refused.is_err(),
            "an aborted transaction refuses the next statement"
        );
        // Dropped without rollback, as a handler returning the error does.
    }
    assert_clean(&db, "dropped-after-error").await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_panic_with_a_transaction_open_leaves_its_connection_idle() {
    let db = TestDb::new().await;
    let pool = db.app.clone();
    let panicked = tokio::spawn(async move {
        let mut tx = colophon_db::begin_tx(&pool).await.expect("begin");
        sqlx::query("SELECT 1")
            .execute(&mut *tx)
            .await
            .expect("a statement");
        panic!("a handler bug with its transaction open");
    })
    .await;
    assert!(
        panicked.is_err_and(|error| error.is_panic()),
        "the task must have panicked"
    );
    assert_clean(&db, "panic-mid-transaction").await;
    db.drop().await;
}
