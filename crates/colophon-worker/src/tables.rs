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
/// `BATCH` a table a pass (ten for the tables strangers grow, D-165), so a pass that removed
/// nothing ends the tick early and a deep backlog drains `PASSES` batches at a time, never one
/// giant transaction. At the hourly default that is 100,000 rows a table an hour, and a
/// million of magic links and of sign-in keys, inside [`TICK_DEADLINE`].
const PASSES: u32 = 200;

/// The longest a tick keeps starting passes: past it the tick ends after the pass under way,
/// and the next interval carries on, every delete so far being committed per pass already. A
/// pass is never cut short (a pass dropped mid-COMMIT leaves unknown whether it committed);
/// its statements are bounded server side instead ([`colophon_db::bound_background`]).
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

    /// Ticks until `cancel` fires; a cancellation is heard between passes, so the pass under
    /// way when it fires runs to its end.
    pub async fn run(self, cancel: CancellationToken) {
        tracing::info!("table retention sweep started");
        loop {
            if cancel.is_cancelled() {
                break;
            }
            self.tick(&cancel).await;
            tokio::select! {
                () = cancel.cancelled() => break,
                () = tokio::time::sleep(self.interval) => {}
            }
        }
        tracing::info!("table retention sweep stopped");
    }

    /// Up to [`PASSES`] passes, stopping as soon as one removes nothing, when `cancel` fires
    /// or once [`TICK_DEADLINE`] has gone by. A failed pass ends the tick: the next interval
    /// runs it again — every delete is committed per pass, so a partial tick is progress kept,
    /// never work to undo.
    async fn tick(&self, cancel: &CancellationToken) {
        let deadline = std::time::Instant::now() + TICK_DEADLINE;
        let stopping = || cancel.is_cancelled();
        for _ in 0..PASSES {
            if cancel.is_cancelled() {
                break;
            }
            if std::time::Instant::now() >= deadline {
                tracing::warn!("a table retention tick ran out of time; carrying on next interval");
                break;
            }
            match colophon_db::retention::sweep(
                &self.pool,
                &colophon_db::retention::Retention::DEFAULT,
                colophon_db::retention::BATCH,
                colophon_db::Stop::new(&stopping),
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
                        magic_links = swept.magic_links,
                        sign_in_throttle = swept.sign_in_throttle,
                        preview_grants = swept.preview_grants,
                        "table retention pass"
                    );
                }
                // Told to stop between statements: rolled back, run again next start.
                Err(colophon_db::DbError::Stopped) => break,
                Err(error) => {
                    tracing::warn!(error = %error, "a table retention pass failed; trying again next interval");
                    break;
                }
            }
        }
    }
}
