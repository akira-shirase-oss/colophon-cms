//! D-239's lock timeout: what each pool's sessions wait for a lock, and that a background unit
//! keeps its own (D-236) whatever its pool says. Needs the stack and migrations.

#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::time::Duration;

use colophon_db::{PgPool, PoolSettings, REQUEST_LOCK_TIMEOUT, Stop, transactions};
use common::TestDb;

fn pool(db: &TestDb, settings: PoolSettings) -> PgPool {
    colophon_db::connect_lazy(
        &format!(
            "postgres://colophon_app:colophon-dev@{}/{}",
            common::host(),
            db.name
        ),
        settings,
    )
    .expect("a pool")
}

/// (`lock_timeout`, `statement_timeout`) as one of `pool`'s sessions has them.
async fn timeouts(executor: impl sqlx::PgExecutor<'_>) -> (String, String) {
    sqlx::query_as("SELECT current_setting('lock_timeout'), current_setting('statement_timeout')")
        .fetch_one(executor)
        .await
        .expect("the settings")
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn the_request_pool_waits_for_a_lock_less_long_than_a_statement_runs() {
    let db = TestDb::new().await;
    assert_eq!(REQUEST_LOCK_TIMEOUT, Duration::from_secs(3));
    let requests = pool(&db, PoolSettings::default());
    assert_eq!(
        timeouts(&requests).await,
        ("3s".to_owned(), "5s".to_owned())
    );
    // A pool that asks for none (the migrator's, the worker's) waits as long as a statement runs.
    let unbounded = pool(
        &db,
        PoolSettings {
            lock_timeout: None,
            ..PoolSettings::default()
        },
    );
    assert_eq!(
        timeouts(&unbounded).await,
        ("0".to_owned(), "5s".to_owned())
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_background_unit_keeps_its_own_bounds_whatever_its_pool_says() {
    let db = TestDb::new().await;
    for lock_timeout in [
        None,
        Some(Duration::from_secs(1)),
        Some(REQUEST_LOCK_TIMEOUT),
    ] {
        let pool = pool(
            &db,
            PoolSettings {
                lock_timeout,
                ..PoolSettings::default()
            },
        );
        let mut unit = transactions::begin_background(&pool, Stop::NEVER)
            .await
            .expect("a unit");
        assert_eq!(
            timeouts(&mut *unit).await,
            ("3s".to_owned(), "4s".to_owned()),
            "a pool with {lock_timeout:?}"
        );
        unit.rollback().await.expect("rollback");
        // And its session is the pool's again after it.
        let expected =
            lock_timeout.map_or_else(|| "0".to_owned(), |wait| format!("{}s", wait.as_secs()));
        assert_eq!(timeouts(&pool).await.0, expected);
        pool.close().await;
    }
}
