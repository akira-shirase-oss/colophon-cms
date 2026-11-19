//! D-217 against real PostgreSQL: a caller dropped while its `BEGIN` is at the server (a client
//! that went away, a request deadline) must not leave the pooled connection inside a
//! transaction. On sqlx 0.9.0's own `begin()` it did: the next borrower's autocommit writes ran
//! inside the leftover transaction, so a session just stored read back as missing (the testbed's
//! sign-in 401), and its `BEGIN` drew "there is already a transaction in progress". Needs the
//! local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::future::Future;
use std::time::Duration;

use colophon_db::{PgPool, PoolSettings, identity, projects, scope};
use common::TestDb;
use time::OffsetDateTime;

/// Connections on the test database that sit inside a transaction with nothing running.
async fn waiting_in_transaction(db: &TestDb) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM pg_stat_activity
         WHERE datname = $1 AND usename = 'colophon_app' AND state = 'idle in transaction'",
    )
    .bind(&db.name)
    .fetch_one(&db.admin)
    .await
    .expect("read pg_stat_activity")
}

/// Drops the future `make` returns at the moment the server has begun its transaction and the
/// answer is not yet read: exactly what a cancelled request does. A try whose begin finished
/// within one poll is rolled back and made again.
async fn cancel_mid_begin<F, T, E>(db: &TestDb, make: impl Fn() -> F)
where
    F: Future<Output = Result<T, E>>,
    E: std::fmt::Debug,
{
    for _ in 0..200 {
        let mut begin = std::pin::pin!(make());
        loop {
            let finished = tokio::select! {
                biased;
                done = &mut begin => Some(done),
                () = std::future::ready(()) => None,
            };
            if let Some(done) = finished {
                drop(done.expect("begin"));
                break;
            }
            if waiting_in_transaction(db).await == 1 {
                return; // `begin` drops here, its BEGIN answered and unread.
            }
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("never caught a begin in flight in 200 tries");
}

/// Whether the connection `pool` hands out next is outside any transaction.
async fn idle(pool: &PgPool) -> bool {
    {
        // The simple protocol: one message, so the statement's time is its transaction's.
        let row = sqlx::raw_sql("SELECT now() = statement_timestamp()")
            .fetch_one(pool)
            .await
            .expect("ask the connection");
        sqlx::Row::get(&row, 0)
    }
}

fn app_url(db: &TestDb) -> String {
    format!(
        "postgres://colophon_app:colophon-dev@{}/{}",
        common::host(),
        db.name
    )
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_cancelled_begin_tx_leaves_the_pooled_connection_outside_any_transaction() {
    let db = TestDb::new().await;
    // `db.app` is one connection with no release check: what begin_tx alone guarantees.
    cancel_mid_begin(&db, || colophon_db::begin_tx(&db.app)).await;
    assert!(
        idle(&db.app).await,
        "a cancelled begin_tx left a transaction open"
    );
    cancel_mid_begin(&db, || colophon_db::begin_tx_snapshot(&db.app)).await;
    assert!(
        idle(&db.app).await,
        "a cancelled begin_tx_snapshot left a transaction open"
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_session_stored_after_a_cancelled_begin_is_seen_by_the_next_request() {
    let db = TestDb::new().await;
    let person = identity::record_sign_in(&db.app, "test", "d217", None)
        .await
        .expect("an identity");
    let other = db.app_pool(1).await;
    // A project-scoped read cancelled as its request goes away, then the sign-in's INSERT.
    cancel_mid_begin(&db, || {
        scope::begin(
            &db.app,
            scope::Scope {
                identity: Some(person),
                project: None,
            },
        )
    })
    .await;
    let token = [7u8; 32];
    identity::create_session(
        &db.app,
        person,
        &token,
        &[8u8; 32],
        OffsetDateTime::now_utc() + Duration::from_secs(3600),
    )
    .await
    .expect("store the session");
    assert!(
        identity::find_session(&other, &token)
            .await
            .expect("look it up")
            .is_some(),
        "the session was written inside a transaction nobody commits"
    );
    other.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_pool_closes_a_connection_handed_back_inside_a_transaction() {
    let db = TestDb::new().await;
    let pool = colophon_db::connect_lazy(
        &app_url(&db),
        PoolSettings {
            max_connections: 1,
            ..PoolSettings::default()
        },
    )
    .expect("a pool");
    let before = colophon_db::transactions::dirty_releases();
    // sqlx's own begin, past begin_tx: the cancel that leaves the transaction open.
    cancel_mid_begin(&db, || pool.begin()).await;
    assert!(idle(&pool).await, "the leftover transaction was pooled");
    // And one begun by hand that has written: any way of leaving one open.
    let mut conn = pool.acquire().await.expect("a connection");
    sqlx::raw_sql("BEGIN; CREATE TEMPORARY TABLE d217 (x int)")
        .execute(&mut *conn)
        .await
        .expect("a hand-made transaction");
    drop(conn);
    assert!(idle(&pool).await, "the written transaction was pooled");
    assert!(
        colophon_db::transactions::dirty_releases() >= before + 2,
        "both closes counted"
    );
    pool.close().await;
    db.drop().await;
}

/// The row-level-security question: with a leftover transaction on the connection (here made
/// with sqlx's own begin, on a pool without the release check, so both guards are off), can a
/// later borrower read under a scope it didn't set? The scope is set only after a begin has
/// returned, and so only inside a transaction sqlx counts; a leftover one never carries a
/// scope, and the borrower that sets one ends the leftover with its own commit or rollback.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn after_a_leftover_transaction_no_borrower_reads_under_a_scope_it_did_not_set() {
    let db = TestDb::new().await;
    let owner = db.administrator("d217-owner").await;
    let project = projects::create(&db.app, owner, "d217", "D-217")
        .await
        .expect("a project")
        .id;
    let scoped = scope::Scope {
        identity: Some(owner),
        project: Some(project),
    };
    let unscoped = || async {
        let row: (Option<String>, Option<String>, i64) = sqlx::query_as(
            "SELECT current_setting('app.identity_id', true), current_setting('app.project_id', true),
                    (SELECT count(*) FROM projects)",
        )
        .fetch_one(&db.app)
        .await
        .expect("an unscoped read");
        assert!(
            row.0.as_deref().unwrap_or("").is_empty() && row.1.as_deref().unwrap_or("").is_empty(),
            "an unscoped statement ran under {row:?}"
        );
        assert_eq!(row.2, 0, "an unscoped statement saw a project");
    };

    for commit in [true, false] {
        cancel_mid_begin(&db, || db.app.begin()).await;
        assert!(!idle(&db.app).await, "the leftover transaction is there");
        unscoped().await;
        let mut tx = scope::begin(&db.app, scoped).await.expect("a scoped begin");
        let seen: i64 = sqlx::query_scalar("SELECT count(*) FROM projects")
            .fetch_one(&mut *tx)
            .await
            .expect("a scoped read");
        assert_eq!(seen, 1, "the scope works inside the leftover transaction");
        if commit {
            tx.commit().await.expect("commit");
        } else {
            drop(tx);
        }
        assert!(
            idle(&db.app).await,
            "the scoped transaction ended the leftover"
        );
        unscoped().await;
    }
    db.drop().await;
}
