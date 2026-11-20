//! D-226's tripwire and the logs, with the log layers filtered as the server filters them: the
//! startup self-test, which makes a nested `BEGIN` on purpose, logs no WARN, while a real nested
//! `BEGIN` still logs exactly one (sqlx's notice), beside the tripwire's ERROR. Smokes fail on any
//! server WARN, so the self-test's own notice failed every smoke (train-2, CI 44698136206). One
//! test in its own binary: the subscriber is global and the counts are process-wide. Needs the
//! local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::sync::Mutex;

use common::TestDb;
use tracing_subscriber::Layer as _;
use tracing_subscriber::filter::{FilterExt as _, LevelFilter};
use tracing_subscriber::layer::SubscriberExt as _;

/// The levels and targets of the events the log layer was handed.
static LOGGED: Mutex<Vec<(tracing::Level, String)>> = Mutex::new(Vec::new());

struct Recorder;

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Recorder {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        let metadata = event.metadata();
        LOGGED
            .lock()
            .expect("the log record")
            .push((*metadata.level(), metadata.target().to_owned()));
    }
}

fn warnings_since(mark: usize, level: tracing::Level) -> Vec<String> {
    LOGGED.lock().expect("the log record")[mark..]
        .iter()
        .filter(|(l, _)| *l == level)
        .map(|(_, target)| target.clone())
        .collect()
}

fn mark() -> usize {
    LOGGED.lock().expect("the log record").len()
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_self_test_logs_no_warning_and_a_real_nested_begin_logs_one() {
    let subscriber = tracing_subscriber::registry()
        .with(Recorder.with_filter(
            LevelFilter::WARN.and(colophon_db::transactions::hide_the_self_test_notice()),
        ))
        .with(colophon_db::transactions::begin_tripwire());
    tracing::subscriber::set_global_default(subscriber).expect("one subscriber in this binary");
    let db = TestDb::new().await;
    let url = format!(
        "postgres://colophon_app:colophon-dev@{}/{}",
        common::host(),
        db.name
    );

    let before = mark();
    colophon_db::arm_release_guard(&url)
        .await
        .expect("the tripwire proves itself");
    assert_eq!(
        warnings_since(before, tracing::Level::WARN),
        Vec::<String>::new(),
        "the self-test's deliberate nested BEGIN must log no WARN"
    );

    // A real one: a BEGIN by hand leaves the pooled connection inside a transaction.
    let mut conn = db.app.acquire().await.expect("a connection");
    sqlx::raw_sql("BEGIN")
        .execute(&mut *conn)
        .await
        .expect("a hand-made transaction");
    drop(conn);
    let before = mark();
    let refused = colophon_db::begin_tx(&db.app).await;
    assert!(
        matches!(refused, Err(sqlx::Error::Protocol(_))),
        "refused: {refused:?}"
    );
    assert_eq!(
        warnings_since(before, tracing::Level::WARN),
        vec!["sqlx::postgres::notice".to_owned()],
        "a real nested BEGIN logs exactly one WARN, sqlx's notice"
    );
    assert_eq!(
        warnings_since(before, tracing::Level::ERROR).len(),
        1,
        "and the tripwire's one ERROR"
    );
    db.drop().await;
}
