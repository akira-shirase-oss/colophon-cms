//! The worker (docs/modules/05-worker.md; D-046). It finds all its work in Postgres: it claims
//! due delivery jobs under a lease, announces each release to its destination (or, for a CDN, asks
//! it to purge what changed, D-050; for a probe, looks for the release from where it runs and
//! keeps what it saw, D-051) with the job's stable idempotency key, and records the outcome only
//! through the lease it holds. A worker
//! killed mid-attempt leaves a lease that runs out; the job is claimed again and the attempt
//! repeats with the same key, so the destination can drop the duplicate (F03). An announcement
//! never goes out after a newer one for the same destination (F05). No database transaction is
//! open while a destination is being called (W1).

mod fault;
pub mod guard;

use std::{sync::Arc, time::Duration};

use colophon_db::{
    PgPool,
    jobs::{self, Announcement, Claimed, Finish, Observation},
};
use colophon_domain::{
    cache,
    job::{self, Next, Outcome},
    receipt,
};
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
    /// Each attempt's deadline (`colophon_domain::job::ATTEMPT_DEADLINE_SECS` by default);
    /// shorter than the lease.
    pub attempt: Duration,
    /// The most attempts in flight at once.
    pub slots: usize,
    /// Whether destinations may be private addresses (a development server on loopback only).
    pub allow_private: bool,
    /// Where it runs, as probes report it (`COLOPHON_PROBE_LOCATION`); it only runs probes pinned
    /// to this location, or to none.
    pub location: String,
}

// The worker gets its own small database pool (colophon-server's COLOPHON_WORKER_DB_CONNECTIONS),
// so delivering announcements can never starve editors' requests of connections.

/// What one attempt sends.
enum Message {
    /// A webhook hears about the release.
    Announce(Announcement),
    /// A CDN purges these tags (`colophon_domain::cache::purge_tags`); empty when the releases it
    /// covers changed nothing a CDN could hold.
    Purge(Vec<String>),
}

/// What one attempt does.
enum Work {
    /// Posts a message.
    Post(Message),
    /// Looks for the release (a probe).
    Probe,
}

/// The most of a probe's answer it reads, for the marker and the hash.
const MAX_PROBE_BODY: usize = 1024 * 1024;

/// What a probe found on one look.
struct Look {
    outcome: Outcome,
    seen: Option<i64>,
    status: Option<u16>,
    body_hash: Option<[u8; 32]>,
}

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
            .timeout(settings.attempt)
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
            // The database starts each lease while running the claim, so a clock started before
            // asking can only overestimate how much of the lease is gone, never underestimate it.
            let asked_at = std::time::Instant::now();
            let claimed = match jobs::claim(
                &self.pool,
                &self.settings.owner,
                &self.settings.location,
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
                    worker.attempt(job, asked_at).await;
                    drop(held);
                });
            }
        }
        tracker.close();
        tracker.wait().await;
        tracing::info!("worker stopped");
    }

    /// What `job`'s attempt will do, or `None` if a newer release's job for the same destination
    /// makes it obsolete.
    async fn prepare(&self, job: &Claimed) -> Result<Option<Work>, colophon_db::DbError> {
        if job.kind == "probe" {
            jobs::newer(&self.pool, job)
                .await
                .map(|newer| (!newer).then_some(Work::Probe))
        } else if job.kind == "cdn" {
            jobs::purge(&self.pool, job).await.map(|found| {
                found.map(|purge| {
                    let entries: Vec<(String, u128)> = purge
                        .entries
                        .into_iter()
                        .map(|(collection, entry)| (collection, entry.as_u128()))
                        .collect();
                    Work::Post(Message::Purge(cache::purge_tags(
                        job.project_id.as_u128(),
                        job.environment_id.as_u128(),
                        &entries,
                    )))
                })
            })
        } else {
            jobs::prepare(&self.pool, job)
                .await
                .map(|found| found.map(|announcement| Work::Post(Message::Announce(announcement))))
        }
    }

    /// One attempt at one job, from the supersession check to the record of its outcome.
    /// `asked_at` is when the claim was requested: no later than the lease's start.
    async fn attempt(&self, job: Claimed, asked_at: std::time::Instant) {
        let prepared = self.prepare(&job).await;
        let work = match prepared {
            Ok(Some(Work::Post(Message::Purge(tags)))) if tags.is_empty() => {
                let finish = Finish {
                    status: job::Status::Succeeded.as_str(),
                    next_attempt_at: None,
                    http_status: None,
                    error: None,
                    observation: None,
                };
                self.record(&job, &finish).await;
                return;
            }
            Ok(Some(work)) => work,
            Ok(None) => {
                let finish = Finish {
                    status: job::Status::Superseded.as_str(),
                    next_attempt_at: None,
                    http_status: None,
                    error: Some("a newer release is on its way to this destination"),
                    observation: None,
                };
                self.record(&job, &finish).await;
                return;
            }
            Err(error) => {
                // The lease runs out and the job is claimed again.
                tracing::warn!(job = %job.id, error = %error, "preparing the delivery failed");
                return;
            }
        };
        // Never send so late that the lease could run out with the request still on its way: the
        // next holder must not race this one (D-047). The job is claimed again once it does.
        if asked_at.elapsed() + self.settings.attempt >= self.settings.lease {
            tracing::warn!(job = %job.id, "gave up before sending: too close to the lease's end");
            return;
        }
        let (outcome, look) = match work {
            Work::Probe => {
                let look = self.look(&job).await;
                (look.outcome.clone(), Some(look))
            }
            Work::Post(message) => (self.post(&job, &message).await, None),
        };
        fault::point("worker:after-dispatch");
        let attempts = u32::try_from(job.attempts).unwrap_or(u32::MAX);
        let max_attempts = if look.is_some() {
            job::MAX_PROBE_ATTEMPTS
        } else {
            job::MAX_ATTEMPTS
        };
        let next = job::after_attempt_within(
            &outcome,
            attempts,
            max_attempts,
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
        let observation = look.as_ref().map(|look| Observation {
            location: &self.settings.location,
            generation: look.seen,
            http_status: look.status.and_then(|status| i16::try_from(status).ok()),
            body_hash: look.body_hash,
            error: match &look.outcome {
                Outcome::Delivered { .. } => None,
                Outcome::Retryable { reason, .. } | Outcome::Terminal { reason } => {
                    Some(reason.as_str())
                }
            },
        });
        let finish = Finish {
            status: status.as_str(),
            next_attempt_at,
            http_status: look
                .as_ref()
                .and_then(|look| look.status)
                .and_then(|status| i16::try_from(status).ok())
                .or(http_status),
            error,
            observation,
        };
        self.record(&job, &finish).await;
    }

    /// Fetches a probe's URL and reads which release the answer was built from: the
    /// `Colophon-Generation` header, or else a marker in the first megabyte of the body.
    async fn look(&self, job: &Claimed) -> Look {
        let failed = |outcome| Look {
            outcome,
            seen: None,
            status: None,
            body_hash: None,
        };
        if let Err(reason) =
            colophon_domain::destination::check_url(&job.url, self.settings.allow_private)
        {
            return failed(Outcome::Terminal { reason });
        }
        let sent = self
            .http
            .get(&job.url)
            .header("accept", "text/html, application/json")
            .send()
            .await;
        let mut response = match sent {
            Ok(response) => response,
            Err(error) => return failed(self.unsent(&error)),
        };
        let status = response.status().as_u16();
        let header = response
            .headers()
            .get("colophon-generation")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<i64>().ok());
        let mut body = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    let room = MAX_PROBE_BODY.saturating_sub(body.len());
                    body.extend_from_slice(&chunk[..chunk.len().min(room)]);
                    if body.len() >= MAX_PROBE_BODY {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let mut look = failed(self.unsent(&error));
                    look.status = Some(status);
                    return look;
                }
            }
        }
        let seen = header.or_else(|| receipt::marker(&String::from_utf8_lossy(&body)));
        Look {
            outcome: receipt::probe(status, seen, job.generation),
            seen,
            status: Some(status),
            body_hash: Some(sha256(&body)),
        }
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

    /// Posts the announcement or the purge; what happened, as the domain classifies it.
    async fn post(&self, job: &Claimed, message: &Message) -> Outcome {
        if let Err(reason) =
            colophon_domain::destination::check_url(&job.url, self.settings.allow_private)
        {
            return Outcome::Terminal { reason };
        }
        let body = match message {
            Message::Announce(announcement) => {
                let entries: Vec<_> = announcement
                    .entries
                    .iter()
                    .map(|(id, revision, collection, title)| {
                        json!({"id": id, "revision_id": revision, "collection": collection, "title": title})
                    })
                    .collect();
                json!({
                    "event": "release.committed",
                    "project": job.project_id,
                    "environment": announcement.environment,
                    "epoch": announcement.epoch,
                    "generation": job.generation,
                    "changeset": announcement.changeset_id,
                    "published_at": announcement.published_at.unix_timestamp(),
                    "entries": entries,
                })
            }
            // The shape of a purge by tag.
            Message::Purge(tags) => json!({ "tags": tags }),
        };
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
            Err(error) => self.unsent(&error),
        }
    }

    /// What a request that got no answer means: a refused address is final; a timeout or a
    /// connection failure is worth another try.
    #[expect(
        clippy::unused_self,
        reason = "a method, beside post and look, so the three read alike"
    )]
    fn unsent(&self, error: &reqwest::Error) -> Outcome {
        match guard::blocked(error) {
            Some(reason) => Outcome::Terminal { reason },
            None if error.is_timeout() => Outcome::Retryable {
                reason: "timed out".to_owned(),
                retry_after_ms: None,
            },
            None => Outcome::Retryable {
                reason: "could not connect".to_owned(),
                retry_after_ms: None,
            },
        }
    }
}

/// The SHA-256 of `bytes`: what a probe's evidence records of the answer it read.
fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, bytes);
    let mut out = [0; 32];
    out.copy_from_slice(digest.as_ref());
    out
}
