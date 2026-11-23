//! The staff auth log and the sign-up cap (D-235, migration 0095) against real PostgreSQL: the
//! log is append-only for everyone but its retention sweep, which keeps the audit age; the
//! application role may only insert into it; the cap counts every attempt from one address in
//! an hour, under concurrency too, and starts over when the window ends. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::sync::Arc;

use colophon_db::{
    retention,
    staff_auth::{self, Event, Line},
};
use common::TestDb;
use uuid::Uuid;

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_staff_auth_log_takes_inserts_only_and_refuses_every_change() {
    let db = TestDb::new().await;
    let someone = db.person("someone").await;
    staff_auth::record(
        &db.app,
        Line {
            identity: Some(someone),
            ..Line::new(Event::SignedIn, "203.0.113.1")
        },
    )
    .await
    .expect("the application role inserts");
    // It can't read, change or remove what it wrote.
    for sql in [
        "SELECT count(*) FROM staff_auth_events",
        "UPDATE staff_auth_events SET detail = 'forged'",
        "DELETE FROM staff_auth_events",
        "TRUNCATE staff_auth_events",
    ] {
        let error = sqlx::query(sql)
            .execute(&db.app)
            .await
            .expect_err(sql)
            .into_database_error()
            .expect("a database error");
        assert_eq!(error.code().as_deref(), Some("42501"), "{sql}: {error}");
    }
    // Not even the table's owner changes it outside the sweep: the trigger refuses.
    let owner = db.owner().await;
    for sql in [
        "UPDATE staff_auth_events SET detail = 'forged'",
        "DELETE FROM staff_auth_events",
        "TRUNCATE staff_auth_events",
    ] {
        let error = sqlx::query(sql)
            .execute(&owner)
            .await
            .expect_err(sql)
            .into_database_error()
            .expect("a database error");
        assert_eq!(error.code().as_deref(), Some("42501"), "{sql}: {error}");
    }
    let lines: i64 = sqlx::query_scalar("SELECT count(*) FROM staff_auth_events")
        .fetch_one(&owner)
        .await
        .expect("count");
    assert_eq!(lines, 1);
    // A reason is a short word, never free text a caller could fill with a secret.
    let error = sqlx::query(
        "INSERT INTO staff_auth_events (id, event, detail) VALUES ($1, 'signed_in', 'my password is hunter2')",
    )
    .bind(Uuid::now_v7())
    .execute(&db.app)
    .await
    .expect_err("free text")
    .into_database_error()
    .expect("a database error");
    assert_eq!(error.code().as_deref(), Some("23514"), "{error}");
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn retention_keeps_the_staff_auth_log_for_the_audit_age_and_ends_signup_windows() {
    let db = TestDb::new().await;
    let owner = db.owner().await;
    // One line well inside the audit age, one past it; one window ended, one current.
    for days in [10_i32, 100] {
        sqlx::query("ALTER TABLE staff_auth_events DISABLE TRIGGER staff_auth_events_append_only")
            .execute(&owner)
            .await
            .expect("seed");
        sqlx::query(
            "INSERT INTO staff_auth_events (id, at, event, address)
             VALUES ($1, now() - make_interval(days => $2), 'signed_in', 'seed')",
        )
        .bind(Uuid::now_v7())
        .bind(days)
        .execute(&owner)
        .await
        .expect("seed a line");
        sqlx::query("ALTER TABLE staff_auth_events ENABLE TRIGGER staff_auth_events_append_only")
            .execute(&owner)
            .await
            .expect("seed");
    }
    sqlx::query(
        "INSERT INTO signup_throttle (address, window_started, attempts) VALUES
         ('198.51.100.1', now() - interval '2 hours', 5), ('198.51.100.2', now(), 5)",
    )
    .execute(&owner)
    .await
    .expect("seed windows");
    let swept = retention::sweep(&db.app, &retention::Retention::default(), 1000)
        .await
        .expect("sweep");
    assert_eq!((swept.staff_auth, swept.signup_throttle), (1, 1));
    let left: Vec<i32> = sqlx::query_scalar(
        "SELECT extract(day FROM now() - at)::int FROM staff_auth_events WHERE address = 'seed'",
    )
    .fetch_all(&owner)
    .await
    .expect("left");
    assert_eq!(left, [10], "the line inside the audit age stays");
    let windows: Vec<String> = sqlx::query_scalar("SELECT address FROM signup_throttle")
        .fetch_all(&owner)
        .await
        .expect("windows");
    assert_eq!(windows, ["198.51.100.2"]);
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_signup_cap_counts_every_attempt_even_racing_and_starts_over_after_the_hour() {
    let db = TestDb::new().await;
    let pool = db.app_pool(16).await;
    let barrier = Arc::new(tokio::sync::Barrier::new(16));
    let mut racing = tokio::task::JoinSet::new();
    for _ in 0..16 {
        let (pool, barrier) = (pool.clone(), Arc::clone(&barrier));
        racing.spawn(async move {
            barrier.wait().await;
            staff_auth::signup_attempt(&pool, "198.51.100.7", 10)
                .await
                .expect("attempt")
        });
    }
    let outcomes = racing.join_all().await;
    assert_eq!(
        outcomes.iter().filter(|o| o.is_none()).count(),
        10,
        "exactly the limit admitted: {outcomes:?}"
    );
    assert!(
        outcomes
            .iter()
            .flatten()
            .all(|wait| (3500..=3600).contains(wait)),
        "{outcomes:?}"
    );
    // Another address has its own count.
    assert_eq!(
        staff_auth::signup_attempt(&pool, "198.51.100.8", 10)
            .await
            .expect("attempt"),
        None
    );
    // An ended window starts over at one.
    let owner = db.owner().await;
    sqlx::query(
        "UPDATE signup_throttle SET window_started = now() - interval '61 minutes'
         WHERE address = '198.51.100.7'",
    )
    .execute(&owner)
    .await
    .expect("age the window");
    assert_eq!(
        staff_auth::signup_attempt(&pool, "198.51.100.7", 10)
            .await
            .expect("attempt"),
        None
    );
    let attempts: i32 =
        sqlx::query_scalar("SELECT attempts FROM signup_throttle WHERE address = '198.51.100.7'")
            .fetch_one(&owner)
            .await
            .expect("attempts");
    assert_eq!(attempts, 1);
    // The application role reaches the table only through the function.
    let error = sqlx::query("SELECT count(*) FROM signup_throttle")
        .execute(&db.app)
        .await
        .expect_err("no direct read")
        .into_database_error()
        .expect("a database error");
    assert_eq!(error.code().as_deref(), Some("42501"), "{error}");
    owner.close().await;
    pool.close().await;
    db.drop().await;
}
