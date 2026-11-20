//! The pending-payload fold (D-216, migration 0078). The installation's pending payload is a
//! running total: every write to the tables it weighs appends its change to a log, and an
//! admission folds the log into the counter at its check. This loop folds it too, every
//! [`INTERVAL`], so the log stays short while nothing is being admitted, and reports the log's
//! length and age, warning when nothing has folded it for a while. A fold never waits for an
//! admission: when one holds the counter, this turn is skipped (the admission folds instead).

use std::time::Duration;

use colophon_db::PgPool;
use tokio_util::sync::CancellationToken;

/// How often the log is folded.
pub const INTERVAL: Duration = Duration::from_secs(5);

/// The longest one fold may take; a stalled one is abandoned and retried next interval.
const DEADLINE: Duration = Duration::from_secs(5);

/// A log longer than this, or a delta older than [`STALE`], means folds aren't keeping up.
const LONG: i64 = 50_000;
const STALE: f64 = 300.0;

/// Folds the payload log every [`INTERVAL`] until cancelled.
pub struct Folder {
    pool: PgPool,
}

impl Folder {
    /// A folder over the worker's pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Folds until `cancel` fires.
    pub async fn run(self, cancel: CancellationToken) {
        loop {
            tokio::select! {
                () = cancel.cancelled() => break,
                () = self.tick() => {}
            }
            tokio::select! {
                () = cancel.cancelled() => break,
                () = tokio::time::sleep(INTERVAL) => {}
            }
        }
    }

    /// One fold, then the log's length and age: logged, and a warning when it is long or old.
    async fn tick(&self) {
        match tokio::time::timeout(DEADLINE, colophon_db::work::fold_payload(&self.pool)).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                tracing::warn!(error = %error, "the payload fold failed; trying again next interval");
            }
            Err(_) => {
                tracing::warn!("the payload fold ran out of time; trying again next interval");
            }
        }
        match tokio::time::timeout(DEADLINE, colophon_db::work::payload_log(&self.pool)).await {
            Ok(Ok((deltas, oldest_seconds))) if deltas > LONG || oldest_seconds > STALE => {
                tracing::warn!(
                    deltas,
                    oldest_seconds,
                    "the payload log isn't being folded: admissions and the fold are behind"
                );
            }
            Ok(Ok((deltas, oldest_seconds))) => {
                tracing::debug!(deltas, oldest_seconds, "payload log");
            }
            Ok(Err(error)) => tracing::warn!(error = %error, "could not read the payload log"),
            Err(_) => tracing::warn!("reading the payload log ran out of time"),
        }
    }
}
