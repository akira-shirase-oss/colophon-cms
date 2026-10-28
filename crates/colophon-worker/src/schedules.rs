//! Runs schedules as they fall due (D-060). A loop of its own beside the deliveries, so a slow
//! destination never holds a schedule back: every `poll`, it asks the database what is due and
//! runs each one in time order. Running one publishes it once or blocks it with the reason
//! (`colophon_db::schedules::run`); two workers asking at once never both publish it.

use std::time::Duration;

use colophon_db::{
    PgPool,
    schedules::{self, Ran},
};
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;

/// How many due schedules one look takes; the rest wait for the next look, a poll later.
const BATCH: i64 = 20;

/// Runs due schedules from `pool` until `cancel` fires.
pub struct Scheduler {
    pool: PgPool,
    poll: Duration,
    job_lifetime: Duration,
}

impl Scheduler {
    /// A scheduler over `pool`, looking every `every`. Releases it publishes get delivery jobs
    /// with the usual lifetime, as the API's publish gives them.
    pub const fn new(pool: PgPool, every: Duration) -> Self {
        Self {
            pool,
            poll: every,
            job_lifetime: Duration::from_secs(colophon_domain::job::LIFETIME_SECS.unsigned_abs()),
        }
    }

    /// Looks for due schedules and runs them, until `cancel` fires; a run in progress finishes
    /// first, since stopping halfway would only roll it back.
    pub async fn run(self, cancel: CancellationToken) {
        tracing::info!("scheduler started");
        loop {
            let now = OffsetDateTime::now_utc();
            match schedules::due(&self.pool, now, BATCH).await {
                Ok(due) => {
                    for schedule in due {
                        if cancel.is_cancelled() {
                            break;
                        }
                        let started = OffsetDateTime::now_utc();
                        match schedules::run(&self.pool, &schedule, started, self.job_lifetime)
                            .await
                        {
                            Ok(Ran::Published {
                                generation,
                                delayed,
                            }) => tracing::info!(
                                project = %schedule.project_id,
                                schedule = %schedule.id,
                                generation,
                                delayed,
                                "a schedule published"
                            ),
                            Ok(Ran::Blocked { reason }) => tracing::info!(
                                project = %schedule.project_id,
                                schedule = %schedule.id,
                                %reason,
                                "a schedule was blocked"
                            ),
                            Ok(Ran::Gone) => {}
                            Err(error) => tracing::warn!(
                                schedule = %schedule.id,
                                error = %error,
                                "running a schedule failed; trying again"
                            ),
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(error = %error, "finding due schedules failed; trying again");
                }
            }
            tokio::select! {
                () = cancel.cancelled() => break,
                () = tokio::time::sleep(self.poll) => {}
            }
        }
        tracing::info!("scheduler stopped");
    }
}
