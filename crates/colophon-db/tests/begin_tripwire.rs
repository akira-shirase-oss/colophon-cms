//! D-226's begin tripwire with it installed, as the server installs it: the startup self-test arms
//! it, a desynced connection's next begin is refused and rolled back, and once armed a transaction's
//! release costs no statement of its own (one release in `CANARY_EVERY` is still checked). The
//! subscriber is global (sqlx emits the notice inside the begin's own task), so these share one
//! test binary; `begin_tripwire_off.rs` proves the fallback with the target filtered out. Needs the
//! local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::sync::Once;
use std::time::Duration;

use colophon_db::transactions::{CANARY_EVERY, begin_trips, release_checks};
use colophon_db::{PgPool, PoolSettings};
use common::TestDb;
use tracing_subscriber::layer::SubscriberExt as _;

/// The server's subscriber, reduced to what matters here: the tripwire, with its own filter.
fn install() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let subscriber =
            tracing_subscriber::registry().with(colophon_db::transactions::begin_tripwire());
        tracing::subscriber::set_global_default(subscriber)
            .expect("one global subscriber for this binary");
    });
}

fn app_url(db: &TestDb) -> String {
    format!(
        "postgres://colophon_app:colophon-dev@{}/{}",
        common::host(),
        db.name
    )
}

async fn idle(pool: &PgPool) -> bool {
    let row = sqlx::raw_sql(
        "SELECT now() = statement_timestamp() AND pg_current_xact_id_if_assigned() IS NULL",
    )
    .fetch_one(pool)
    .await
    .expect("ask the connection");
    sqlx::Row::get(&row, 0)
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_startup_self_test_arms_the_tripwire() {
    install();
    let db = TestDb::new().await;
    colophon_db::arm_release_guard(&app_url(&db))
        .await
        .expect("the tripwire proves itself");
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_begin_on_a_connection_already_in_a_transaction_is_refused_and_rolled_back() {
    install();
    let db = TestDb::new().await;
    // `db.app`: one connection, no release check. A BEGIN sent outside the transaction manager (what
    // tests/one_begin.rs forbids in code) leaves it inside a transaction that sqlx doesn't know of.
    let mut conn = db.app.acquire().await.expect("a connection");
    sqlx::raw_sql("BEGIN")
        .execute(&mut *conn)
        .await
        .expect("a hand-made transaction");
    drop(conn);
    let trips = begin_trips();
    let refused = colophon_db::begin_tx(&db.app).await;
    assert!(
        matches!(refused, Err(sqlx::Error::Protocol(_))),
        "a begin inside a transaction must be refused, not handed out: {refused:?}"
    );
    assert_eq!(begin_trips(), trips + 1, "the refusal is counted");
    assert!(
        idle(&db.app).await,
        "the refusal rolled the leftover transaction back"
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn once_armed_a_transaction_costs_no_extra_statement_at_release() {
    const N: u64 = 40;
    install();
    let db = TestDb::new().await;
    colophon_db::arm_release_guard(&app_url(&db))
        .await
        .expect("armed");
    // The pool the server builds, release hook and all, on one connection.
    let pool = colophon_db::connect_lazy(
        &app_url(&db),
        PoolSettings {
            max_connections: 1,
            ..PoolSettings::default()
        },
    )
    .expect("a pool");
    sqlx::raw_sql("SELECT 1")
        .execute(&pool)
        .await
        .expect("open the connection");
    // That statement's own release runs in a task of sqlx's: let it finish before counting.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let before = release_checks();
    for _ in 0..N {
        let mut tx = colophon_db::begin_tx(&pool).await.expect("begin");
        sqlx::query("SELECT 1")
            .execute(&mut *tx)
            .await
            .expect("a statement");
        tx.commit().await.expect("commit");
    }
    // The pool returns a connection in a task of its own: let the last one finish.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let checks = release_checks() - before;
    // On Neon each check is a round trip the connection spends before it can be reused (D-226: the
    // check doubled a read's statements). Armed, only the canary's one in CANARY_EVERY remains.
    assert!(
        checks <= N.div_ceil(CANARY_EVERY),
        "{N} transactions sent {checks} release-check statements; armed, at most {} may",
        N.div_ceil(CANARY_EVERY)
    );
    pool.close().await;
    db.drop().await;
}
