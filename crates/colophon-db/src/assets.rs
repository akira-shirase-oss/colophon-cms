//! Assets and their verify jobs (docs/modules/07-media.md). The API inserts an asset row and its
//! job in the upload's transaction, so a recorded upload always has its check enqueued; workers
//! claim jobs as `colophon_worker` under the same lease discipline as delivery jobs (D-046), and
//! [`finish`] lands the asset's new state and the job's outcome together — a lease that expired
//! writes neither.

use std::time::Duration;

use sqlx::{PgPool, Postgres, Transaction, types::time::OffsetDateTime};
use uuid::Uuid;

use crate::{DbError, scope::ProjectTransaction};

/// An asset as the API lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// Its id.
    pub id: Uuid,
    /// `quarantined`, `usable` or `rejected`.
    pub state: String,
    /// What the uploader called it.
    pub file_name: String,
    /// What it claimed to be (untrusted).
    pub declared_type: String,
    /// What the signature proved, once verified.
    pub detected_type: Option<String>,
    /// The upload's byte count.
    pub size_bytes: i64,
    /// The decoded dimensions, once verified.
    pub width: Option<i32>,
    /// The decoded dimensions, once verified.
    pub height: Option<i32>,
    /// Why it was rejected, if it was.
    pub rejected_reason: Option<String>,
    /// Rendition names the worker wrote beside the original (M.3).
    pub derivatives: Vec<String>,
    /// When it was uploaded.
    pub created_at: OffsetDateTime,
}

/// Records the asset and its verify job; `job_id`/`idempotency_key` name the job so a replayed
/// upload can't enqueue a second check.
#[allow(clippy::too_many_arguments, reason = "one row's worth of fields")]
pub async fn insert(
    tx: &mut ProjectTransaction,
    asset: Uuid,
    file_name: &str,
    declared_type: &str,
    size_bytes: i64,
    quarantine_key: &str,
    created_by: Uuid,
    job: Uuid,
    idempotency_key: &str,
    deadline_at: OffsetDateTime,
) -> Result<(), DbError> {
    sqlx::query!(
        "INSERT INTO assets (project_id, environment_id, id, file_name, declared_type, size_bytes,
                             quarantine_key, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        tx.project,
        tx.environment,
        asset,
        file_name,
        declared_type,
        size_bytes,
        quarantine_key,
        created_by
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    sqlx::query!(
        "INSERT INTO asset_jobs (project_id, environment_id, id, asset_id, kind, idempotency_key, deadline_at)
         VALUES ($1, $2, $3, $4, 'verify', $5, $6)",
        tx.project,
        tx.environment,
        job,
        asset,
        idempotency_key,
        deadline_at
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// The environment's assets, newest first (at most 100).
pub async fn list(tx: &mut ProjectTransaction) -> Result<Vec<Asset>, DbError> {
    sqlx::query_as!(
        Asset,
        "SELECT id, state, file_name, declared_type, detected_type, size_bytes, width, height,
                rejected_reason, derivatives, created_at
         FROM assets
         WHERE project_id = $1 AND environment_id = $2
         ORDER BY created_at DESC, id DESC
         LIMIT 100",
        tx.project,
        tx.environment
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// One asset, or `None`.
pub async fn get(tx: &mut ProjectTransaction, asset: Uuid) -> Result<Option<Asset>, DbError> {
    sqlx::query_as!(
        Asset,
        "SELECT id, state, file_name, declared_type, detected_type, size_bytes, width, height,
                rejected_reason, derivatives, created_at
         FROM assets
         WHERE project_id = $1 AND environment_id = $2 AND id = $3",
        tx.project,
        tx.environment,
        asset
    )
    .fetch_optional(&mut *tx.transaction)
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

/// An asset job a worker holds the lease on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claimed {
    /// Its project.
    pub project_id: Uuid,
    /// Its environment.
    pub environment_id: Uuid,
    /// Its id.
    pub id: Uuid,
    /// The asset it verifies.
    pub asset_id: Uuid,
    /// Where the upload waits in the object store.
    pub quarantine_key: String,
    /// Its declared type and byte count, for the record.
    pub declared_type: String,
    /// The key every attempt sends.
    pub idempotency_key: String,
    /// This attempt's number: 1 for the first.
    pub attempts: i32,
    /// The lease generation this claim holds.
    pub lease_generation: i32,
    /// When the job's lifetime ends.
    pub deadline_at: OffsetDateTime,
}

/// Claims up to `limit` due asset jobs for `owner` under `lease`, rows another worker is
/// claiming skipped not waited for (the delivery jobs' discipline, minus destinations).
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
             SELECT j.project_id, j.environment_id, j.id
             FROM asset_jobs j
             WHERE ((j.status IN ('pending', 'retrying') AND j.next_attempt_at <= now())
                    OR (j.status = 'running' AND j.lease_expires_at < now()))
               AND j.deadline_at > now()
             ORDER BY j.next_attempt_at
             LIMIT $3
             FOR UPDATE OF j SKIP LOCKED
         )
         UPDATE asset_jobs j SET status = 'running', lease_owner = $1,
                            lease_expires_at = now() + make_interval(secs => $2::bigint / 1000.0),
                            lease_generation = j.lease_generation + 1, attempts = j.attempts + 1,
                            updated_at = now()
         FROM due, assets a
         WHERE j.project_id = due.project_id AND j.environment_id = due.environment_id AND j.id = due.id
           AND a.project_id = j.project_id AND a.environment_id = j.environment_id AND a.id = j.asset_id
         RETURNING j.project_id, j.environment_id, j.id, j.asset_id, a.quarantine_key,
                   a.declared_type, j.idempotency_key, j.attempts, j.lease_generation, j.deadline_at",
        owner,
        lease_ms,
        limit
    )
    .fetch_all(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(claimed)
}

/// What the verify attempt proved about the asset; written with the job's finish or not at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It verifies: the detected type, dimensions, the stripped object's digest and public key.
    Usable {
        /// The signature-proved MIME type.
        detected_type: &'static str,
        /// Decoded width.
        width: i32,
        /// Decoded height.
        height: i32,
        /// The stripped bytes' SHA-256.
        sha256: Vec<u8>,
        /// Where the stripped object lives.
        public_key: String,
        /// Rendition names the worker wrote beside the original (M.3).
        derivatives: Vec<String>,
    },
    /// It was refused: the reason, safe to show the uploader.
    Rejected {
        /// Why, in words.
        reason: String,
    },
    /// The upload's object never landed: quarantine stayed empty. Transient — the job retries.
    Missing,
}

/// How the attempt ended, recorded with the asset's new state in one transaction; false when
/// `owner` no longer holds the lease (the job was claimed again) and nothing is written.
pub async fn finish(
    pool: &PgPool,
    job: &Claimed,
    owner: &str,
    status: &str,
    next_attempt_at: Option<OffsetDateTime>,
    error: Option<&str>,
    outcome: &Outcome,
) -> Result<bool, DbError> {
    let mut transaction = worker(pool).await?;
    let error: Option<String> = error.map(|text| text.chars().take(500).collect());
    let updated = sqlx::query!(
        "UPDATE asset_jobs SET status = $6, lease_owner = NULL, lease_expires_at = NULL,
                              next_attempt_at = coalesce($7, next_attempt_at), last_error = $8,
                              updated_at = now(),
                              finished_at = CASE WHEN $6 IN ('succeeded', 'failed', 'superseded') THEN now() END
         WHERE project_id = $1 AND environment_id = $2 AND id = $3
           AND status = 'running' AND lease_owner = $4 AND lease_generation = $5",
        job.project_id,
        job.environment_id,
        job.id,
        owner,
        job.lease_generation,
        status,
        next_attempt_at,
        error
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    if updated.rows_affected() != 1 {
        return Ok(false);
    }
    match outcome {
        Outcome::Usable {
            detected_type,
            width,
            height,
            sha256,
            public_key,
            derivatives,
        } => {
            sqlx::query!(
                "UPDATE assets SET state = 'usable', detected_type = $4, width = $5, height = $6,
                                   sha256 = $7, public_key = $8, derivatives = $9, verified_at = now()
                 WHERE project_id = $1 AND environment_id = $2 AND id = $3",
                job.project_id,
                job.environment_id,
                job.asset_id,
                *detected_type,
                width,
                height,
                sha256,
                public_key,
                derivatives as &[String]
            )
            .execute(&mut *transaction)
            .await
            .map_err(DbError::from_write)?;
        }
        Outcome::Rejected { reason } => {
            sqlx::query!(
                "UPDATE assets SET state = 'rejected', rejected_reason = $4, verified_at = now()
                 WHERE project_id = $1 AND environment_id = $2 AND id = $3",
                job.project_id,
                job.environment_id,
                job.asset_id,
                reason
            )
            .execute(&mut *transaction)
            .await
            .map_err(DbError::from_write)?;
        }
        Outcome::Missing => {}
    }
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(true)
}

/// Fails unfinished asset jobs that ran out of time (swept with the delivery jobs'); how many.
pub async fn expire(pool: &PgPool) -> Result<u64, DbError> {
    let mut transaction = worker(pool).await?;
    let expired = sqlx::query!(
        "UPDATE asset_jobs SET status = 'failed', last_error = 'ran out of time',
                              lease_owner = NULL, lease_expires_at = NULL,
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
