//! The retention sweep (docs/modules/07-media.md, spec §18, D-099, D-136). One bounded pass,
//! then a sleep: enqueue an `expire` job for every asset that has been collectable longer
//! than `expiry` — quarantined, rejected, or usable with nothing naming it — and remove the
//! objects no row needs: `quarantine/` keys no still-open work holds, and `public/` keys no
//! current or versioned `public_key` serves from. The jobs ride the delete attempt end to
//! end: under the row lock `mark_deleting` makes the verdict that counts, so a pass that
//! guesses wrong costs a failed job, never a referenced object. Every effect is idempotent,
//! so a pass cut anywhere — crash, cancel, deadline — is simply run again.

use std::time::Duration;

use colophon_db::PgPool;
use futures_util::StreamExt;
use object_store::ObjectStoreExt;
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;

/// How many `expire` jobs one pass may enqueue; the rest wait for the next pass.
const ENQUEUE: i64 = 200;

/// How many objects under `quarantine/` one pass inspects; the rest wait for the next.
const ORPHANS: usize = 500;

/// The longest a pass may take: a stall anywhere inside is abandoned and retried whole next
/// interval, every effect it made being idempotent.
const PASS_DEADLINE: Duration = Duration::from_secs(120);

/// Runs the sweep: `enqueue_expired` every `interval`, then the orphan listing, until `cancel`
/// fires. A pass in progress when the deadline hits is dropped mid-await — safe, because its
/// INSERT is one statement and its object removals are idempotent.
pub struct Sweeper {
    pool: PgPool,
    store: colophon_media::Shared,
    interval: Duration,
    expiry: Duration,
    job_lifetime: Duration,
}

impl Sweeper {
    /// A sweeper over `pool` and `store`, passing every `interval` and collecting what is
    /// `expiry` old (spec §18's 24 hours by default; `COLOPHON_ASSET_*_MS` in config).
    pub const fn new(
        pool: PgPool,
        store: colophon_media::Shared,
        interval: Duration,
        expiry: Duration,
    ) -> Self {
        Self {
            pool,
            store,
            interval,
            expiry,
            job_lifetime: Duration::from_secs(colophon_domain::job::LIFETIME_SECS.unsigned_abs()),
        }
    }

    /// Passes until `cancel` fires; between items a cancellation is heard at the next await.
    pub async fn run(self, cancel: CancellationToken) {
        tracing::info!("asset retention sweep started");
        loop {
            let pass = tokio::select! {
                () = cancel.cancelled() => break,
                pass = tokio::time::timeout(PASS_DEADLINE, self.pass()) => pass,
            };
            if pass.is_err() {
                tracing::warn!("an asset sweep pass ran out of time; trying again next pass");
            }
            tokio::select! {
                () = cancel.cancelled() => break,
                () = tokio::time::sleep(self.interval) => {}
            }
        }
        tracing::info!("asset retention sweep stopped");
    }

    /// One pass: the expiry enqueue, then the orphan listing.
    async fn pass(&self) {
        let cutoff = OffsetDateTime::now_utc() - self.expiry;
        let deadline = OffsetDateTime::now_utc() + self.job_lifetime;
        match colophon_db::assets::enqueue_expired(&self.pool, cutoff, deadline, ENQUEUE).await {
            Ok(0) => {}
            Ok(expired) => tracing::info!(expired, "expire jobs enqueued"),
            Err(error) => {
                tracing::warn!(error = %error, "the sweep's expiry enqueue failed");
            }
        }
        self.orphans(cutoff).await;
        self.public_orphans(cutoff).await;
    }

    /// At most [`ORPHANS`] objects under `public/`: removes each older than `cutoff` which no
    /// asset's current `public_key` and no `asset_versions` row serves from (D-136) — the
    /// tombstones of a mid-swing crash, an orphaned promotion the worker couldn't clean, or
    /// any other object that outlived its record. A promoted object is recorded within the
    /// attempt deadline, far inside `expiry`, so the age guard is what keeps a landing
    /// mid-write safe; a deleted asset's objects are gone by its delete job already, and a
    /// live version's key is always recorded, so nothing served is ever listed here.
    async fn public_orphans(&self, cutoff: OffsetDateTime) {
        let cutoff_ms =
            i64::try_from(cutoff.unix_timestamp_nanos() / 1_000_000).unwrap_or(i64::MIN);
        let prefix = object_store::path::Path::from("public");
        let mut objects = self.store.list(Some(&prefix)).take(ORPHANS);
        let mut stale: Vec<object_store::path::Path> = Vec::new();
        while let Some(object) = objects.next().await {
            match object {
                Ok(meta) => {
                    if meta.last_modified.timestamp_millis() < cutoff_ms {
                        stale.push(meta.location);
                    }
                }
                Err(error) => {
                    tracing::warn!(error = %error, "listing public objects failed");
                }
            }
        }
        if stale.is_empty() {
            return;
        }
        let names: Vec<String> = stale.iter().map(ToString::to_string).collect();
        let unrecorded = match colophon_db::assets::unrecorded_public_keys(&self.pool, &names).await
        {
            Ok(unrecorded) => unrecorded
                .into_iter()
                .collect::<std::collections::HashSet<_>>(),
            Err(error) => {
                tracing::warn!(error = %error, "the public orphan sweep's key check failed");
                return;
            }
        };
        for path in stale {
            if !unrecorded.contains(path.as_ref()) {
                continue;
            }
            match self.store.delete(&path).await {
                Ok(()) | Err(object_store::Error::NotFound { .. }) => {}
                Err(error) => tracing::warn!(
                    key = %path,
                    error = %error,
                    "removing an unrecorded public object failed"
                ),
            }
        }
    }

    /// At most [`ORPHANS`] objects under `quarantine/`: removes each whose `last_modified` is
    /// older than `cutoff` and which no `quarantined` asset's `quarantine_key` still names —
    /// so leftovers of verified, rejected and deleting assets go too, not only objects no row
    /// ever recorded (D-099). An object younger than `expiry` is never touched, which is what
    /// protects an upload mid-verify: the row's verdict takes less than `expiry` either way.
    async fn orphans(&self, cutoff: OffsetDateTime) {
        let cutoff_ms =
            i64::try_from(cutoff.unix_timestamp_nanos() / 1_000_000).unwrap_or(i64::MIN);
        let prefix = object_store::path::Path::from("quarantine");
        let mut objects = self.store.list(Some(&prefix)).take(ORPHANS);
        let mut stale: Vec<object_store::path::Path> = Vec::new();
        while let Some(object) = objects.next().await {
            match object {
                Ok(meta) => {
                    if meta.last_modified.timestamp_millis() < cutoff_ms {
                        stale.push(meta.location);
                    }
                }
                Err(error) => {
                    tracing::warn!(error = %error, "listing quarantine objects failed");
                }
            }
        }
        if stale.is_empty() {
            return;
        }
        let names: Vec<String> = stale.iter().map(ToString::to_string).collect();
        let kept = match colophon_db::assets::quarantined_keys(&self.pool, &names).await {
            Ok(kept) => kept.into_iter().collect::<std::collections::HashSet<_>>(),
            Err(error) => {
                tracing::warn!(error = %error, "the orphan sweep's key check failed");
                return;
            }
        };
        for path in stale {
            if kept.contains(path.as_ref()) {
                continue;
            }
            match self.store.delete(&path).await {
                Ok(()) | Err(object_store::Error::NotFound { .. }) => {}
                Err(error) => tracing::warn!(
                    key = %path,
                    error = %error,
                    "removing an orphaned quarantine object failed"
                ),
            }
        }
    }
}
