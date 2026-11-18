//! A test that fails leaves nothing on the stack: its database is dropped however it ends.
//! Needs the stack (`cargo test -- --ignored`), like every other database test here.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::PgPool;
use sqlx::AssertSqlSafe;

/// A test that panics halfway, after making its database, leaves no `colophon_test_*` database
/// behind: the database goes as the test unwinds, not only when the test reaches its end.
#[tokio::test]
#[ignore = "needs the stack: scripts/stack.sh up"]
async fn a_panicking_test_leaves_no_database_behind() {
    let (sender, made) = std::sync::mpsc::channel();
    // The failing "test": its own thread and runtime, so its panic can be watched from here.
    let outcome = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("invariant: a current-thread runtime builds");
        runtime.block_on(async move {
            let db = common::TestDb::new().await;
            sender
                .send(db.name.clone())
                .expect("invariant: the receiver waits below");
            panic!("a test fails halfway, before it drops its database");
        });
    })
    .join();
    assert!(outcome.is_err(), "the failing test panicked, as it should");
    let name = made
        .recv()
        .expect("the failing test made its database first");

    let admin = PgPool::connect(&format!(
        "postgres://colophon:colophon-dev@{}/postgres",
        common::host()
    ))
    .await
    .expect("the stack's database answers");
    let left: Option<i32> = sqlx::query_scalar("SELECT 1 FROM pg_database WHERE datname = $1")
        .bind(&name)
        .fetch_optional(&admin)
        .await
        .expect("read pg_database");
    if left.is_some() {
        // Don't add to the pile this test is about, then fail.
        sqlx::query(AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS {name} WITH (FORCE)"
        )))
        .execute(&admin)
        .await
        .expect("drop the leftover");
    }
    assert!(
        left.is_none(),
        "{name} was left behind by a test that panicked"
    );
}
