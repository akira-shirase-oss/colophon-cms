//! The worker (docs/modules/05-worker.md; D-046). It finds all its work in Postgres: it claims
//! due delivery jobs under a lease, announces each release to its destination with the job's
//! stable idempotency key, and records the outcome only through the lease it holds. A worker
//! killed mid-attempt leaves a lease that runs out; the job is claimed again and the attempt
//! repeats with the same key, so the destination can drop the duplicate (F03). An announcement
//! never goes out after a newer one for the same destination (F05). No database transaction is
//! open while a destination is being called (W1).

mod fault;
pub mod guard;

use std::{sync::Arc, time::Duration};

use colophon_db::{
    PgPool,
    jobs::{self, Announcement, Claimed, Finish},
};
use colophon_domain::job::{self, Next, Outcome};
use serde_json::json;
use time::OffsetDateTime;
use tokio::sync::Semaphore;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

/// How often the worker fails jobs that ran out of time.
const EXPIRY_INTERVAL: Duration = Duration::from_secs(10);

/// How the worker runs.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Who holds the leases it takes: unique per process.
    pub owner: String,
    /// How often it looks for due work.
    pub poll: Duration,
    /// How long a claim lasts; longer than an attempt.
    pub lease: Duration,
    /// The most attempts in flight at once.
    pub slots: usize,
    /// Whether destinations may be private addresses (a development server on loopback only).
    pub allow_private: bool,
}

// The worker gets its own small database pool (colophon-server's COLOPHON_WORKER_DB_CONNECTIONS),
// so delivering announcements can never starve editors' requests of connections.

/// The worker couldn't be built.
#[derive(Debug, thiserror::Error)]
#[error("building the worker's HTTP client")]
pub struct BuildError(#[source] reqwest::Error);

/// The worker. [`Worker::run`] runs it until cancelled.
pub struct Worker {
    pool: PgPool,
    http: reqwest::Client,
    settings: Settings,
    slots: Arc<Semaphore>,
}

/// Any random 64 bits, for jitter: a fresh UUID v7's random part.
fn entropy() -> u64 {
    let bits = uuid::Uuid::now_v7().as_u128();
    u64::try_from(bits & u128::from(u64::MAX)).unwrap_or_default()
}

fn millis(at: OffsetDateTime) -> i64 {
    i64::try_from(at.unix_timestamp_nanos() / 1_000_000).unwrap_or(i64::MAX)
}

/// A `Retry-After` given in seconds.
fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(|seconds| seconds.saturating_mul(1_000))
}

impl Worker {
    /// A worker over `pool`.
    pub fn new(pool: PgPool, settings: Settings) -> Result<Self, BuildError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(job::ATTEMPT_DEADLINE_SECS))
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("colophon/", env!("CARGO_PKG_VERSION")))
            .dns_resolver(Arc::new(guard::PublicOnly {
                allow_private: settings.allow_private,
            }))
            .build()
            .map_err(BuildError)?;
        let slots = Arc::new(Semaphore::new(settings.slots.max(1)));
        Ok(Self {
            pool,
            http,
            settings,
            slots,
        })
    }

    /// Claims and runs due jobs until `cancel` fires, then lets attempts in flight finish. It
    /// claims as soon as a slot is free and only sleeps when it found nothing to do, so a backlog
    /// drains at the speed of the destinations, not of the polling interval.
    pub async fn run(self: Arc<Self>, cancel: CancellationToken) {
        let tracker = TaskTracker::new();
        tracing::info!(owner = %self.settings.owner, "worker started");
        let mut last_expiry: Option<std::time::Instant> = None;
        loop {
            if last_expiry.is_none_or(|at| at.elapsed() >= EXPIRY_INTERVAL) {
                last_expiry = Some(std::time::Instant::now());
                match jobs::expire(&self.pool).await {
                    Ok(0) => {}
                    Ok(expired) => tracing::info!(expired, "jobs ran out of time"),
                    Err(error) => {
                        tracing::warn!(error = %error, "expiring jobs failed; trying again later");
                    }
                }
            }
            let first = tokio::select! {
                () = cancel.cancelled() => break,
                permit = Arc::clone(&self.slots).acquire_owned() => match permit {
                    Ok(permit) => permit,
                    Err(_closed) => break,
                },
            };
            let free = 1 + self.slots.available_permits();
            let claimed = match jobs::claim(
                &self.pool,
                &self.settings.owner,
                self.settings.lease,
                i64::try_from(free).unwrap_or(1),
            )
            .await
            {
                Ok(claimed) => claimed,
                Err(error) => {
                    tracing::warn!(error = %error, "claiming jobs failed; trying again");
                    Vec::new()
                }
            };
            if claimed.is_empty() {
                drop(first);
                tokio::select! {
                    () = cancel.cancelled() => break,
                    () = tokio::time::sleep(self.settings.poll) => {}
                }
                continue;
            }
            let mut permit = Some(first);
            for job in claimed {
                let held = match permit.take() {
                    Some(held) => held,
                    // Counted as free when claiming; nothing else takes slots.
                    None => match Arc::clone(&self.slots).acquire_owned().await {
                        Ok(held) => held,
                        Err(_closed) => break,
                    },
                };
                let worker = Arc::clone(&self);
                tracker.spawn(async move {
                    worker.attempt(job).await;
                    drop(held);
                });
            }
        }
        tracker.close();
        tracker.wait().await;
        tracing::info!("worker stopped");
    }

    /// One attempt at one job, from the supersession check to the record of its outcome.
    async fn attempt(&self, job: Claimed) {
        let announcement = match jobs::prepare(&self.pool, &job).await {
            Ok(Some(announcement)) => announcement,
            Ok(None) => {
                let finish = Finish {
                    status: job::Status::Superseded.as_str(),
                    next_attempt_at: None,
                    http_status: None,
                    error: Some("a newer release is on its way to this destination"),
                };
                self.record(&job, &finish).await;
                return;
            }
            Err(error) => {
                // The lease runs out and the job is claimed again.
                tracing::warn!(job = %job.id, error = %error, "preparing the announcement failed");
                return;
            }
        };
        let outcome = self.post(&job, &announcement).await;
        fault::point("worker:after-dispatch");
        let attempts = u32::try_from(job.attempts).unwrap_or(u32::MAX);
        let next = job::after_attempt(
            &outcome,
            attempts,
            millis(OffsetDateTime::now_utc()),
            millis(job.deadline_at),
            entropy(),
        );
        let (http_status, error) = match &outcome {
            Outcome::Delivered { status } => (i16::try_from(*status).ok(), None),
            Outcome::Retryable { reason, .. } | Outcome::Terminal { reason } => {
                let status = reason
                    .strip_prefix("HTTP ")
                    .and_then(|code| code.parse::<i16>().ok());
                (status, Some(reason.as_str()))
            }
        };
        let (status, next_attempt_at) = match next {
            Next::Succeeded => (job::Status::Succeeded, None),
            Next::Failed => (job::Status::Failed, None),
            Next::Retry { after_ms } => (
                job::Status::Retrying,
                Some(OffsetDateTime::now_utc() + Duration::from_millis(after_ms)),
            ),
        };
        let finish = Finish {
            status: status.as_str(),
            next_attempt_at,
            http_status,
            error,
        };
        self.record(&job, &finish).await;
    }

    async fn record(&self, job: &Claimed, finish: &Finish<'_>) {
        match jobs::finish(&self.pool, job, &self.settings.owner, finish).await {
            Ok(true) => {
                tracing::info!(
                    job = %job.id,
                    generation = job.generation,
                    status = finish.status,
                    attempts = job.attempts,
                    "job attempt recorded"
                );
            }
            Ok(false) => {
                tracing::warn!(job = %job.id, "late result refused: this worker no longer holds the lease");
            }
            Err(error) => {
                tracing::warn!(job = %job.id, error = %error, "recording the attempt failed; the lease will run out");
            }
        }
    }

    /// Posts the announcement; what happened, as the domain classifies it.
    async fn post(&self, job: &Claimed, announcement: &Announcement) -> Outcome {
        if let Err(reason) =
            colophon_domain::destination::check_url(&job.url, self.settings.allow_private)
        {
            return Outcome::Terminal { reason };
        }
        let entries: Vec<_> = announcement
            .entries
            .iter()
            .map(|(id, revision, collection, title)| {
                json!({"id": id, "revision_id": revision, "collection": collection, "title": title})
            })
            .collect();
        let body = json!({
            "event": "release.committed",
            "project": job.project_id,
            "environment": announcement.environment,
            "epoch": announcement.epoch,
            "generation": job.generation,
            "changeset": announcement.changeset_id,
            "published_at": announcement.published_at.unix_timestamp(),
            "entries": entries,
        });
        let sent = self
            .http
            .post(&job.url)
            .header("idempotency-key", &job.idempotency_key)
            .header("colophon-generation", job.generation)
            .json(&body)
            .send()
            .await;
        match sent {
            Ok(response) => {
                let wait = retry_after(response.headers());
                job::classify(response.status().as_u16(), wait)
            }
            Err(error) => match guard::blocked(&error) {
                Some(reason) => Outcome::Terminal { reason },
                None if error.is_timeout() => Outcome::Retryable {
                    reason: "timed out".to_owned(),
                    retry_after_ms: None,
                },
                None => Outcome::Retryable {
                    reason: "could not connect".to_owned(),
                    retry_after_ms: None,
                },
            },
        }
    }
}
