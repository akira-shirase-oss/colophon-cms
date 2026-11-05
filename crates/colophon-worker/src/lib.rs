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
pub mod retention;
pub mod schedules;
pub mod tables;

use std::{sync::Arc, time::Duration};

use colophon_db::{
    PgPool,
    jobs::{self, Announcement, Claimed, Finish, Observation},
};
use colophon_domain::{
    cache,
    job::{self, Next, Outcome},
    migrate, receipt, schema,
};
use object_store::ObjectStoreExt;
use serde_json::json;
use time::OffsetDateTime;
use tokio::sync::Semaphore;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

/// How often the worker fails jobs that ran out of time.
const EXPIRY_INTERVAL: Duration = Duration::from_secs(10);

/// Bulk jobs one worker may drive at once — the spec's "two AI jobs" (§33, D-116).
const BULK_SLOTS: usize = 2;
/// Bulk items read per batch; each still commits in its own transaction.
const BULK_BATCH: i64 = 16;
/// Running bulk jobs scanned per pass.
const BULK_SCAN: i64 = 32;
/// How long a scan with drives in flight waits before looking again.
const BULK_RESPAWN: Duration = Duration::from_millis(250);

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
    /// Where it runs, as probes report it (`COLOPHON_WORKER_LOCATION`); it only runs probes pinned
    /// to this location, or to none.
    pub location: String,
    /// The object store uploads wait in (07-media); `None` means no asset jobs exist to claim.
    pub assets: Option<colophon_media::Shared>,
}

/// The most image verifications in flight at once (07-media: four image workers), on their own
/// semaphore so media load can never take a delivery slot (L14).
const IMAGE_SLOTS: usize = 4;

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

/// What one item's write means for the run driving it.
enum Drive {
    /// Apply the next item.
    Continue,
    /// Stop driving the plan: it ended, or what failed ends every write after this too.
    Stop,
}

/// What a bulk item's write made (D-116): a new revision's id for `applied`, or the
/// terminal outcome and its problem document.
enum BulkWrite {
    /// It applied; the new revision's id, when the write makes one (a delete makes none).
    Applied(Option<uuid::Uuid>),
    /// It ended `conflict`, `refused` or `skipped`, with the problem to record.
    Out(&'static str, serde_json::Value),
}

/// An item's `problem` document — `{"reason","pointer","message"}` — from a refusal, as
/// the API reports one.
fn problem(refusal: &colophon_domain::schema::Refusal) -> serde_json::Value {
    json!({"reason": refusal.code, "pointer": refusal.pointer, "message": refusal.message})
}

/// Where an item's write starts from, as [`Worker::document_for`] resolves it.
enum ItemDoc {
    /// The resolution or the expected revision's document — transform and validate it.
    Write(serde_json::Value),
    /// Stored content that doesn't parse: corrupt, not a planning fault, but the item still
    /// can't proceed — block it with this refusal.
    Block(schema::Refusal),
    /// The entry is gone (its foreign keys say a live item's revision can't be): the apply
    /// answers `skipped` without reading the document it was passed.
    Gone,
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
    image_slots: Arc<Semaphore>,
    /// At most [`BULK_SLOTS`] bulk jobs drive at once (D-116).
    bulk_slots: Arc<Semaphore>,
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
            image_slots: Arc::new(Semaphore::new(IMAGE_SLOTS)),
            bulk_slots: Arc::new(Semaphore::new(BULK_SLOTS)),
        })
    }

    /// Claims and runs due jobs until `cancel` fires, then lets attempts in flight finish. It
    /// claims as soon as a slot is free and only sleeps when it found nothing to do, so a backlog
    /// drains at the speed of the destinations, not of the polling interval.
    #[allow(clippy::too_many_lines, reason = "one spawn per driver family")]
    pub async fn run(self: Arc<Self>, cancel: CancellationToken) {
        let tracker = TaskTracker::new();
        tracing::info!(owner = %self.settings.owner, "worker started");
        if self.settings.assets.is_some() {
            let worker = Arc::clone(&self);
            tracker.spawn(worker.run_assets(cancel.clone(), tracker.clone()));
        }
        {
            let worker = Arc::clone(&self);
            tracker.spawn(worker.run_migrations(cancel.clone()));
        }
        {
            let worker = Arc::clone(&self);
            tracker.spawn(worker.run_bulk(cancel.clone()));
        }
        let mut last_expiry: Option<std::time::Instant> = None;
        loop {
            if last_expiry.is_none_or(|at| at.elapsed() >= EXPIRY_INTERVAL) {
                last_expiry = Some(std::time::Instant::now());
                // One cancel-aware block for the sweeps: each of them can sit in a pool
                // acquisition for the acquire timeout, and a shutdown can't wait on a
                // queued-up chain of them (SIGTERM still drains in seconds). A sweep
                // abandoned mid-flight rolls back; the next pass repeats it.
                let sweeps = async {
                    match jobs::expire(&self.pool).await {
                        Ok(0) => {}
                        Ok(expired) => tracing::info!(expired, "jobs ran out of time"),
                        Err(error) => {
                            tracing::warn!(error = %error, "expiring jobs failed; trying again later");
                        }
                    }
                    match colophon_db::assets::expire(&self.pool).await {
                        Ok(0) | Err(_) => {}
                        Ok(expired) => tracing::info!(expired, "asset jobs ran out of time"),
                    }
                    // The budget reconciler (D-113): free reservations of jobs that ended,
                    // flag held rows that outlived their work as `uncertain` and audit the
                    // discrepancy.
                    match colophon_db::budgets::reconcile(&self.pool).await {
                        Ok(done) if done.released == 0 && done.uncertain == 0 => {}
                        Ok(done) => {
                            tracing::info!(
                                released = done.released,
                                uncertain = done.uncertain,
                                "budgets reconciled"
                            );
                        }
                        Err(error) => {
                            tracing::warn!(error = %error, "reconciling budgets failed; trying again later");
                        }
                    }
                };
                tokio::select! {
                    () = cancel.cancelled() => break,
                    () = sweeps => {}
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

    /// The migration loop (M.5b, docs/modules/05-worker.md): finds `running` plans across
    /// every project and drives each to `done` or `blocked`, one plan at a time — a run is
    /// serial by design, under a session advisory lock on its own connection so at most one
    /// worker drives it, and a crash frees the claim for the next scan. Everything the run
    /// writes is durable in the item's own transaction, so a resumed drive asks every item
    /// what it already knows and skips what finished.
    pub async fn run_migrations(self: Arc<Self>, cancel: CancellationToken) {
        loop {
            let plans = match colophon_db::migrations::running(&self.pool).await {
                Ok(plans) => plans,
                Err(error) => {
                    tracing::warn!(error = %error, "scanning migration plans failed; trying again");
                    Vec::new()
                }
            };
            let mut drove = false;
            for plan in plans {
                if cancel.is_cancelled() {
                    break;
                }
                drove = self.drive_plan(&plan, &cancel).await || drove;
            }
            if !drove {
                tokio::select! {
                    () = cancel.cancelled() => break,
                    () = tokio::time::sleep(self.settings.poll) => {}
                }
            }
        }
        tracing::info!("migration worker stopped");
    }

    /// Claims `plan` on its own connection — the session lock lives on it for the whole drive —
    /// and drives it. `true` when it did work (so the scan loops without sleeping when plans
    /// keep arriving).
    async fn drive_plan(
        &self,
        plan: &colophon_db::migrations::RunningPlan,
        cancel: &CancellationToken,
    ) -> bool {
        let mut conn = match self.pool.acquire().await {
            Ok(conn) => conn,
            Err(error) => {
                tracing::warn!(error = %error, "a connection for the migration drive failed");
                return false;
            }
        };
        let claimed =
            colophon_db::migrations::claim_plan(&mut conn, plan.project, plan.environment, plan.id)
                .await;
        match claimed {
            Ok(true) => {}
            Ok(false) => return false,
            Err(error) => {
                tracing::warn!(plan = %plan.id, error = %error, "claiming the plan failed");
                return false;
            }
        }
        self.drive(&mut conn, plan, cancel).await;
        // The claim is released rather than trusted to the drop, so its connection can go back
        // to the pool clean; a drop would free it anyway on a crash.
        if let Err(error) = colophon_db::migrations::release_plan(
            &mut conn,
            plan.project,
            plan.environment,
            plan.id,
        )
        .await
        {
            tracing::warn!(plan = %plan.id, error = %error, "releasing the plan failed");
        }
        true
    }

    /// One plan's run: parse the target and operations once, stamp the run's target version
    /// ([`colophon_db::migrations::begin`]), then apply items one transaction each until
    /// nothing pending remains — [`colophon_db::migrations::checkpoint`] writes `done` or
    /// `blocked` — or the claim is cancelled, which just leaves the run for the next scan.
    async fn drive(
        &self,
        conn: &mut colophon_db::PgConnection,
        plan: &colophon_db::migrations::RunningPlan,
        cancel: &CancellationToken,
    ) {
        use colophon_db::migrations as runs;
        let parsed = || -> Result<(schema::ContentType, Vec<migrate::Op>), String> {
            let definition: serde_json::Value = serde_json::from_str(&plan.target_definition)
                .map_err(|_| "corrupt_plan".to_owned())?;
            let target =
                schema::ContentType::parse(&definition).map_err(|_| "corrupt_plan".to_owned())?;
            let operations = match &plan.operations {
                Some(text) => {
                    let value: serde_json::Value =
                        serde_json::from_str(text).map_err(|_| "invalid_operations".to_owned())?;
                    migrate::parse(&value).map_err(|_| "invalid_operations".to_owned())?
                }
                None => Vec::new(),
            };
            Ok((target, operations))
        };
        let (target, ops) = match parsed() {
            Ok(parsed) => parsed,
            Err(error) => {
                self.fail_run(conn, plan, &error).await;
                return;
            }
        };
        let Some(target_version) = self.begin_run(conn, plan).await else {
            return;
        };
        tracing::info!(plan = %plan.id, collection = %plan.collection, target_version, "migration run begun");
        let mut after = uuid::Uuid::nil();
        loop {
            let items = {
                let mut tx = match runs::worker_on(conn).await {
                    Ok(tx) => tx,
                    Err(error) => {
                        tracing::warn!(plan = %plan.id, error = %error, "reading the run failed");
                        return;
                    }
                };
                match runs::pending_items(
                    &mut tx,
                    plan.project,
                    plan.environment,
                    plan.id,
                    after,
                    runs::RUN_BATCH,
                )
                .await
                {
                    Ok(items) => items,
                    Err(error) => {
                        tracing::warn!(plan = %plan.id, error = %error, "reading the run's items failed");
                        return;
                    }
                }
            };
            if items.is_empty() {
                // Nothing left to apply: the checkpoint turns the plan `done`, or `blocked`
                // over the items still waiting on a person or a refused write.
                self.checkpoint(conn, plan).await;
                return;
            }
            for item in items {
                if cancel.is_cancelled() {
                    return;
                }
                after = item.entry;
                match self.apply_one(conn, plan, &target, &ops, &item).await {
                    Drive::Continue => {}
                    Drive::Stop => return,
                }
            }
        }
    }

    /// The drive's first transaction: [`colophon_db::migrations::begin`] locks the plan and
    /// the collection's change lock and stamps the run's target version. Its version when the
    /// run may drive; `None` when it shouldn't — `moved` ends it `blocked`, `not_running` and
    /// `missing` mean whoever it was ended it first, and a failed write just leaves it for
    /// the next scan.
    async fn begin_run(
        &self,
        conn: &mut colophon_db::PgConnection,
        plan: &colophon_db::migrations::RunningPlan,
    ) -> Option<i32> {
        use colophon_db::migrations as runs;
        let mut tx = match runs::worker_on(conn).await {
            Ok(tx) => tx,
            Err(error) => {
                tracing::warn!(plan = %plan.id, error = %error, "opening the run failed");
                return None;
            }
        };
        let begun = match runs::begin(&mut tx, plan.project, plan.environment, plan.id).await {
            Ok(begun) => begun,
            Err(error) => {
                tracing::warn!(plan = %plan.id, error = %error, "beginning the run failed");
                return None;
            }
        };
        match begun.outcome.as_str() {
            "begun" | "already" => {
                let version = begun.target_version;
                if let Err(error) = tx.commit().await {
                    tracing::warn!(plan = %plan.id, error = %error, "committing the run's version failed");
                    return None;
                }
                // `already` without a version is corrupt; `begun` always carries one.
                version
            }
            "moved" => {
                if let Err(error) = tx.commit().await {
                    tracing::warn!(plan = %plan.id, error = %error, "closing the begin failed");
                }
                self.fail_run(conn, plan, "collection_moved").await;
                None
            }
            _ => None,
        }
    }

    /// Ends a run no item can be blamed for: bad operations, a moved collection. [`fail_run`]
    /// writes `blocked` with the reason; losing the write just leaves it running for the next
    /// drive to hit the same wall.
    async fn fail_run(
        &self,
        conn: &mut colophon_db::PgConnection,
        plan: &colophon_db::migrations::RunningPlan,
        error: &str,
    ) {
        use colophon_db::migrations as runs;
        let mut tx = match runs::worker_on(conn).await {
            Ok(tx) => tx,
            Err(db_error) => {
                tracing::warn!(plan = %plan.id, error = %db_error, "opening the failed run's write failed");
                return;
            }
        };
        match runs::fail_run(&mut tx, plan.project, plan.environment, plan.id, error).await {
            Ok(Some(state)) => {
                tracing::info!(plan = %plan.id, state = %state.state, error = %error, "migration run ended");
            }
            Ok(None) => {}
            Err(db_error) => {
                tracing::warn!(plan = %plan.id, error = %db_error, "marking the run blocked failed");
            }
        }
        if let Err(db_error) = tx.commit().await {
            tracing::warn!(plan = %plan.id, error = %db_error, "committing the run's end failed");
        }
    }

    /// The run's counters and, when nothing applies any more, its end.
    async fn checkpoint(
        &self,
        conn: &mut colophon_db::PgConnection,
        plan: &colophon_db::migrations::RunningPlan,
    ) {
        use colophon_db::migrations as runs;
        let mut tx = match runs::worker_on(conn).await {
            Ok(tx) => tx,
            Err(error) => {
                tracing::warn!(plan = %plan.id, error = %error, "opening the checkpoint failed");
                return;
            }
        };
        match runs::checkpoint(&mut tx, plan.project, plan.environment, plan.id).await {
            Ok(Some(state)) => {
                if state.state != "running" {
                    tracing::info!(
                        plan = %plan.id,
                        state = %state.state,
                        done = state.done,
                        blocked = state.blocked,
                        error = ?state.error,
                        "migration run ended"
                    );
                }
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(plan = %plan.id, error = %error, "the run's checkpoint failed");
            }
        }
        if let Err(error) = tx.commit().await {
            tracing::warn!(plan = %plan.id, error = %error, "committing the checkpoint failed");
        }
    }

    /// Bulk work, beside [`Self::run_migrations`] (P5, D-116): a scan finds `running` jobs,
    /// each is claimed under a session advisory lock on its own connection, and its items run
    /// one transaction each — opened **as the submitter**, so a revoked token or a demoted
    /// member stops the run honestly instead of borrowing the worker's power. Everything the
    /// drive writes is durable in the item's own transaction, so a resumed drive asks every
    /// item what it already knows and skips what finished. At most `BULK_SLOTS` jobs drive at
    /// once — the spec's two AI jobs.
    pub async fn run_bulk(self: Arc<Self>, cancel: CancellationToken) {
        use colophon_db::bulk;
        let tracker = TaskTracker::new();
        loop {
            let jobs = match bulk::running(&self.pool, BULK_SCAN).await {
                Ok(jobs) => jobs,
                Err(error) => {
                    tracing::warn!(error = %error, "scanning bulk jobs failed; trying again");
                    Vec::new()
                }
            };
            let mut spawned = false;
            for (project, environment, job) in jobs {
                if cancel.is_cancelled() {
                    break;
                }
                let Ok(permit) = Arc::clone(&self.bulk_slots).try_acquire_owned() else {
                    break;
                };
                let worker = Arc::clone(&self);
                let cancelled = cancel.clone();
                tracker.spawn(async move {
                    let _permit = permit;
                    worker
                        .drive_bulk(project, environment, job, &cancelled)
                        .await;
                });
                spawned = true;
            }
            if spawned {
                tokio::select! {
                    () = cancel.cancelled() => break,
                    () = tokio::time::sleep(BULK_RESPAWN) => {}
                }
            } else {
                tokio::select! {
                    () = cancel.cancelled() => break,
                    () = tokio::time::sleep(self.settings.poll) => {}
                }
            }
        }
        tracker.close();
        tracker.wait().await;
        tracing::info!("bulk worker stopped");
    }

    /// Claims `job` on its own connection — the session lock lives on it for the whole
    /// drive — and drives its items, checkpointed per batch, until nothing pending remains
    /// ([`colophon_db::bulk::checkpoint`] writes `done` or `blocked`) or the claim is
    /// cancelled, which just leaves the run for the next scan.
    async fn drive_bulk(
        &self,
        project: uuid::Uuid,
        environment: uuid::Uuid,
        job: uuid::Uuid,
        cancel: &CancellationToken,
    ) {
        use colophon_db::bulk;
        let mut conn = match self.pool.acquire().await {
            Ok(conn) => conn,
            Err(error) => {
                tracing::warn!(error = %error, "a connection for the bulk drive failed");
                return;
            }
        };
        match bulk::claim(&mut conn, project, environment, job).await {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                tracing::warn!(job = %job, error = %error, "claiming the bulk job failed");
                return;
            }
        }
        let job = {
            match bulk::read_job(&mut conn, project, environment, job).await {
                Ok(Some(job)) if job.state == "running" => job,
                _ => {
                    let _dropped = bulk::release(&mut conn, project, environment, job).await;
                    return;
                }
            }
        };
        let Some(collection) = job.collection().map(str::to_owned) else {
            self.fail_bulk(&job).await;
            let _dropped = bulk::release(&mut conn, project, environment, job.id).await;
            return;
        };
        tracing::info!(job = %job.id, collection = %collection, "bulk run begun");
        loop {
            if cancel.is_cancelled() {
                break;
            }
            let items = match bulk::due_items(&self.pool, project, environment, job.id, BULK_BATCH)
                .await
            {
                Ok(items) => items,
                Err(error) => {
                    tracing::warn!(job = %job.id, error = %error, "reading the run's items failed");
                    break;
                }
            };
            if items.is_empty() {
                // Nothing pending — a resumed run can reach this straight away when the
                // crash landed after the last item's commit but before its checkpoint.
                match bulk::checkpoint(&self.pool, project, environment, job.id).await {
                    Ok(true) => tracing::info!(job = %job.id, "bulk run ended"),
                    Ok(false) => {}
                    Err(error) => {
                        tracing::warn!(job = %job.id, error = %error, "the run's checkpoint failed");
                    }
                }
                break;
            }
            let mut stopped = false;
            for item in items {
                if cancel.is_cancelled() {
                    stopped = true;
                    break;
                }
                if matches!(
                    self.apply_bulk_item(&job, &collection, &item).await,
                    Drive::Stop
                ) {
                    stopped = true;
                    break;
                }
            }
            // The batch's checkpoint recomputes counters and, when nothing pending remains,
            // ends the job — and closes its budget reservations in the same commit (D-116).
            match bulk::checkpoint(&self.pool, project, environment, job.id).await {
                Ok(true) => {
                    tracing::info!(job = %job.id, "bulk run ended");
                    break;
                }
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(job = %job.id, error = %error, "the run's checkpoint failed");
                }
            }
            if stopped {
                break;
            }
        }
        if let Err(error) = bulk::release(&mut conn, project, environment, job.id).await {
            tracing::warn!(job = %job.id, error = %error, "releasing the bulk job failed");
        }
    }

    /// One bulk item, one transaction — opened **as the job's submitter**: a token
    /// principal through [`colophon_db::scope::begin_in_environment_as_token`] (a revoked
    /// token answers `None`), a member through `begin_in_environment` (a removed member
    /// the same). Inside it: the item is re-read under its lock, the job's state is
    /// checked, the submitter's permission is re-checked against the *current* policy, the
    /// entry is locked, the base revision is compared, and the write, the item's outcome
    /// and the budget settlement commit together — [`fault::point`] "bulk:after-item"
    /// marks the resume line a crash test kills on.
    #[allow(
        clippy::too_many_lines,
        reason = "each refusal is its own checked step"
    )]
    async fn apply_bulk_item(
        &self,
        job: &colophon_db::bulk::Job,
        collection: &str,
        item: &colophon_db::bulk::Item,
    ) -> Drive {
        use colophon_db::{bulk, scope};
        let (project, environment) = (job.project, job.environment);
        let opened = match job.principal.as_str() {
            "token" => {
                scope::begin_in_environment_as_token(
                    &self.pool,
                    job.created_by,
                    project,
                    environment,
                )
                .await
            }
            _ => {
                scope::begin_in_environment(&self.pool, job.created_by, project, environment).await
            }
        };
        let mut tx = match opened {
            Ok(Some(tx)) => tx,
            // The submitter can no longer act: the token was revoked or the member left
            // the project. The item is refused by name — honest, and what the next ones
            // will hit too (D-116).
            Ok(None) => {
                return self
                    .mark_bulk_item(
                        job,
                        &item.key,
                        "refused",
                        json!({"reason": "submitter_lost_access", "pointer": "",
                               "message": "the submitter can no longer act in this project"}),
                        None,
                    )
                    .await;
            }
            Err(error) => {
                tracing::warn!(job = %job.id, error = %error, "opening an item's write failed");
                return Drive::Stop;
            }
        };
        // What the item is now: another drive can't hold the claim, but a cancel may have
        // skipped it since the batch read.
        let fresh = match bulk::lock_item(&mut tx, job.id, &item.key).await {
            Ok(Some(fresh)) => fresh,
            Ok(None) => return Drive::Continue,
            Err(error) => {
                tracing::warn!(job = %job.id, error = %error, "re-reading an item failed");
                return Drive::Stop;
            }
        };
        if fresh.outcome != "pending" {
            return Drive::Continue;
        }
        // The job itself: cancelled underneath the claim means stop, not more writes.
        match bulk::get(&mut tx, job.id).await {
            Ok(Some(current)) if current.state == "running" => {}
            Ok(_) => return Drive::Stop,
            Err(error) => {
                tracing::warn!(job = %job.id, error = %error, "re-reading the job failed");
                return Drive::Stop;
            }
        }
        let op = match colophon_domain::bulk::parse(&fresh.input) {
            Ok(op) => op,
            Err(refusal) => {
                return self
                    .finish_bulk_item(tx, job, &fresh, "refused", problem(&refusal), None)
                    .await;
            }
        };
        // Lock the entry and read what it is now — the collection check and the base
        // revision run on this, not on the batch's read.
        let entry = match colophon_db::bulk::lock_entry(&mut tx, fresh.entry_id).await {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(job = %job.id, error = %error, "locking an entry failed");
                return Drive::Stop;
            }
        };
        let Some(entry) = entry else {
            return self
                .finish_bulk_item(
                    tx,
                    job,
                    &fresh,
                    "skipped",
                    json!({"reason": "entry_gone", "pointer": "",
                           "message": "the entry isn't in this environment any more"}),
                    None,
                )
                .await;
        };
        if entry.gone {
            return self
                .finish_bulk_item(
                    tx,
                    job,
                    &fresh,
                    "skipped",
                    json!({"reason": "entry_deleted", "pointer": "",
                           "message": "the entry was deleted since the manifest was accepted"}),
                    None,
                )
                .await;
        }
        if entry.collection != collection {
            return self
                .finish_bulk_item(
                    tx,
                    job,
                    &fresh,
                    "skipped",
                    json!({"reason": "collection_mismatch", "pointer": "",
                           "message": "the entry isn't of the job's collection"}),
                    None,
                )
                .await;
        }
        // Execution-time permission: the submitter's role *now*, not at submit (D-116) —
        // a demotion degrades the run honestly.
        let own = entry.created_by == tx.identity;
        let action = if op.is_delete() {
            colophon_domain::policy::Action::ContentDelete
        } else {
            colophon_domain::policy::Action::ContentUpdate
        };
        let permitted = tx.policy.fields(action, collection, own);
        if permitted.is_none() {
            return self
                .finish_bulk_item(
                    tx,
                    job,
                    &fresh,
                    "refused",
                    json!({"reason": "missing_permission", "pointer": "",
                           "message": "your role can't run this operation on this entry"}),
                    None,
                )
                .await;
        }
        if entry.draft_revision_id != fresh.expected_revision_id {
            return self
                .finish_bulk_item(
                    tx,
                    job,
                    &fresh,
                    "conflict",
                    json!({"reason": "revision_moved", "pointer": "",
                           "message": "the draft moved since the manifest was accepted",
                           "current": entry.draft_revision_id}),
                    None,
                )
                .await;
        }
        let outcome = if op.is_delete() {
            match colophon_db::entries::delete(
                &mut tx,
                collection,
                fresh.entry_id,
                fresh.expected_revision_id,
            )
            .await
            {
                Ok(colophon_db::entries::Deleted::Deleted { .. }) => BulkWrite::Applied(None),
                Ok(colophon_db::entries::Deleted::Stale { current }) => BulkWrite::Out(
                    "conflict",
                    json!({"reason": "revision_moved", "pointer": "",
                        "message": "the draft moved since the manifest was accepted", "current": current}),
                ),
                Ok(colophon_db::entries::Deleted::NotFound) => BulkWrite::Out(
                    "skipped",
                    json!({"reason": "entry_deleted", "pointer": "",
                        "message": "the entry was deleted since the manifest was accepted"}),
                ),
                Ok(colophon_db::entries::Deleted::Published) => BulkWrite::Out(
                    "refused",
                    json!({"reason": "entry_published", "pointer": "",
                        "message": "the entry is published; unpublish it before a bulk delete"}),
                ),
                Ok(colophon_db::entries::Deleted::Referenced(_)) => BulkWrite::Out(
                    "refused",
                    json!({"reason": "still_referenced", "pointer": "",
                        "message": "another entry or a publication still references it"}),
                ),
                Ok(colophon_db::entries::Deleted::InChangeset(_)) => BulkWrite::Out(
                    "refused",
                    json!({"reason": "entry_in_changeset", "pointer": "",
                        "message": "an open changeset still pins the entry"}),
                ),
                Err(error) => {
                    tracing::warn!(job = %job.id, error = %error, "an item's delete failed");
                    return Drive::Stop;
                }
            }
        } else {
            match self
                .bulk_write(&mut tx, job, collection, &fresh, &op, own)
                .await
            {
                Ok(write) => write,
                Err(()) => return Drive::Stop,
            }
        };
        match outcome {
            BulkWrite::Applied(new_revision) => {
                if let Err(error) =
                    colophon_db::budgets::settle_bulk_document(&mut tx.transaction, project, job.id)
                        .await
                {
                    tracing::warn!(job = %job.id, error = %error, "settling an item's budget failed");
                    return Drive::Stop;
                }
                self.finish_bulk_item(
                    tx,
                    job,
                    &fresh,
                    "applied",
                    serde_json::Value::Null,
                    new_revision,
                )
                .await
            }
            BulkWrite::Out(outcome, problem) => {
                if let Err(error) = colophon_db::budgets::release_bulk_document(
                    &mut tx.transaction,
                    project,
                    job.id,
                )
                .await
                {
                    tracing::warn!(job = %job.id, error = %error, "releasing an item's budget failed");
                    return Drive::Stop;
                }
                self.finish_bulk_item(tx, job, &fresh, outcome, problem, None)
                    .await
            }
        }
    }

    /// The document a write op produces and its revision write, inside the item's
    /// transaction: `update` takes the manifest's document, `set`/`move` transform the
    /// stored one; then the same hidden-field restore, field-permission check and content
    /// type validation a save runs — the write itself is `entries::save_covered`, the
    /// save's own mechanics with the document already covered by the job's reservation.
    #[allow(
        clippy::too_many_lines,
        reason = "each validation layer is its own checked step"
    )]
    async fn bulk_write(
        &self,
        tx: &mut colophon_db::scope::ProjectTransaction,
        job: &colophon_db::bulk::Job,
        collection: &str,
        item: &colophon_db::bulk::Item,
        op: &colophon_domain::bulk::Op,
        own: bool,
    ) -> Result<BulkWrite, ()> {
        use colophon_domain::policy;
        // The stored document is the base for transforms and the field rules — the draft
        // points at `expected` (checked above), so that revision's content is what `set`
        // and `move` transform and what `update` is compared to.
        let stored = match colophon_db::entries::revision(
            tx,
            collection,
            item.entry_id,
            item.expected_revision_id,
        )
        .await
        {
            Ok(Some((_, content))) => content,
            Ok(None) => {
                return Ok(BulkWrite::Out(
                    "conflict",
                    json!({"reason": "revision_gone", "pointer": "",
                           "message": "the base revision isn't readable any more"}),
                ));
            }
            Err(error) => {
                tracing::warn!(job = %job.id, error = %error, "reading a base revision failed");
                return Err(());
            }
        };
        let Ok(document) = serde_json::from_str::<serde_json::Value>(&stored) else {
            tracing::warn!(job = %job.id, "a stored document isn't JSON");
            return Err(());
        };
        let candidate = match colophon_domain::bulk::apply(op, &document) {
            Ok(candidate) => candidate,
            Err(refusal) => {
                return Ok(BulkWrite::Out("refused", problem(&refusal)));
            }
        };
        let read = tx
            .policy
            .fields(
                colophon_domain::policy::Action::ContentRead,
                collection,
                own,
            )
            .unwrap_or(colophon_domain::policy::Fields::All);
        let candidate = match policy::restore_hidden(candidate, &stored, &read) {
            Ok(candidate) => candidate,
            Err(refusal) => {
                return Ok(BulkWrite::Out("refused", problem(&refusal)));
            }
        };
        // The collection's current definition: the write is validated against the version
        // that is current when it commits, under the type's shared lock — as saves do.
        if let Err(error) = colophon_db::content_types::lock_for_write(tx, collection).await {
            tracing::warn!(job = %job.id, error = %error, "locking the content type failed");
            return Err(());
        }
        let version = match colophon_db::content_types::current(tx, collection).await {
            Ok(Some(version)) => version,
            Ok(None) => {
                return Ok(BulkWrite::Out(
                    "skipped",
                    json!({"reason": "collection_gone", "pointer": "",
                           "message": "the collection isn't here any more"}),
                ));
            }
            Err(error) => {
                tracing::warn!(job = %job.id, error = %error, "reading the content type failed");
                return Err(());
            }
        };
        let Ok(definition) = serde_json::from_str::<serde_json::Value>(&version.definition) else {
            tracing::warn!(job = %job.id, "a stored definition isn't JSON");
            return Err(());
        };
        let Ok(content_type) = colophon_domain::schema::ContentType::parse(&definition) else {
            tracing::warn!(job = %job.id, "a stored definition doesn't parse");
            return Err(());
        };
        let content = match content_type.validate(&candidate) {
            Ok(content) => content,
            Err(refusal) => {
                return Ok(BulkWrite::Out("refused", problem(&refusal)));
            }
        };
        let update = tx
            .policy
            .fields(
                colophon_domain::policy::Action::ContentUpdate,
                collection,
                own,
            )
            .unwrap_or(colophon_domain::policy::Fields::All);
        if let Err(refusal) =
            policy::only_permitted_changes(content.canonical_json(), &stored, &update)
        {
            return Ok(BulkWrite::Out("refused", problem(&refusal)));
        }
        let unique_fields = content_type.unique_fields();
        let hash = sha256(content.canonical_json().as_bytes());
        let revision = colophon_db::entries::NewRevision {
            id: uuid::Uuid::now_v7(),
            content: content.canonical_json(),
            content_hash: &hash,
            title: content.title(),
            slug: content.slug(),
            schema_version: version.version,
            references: content.references(),
            assets: content.assets(),
            unique: &unique_fields,
        };
        match colophon_db::entries::save_covered(
            tx,
            item.entry_id,
            item.expected_revision_id,
            revision,
        )
        .await
        {
            Ok(colophon_db::entries::Saved::Saved(draft)) => {
                Ok(BulkWrite::Applied(Some(draft.revision.id)))
            }
            Ok(colophon_db::entries::Saved::Stale { current }) => Ok(BulkWrite::Out(
                "conflict",
                json!({"reason": "revision_moved", "pointer": "",
                       "message": "the draft moved since the manifest was accepted",
                       "current": current}),
            )),
            Ok(colophon_db::entries::Saved::NotFound) => Ok(BulkWrite::Out(
                "skipped",
                json!({"reason": "entry_gone", "pointer": "",
                       "message": "the entry isn't in this environment any more"}),
            )),
            Err(error) => {
                let (reason, pointer, message) = match &error {
                    colophon_db::DbError::SlugTaken => (
                        "slug_conflict",
                        "/slug".to_owned(),
                        "the slug is taken".to_owned(),
                    ),
                    colophon_db::DbError::ValueTaken => (
                        "value_conflict",
                        String::new(),
                        "a unique value is taken".to_owned(),
                    ),
                    colophon_db::DbError::MissingReference { field, .. } => (
                        "missing_reference",
                        format!("/{field}"),
                        "a reference names no entry".to_owned(),
                    ),
                    colophon_db::DbError::EntryDeleted { .. } => (
                        "missing_reference",
                        String::new(),
                        "a reference names no entry".to_owned(),
                    ),
                    colophon_db::DbError::MissingAsset { .. } => (
                        "missing_asset",
                        String::new(),
                        "an asset field names no asset".to_owned(),
                    ),
                    _ => {
                        tracing::warn!(job = %job.id, error = %error, "an item's write failed");
                        return Err(());
                    }
                };
                Ok(BulkWrite::Out(
                    "refused",
                    json!({"reason": reason, "pointer": pointer, "message": message}),
                ))
            }
        }
    }

    /// Ends an item inside its own transaction and commits it — the outcome (and for an
    /// applied write, the revision it made and the settled budget) land together. A crash
    /// before the commit rolls everything back; [`fault::point`] after it marks where a
    /// crash leaves a committed outcome to be skipped on the resume.
    async fn finish_bulk_item(
        &self,
        mut tx: colophon_db::scope::ProjectTransaction,
        job: &colophon_db::bulk::Job,
        item: &colophon_db::bulk::Item,
        outcome: &str,
        problem: serde_json::Value,
        new_revision: Option<uuid::Uuid>,
    ) -> Drive {
        use colophon_db::bulk;
        let problem = if problem.is_null() {
            None
        } else {
            Some(problem)
        };
        if let Err(error) = bulk::mark(
            &mut tx.transaction,
            job.project,
            job.environment,
            job.id,
            &item.key,
            outcome,
            problem.as_ref(),
            new_revision,
        )
        .await
        {
            tracing::warn!(job = %job.id, error = %error, "writing an item's outcome failed");
            return Drive::Stop;
        }
        if let Err(error) = tx.transaction.commit().await {
            tracing::warn!(job = %job.id, error = %error, "committing an item failed");
            return Drive::Stop;
        }
        // A crash here is the resume case: the item's outcome is committed, so the next
        // drive asks it and moves on (the fault tests prove it).
        fault::point("bulk:after-item");
        Drive::Continue
    }

    /// Marks an item when no submitter transaction could open (the token was revoked, the
    /// member left): a `colophon_worker` write, one commit, and the item's held document
    /// released — the same shape as a refused item.
    async fn mark_bulk_item(
        &self,
        job: &colophon_db::bulk::Job,
        key: &str,
        outcome: &str,
        problem: serde_json::Value,
        new_revision: Option<uuid::Uuid>,
    ) -> Drive {
        use colophon_db::bulk;
        let mut conn = match self.pool.acquire().await {
            Ok(conn) => conn,
            Err(error) => {
                tracing::warn!(job = %job.id, error = %error, "a connection for a mark failed");
                return Drive::Stop;
            }
        };
        let mut tx = match colophon_db::migrations::worker_on(&mut conn).await {
            Ok(tx) => tx,
            Err(error) => {
                tracing::warn!(job = %job.id, error = %error, "opening a mark failed");
                return Drive::Stop;
            }
        };
        let wrote = async {
            bulk::mark(
                &mut tx,
                job.project,
                job.environment,
                job.id,
                key,
                outcome,
                Some(&problem),
                new_revision,
            )
            .await?;
            colophon_db::budgets::release_bulk_document(&mut tx, job.project, job.id).await
        };
        match wrote.await {
            Ok(()) => {
                if let Err(error) = tx.commit().await {
                    tracing::warn!(job = %job.id, error = %error, "committing a mark failed");
                    return Drive::Stop;
                }
            }
            Err(error) => {
                tracing::warn!(job = %job.id, error = %error, "marking an item failed");
                return Drive::Stop;
            }
        }
        Drive::Continue
    }

    /// Ends a job whose operation document can't be read: `blocked` with the reason —
    /// losing the write just leaves it running for the next drive to hit the same wall.
    async fn fail_bulk(&self, job: &colophon_db::bulk::Job) {
        if let Err(error) =
            colophon_db::bulk::mark_corrupt(&self.pool, job.project, job.environment, job.id).await
        {
            tracing::warn!(job = %job.id, error = %error, "marking the job blocked failed");
        }
    }

    /// One item, one transaction: re-read its outcome, build the document the apply writes —
    /// a person's resolution, or the expected revision transformed by the plan's operations —
    /// validate it against the target, and let migration 0041's apply lock the entry and
    /// decide. A refused transform or an apply-time error marks the item `blocked` with the
    /// refusal as its problem; the checkpoint that ends the same transaction ends the plan
    /// with it.
    async fn apply_one(
        &self,
        conn: &mut colophon_db::PgConnection,
        plan: &colophon_db::migrations::RunningPlan,
        target: &schema::ContentType,
        ops: &[migrate::Op],
        item: &colophon_db::migrations::RunItem,
    ) -> Drive {
        use colophon_db::migrations as runs;
        let mut tx = match runs::worker_on(conn).await {
            Ok(tx) => tx,
            Err(error) => {
                tracing::warn!(plan = %plan.id, error = %error, "opening an item's write failed");
                return Drive::Stop;
            }
        };
        // What the item is now: it may have been resolved or blocked since the batch read.
        let fresh =
            match runs::item_for_run(&mut tx, plan.project, plan.environment, plan.id, item.entry)
                .await
            {
                Ok(Some(fresh)) => fresh,
                Ok(None) => return Drive::Stop,
                Err(error) => {
                    tracing::warn!(plan = %plan.id, error = %error, "re-reading an item failed");
                    return Drive::Stop;
                }
            };
        if !matches!(
            fresh.outcome.as_str(),
            "ok" | "migrates" | "needs_input" | "resolved"
        ) {
            return Drive::Continue;
        }
        let document = match self.document_for(&mut tx, plan, &fresh).await {
            Ok(ItemDoc::Write(document)) => document,
            Ok(ItemDoc::Block(refusal)) => {
                return self.block_item(tx, plan, fresh.entry, &refusal).await;
            }
            Ok(ItemDoc::Gone) => serde_json::Value::Object(serde_json::Map::new()),
            Err(()) => return Drive::Stop,
        };
        let document = if fresh.outcome == "resolved" {
            document
        } else {
            match migrate::apply(&document, ops) {
                Ok(document) => document,
                Err(refusal) => {
                    return self.block_item(tx, plan, fresh.entry, &refusal).await;
                }
            }
        };
        let content = match target.validate(&document) {
            Ok(content) => content,
            Err(refusal) => {
                return self.block_item(tx, plan, fresh.entry, &refusal).await;
            }
        };
        let unique_fields = target.unique_fields();
        let hash = sha256(content.canonical_json().as_bytes());
        let applied = applied_for(&content, &hash, &unique_fields);
        let outcome = runs::apply_item(
            &mut tx,
            plan.project,
            plan.environment,
            plan.id,
            fresh.entry,
            fresh.expected_revision,
            &applied,
        )
        .await;
        let state = match outcome {
            // `not_running` or `missing` — the plan ended under the claim; stop asking.
            Ok(outcome) if outcome == "not_running" || outcome == "missing" => {
                return Drive::Stop;
            }
            // `applied`, `skipped`, or a refusal the function itself recorded on the item
            // (`draft_moved`, `missing_reference`, `missing_asset`): commit it all.
            Ok(_) => runs::checkpoint(&mut tx, plan.project, plan.environment, plan.id).await,
            Err(error) => {
                // The transaction is dead with the function's own marks; a fresh one says why
                // the item can't proceed — the refusal shape a `needs_input` item already
                // uses, so a person reads one kind of problem.
                let reason = match &error {
                    colophon_db::DbError::SlugTaken => "slug_conflict",
                    colophon_db::DbError::ValueTaken => "value_conflict",
                    colophon_db::DbError::EntryDeleted { .. } => "missing_reference",
                    _ => "write_failed",
                };
                drop(tx);
                return self.block_item_fresh(conn, plan, fresh.entry, reason).await;
            }
        };
        let done = match state {
            Ok(Some(state)) => state.state != "running",
            Ok(None) => true,
            Err(error) => {
                tracing::warn!(plan = %plan.id, error = %error, "the item's checkpoint failed");
                false
            }
        };
        if let Err(error) = tx.commit().await {
            tracing::warn!(plan = %plan.id, error = %error, "committing an item failed");
            return Drive::Stop;
        }
        // A crash here is the resume case: the item's outcome is committed, so the next drive
        // asks it and moves on (the fault tests prove it).
        fault::point("migration:after-item");
        if done { Drive::Stop } else { Drive::Continue }
    }

    /// The document an item's write starts from, inside the transaction the caller opened. A
    /// `resolved` item's resolution is the document itself; anything else is the expected
    /// revision's stored document, to be transformed by the declared operations.
    async fn document_for(
        &self,
        tx: &mut colophon_db::Transaction<'_, colophon_db::Postgres>,
        plan: &colophon_db::migrations::RunningPlan,
        item: &colophon_db::migrations::RunItem,
    ) -> Result<ItemDoc, ()> {
        use colophon_db::migrations as runs;
        let text: Option<String> = if item.outcome == "resolved" {
            item.resolution.clone()
        } else {
            match runs::expected_content(
                tx,
                plan.project,
                plan.environment,
                item.entry,
                item.expected_revision,
            )
            .await
            {
                Ok(text) => text,
                Err(error) => {
                    tracing::warn!(plan = %plan.id, error = %error, "reading a revision failed");
                    return Err(());
                }
            }
        };
        Ok(match text {
            None => ItemDoc::Gone,
            Some(text) => match serde_json::from_str(&text) {
                Ok(document) => ItemDoc::Write(document),
                Err(_) => ItemDoc::Block(schema::Refusal {
                    code: "invalid_document",
                    pointer: String::new(),
                    message: "the item's stored document does not parse".to_owned(),
                }),
            },
        })
    }

    /// Marks the item `blocked` with a refusal as its problem, in the transaction the caller
    /// made — the transform and validation refusals, where the caller's reads are still live —
    /// then the run's checkpoint, which ends the plan over it. Takes the transaction: it
    /// commits it.
    async fn block_item(
        &self,
        mut tx: colophon_db::Transaction<'_, colophon_db::Postgres>,
        plan: &colophon_db::migrations::RunningPlan,
        entry: uuid::Uuid,
        refusal: &schema::Refusal,
    ) -> Drive {
        use colophon_db::migrations as runs;
        let problem = json!({
            "reason": refusal.code,
            "pointer": refusal.pointer,
            "message": refusal.message,
        });
        if let Err(error) = runs::mark_item(
            &mut tx,
            plan.project,
            plan.environment,
            plan.id,
            entry,
            "blocked",
            Some(problem),
        )
        .await
        {
            tracing::warn!(plan = %plan.id, error = %error, "marking an item blocked failed");
            return Drive::Stop;
        }
        let done = match runs::checkpoint(&mut tx, plan.project, plan.environment, plan.id).await {
            Ok(Some(state)) => state.state != "running",
            Ok(None) => true,
            Err(error) => {
                tracing::warn!(plan = %plan.id, error = %error, "the block's checkpoint failed");
                false
            }
        };
        if let Err(error) = tx.commit().await {
            tracing::warn!(plan = %plan.id, error = %error, "committing the block failed");
            return Drive::Stop;
        }
        if done { Drive::Stop } else { Drive::Continue }
    }

    /// Marks the item `blocked` in a fresh transaction — the one its write died in is gone —
    /// with `reason` as the problem. Then the plan's checkpoint, which ends the run over it.
    async fn block_item_fresh(
        &self,
        conn: &mut colophon_db::PgConnection,
        plan: &colophon_db::migrations::RunningPlan,
        entry: uuid::Uuid,
        reason: &str,
    ) -> Drive {
        use colophon_db::migrations as runs;
        let mut tx = match runs::worker_on(conn).await {
            Ok(tx) => tx,
            Err(error) => {
                tracing::warn!(plan = %plan.id, error = %error, "opening the block failed");
                return Drive::Stop;
            }
        };
        let problem = json!({
            "reason": reason,
            "pointer": "",
            "message": match reason {
                "slug_conflict" => "the rewritten slug collides with another entry's; fix it and plan again",
                "value_conflict" => "a rewritten unique value collides with another entry's; fix it and plan again",
                "missing_reference" => "a reference names an entry that is not there; fix it and plan again",
                _ => "the revision write failed; plan again",
            },
        });
        if let Err(error) = runs::mark_item(
            &mut tx,
            plan.project,
            plan.environment,
            plan.id,
            entry,
            "blocked",
            Some(problem),
        )
        .await
        {
            tracing::warn!(plan = %plan.id, error = %error, "marking an item blocked failed");
        }
        if let Err(error) = runs::checkpoint(&mut tx, plan.project, plan.environment, plan.id).await
        {
            tracing::warn!(plan = %plan.id, error = %error, "the block's checkpoint failed");
        }
        if let Err(error) = tx.commit().await {
            tracing::warn!(plan = %plan.id, error = %error, "committing the block failed");
        }
        Drive::Stop
    }

    /// The asset-jobs loop (07-media): same claim discipline as the delivery loop, on the image
    /// pool's own semaphore so decodes can never take a delivery slot (L14). Runs only when the
    /// worker was built with an object store.
    async fn run_assets(self: Arc<Self>, cancel: CancellationToken, tracker: TaskTracker) {
        loop {
            let first = tokio::select! {
                () = cancel.cancelled() => break,
                permit = Arc::clone(&self.image_slots).acquire_owned() => match permit {
                    Ok(permit) => permit,
                    Err(_closed) => break,
                },
            };
            let free = 1 + self.image_slots.available_permits();
            let asked_at = std::time::Instant::now();
            let claimed = match colophon_db::assets::claim(
                &self.pool,
                &self.settings.owner,
                self.settings.lease,
                i64::try_from(free).unwrap_or(1),
            )
            .await
            {
                Ok(claimed) => claimed,
                Err(error) => {
                    tracing::warn!(error = %error, "claiming asset jobs failed; trying again");
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
                    None => match Arc::clone(&self.image_slots).acquire_owned().await {
                        Ok(held) => held,
                        Err(_closed) => break,
                    },
                };
                let worker = Arc::clone(&self);
                tracker.spawn(async move {
                    worker.attempt_asset(job, asked_at).await;
                    drop(held);
                });
            }
        }
        tracing::info!("asset worker stopped");
    }

    /// One asset attempt, by kind: `verify` proves the quarantined object and promotes it;
    /// `delete` and the sweep's `expire` remove everything the asset ever wrote through the
    /// same attempt (media M3, D-098, D-099). A kind this worker doesn't know fails outright —
    /// it would never succeed.
    async fn attempt_asset(&self, job: colophon_db::assets::Claimed, asked_at: std::time::Instant) {
        let attempts = u32::try_from(job.attempts).unwrap_or(u32::MAX);
        // A claim almost out of lease gets no attempt: it is claimed again when the lease lapses.
        if asked_at.elapsed() + self.settings.attempt >= self.settings.lease {
            return;
        }
        let Some(store) = self.settings.assets.clone() else {
            return;
        };
        match job.kind.as_str() {
            "verify" => self.verify(&job, attempts, &store, asked_at).await,
            "delete" | "expire" => self.remove(&job, attempts, &store, asked_at).await,
            other => {
                tracing::warn!(job = %job.id, kind = %other, "an asset job kind this worker doesn't do");
                self.finish_asset(
                    &job,
                    attempts,
                    "failed",
                    Some("an asset job kind this worker doesn't do"),
                    &colophon_db::assets::Outcome::Missing,
                )
                .await;
            }
        }
    }

    /// One verify attempt: fetch the quarantined object, prove it, move it to its public key and
    /// record both outcomes — the asset's new state and the job's — in one transaction.
    async fn verify(
        &self,
        job: &colophon_db::assets::Claimed,
        attempts: u32,
        store: &colophon_media::Shared,
        asked_at: std::time::Instant,
    ) {
        let budget = self.settings.attempt.saturating_sub(asked_at.elapsed());
        let Some(bytes) = self.quarantined(job, attempts, store, budget).await else {
            return;
        };
        // The decode is CPU-bound and its own worst case is bounded (20 MiB in, strict limits
        // out); it runs on a blocking thread so a pathological file never stalls this worker.
        let checked =
            tokio::task::spawn_blocking(move || colophon_media::verify_image(&bytes)).await;
        let key = colophon_media::public_key(
            &job.project_id.to_string(),
            &job.environment_id.to_string(),
            &job.asset_id.to_string(),
        );
        match checked {
            Ok(Ok(verified)) => {
                self.promote(job, attempts, store, key, verified).await;
            }
            Ok(Err(reason)) => {
                self.finish_asset(
                    job,
                    attempts,
                    "succeeded",
                    None,
                    &colophon_db::assets::Outcome::Rejected {
                        reason: reason.to_string(),
                    },
                )
                .await;
            }
            Err(join) => {
                self.retry_asset(job, attempts, &trim(&join.to_string()))
                    .await;
            }
        }
    }

    /// One delete attempt (media M3, D-098): `mark_deleting` locks the asset and rechecks who
    /// still names it — the verdict that counts — then every object it ever wrote is removed
    /// outside any transaction. The removals are idempotent, so a crash or a second delete job
    /// just re-deletes what is already gone, and a store that refuses one retries the job.
    async fn remove(
        &self,
        job: &colophon_db::assets::Claimed,
        attempts: u32,
        store: &colophon_media::Shared,
        asked_at: std::time::Instant,
    ) {
        use colophon_db::assets::{Deletion, Outcome};
        let keys = match colophon_db::assets::mark_deleting(&self.pool, job).await {
            // The lease runs out and the job is claimed again.
            Err(error) => {
                tracing::warn!(job = %job.id, error = %error, "the deletion's recheck failed");
                return;
            }
            Ok(Deletion::Gone) => {
                self.finish_asset(job, attempts, "succeeded", None, &Outcome::Deleted)
                    .await;
                return;
            }
            Ok(Deletion::StillReferenced) => {
                self.finish_asset(
                    job,
                    attempts,
                    "failed",
                    Some("still referenced"),
                    &Outcome::StillReferenced,
                )
                .await;
                return;
            }
            Ok(Deletion::Held) => {
                self.finish_asset(
                    job,
                    attempts,
                    "failed",
                    Some("not in a state a delete may act on"),
                    &Outcome::StillReferenced,
                )
                .await;
                return;
            }
            Ok(Deletion::Removing {
                public_key,
                derivatives,
                quarantine_key,
            }) => {
                let mut keys = Vec::with_capacity(derivatives.len() + 2);
                if let Some(public) = public_key {
                    keys.push(public.clone());
                    keys.extend(derivatives.iter().map(|name| format!("{public}/{name}")));
                }
                keys.push(quarantine_key);
                keys
            }
        };
        let budget = self.settings.attempt.saturating_sub(asked_at.elapsed());
        for key in keys {
            match tokio::time::timeout(budget, store.delete(&object_store::path::Path::from(key)))
                .await
            {
                Ok(Ok(()) | Err(object_store::Error::NotFound { .. })) => {}
                Ok(Err(error)) => {
                    self.retry_asset(job, attempts, &trim(&error.to_string()))
                        .await;
                    return;
                }
                Err(_) => {
                    self.retry_asset(job, attempts, "removing the objects timed out")
                        .await;
                    return;
                }
            }
        }
        self.finish_asset(job, attempts, "succeeded", None, &Outcome::Deleted)
            .await;
    }

    /// The quarantined object's bytes, or `None` after recording why the job retries.
    async fn quarantined(
        &self,
        job: &colophon_db::assets::Claimed,
        attempts: u32,
        store: &colophon_media::Shared,
        budget: Duration,
    ) -> Option<bytes::Bytes> {
        let quarantine = object_store::path::Path::from(job.quarantine_key.clone());
        let object = match tokio::time::timeout(budget, store.get(&quarantine)).await {
            Err(_) => {
                self.retry_asset(job, attempts, "reading the object timed out")
                    .await;
                return None;
            }
            Ok(Err(object_store::Error::NotFound { .. })) => {
                self.retry_asset(job, attempts, "the upload's object is missing")
                    .await;
                return None;
            }
            Ok(Err(error)) => {
                self.retry_asset(job, attempts, &trim(&error.to_string()))
                    .await;
                return None;
            }
            Ok(Ok(object)) => object,
        };
        let Ok(Ok(bytes)) = tokio::time::timeout(budget, object.bytes()).await else {
            self.retry_asset(job, attempts, "reading the object timed out")
                .await;
            return None;
        };
        Some(bytes)
    }

    /// A verified image: the stripped bytes to the public key, then usable — or a retry when the
    /// store refuses the promotion write.
    async fn promote(
        &self,
        job: &colophon_db::assets::Claimed,
        attempts: u32,
        store: &colophon_media::Shared,
        key: String,
        verified: colophon_media::Verified,
    ) {
        // The object's own record of what it is: the origin serves the stored type, so a
        // re-promotion can't drift from what was verified, and nothing reads the row to serve.
        let mut attributes = object_store::Attributes::new();
        attributes.insert(
            object_store::Attribute::ContentType,
            verified.detected_type.into(),
        );
        let written = store
            .put_opts(
                &object_store::path::Path::from(key.clone()),
                verified.stripped.clone().into(),
                object_store::PutOptions {
                    attributes: attributes.clone(),
                    ..object_store::PutOptions::default()
                },
            )
            .await;
        if let Err(error) = written {
            self.retry_asset(job, attempts, &trim(&error.to_string()))
                .await;
            return;
        }
        // The renditions beside it: `public/…/{asset}/{name}`, each tagged with the same
        // detected type. A refused write retries the whole job — the row commits only when
        // every object the origin may name has landed.
        let mut names = Vec::new();
        for rendition in &verified.renditions {
            let at = format!("{key}/{}", rendition.name);
            let written = store
                .put_opts(
                    &object_store::path::Path::from(at),
                    rendition.bytes.clone().into(),
                    object_store::PutOptions {
                        attributes: attributes.clone(),
                        ..object_store::PutOptions::default()
                    },
                )
                .await;
            if let Err(error) = written {
                self.retry_asset(job, attempts, &trim(&error.to_string()))
                    .await;
                return;
            }
            names.push(rendition.name.to_owned());
        }
        self.finish_asset(
            job,
            attempts,
            "succeeded",
            None,
            &colophon_db::assets::Outcome::Usable {
                detected_type: verified.detected_type,
                width: i32::try_from(verified.width).unwrap_or(i32::MAX),
                height: i32::try_from(verified.height).unwrap_or(i32::MAX),
                sha256: verified.sha256.to_vec(),
                public_key: key,
                derivatives: names,
            },
        )
        .await;
    }

    /// A transient failure: the job tries again, the asset stays quarantined.
    async fn retry_asset(&self, job: &colophon_db::assets::Claimed, attempts: u32, reason: &str) {
        self.finish_asset(
            job,
            attempts,
            "retrying",
            Some(reason),
            &colophon_db::assets::Outcome::Missing,
        )
        .await;
    }

    /// Records an asset attempt's end; a lost lease writes nothing (the job was claimed again).
    async fn finish_asset(
        &self,
        job: &colophon_db::assets::Claimed,
        attempts: u32,
        status: &str,
        error: Option<&str>,
        outcome: &colophon_db::assets::Outcome,
    ) {
        // A retryable failure schedules the next attempt; a job that has run out of attempts or
        // out of lifetime is recorded `failed` instead.
        let (status, next_attempt_at) = if status == "retrying" {
            let outcome = job::Outcome::Retryable {
                reason: error.unwrap_or("trying again").to_owned(),
                retry_after_ms: None,
            };
            match job::after_attempt(
                &outcome,
                attempts,
                millis(OffsetDateTime::now_utc()),
                millis(job.deadline_at),
                entropy(),
            ) {
                job::Next::Retry { after_ms } => (
                    "retrying",
                    Some(
                        OffsetDateTime::now_utc()
                            + time::Duration::milliseconds(
                                i64::try_from(after_ms).unwrap_or(i64::MAX),
                            ),
                    ),
                ),
                _ => ("failed", None),
            }
        } else {
            (status, None)
        };
        match colophon_db::assets::finish(
            &self.pool,
            job,
            &self.settings.owner,
            status,
            next_attempt_at,
            error,
            outcome,
        )
        .await
        {
            Ok(true) => {}
            Ok(false) => tracing::debug!(job = %job.id, "asset lease was already gone"),
            Err(db_error) => {
                tracing::warn!(job = %job.id, error = %db_error, "recording the asset attempt failed; the lease will run out");
            }
        }
    }
}

/// At most this much of an error's text reaches a job's `last_error`.
fn trim(error: &str) -> String {
    error.chars().take(500).collect()
}

/// A validated document as [`colophon_db::migrations::apply_item`] writes it: the canonical
/// text and hash plus the metadata — title, slug, reference and asset rows — the caller
/// computed from it, as `entries::NewRevision` carries them for a save.
fn applied_for<'a>(
    content: &'a schema::Content,
    hash: &'a [u8; 32],
    unique: &'a [String],
) -> colophon_db::migrations::Applied<'a> {
    let references = content
        .references()
        .iter()
        .map(|reference| {
            json!({
                "field": reference.field,
                "position": reference.position,
                "collection": reference.collection,
                "entry": reference.entry,
            })
        })
        .collect();
    let assets = content
        .assets()
        .iter()
        .map(|asset| {
            json!({
                "field": asset.field,
                "position": asset.position,
                "asset": asset.asset,
            })
        })
        .collect();
    colophon_db::migrations::Applied {
        revision: uuid::Uuid::now_v7(),
        content: content.canonical_json(),
        content_hash: hash,
        title: content.title(),
        slug: content.slug(),
        references: serde_json::Value::Array(references),
        assets: serde_json::Value::Array(assets),
        unique,
    }
}

/// The SHA-256 of `bytes`: what a probe's evidence records of the answer it read.
fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, bytes);
    let mut out = [0; 32];
    out.copy_from_slice(digest.as_ref());
    out
}
