//! Assets and their jobs (docs/modules/07-media.md). The API inserts an asset row and its
//! verify job in the upload's transaction, so a recorded upload always has its check enqueued,
//! and a delete request's transaction holds only the job row and the audit event — 202 means
//! requested, not gone. Workers claim jobs as `colophon_worker` under the same lease discipline
//! as delivery jobs (D-046): a verify attempt lands `usable`/`rejected`, a delete attempt's
//! [`mark_deleting`] rechecks under the row lock who still names the asset — a live draft or
//! any publication at all, which pins it forever (D-098) — before a byte is removed outside
//! any transaction, and [`finish`] lands the asset's new state and the job's outcome together
//! — a lease that expired writes neither.

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

/// An entry that still names the asset, as the deletion's keep-check counts it (media M3,
/// D-098): through a live draft's revision, or through any publication of it — a publication
/// pins its assets forever, closed or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetReferrer {
    /// The entry.
    pub entry_id: Uuid,
    /// Its collection.
    pub collection: String,
    /// The field naming the asset.
    pub field: String,
    /// Its draft names the asset.
    pub in_draft: bool,
    /// A publication of it names the asset — any one: published history counts too.
    pub published: bool,
}

/// At most this many referrers a list names.
pub const MAX_REFERRERS: i64 = 100;

/// The entries that still name `asset`: what blocks its deletion, and who the references
/// endpoint and the `still_referenced` refusal list. The same rule as the worker's recheck
/// inside [`mark_deleting`], without the lock — a request-time answer, not the authority.
pub async fn references(
    tx: &mut ProjectTransaction,
    asset: Uuid,
) -> Result<Vec<AssetReferrer>, DbError> {
    let rows = sqlx::query!(
        r#"SELECT entry_id AS "entry_id!", collection AS "collection!", field AS "field!",
                  bool_or(draft) AS "in_draft!", bool_or(live) AS "published!"
           FROM (
               SELECT e.id AS entry_id, e.collection, r.field, true AS draft, false AS live
               FROM asset_references r
               JOIN entries e
                 ON e.project_id = r.project_id AND e.environment_id = r.environment_id
                AND e.draft_revision_id = r.revision_id AND e.deleted_at IS NULL
               WHERE r.project_id = $1 AND r.environment_id = $2 AND r.asset_id = $3
               UNION ALL
               SELECT p.entry_id, p.collection, r.field, false, true
               FROM asset_references r
               JOIN publications p
                 ON p.project_id = r.project_id AND p.environment_id = r.environment_id
                AND p.revision_id = r.revision_id
               WHERE r.project_id = $1 AND r.environment_id = $2 AND r.asset_id = $3
           ) named
           GROUP BY entry_id, collection, field
           ORDER BY collection, entry_id, field
           LIMIT $4"#,
        tx.project,
        tx.environment,
        asset,
        MAX_REFERRERS
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(rows
        .into_iter()
        .map(|row| AssetReferrer {
            entry_id: row.entry_id,
            collection: row.collection,
            field: row.field,
            in_draft: row.in_draft,
            published: row.published,
        })
        .collect())
}

/// What asking to delete an asset found; nothing is written unless it is [`DeleteAsked::Queued`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteAsked {
    /// No such asset in this environment.
    Unknown,
    /// `quarantined`: the verify job may still land it `usable`, so deleting it now would leave
    /// a job whose outcome nobody asked for. Refused — the request may be repeated once settled.
    Quarantined,
    /// `deleting` or `deleted` already: nothing left to ask for.
    Gone,
    /// A live draft or a publication still names it, so its objects stay.
    Referenced(Vec<AssetReferrer>),
    /// The delete job is queued; the asset as found, for the audit event and the answer.
    Queued {
        /// The asset as the request found it.
        asset: Asset,
    },
}

/// Records the delete request: `job` as an `asset_jobs` row of kind `delete` under
/// `idempotency_key` (the request's, namespaced so it can't meet an upload's job key), its
/// deadline the same lifetime a verify gets. The request-time checks ride along — an asset
/// that isn't there, still in quarantine, already going, or still named gets its verdict and
/// no job; the check that counts is the worker's, under the row lock in [`mark_deleting`].
pub async fn request_delete(
    tx: &mut ProjectTransaction,
    asset: Uuid,
    job: Uuid,
    idempotency_key: &str,
    deadline_at: OffsetDateTime,
) -> Result<DeleteAsked, DbError> {
    let Some(found) = get(tx, asset).await? else {
        return Ok(DeleteAsked::Unknown);
    };
    match found.state.as_str() {
        "quarantined" => return Ok(DeleteAsked::Quarantined),
        "deleting" | "deleted" => return Ok(DeleteAsked::Gone),
        _ => {}
    }
    let referrers = references(tx, asset).await?;
    if !referrers.is_empty() {
        return Ok(DeleteAsked::Referenced(referrers));
    }
    sqlx::query!(
        "INSERT INTO asset_jobs (project_id, environment_id, id, asset_id, kind, idempotency_key, deadline_at)
         VALUES ($1, $2, $3, $4, 'delete', $5, $6)",
        tx.project,
        tx.environment,
        job,
        asset,
        format!("delete:{idempotency_key}"),
        deadline_at
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(DeleteAsked::Queued { asset: found })
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
    /// What it is for: `verify`, `delete`, `expire`.
    pub kind: String,
    /// The asset it works on.
    pub asset_id: Uuid,
    /// Where the upload waits in the object store.
    pub quarantine_key: String,
    /// Where the verified object is served from, once verification lands it.
    pub public_key: Option<String>,
    /// The rendition names written beside the public key (`{public_key}/{name}`).
    pub derivatives: Vec<String>,
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
         RETURNING j.project_id, j.environment_id, j.id, j.kind, j.asset_id, a.quarantine_key,
                   a.public_key, a.derivatives, a.declared_type, j.idempotency_key, j.attempts,
                   j.lease_generation, j.deadline_at",
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

/// What a delete attempt decided under the asset's row lock (media M3, D-098).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Deletion {
    /// `deleting` now — or an earlier attempt marked it and died mid-objects, so its list is
    /// handed out again for the idempotent removals to repeat.
    Removing {
        /// The public object, if verification landed one.
        public_key: Option<String>,
        /// The rendition names written beside it (`{public_key}/{name}`).
        derivatives: Vec<String>,
        /// The quarantine object it started as.
        quarantine_key: String,
    },
    /// `deleted` already: a replayed attempt has nothing left to do.
    Gone,
    /// A live draft or a publication still names it: the job fails, the asset stays.
    StillReferenced,
    /// A state a delete may not act on (`quarantined`): the job fails, the asset stays.
    Held,
}

/// The delete attempt's first half (media M3, D-098): locks the asset FOR UPDATE — the lock
/// new references (`asset_references_asset_live`) and publications
/// (`publications_asset_live`) wait on for the same row — then rechecks, with everything
/// committed before the lock was granted visible, that nothing still names it, and marks it
/// `deleting`. `Removing` hands the attempt every object key to remove, all outside any
/// transaction; [`finish`] with [`Outcome::Deleted`] is the second half once they are gone.
pub async fn mark_deleting(pool: &PgPool, job: &Claimed) -> Result<Deletion, DbError> {
    let mut transaction = worker(pool).await?;
    let row = sqlx::query!(
        "SELECT state, public_key, quarantine_key, derivatives
         FROM assets
         WHERE project_id = $1 AND environment_id = $2 AND id = $3
         FOR UPDATE",
        job.project_id,
        job.environment_id,
        job.asset_id
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    let Some(row) = row else {
        // The asset's own foreign key keeps a job from outliving it; unreachable in practice.
        transaction.commit().await.map_err(DbError::from_query)?;
        return Ok(Deletion::Gone);
    };
    let decided = match row.state.as_str() {
        "deleted" => Deletion::Gone,
        "deleting" => Deletion::Removing {
            public_key: row.public_key,
            derivatives: row.derivatives,
            quarantine_key: row.quarantine_key,
        },
        "quarantined" => Deletion::Held,
        _ => {
            // usable or rejected: nothing may still name it — a live entry's draft revision,
            // or any publication's (closed or not: publications pin forever).
            let referenced = sqlx::query_scalar!(
                r#"SELECT EXISTS (
                       SELECT 1 FROM asset_references r
                       WHERE r.project_id = $1 AND r.environment_id = $2 AND r.asset_id = $3
                         AND (EXISTS (SELECT 1 FROM entries e
                                      WHERE e.project_id = r.project_id
                                        AND e.environment_id = r.environment_id
                                        AND e.draft_revision_id = r.revision_id
                                        AND e.deleted_at IS NULL)
                              OR EXISTS (SELECT 1 FROM publications p
                                         WHERE p.project_id = r.project_id
                                           AND p.environment_id = r.environment_id
                                           AND p.revision_id = r.revision_id))
                   ) AS "referenced!""#,
                job.project_id,
                job.environment_id,
                job.asset_id
            )
            .fetch_one(&mut *transaction)
            .await
            .map_err(DbError::from_query)?;
            if referenced {
                Deletion::StillReferenced
            } else {
                sqlx::query!(
                    "UPDATE assets SET state = 'deleting'
                     WHERE project_id = $1 AND environment_id = $2 AND id = $3",
                    job.project_id,
                    job.environment_id,
                    job.asset_id
                )
                .execute(&mut *transaction)
                .await
                .map_err(DbError::from_write)?;
                Deletion::Removing {
                    public_key: row.public_key,
                    derivatives: row.derivatives,
                    quarantine_key: row.quarantine_key,
                }
            }
        }
    };
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(decided)
}

/// What the attempt proved about the asset, or decided for it; written with the job's finish
/// or not at all.
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
    /// A delete's objects are gone: the asset becomes `deleted`, its last state.
    Deleted,
    /// A delete's recheck found it still named, or a state no delete may act on: the asset
    /// stays exactly as it is (the job records the verdict and fails).
    StillReferenced,
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
        // A verify verdict lands only while the asset still waits for one: it must never
        // resurrect an asset a delete already moved through `deleting` into `deleted`.
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
                 WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND state = 'quarantined'",
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
                 WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND state = 'quarantined'",
                job.project_id,
                job.environment_id,
                job.asset_id,
                reason
            )
            .execute(&mut *transaction)
            .await
            .map_err(DbError::from_write)?;
        }
        // `deleting` only ever reaches `deleted`: a claimed-again job or a crash between the
        // stages can't flip it back.
        Outcome::Deleted => {
            sqlx::query!(
                "UPDATE assets SET state = 'deleted'
                 WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND state = 'deleting'",
                job.project_id,
                job.environment_id,
                job.asset_id
            )
            .execute(&mut *transaction)
            .await
            .map_err(DbError::from_write)?;
        }
        Outcome::Missing | Outcome::StillReferenced => {}
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
