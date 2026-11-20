//! Reads and writes draw on pools of their own (D-225): writers stuck behind a lock can take
//! every write connection and reads still answer at once, and a read abandoned mid-statement
//! gives its connection back within the read pool's statement timeout. Both fail on a single
//! shared pool: the stuck writers hold every connection, so reads wait out the acquire timeout
//! and answer 503; an abandoned read's statement runs on to the one pool's 5 s timeout (sqlx 0.9
//! can't cancel a statement whose future was dropped, D-226). Needs the stack and
//! scripts/migrate.sh.
#![expect(
    clippy::expect_used,
    reason = "integration tests: a failed request or setup step is a test failure"
)]

mod common;

use std::time::{Duration, Instant};

use common::{
    ORIGIN, Server, app, article, call, create_article, entries, key, owner_database, project,
    sign_in,
};
use reqwest::{Method, StatusCode, header};

/// Each pool's size for these tests: small, so a handful of stuck writers fill it.
const CONNECTIONS: &str = "6";
/// The read pool's statement timeout (D-225): the bound on an abandoned read.
const READ_STATEMENT_TIMEOUT: Duration = Duration::from_secs(2);

async fn start() -> Server {
    Server::start_with(&[
        // Today's one pool, and D-225's two: the same small size for each.
        ("COLOPHON_DB_MAX_CONNECTIONS", CONNECTIONS),
        ("COLOPHON_DB_READ_CONNECTIONS", CONNECTIONS),
        ("COLOPHON_DB_WRITE_CONNECTIONS", CONNECTIONS),
    ])
    .await
}

/// The server's backends (the application login) waiting on a lock.
async fn waiting_on_locks(owner: &sqlx::PgPool) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM pg_stat_activity
         WHERE datname = current_database() AND usename = 'colophon_app'
           AND wait_event_type = 'Lock'",
    )
    .fetch_one(owner)
    .await
    .expect("pg_stat_activity")
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn reads_answer_at_once_while_writers_hold_every_write_connection() {
    let _server = start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let id = create_article(&owner, &project, "Held").await;
    let path = format!("{}/{id}", entries(&project));
    let read = call(&owner, Method::GET, &path, None, &[])
        .await
        .expect("read");
    let etag = read.etag().expect("an etag");

    // The entry's row is held, so every save of it waits, each holding a write connection.
    let database = owner_database().await;
    let mut holder = database.begin().await.expect("begin");
    sqlx::query("SELECT 1 FROM entries WHERE id = $1::uuid FOR UPDATE")
        .bind(&id)
        .execute(&mut *holder)
        .await
        .expect("hold the entry");
    let mut writers = tokio::task::JoinSet::new();
    for n in 0..10 {
        let (owner, path, etag) = (owner.clone(), path.clone(), etag.clone());
        writers.spawn(async move {
            call(
                &owner,
                Method::PUT,
                &path,
                Some(&article(&format!("Save {n}"), "text")),
                &[("idempotency-key", &key()), ("if-match", &etag)],
            )
            .await
        });
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while waiting_on_locks(&database).await < 6 {
        assert!(Instant::now() < deadline, "the writers never filled the pool");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // A hundred reads, ten at a time: every one answered, fast.
    let mut latencies = Vec::new();
    for _ in 0..10 {
        let mut batch = tokio::task::JoinSet::new();
        for _ in 0..10 {
            let (owner, list) = (owner.clone(), entries(&project));
            batch.spawn(async move {
                let started = Instant::now();
                let reply = call(&owner, Method::GET, &list, None, &[])
                    .await
                    .expect("read");
                (reply.status, started.elapsed())
            });
        }
        while let Some(done) = batch.join_next().await {
            let (status, took) = done.expect("reader");
            assert_eq!(status, StatusCode::OK, "a read refused while writers waited");
            assert!(took < Duration::from_secs(1), "a read took {took:?}");
            latencies.push(took);
        }
    }
    latencies.sort_unstable();
    let p99 = latencies[latencies.len() * 99 / 100 - 1];
    assert!(p99 < Duration::from_millis(200), "reads p99 {p99:?} with writers stuck");

    holder.rollback().await.expect("release");
    while let Some(done) = writers.join_next().await {
        let reply = done.expect("writer").expect("request");
        assert!(
            reply.status.is_success() || reply.status == StatusCode::PRECONDITION_FAILED,
            "a writer: {} {}",
            reply.status,
            reply.body
        );
    }
    database.close().await;
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn an_abandoned_read_gives_its_connection_back_within_the_read_timeout() {
    let _server = start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    create_article(&owner, &project, "Slow").await;

    // Every read of entries waits behind this lock; the clients give up after 300 ms.
    let database = owner_database().await;
    let mut holder = database.begin().await.expect("begin");
    sqlx::query("LOCK TABLE entries IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *holder)
        .await
        .expect("lock entries");
    let impatient = reqwest::Client::builder()
        .timeout(Duration::from_millis(300))
        .build()
        .expect("client");
    for _ in 0..4 {
        let abandoned = impatient
            .get(format!("{ORIGIN}{}", entries(&project)))
            .header(header::COOKIE, &owner.cookie)
            .send()
            .await;
        assert!(abandoned.is_err(), "the read gave up: {abandoned:?}");
    }
    let abandoned_at = Instant::now();
    assert!(waiting_on_locks(&database).await > 0, "the reads are stuck");

    // Their statements run on at the server until the read pool's statement timeout ends them.
    let deadline = abandoned_at + READ_STATEMENT_TIMEOUT + Duration::from_secs(1);
    while waiting_on_locks(&database).await > 0 {
        assert!(
            Instant::now() < deadline,
            "abandoned reads still hold their connections after {:?}",
            abandoned_at.elapsed()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    holder.rollback().await.expect("release");

    // And the pool is whole: a read answers at once.
    let started = Instant::now();
    let reply = app()
        .get(format!("{ORIGIN}{}", entries(&project)))
        .header(header::COOKIE, &owner.cookie)
        .send()
        .await
        .expect("read");
    assert_eq!(reply.status(), StatusCode::OK);
    assert!(started.elapsed() < Duration::from_secs(1));
    database.close().await;
}
