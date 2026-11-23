//! D-226's fallback: with sqlx's notices filtered out before the tripwire (a log filter that turns
//! the `sqlx::postgres::notice` target off globally), the startup self-test fails and says why, and
//! every release stays checked, as before D-226: safety never degrades silently. A binary of its
//! own, since the subscriber is global. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::sync::Once;
use std::time::Duration;

use colophon_db::PoolSettings;
use colophon_db::transactions::{TripwireOff, release_checks};
use common::TestDb;
use tracing_subscriber::layer::SubscriberExt as _;

/// The tripwire installed, but behind a global filter that hides sqlx's notices from every layer.
fn install() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let subscriber = tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new(
                "info,sqlx::postgres::notice=off",
            ))
            .with(colophon_db::transactions::begin_tripwire());
        tracing::subscriber::set_global_default(subscriber)
            .expect("one global subscriber for this binary");
    });
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn with_the_notices_filtered_out_the_self_test_fails_and_every_release_stays_checked() {
    const N: u64 = 10;
    install();
    let db = TestDb::new().await;
    let url = format!(
        "postgres://colophon_app:colophon-dev@{}/{}",
        common::host(),
        db.name
    );
    let off = colophon_db::arm_release_guard(&url)
        .await
        .expect_err("blinded, the tripwire can't prove itself");
    assert!(
        matches!(off, TripwireOff::NoNotice),
        "the reason names the missing notice: {off}"
    );
    let pool = colophon_db::connect_lazy(
        &url,
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
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        release_checks() - before,
        N,
        "not armed, every release is checked"
    );
    pool.close().await;
    db.drop().await;
}
