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
    let mut transaction = pool.begin().await.map_err(DbError::from_query)?;
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
/// claim); how many.
pub async fn expire(pool: &PgPool) -> Result<u64, DbError> {
    let mut transaction = worker(pool).await?;
    let expired = sqlx::query!(
        "UPDATE jobs SET status = 'failed', last_error = 'ran out of time', lease_owner = NULL, lease_expires_at = NULL,
                         updated_at = now(), finished_at = now()
         WHERE deadline_at <= now() AND status IN ('pending', 'retrying', 'running')
           AND (status <> 'running' OR lease_expires_at < now())"
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(expired.rows_affected())
}

/// Claims up to `limit` due jobs for `owner`, for `lease`: waiting jobs whose time has come, and
/// running ones whose lease ran out, within their lifetime. Rows another worker is claiming are
/// skipped, not waited for.
pub async fn claim(
    pool: &PgPool,
    owner: &str,
    lease: Duration,
    limit: i64,
) -> Result<Vec<Claimed>, DbError> {
    let mut transaction = worker(pool).await?;
    let lease_ms = i64::try_from(lease.as_millis()).unwrap_or(i64::MAX);
    let claimed = sqlx::query_as!(
        Claimed,
        "WITH due AS (
             SELECT project_id, environment_id, id FROM jobs
             WHERE ((status IN ('pending', 'retrying') AND next_attempt_at <= now())
                    OR (status = 'running' AND lease_expires_at < now()))
               AND deadline_at > now()
             ORDER BY next_attempt_at
             LIMIT $3
             FOR UPDATE SKIP LOCKED
         )
         UPDATE jobs j SET status = 'running', lease_owner = $1,
                           lease_expires_at = now() + make_interval(secs => $2::bigint / 1000.0),
                           lease_generation = j.lease_generation + 1, attempts = j.attempts + 1, updated_at = now()
         FROM due, destinations d
         WHERE j.project_id = due.project_id AND j.environment_id = due.environment_id AND j.id = due.id
           AND d.project_id = j.project_id AND d.environment_id = j.environment_id AND d.id = j.destination_id
         RETURNING j.project_id, j.environment_id, j.id, j.destination_id, d.url, j.generation, j.idempotency_key,
                   j.attempts, j.lease_generation, j.deadline_at",
        owner,
        lease_ms,
        limit.clamp(1, 100)
    )
    .fetch_all(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(claimed)
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
}

/// Records how `job`'s attempt ended, only if `owner` still holds that lease; false if it
/// doesn't (the lease expired and the job was claimed again), and nothing is written.
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
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(updated.rows_affected() == 1)
}
