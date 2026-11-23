//! Delivery jobs (docs/modules/05-worker.md; D-046). The API inserts them with a release (see
//! [`crate::releases::commit`]) and lists them; workers claim, run and finalize them as
//! `colophon_worker`, which sees jobs in every project. A claim is a lease: owner, expiry and a
//! lease generation that goes up with every claim. [`finish`] only lands for the current lease,
//! so a late result from a lease that already expired and was claimed again is refused.

use std::time::Duration;

use sqlx::{PgPool, Postgres, Transaction, types::time::OffsetDateTime};
use uuid::Uuid;

use crate::{DbError, scope::ProjectTransaction};

/// A job as the API lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// Its id.
    pub id: Uuid,
    /// Its destination.
    pub destination_id: Uuid,
    /// The destination's name.
    pub destination_name: String,
    /// The release it announces.
    pub generation: i64,
    /// Where it stands.
    pub status: String,
    /// Attempts so far.
    pub attempts: i32,
    /// The last attempt's HTTP status, if it got one.
    pub last_http_status: Option<i16>,
    /// What went wrong last, if anything.
    pub last_error: Option<String>,
    /// When it was created.
    pub created_at: OffsetDateTime,
    /// When it finished, if it has.
    pub finished_at: Option<OffsetDateTime>,
}

/// The environment's jobs, newest first (at most 100); only one release's when given.
pub async fn list(
    tx: &mut ProjectTransaction,
    generation: Option<i64>,
) -> Result<Vec<Job>, DbError> {
    sqlx::query_as!(
        Job,
        "SELECT j.id, j.destination_id, d.name AS destination_name, j.generation, j.status, j.attempts,
                j.last_http_status, j.last_error, j.created_at, j.finished_at
         FROM jobs j
         JOIN destinations d ON d.project_id = j.project_id AND d.environment_id = j.environment_id
                            AND d.id = j.destination_id
         WHERE j.project_id = $1 AND j.environment_id = $2 AND ($3::bigint IS NULL OR j.generation = $3)
         ORDER BY j.generation DESC, j.id DESC
         LIMIT 100",
        tx.project,
        tx.environment,
        generation
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// A worker's transaction: as `colophon_worker`, for its duration.
async fn worker(pool: &PgPool) -> Result<Transaction<'static, Postgres>, DbError> {
    let mut transaction = crate::begin_tx(pool).await.map_err(DbError::from_query)?;
    sqlx::query("SET LOCAL ROLE colophon_worker")
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    Ok(transaction)
}

/// A job a worker holds the lease on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claimed {
    /// Its project.
    pub project_id: Uuid,
    /// Its environment.
    pub environment_id: Uuid,
    /// Its id.
    pub id: Uuid,
    /// Its destination.
    pub destination_id: Uuid,
    /// The destination's kind: `webhook` (announce the release), `cdn` (purge what changed) or
    /// `probe` (look for the release).
    pub kind: String,
    /// Where to post.
    pub url: String,
    /// The release it announces.
    pub generation: i64,
    /// The key every attempt sends.
    pub idempotency_key: String,
    /// This attempt's number: 1 for the first.
    pub attempts: i32,
    /// The lease generation this claim holds.
    pub lease_generation: i32,
    /// When the job's lifetime ends.
    pub deadline_at: OffsetDateTime,
}

/// Fails unfinished jobs that ran out of time (a worker runs this now and then, not on every
/// claim); how many. A sweep never waits: it takes only the rows it can lock at once (`SKIP
/// LOCKED`), and a job another transaction holds (a release superseding it, a worker finishing
/// it) is the next pass's, so the sweep can never be part of a lock cycle (CI 44719897863: two
/// sweeps and a supersede, each locking rows in its own order, deadlocked). Each row it takes is
/// updated by its key and generation (D-237): on the key alone the planner, taking a project and an
/// environment as independent, found rows through `jobs_by_generation` on the environment and read
/// every job of the environment for each.
pub async fn expire(pool: &PgPool, stop: crate::Stop<'_>) -> Result<u64, DbError> {
    let mut transaction = crate::transactions::background_worker(pool, stop).await?;
    let expired = sqlx::query!(
        "WITH due AS (
             SELECT project_id, environment_id, id, generation FROM jobs
             WHERE deadline_at <= now() AND status IN ('pending', 'retrying', 'running')
               AND (status <> 'running' OR lease_expires_at < now())
             ORDER BY id
             FOR UPDATE SKIP LOCKED
         )
         UPDATE jobs j SET status = 'failed', last_error = 'ran out of time', lease_owner = NULL,
                           lease_expires_at = NULL, updated_at = now(), finished_at = now()
         FROM due
         WHERE j.project_id = due.project_id AND j.environment_id = due.environment_id AND j.id = due.id
           AND j.generation = due.generation"
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(expired.rows_affected())
}

/// Claims up to `limit` due jobs for `owner`, a worker at `location`, for `lease`: waiting jobs
/// whose time has come, within their lifetime, whose destination has no other job in flight
/// under a live lease (one announcement in flight per destination, D-047) and isn't a probe
/// pinned to another location (D-051). Rows another worker is claiming are skipped, not waited
/// for.
///
/// A claim reads a few blocks however large the backlog, the history and the installation
/// (D-237): waiting jobs only, by an ordered scan of `jobs_due` that stops at the limit; each
/// claimed job found by its key and generation carried out of `due`, and its destination's
/// fields by scalar subqueries on the destination's full key (joining `destinations` on the
/// environment alone made the planner find each claimed job by reading every job of its
/// environment). Planned for its own limit every call (`plan_cache_mode = force_custom_plan`):
/// a generic plan, which a prepared statement settles on after five calls, can't know `LIMIT
/// $3` is a handful, and read every destination of the installation twice (1,650 buffers a claim
/// at 12,000 destinations, against 309) or every job (36,000) for the UPDATE's join. A job whose worker stopped answering is made
/// waiting again by [`release_lapsed`] on the sweep's interval, rather than looked for here: an
/// `OR` over running jobs whose lease ran out could use no index in order, so every claim read
/// and sorted the whole backlog to keep a handful (631-655 blocks a claim at 6,000 waiting).
pub async fn claim(
    pool: &PgPool,
    owner: &str,
    location: &str,
    lease: Duration,
    limit: i64,
    stop: crate::Stop<'_>,
) -> Result<Vec<Claimed>, DbError> {
    // A background unit: a stop while it waits for a connection ends it, nothing claimed.
    let mut transaction = crate::transactions::background_worker(pool, stop).await?;
    // Planned for this call's limit every time, never once for all (see above).
    sqlx::query("SET LOCAL plan_cache_mode = force_custom_plan")
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    let lease_ms = i64::try_from(lease.as_millis()).unwrap_or(i64::MAX);
    let claimed = sqlx::query_as!(
        Claimed,
        r#"WITH due AS (
             SELECT j.project_id, j.environment_id, j.id, j.destination_id, j.generation
             FROM jobs j
             WHERE j.status IN ('pending', 'retrying') AND j.next_attempt_at <= now()
               AND j.deadline_at > now()
               AND NOT EXISTS (
                   SELECT 1 FROM jobs busy
                   WHERE busy.project_id = j.project_id AND busy.environment_id = j.environment_id
                     AND busy.destination_id = j.destination_id AND busy.status = 'running'
                     AND busy.lease_expires_at >= now() AND busy.id <> j.id
               )
               AND EXISTS (
                   SELECT 1 FROM destinations here
                   WHERE here.project_id = j.project_id AND here.environment_id = j.environment_id
                     AND here.id = j.destination_id AND (here.location IS NULL OR here.location = $4)
               )
             ORDER BY j.next_attempt_at
             LIMIT $3
             FOR UPDATE OF j SKIP LOCKED
         )
         UPDATE jobs j SET status = 'running', lease_owner = $1,
                           lease_expires_at = now() + make_interval(secs => $2::bigint / 1000.0),
                           lease_generation = j.lease_generation + 1, attempts = j.attempts + 1, updated_at = now()
         FROM due
         WHERE j.project_id = due.project_id AND j.environment_id = due.environment_id AND j.id = due.id
           AND j.generation = due.generation
         RETURNING j.project_id, j.environment_id, j.id, j.destination_id,
                   (SELECT d.kind FROM destinations d
                    WHERE d.project_id = j.project_id AND d.environment_id = j.environment_id
                      AND d.id = j.destination_id) AS "kind!",
                   (SELECT d.url FROM destinations d
                    WHERE d.project_id = j.project_id AND d.environment_id = j.environment_id
                      AND d.id = j.destination_id) AS "url!",
                   j.generation, j.idempotency_key, j.attempts, j.lease_generation, j.deadline_at"#,
        owner,
        lease_ms,
        limit.clamp(1, 100),
        location
    )
    .fetch_all(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(claimed)
}

/// How many lapsed jobs one [`release_lapsed`] pass makes waiting again.
pub const RELEASE_BATCH: i64 = 200;

/// The longest one sweep keeps starting [`release_lapsed`] passes: under the worker's 10 s sweep
/// interval, so a sweep never runs into the next.
pub const RELEASE_BUDGET: Duration = Duration::from_secs(5);

/// Makes delivery jobs whose lease ran out waiting again (D-237): the worker that held one
/// stopped answering (a crash, a kill, a partition) and the job is anyone's. It goes back to
/// `retrying` exactly as it was otherwise, its place kept (`next_attempt_at` was due when it was
/// claimed, so the next [`claim`] takes it before newer work), its attempts, deadline, idempotency
/// key and generation untouched: the attempt it lost may have reached the destination, so the
/// next one re-announces under the same key (F03), and a newer release supersedes it as it does
/// any waiting job. Its old holder's late [`finish`] is refused, since that needs the job running
/// under that holder's lease and generation (F03, D-046). Runs on the worker's sweep interval, so
/// such a job is claimed again within that interval of its lease running out. Never waits: a job
/// another transaction holds is the next pass's (`SKIP LOCKED`, in id order, the one order every
/// multi-row lock on the work tables uses). A pass is one bounded statement of at most
/// [`RELEASE_BATCH`] jobs under the background bounds (D-236), each updated by its key and
/// generation (as [`expire`]'s). How many it released.
pub async fn release_lapsed(pool: &PgPool, stop: crate::Stop<'_>) -> Result<u64, DbError> {
    let mut transaction = crate::transactions::background_worker(pool, stop).await?;
    let released = sqlx::query!(
        "WITH lapsed AS (
             SELECT project_id, environment_id, id, generation FROM jobs
             WHERE status = 'running' AND lease_expires_at < now() AND deadline_at > now()
             ORDER BY id
             LIMIT $1
             FOR UPDATE SKIP LOCKED
         )
         UPDATE jobs j SET status = 'retrying', lease_owner = NULL, lease_expires_at = NULL,
                           updated_at = now()
         FROM lapsed
         WHERE j.project_id = lapsed.project_id AND j.environment_id = lapsed.environment_id
           AND j.id = lapsed.id AND j.generation = lapsed.generation",
        RELEASE_BATCH
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_query)?
    .rows_affected();
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(released)
}

/// One sweep's release of lapsed delivery jobs: [`release_lapsed`] passes, each its own short
/// transaction, back to back until one comes back short of [`RELEASE_BATCH`], `stop` is requested
/// (asked between passes and before each begins, D-236) or `budget` has gone by. How many it
/// released.
pub async fn release_lapsed_all(
    pool: &PgPool,
    budget: Duration,
    stop: crate::Stop<'_>,
) -> Result<u64, DbError> {
    let started = std::time::Instant::now();
    let full = u64::try_from(RELEASE_BATCH).unwrap_or(u64::MAX);
    let mut total = 0;
    loop {
        let released = release_lapsed(pool, stop).await?;
        total += released;
        if released < full || stop.requested() || started.elapsed() >= budget {
            return Ok(total);
        }
    }
}

/// What a release announcement says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    /// The environment's slug.
    pub environment: String,
    /// The environment's epoch.
    pub epoch: i64,
    /// The changeset it published.
    pub changeset_id: Uuid,
    /// When.
    pub published_at: OffsetDateTime,
    /// The entries it changed: (entry, revision, collection, title).
    pub entries: Vec<(Uuid, Uuid, String, String)>,
}

/// The announcement `job` should send, or `None` if a newer release is already on its way to the
/// same destination: then this one must never be sent after it. One transaction.
pub async fn prepare(pool: &PgPool, job: &Claimed) -> Result<Option<Announcement>, DbError> {
    let mut transaction = worker(pool).await?;
    let newer = sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM jobs
               WHERE project_id = $1 AND environment_id = $2 AND destination_id = $3
                 AND generation > $4 AND status <> 'superseded'
           ) AS "newer!""#,
        job.project_id,
        job.environment_id,
        job.destination_id,
        job.generation
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    if newer {
        return Ok(None);
    }
    let release = sqlx::query!(
        "SELECT e.slug, r.epoch, r.changeset_id, r.published_at
         FROM releases r JOIN environments e ON e.project_id = r.project_id AND e.id = r.environment_id
         WHERE r.project_id = $1 AND r.environment_id = $2 AND r.generation = $3",
        job.project_id,
        job.environment_id,
        job.generation
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    let entries = sqlx::query!(
        "SELECT entry_id, revision_id, collection, title FROM publications
         WHERE project_id = $1 AND environment_id = $2 AND valid_from = $3
         ORDER BY entry_id",
        job.project_id,
        job.environment_id,
        job.generation
    )
    .fetch_all(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(Some(Announcement {
        environment: release.slug,
        epoch: release.epoch,
        changeset_id: release.changeset_id,
        published_at: release.published_at,
        entries: entries
            .into_iter()
            .map(|row| (row.entry_id, row.revision_id, row.collection, row.title))
            .collect(),
    }))
}

/// Whether a newer release's job for the same destination is on its way, so `job` would only
/// repeat what that one does (a probe that sees the newer release sees this one's changes too).
pub async fn newer(pool: &PgPool, job: &Claimed) -> Result<bool, DbError> {
    let mut transaction = worker(pool).await?;
    let newer = sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM jobs
               WHERE project_id = $1 AND environment_id = $2 AND destination_id = $3
                 AND generation > $4 AND status <> 'superseded'
           ) AS "newer!""#,
        job.project_id,
        job.environment_id,
        job.destination_id,
        job.generation
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(newer)
}

/// What a CDN destination's job purges: every entry (collection, entry) that the releases since
/// its last successful purge, up to and including the job's, published, replaced or took down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Purge {
    /// The generation after which this purge starts: the last one purged successfully (0 if none).
    pub since: i64,
    /// The entries, by collection then id, at most [`MAX_PURGE_ENTRIES`].
    pub entries: Vec<(String, Uuid)>,
}

/// The most entries [`purge`] reads; more than the domain names one by one anyway, so it purges
/// the environment instead (`colophon_domain::cache::MAX_PURGE_TAGS`).
pub const MAX_PURGE_ENTRIES: i64 = 100;

/// The purge a CDN destination's `job` should send, or `None` if a newer release's purge is on
/// its way to the same destination: that one covers this one's releases too. One transaction.
pub async fn purge(pool: &PgPool, job: &Claimed) -> Result<Option<Purge>, DbError> {
    let mut transaction = worker(pool).await?;
    let row = sqlx::query!(
        r#"SELECT EXISTS (
                   SELECT 1 FROM jobs
                   WHERE project_id = $1 AND environment_id = $2 AND destination_id = $3
                     AND generation > $4 AND status <> 'superseded'
               ) AS "newer!",
               coalesce((SELECT max(generation) FROM jobs
                         WHERE project_id = $1 AND environment_id = $2 AND destination_id = $3
                           AND generation < $4 AND status = 'succeeded'), 0) AS "since!""#,
        job.project_id,
        job.environment_id,
        job.destination_id,
        job.generation
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    if row.newer {
        return Ok(None);
    }
    let entries = sqlx::query!(
        r#"SELECT collection AS "collection!", entry_id AS "entry_id!" FROM (
               SELECT collection, entry_id FROM publications
               WHERE project_id = $1 AND environment_id = $2 AND valid_from > $3 AND valid_from <= $4
               UNION
               SELECT collection, entry_id FROM publications
               WHERE project_id = $1 AND environment_id = $2 AND valid_to > $3 AND valid_to <= $4
               UNION
               SELECT collection, entry_id FROM tombstones
               WHERE project_id = $1 AND environment_id = $2 AND generation > $3 AND generation <= $4
           ) changed
           ORDER BY collection, entry_id
           LIMIT $5"#,
        job.project_id,
        job.environment_id,
        row.since,
        job.generation,
        MAX_PURGE_ENTRIES
    )
    .fetch_all(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(Some(Purge {
        since: row.since,
        entries: entries
            .into_iter()
            .map(|row| (row.collection, row.entry_id))
            .collect(),
    }))
}

/// What a probe saw on one attempt (D-051).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation<'a> {
    /// The worker's location label.
    pub location: &'a str,
    /// The generation the answer named, if it named one.
    pub generation: Option<i64>,
    /// The HTTP status, if there was an answer.
    pub http_status: Option<i16>,
    /// The SHA-256 of the body read, if one was.
    pub body_hash: Option<[u8; 32]>,
    /// What went wrong, if anything (at most 500 characters).
    pub error: Option<&'a str>,
}

/// How an attempt ended, as recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finish<'a> {
    /// `succeeded`, `retrying`, `failed` or `superseded`.
    pub status: &'a str,
    /// For `retrying`: when to try again.
    pub next_attempt_at: Option<OffsetDateTime>,
    /// The HTTP status, if there was one.
    pub http_status: Option<i16>,
    /// What went wrong, if anything (at most 500 characters).
    pub error: Option<&'a str>,
    /// For a probe: what it saw, stored with the result.
    pub observation: Option<Observation<'a>>,
}

/// Records how `job`'s attempt ended, and for a probe what it saw, only if `owner` still holds
/// that lease; false if it doesn't (the lease expired and the job was claimed again), and nothing
/// is written.
pub async fn finish(
    pool: &PgPool,
    job: &Claimed,
    owner: &str,
    finish: &Finish<'_>,
) -> Result<bool, DbError> {
    let mut transaction = worker(pool).await?;
    let error: Option<String> = finish.error.map(|text| text.chars().take(500).collect());
    let updated = sqlx::query!(
        "UPDATE jobs SET status = $6, lease_owner = NULL, lease_expires_at = NULL,
                         next_attempt_at = coalesce($7, next_attempt_at),
                         last_http_status = $8, last_error = $9, updated_at = now(),
                         finished_at = CASE WHEN $6 IN ('succeeded', 'failed', 'superseded') THEN now() END
         WHERE project_id = $1 AND environment_id = $2 AND id = $3
           AND status = 'running' AND lease_owner = $4 AND lease_generation = $5",
        job.project_id,
        job.environment_id,
        job.id,
        owner,
        job.lease_generation,
        finish.status,
        finish.next_attempt_at,
        finish.http_status,
        error
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    if updated.rows_affected() != 1 {
        return Ok(false);
    }
    // The job's budget reservation closes with its outcome (D-113): delivered or given up
    // settles the slot it held; superseded releases it — the job can never run now. A retry
    // keeps it held: the job is still in flight.
    if let Some(close) = colophon_domain::job::Status::parse(finish.status)
        .and_then(colophon_domain::budget::close_for)
    {
        crate::budgets::close_job(
            &mut transaction,
            job.project_id,
            job.environment_id,
            job.id,
            close,
        )
        .await?;
    }
    if let Some(seen) = &finish.observation {
        let error: Option<String> = seen.error.map(|text| text.chars().take(500).collect());
        sqlx::query!(
            "INSERT INTO observations (project_id, environment_id, id, job_id, destination_id, generation, location,
                                       observed_generation, http_status, body_hash, error)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
            job.project_id,
            job.environment_id,
            Uuid::now_v7(),
            job.id,
            job.destination_id,
            job.generation,
            seen.location,
            seen.generation,
            seen.http_status,
            seen.body_hash.as_ref().map(|hash| &hash[..]),
            error
        )
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_write)?;
    }
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(true)
}
