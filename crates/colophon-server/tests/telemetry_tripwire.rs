//! The server's whole log stack (`telemetry::subscriber`, as `init` installs it) with D-224's
//! slow-statement tiers set, against D-226's begin tripwire: the startup self-test's deliberate
//! nested `BEGIN` writes no WARN line, and a real nested `BEGIN` still writes exactly one
//! (sqlx's notice) beside the tripwire's one ERROR. Each branch was green alone; this proves the
//! tier filter layer, the per-layer log filter and the tripwire's own filter together. One test
//! in its own binary: the subscriber is global and the tripwire's counts are process-wide. Needs
//! the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

#[path = "../../colophon-db/tests/common/mod.rs"]
mod db;

use std::{
    io,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use colophon_server::{config::LogFormat, telemetry};
use tracing_subscriber::fmt::MakeWriter;

/// The bytes the log layers wrote.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

impl Captured {
    /// How many bytes are written so far: a mark to read the lines after.
    fn mark(&self) -> usize {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    /// The (level, target) of each line written after `mark`.
    fn since(&self, mark: usize) -> Vec<(String, String)> {
        let bytes = self.0.lock().unwrap_or_else(PoisonError::into_inner)[mark..].to_vec();
        String::from_utf8(bytes)
            .expect("JSON lines are UTF-8")
            .lines()
            .map(|line| {
                let line: serde_json::Value = serde_json::from_str(line).expect("a JSON line");
                (
                    line["level"].as_str().unwrap_or_default().to_owned(),
                    line["target"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect()
    }

    fn at(&self, mark: usize, level: &str) -> Vec<String> {
        self.since(mark)
            .into_iter()
            .filter(|(l, _)| l == level)
            .map(|(_, target)| target)
            .collect()
    }
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_log_stack_with_statement_tiers_keeps_the_tripwire_and_hides_the_self_test() {
    // As the server starts: the tiers first (every pool then offers statements at INFO), then
    // the subscriber, with the filter CI's smokes run with.
    colophon_db::set_slow_statements(colophon_db::SlowStatements {
        info: Duration::from_millis(100),
        warn: Duration::from_secs(1),
    });
    let captured = Captured::default();
    let subscriber = telemetry::subscriber(
        LogFormat::Json,
        "info,sqlx::query=info,colophon_api::refusals=debug",
        captured.clone(),
    )
    .expect("the server's subscriber builds");
    tracing::subscriber::set_global_default(subscriber).expect("one subscriber in this binary");

    let db = db::TestDb::new().await;
    let url = format!(
        "postgres://colophon_app:colophon-dev@{}/{}",
        db::host(),
        db.name
    );

    let before = captured.mark();
    colophon_db::arm_release_guard(&url)
        .await
        .expect("the tripwire proves itself through the whole stack");
    assert_eq!(
        captured.at(before, "WARN"),
        Vec::<String>::new(),
        "the self-test's deliberate nested BEGIN must write no WARN"
    );

    // A real one: a BEGIN by hand leaves the pooled connection inside a transaction.
    let mut conn = db.app.acquire().await.expect("a connection");
    sqlx::raw_sql("BEGIN")
        .execute(&mut *conn)
        .await
        .expect("a hand-made transaction");
    drop(conn);
    let before = captured.mark();
    let refused = colophon_db::begin_tx(&db.app).await;
    assert!(
        matches!(refused, Err(sqlx::Error::Protocol(_))),
        "refused: {refused:?}"
    );
    assert_eq!(
        captured.at(before, "WARN"),
        vec!["sqlx::postgres::notice".to_owned()],
        "a real nested BEGIN writes exactly one WARN, sqlx's notice"
    );
    assert_eq!(
        captured.at(before, "ERROR").len(),
        1,
        "and the tripwire's one ERROR"
    );
    db.drop().await;
}
