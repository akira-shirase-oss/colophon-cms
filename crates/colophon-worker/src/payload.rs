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

    /// Folds until `cancel` fires. A tick under way when it fires runs to its end: a fold
    /// dropped mid-COMMIT would leave unknown whether it committed. Each statement in it is
    /// bounded server side ([`colophon_db::bound_background`]), so that end always comes.
    pub async fn run(self, cancel: CancellationToken) {
        tracing::info!("payload fold started");
        loop {
            if cancel.is_cancelled() {
                break;
            }
            self.tick().await;
            tokio::select! {
                () = cancel.cancelled() => break,
                () = tokio::time::sleep(INTERVAL) => {}
            }
        }
        tracing::info!("payload fold stopped");
    }

    /// One fold, then the log's length and age: logged, and a warning when it is long or old.
    async fn tick(&self) {
        if let Err(error) = colophon_db::work::fold_payload(&self.pool).await {
            tracing::warn!(error = %error, "the payload fold failed; trying again next interval");
        }
        match colophon_db::work::payload_log(&self.pool).await {
            Ok((deltas, oldest_seconds)) if deltas > LONG || oldest_seconds > STALE => {
                tracing::warn!(
                    deltas,
                    oldest_seconds,
                    "the payload log isn't being folded: admissions and the fold are behind"
                );
            }
            Ok((deltas, oldest_seconds)) => {
                tracing::debug!(deltas, oldest_seconds, "payload log");
            }
            Err(error) => tracing::warn!(error = %error, "could not read the payload log"),
        }
    }
}
