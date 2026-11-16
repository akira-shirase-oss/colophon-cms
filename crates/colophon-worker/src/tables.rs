//! The table-retention sweep (spec §27, §33; D-135; docs/modules/05-worker.md "Retention",
//! docs/modules/02-db.md "Retention"). Beside media's own sweep ([`crate::retention`]), one
//! bounded pass every `interval` over every table whose rows outlive their business meaning —
//! terminal jobs and their receipts, the settled ledger, dead sign-in and setup state, the
//! audit log — as `colophon_worker`, in one transaction a pass, never more than `BATCH` rows a
//! table. [`colophon_db::retention::sweep`] holds the ages, the per-table rules and the order;
//! this loop only says when and how hard to lean on a backlog.
//!
//! What this never touches is decided there too: `deletion_ledger` is append-only by design,
//! content history is the product, and the uncertain stay `uncertain` for the reconciler —
//! retention deletes rows whose meaning expired, never rows a reader could still need.

use std::time::Duration;

use colophon_db::PgPool;
use tokio_util::sync::CancellationToken;

/// Passes one tick may run back to back while rows are still going: a backlog drains at
/// `BATCH` a table a pass, so a pass that removed nothing ends the tick early and a deep
/// backlog drains `PASSES` batches at a time, never one giant transaction.
const PASSES: u32 = 20;

/// The longest a tick may take: a stall inside is abandoned and retried whole next
/// interval, every delete it made being committed per pass already.
const TICK_DEADLINE: Duration = Duration::from_secs(120);

/// Runs the sweep: [`colophon_db::retention::sweep`] every `interval`, with up to
/// `PASSES` passes a tick while a backlog still has rows, until `cancel` fires.
pub struct Sweeper {
    pool: PgPool,
    interval: Duration,
}

impl Sweeper {
    /// A sweeper over `pool`, ticking every `interval` (`COLOPHON_TABLE_SWEEP_MS`).
    pub const fn new(pool: PgPool, interval: Duration) -> Self {
        Self { pool, interval }
    }

    /// Ticks until `cancel` fires; between passes a cancellation is heard at the next await.
    pub async fn run(self, cancel: CancellationToken) {
        tracing::info!("table retention sweep started");
        loop {
            let tick = tokio::select! {
                () = cancel.cancelled() => break,
                tick = tokio::time::timeout(TICK_DEADLINE, self.tick()) => tick,
            };
            if tick.is_err() {
                tracing::warn!(
                    "a table retention tick ran out of time; trying again next interval"
                );
            }
            tokio::select! {
                () = cancel.cancelled() => break,
                () = tokio::time::sleep(self.interval) => {}
            }
        }
        tracing::info!("table retention sweep stopped");
    }

    /// Up to [`PASSES`] passes, stopping as soon as one removes nothing. A failed pass ends
    /// the tick: the next interval runs it again — every delete is committed per pass, so a
    /// partial tick is progress kept, never work to undo.
    async fn tick(&self) {
        for _ in 0..PASSES {
            match colophon_db::retention::sweep(
                &self.pool,
                &colophon_db::retention::Retention::DEFAULT,
                colophon_db::retention::BATCH,
            )
            .await
            {
                Ok(swept) if swept.total() == 0 => break,
                Ok(swept) => {
                    tracing::info!(
                        jobs = swept.jobs,
                        observations = swept.observations,
                        asset_jobs = swept.asset_jobs,
                        bulk_items = swept.bulk_items,
                        bulk_jobs = swept.bulk_jobs,
                        migration_items = swept.migration_items,
                        migration_plans = swept.migration_plans,
                        schedules = swept.schedules,
                        reservations = swept.reservations,
                        audit = swept.audit,
                        idempotency = swept.idempotency,
                        sessions = swept.sessions,
                        access_tokens = swept.access_tokens,
                        invitations = swept.invitations,
                        device_logins = swept.device_logins,
                        sign_in_attempts = swept.sign_in_attempts,
                        setup_attempts = swept.setup_attempts,
                        setup_codes = swept.setup_codes,
                        "table retention pass"
                    );
                }
                Err(error) => {
                    tracing::warn!(error = %error, "a table retention pass failed; trying again next interval");
                    break;
                }
            }
        }
    }
}
