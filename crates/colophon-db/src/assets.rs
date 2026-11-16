//! Assets and their jobs (docs/modules/07-media.md). The API inserts an asset row and its
//! verify job in the upload's transaction, so a recorded upload always has its check enqueued,
//! and a delete request's transaction holds only the job row and the audit event — 202 means
//! requested, not gone. Workers claim jobs as `colophon_worker` under the same lease discipline
//! as delivery jobs (D-046): a verify attempt lands `usable`/`rejected`, a delete attempt's
//! [`mark_deleting`] rechecks under the row lock who still names the asset — a live draft or
//! any publication at all, which pins it forever (D-098) — before a byte is removed outside
//! any transaction, and [`finish`] lands the asset's new state and the job's outcome together
//! — a lease that expired writes neither. The retention sweep's `expire` jobs ride the same
//! attempt: [`enqueue_expired`] commits the intents and [`mark_deleting`] makes the same
//! verdict for them (D-099).

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
    /// The current version's digest, once verified: the `…/{asset}/{sha}` URL's selector (D-136).
    pub sha256: Option<Vec<u8>>,
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

/// One verified version, as [`versions`] lists it (D-136): the immutable record of a landing
/// verify attempt — its content digest, its literal object key, and the renditions written
/// under it. Rows are never updated and never removed; a deleted asset keeps its history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// Which landing it was: 1 first, each distinct later content max+1.
    pub seq: i32,
    /// What this upload claimed.
    pub declared_type: String,
    /// Its byte count as declared.
    pub size_bytes: i64,
    /// What the signature proved.
    pub detected_type: String,
    /// The stripped bytes' digest — the `…/{asset}/{sha}` pinned URL's selector.
    pub sha256: Vec<u8>,
    /// The decoded dimensions.
    pub width: i32,
    /// The decoded dimensions.
    pub height: i32,
    /// The literal object key the original lives at; renditions sit under it.
    pub public_key: String,
    /// The rendition names written under `public_key`.
    pub derivatives: Vec<String>,
    /// When it landed.
    pub created_at: OffsetDateTime,
}

/// Every verified version of `asset`, oldest first — the asset's byte history (D-136).
pub async fn versions(tx: &mut ProjectTransaction, asset: Uuid) -> Result<Vec<Version>, DbError> {
    sqlx::query_as!(
        Version,
        "SELECT seq, declared_type, size_bytes, detected_type, sha256, width, height,
                public_key, derivatives, created_at
         FROM asset_versions
         WHERE project_id = $1 AND environment_id = $2 AND asset_id = $3
         ORDER BY seq",
        tx.project,
        tx.environment,
        asset
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
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
    // The verify job is durable work: admit before recording it (D-131). The caller's
    // transaction rolls back the asset row with the refusal, so an object already put to
    // quarantine is left to the orphan sweep, never half-accepted.
    crate::work::admit(tx, 1).await?;
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
    // The upload's recorded size and the job's key are payload metadata; the install-wide
    // byte bound answers with them counted, not just the project's row count (D-135).
    crate::work::check_payload(tx).await
}

/// The environment's assets, newest first (at most 100).
pub async fn list(tx: &mut ProjectTransaction) -> Result<Vec<Asset>, DbError> {
    sqlx::query_as!(
        Asset,
        "SELECT id, state, file_name, declared_type, detected_type, sha256, size_bytes, width,
                height, rejected_reason, derivatives, created_at
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
        "SELECT id, state, file_name, declared_type, detected_type, sha256, size_bytes, width,
                height, rejected_reason, derivatives, created_at
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
    // The delete job is durable work — and only here, after the verdicts, so an asset that
    // owes a different answer still gets it rather than a `work_full` (D-131).
    crate::work::admit(tx, 1).await?;
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
    // The delete job's payload metadata counts too (D-135); like the admission above, the
    // check sits after the verdicts so an asset that owes a different answer still gets it.
    crate::work::check_payload(tx).await?;
    Ok(DeleteAsked::Queued { asset: found })
}

/// What asking to replace an asset's bytes found; nothing is written unless it is
/// [`ReplaceAsked::Queued`] (D-136).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplaceAsked {
    /// No such asset in this environment.
    Unknown,
    /// `quarantined`: the first upload's verdict is still out — wait for it to settle, then
    /// replace (same answer as a delete).
    Quarantined,
    /// `rejected`: there is no version to replace — a rejected upload is redone as a new
    /// upload, never swapped under its id.
    Rejected,
    /// `deleting` or `deleted` already: nothing left to replace.
    Gone,
    /// The reverify job is queued; the asset as found, for the answer. It stays `usable` on
    /// the current version until the replacement's verify lands or fails.
    Queued {
        /// The asset as the request found it.
        asset: Box<Asset>,
    },
}

/// Everything the reverify row records that isn't the asset's own identity: what the API
/// staged in quarantine and the job row's bookkeeping ([`request_replace`]'s one bundle,
/// D-136).
#[derive(Debug, Clone)]
pub struct Replace<'a> {
    /// Where the replacement's bytes wait in the object store.
    pub quarantine_key: &'a str,
    /// Its declared MIME type — swung onto the asset and its version row at landing.
    pub declared_type: &'a str,
    /// Its declared byte count, swung the same way.
    pub size_bytes: i64,
    /// The job row's id.
    pub job: Uuid,
    /// The request's idempotency key (namespaced `reverify:` on the job row).
    pub idempotency_key: &'a str,
    /// When the job's lifetime ends.
    pub deadline_at: OffsetDateTime,
}

/// Records a replace request (D-136): `job` as an `asset_jobs` row of kind `reverify` under
/// `reverify:{idempotency_key}`, carrying the fresh `quarantine_key` the API already wrote the
/// bytes to and the upload's declared type and size — the columns the verdict swings onto the
/// asset and its version row at landing. Only `usable` takes a replacement: quarantine still
/// owes a verdict, rejected has no version to swap, and gone is gone. References never gate a
/// replace — pinning the asset is what its versions are for.
pub async fn request_replace(
    tx: &mut ProjectTransaction,
    asset: Uuid,
    replace: &Replace<'_>,
) -> Result<ReplaceAsked, DbError> {
    let Some(found) = get(tx, asset).await? else {
        return Ok(ReplaceAsked::Unknown);
    };
    match found.state.as_str() {
        "quarantined" => return Ok(ReplaceAsked::Quarantined),
        "rejected" => return Ok(ReplaceAsked::Rejected),
        "deleting" | "deleted" => return Ok(ReplaceAsked::Gone),
        _ => {}
    }
    // The reverify job is durable work — admitted after the verdicts, so an asset that owes a
    // different answer still gets it rather than a `work_full` (D-131).
    crate::work::admit(tx, 1).await?;
    sqlx::query!(
        "INSERT INTO asset_jobs (project_id, environment_id, id, asset_id, kind,
                                 idempotency_key, deadline_at,
                                 quarantine_key, declared_type, size_bytes)
         VALUES ($1, $2, $3, $4, 'reverify', $5, $6, $7, $8, $9)",
        tx.project,
        tx.environment,
        replace.job,
        asset,
        format!("reverify:{}", replace.idempotency_key),
        replace.deadline_at,
        replace.quarantine_key,
        replace.declared_type,
        replace.size_bytes
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(ReplaceAsked::Queued {
        asset: Box::new(found),
    })
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
    /// What it is for: `verify`, `reverify`, `delete`, `expire`.
    pub kind: String,
    /// The asset it works on.
    pub asset_id: Uuid,
    /// Where this attempt's upload waits in the object store: the job's own key for a
    /// `reverify` (the replacement's object), else the asset's first-upload key (D-136).
    pub quarantine_key: String,
    /// The upload's declared type — the job's for a `reverify`, the asset's otherwise.
    pub declared_type: String,
    /// The upload's declared byte count — the job's for a `reverify`, the asset's otherwise.
    pub size_bytes: i64,
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
         RETURNING j.project_id, j.environment_id, j.id, j.kind, j.asset_id,
                   coalesce(j.quarantine_key, a.quarantine_key) AS \"quarantine_key!\",
                   coalesce(j.declared_type, a.declared_type) AS \"declared_type!\",
                   coalesce(j.size_bytes, a.size_bytes) AS \"size_bytes!\",
                   j.idempotency_key, j.attempts,
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
    /// `deleting` now — or an earlier attempt marked it and died mid-objects, so its keys are
    /// handed out again for the idempotent removals to repeat (D-136: the worker lists the
    /// asset's whole `public/…/{asset}` prefix rather than trusting recorded rows, so versions,
    /// renditions and strays go together; the quarantine objects are the only keys a prefix
    /// can't reach).
    Removing {
        /// Every quarantine object that could still exist: the asset's own key plus any open
        /// verify/reverify job's — superseded below, but their uploads may already be bytes.
        quarantine_keys: Vec<String>,
    },
    /// `deleted` already: a replayed attempt has nothing left to do.
    Gone,
    /// A live draft or a publication still names it: the job fails, the asset stays.
    StillReferenced,
    /// A state the job's kind may not act on: a `delete` meets `quarantined` (the request
    /// refuses it too, `asset_quarantined`). An `expire` is never held — an abandoned
    /// quarantine is exactly what it collects (D-099).
    Held,
}

/// The delete attempt's first half (media M3, D-098) — and the expire job's, which is the
/// same attempt (D-099): locks the asset FOR UPDATE — the lock new references
/// (`asset_references_asset_live`) and publications (`publications_asset_live`) wait on for
/// the same row — then rechecks, with everything committed before the lock was granted
/// visible, that nothing still names it, and marks it `deleting`. `Removing` hands the
/// attempt every object key to remove, all outside any transaction; [`finish`] with
/// [`Outcome::Deleted`] is the second half once they are gone. The only difference a job's
/// kind makes: `quarantined` holds a `delete` (verification may still land it `usable`)
/// while an `expire` rechecks and collects it like any other settled state.
pub async fn mark_deleting(pool: &PgPool, job: &Claimed) -> Result<Deletion, DbError> {
    let mut transaction = worker(pool).await?;
    let row = sqlx::query!(
        "SELECT state, quarantine_key
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
        "deleting" => removing(&mut transaction, job, row.quarantine_key).await?,
        // An upload still waiting on its verdict is held against a delete — the request
        // refuses it too — but not against an expire: an abandoned quarantine is exactly
        // what the sweep collects, and the recheck below pins it like any other state.
        "quarantined" if job.kind != "expire" => Deletion::Held,
        _ => {
            // usable or rejected, or quarantined under an expire: nothing may still name it
            // — a live entry's draft revision, or any publication's (closed or not:
            // publications pin forever).
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
                removing(&mut transaction, job, row.quarantine_key).await?
            }
        }
    };
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(decided)
}

/// `mark_deleting`'s second half, shared by the fresh mark and the replayed attempt: supersede
/// the asset's not-yet-started verify/reverify jobs (the asset is on its way out, so a
/// pending verdict has nobody to land on — a running one finishes itself when its swing
/// meets `deleting`, D-136) and hand back every quarantine key that could hold bytes.
async fn removing(
    transaction: &mut Transaction<'static, Postgres>,
    job: &Claimed,
    asset_key: String,
) -> Result<Deletion, DbError> {
    let open_jobs = sqlx::query!(
        "UPDATE asset_jobs SET status = 'superseded', last_error = 'the asset is being deleted',
                               lease_owner = NULL, lease_expires_at = NULL,
                               updated_at = now(), finished_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND asset_id = $3
           AND kind IN ('verify', 'reverify') AND status IN ('pending', 'retrying')
           AND quarantine_key IS NOT NULL
         RETURNING quarantine_key",
        job.project_id,
        job.environment_id,
        job.asset_id
    )
    .fetch_all(&mut **transaction)
    .await
    .map_err(DbError::from_write)?;
    let mut quarantine_keys = vec![asset_key];
    quarantine_keys.extend(open_jobs.into_iter().filter_map(|job| job.quarantine_key));
    Ok(Deletion::Removing { quarantine_keys })
}

/// What the attempt proved about the asset, or decided for it; written with the job's finish
/// or not at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It verifies: the detected type, dimensions, the stripped object's digest and the
    /// literal object key it was promoted to (`public/…/{asset}/{sha-hex}` under D-136 —
    /// the same key a retry writes again, and the row `asset_versions` keeps).
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

/// How [`finish`] left the attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finish {
    /// The job's row and every write the outcome asked for landed in one transaction.
    Applied,
    /// `owner` no longer holds the lease (the job was claimed again); nothing was written.
    Lost,
    /// The lease held and the job row wrote — `superseded`, whatever `status` was asked —
    /// but a verify verdict found no asset state left to land on (a delete moved it through
    /// `deleting`, or the row went another way). The objects the worker already promoted are
    /// nobody's now: it must remove them, they are recorded on no row (D-136).
    Orphaned,
}

/// How the attempt ended, recorded with the asset's new state in one transaction;
/// [`Finish::Lost`] when `owner` no longer holds the lease (the job was claimed again) and
/// nothing is written.
pub async fn finish(
    pool: &PgPool,
    job: &Claimed,
    owner: &str,
    status: &str,
    next_attempt_at: Option<OffsetDateTime>,
    error: Option<&str>,
    outcome: &Outcome,
) -> Result<Finish, DbError> {
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
        return Ok(Finish::Lost);
    }
    let mut landed = Finish::Applied;
    match outcome {
        // A verify verdict lands only on a state still waiting for one — a first upload's
        // `quarantined`, a replacement's `usable` (D-136): it must never resurrect an asset a
        // delete already moved through `deleting` into `deleted`, nor one `rejected` since.
        Outcome::Usable { .. } => landed = land_usable(&mut transaction, job, outcome).await?,
        // A refused first upload moves `quarantined` to `rejected`; a refused replacement
        // leaves `usable` serving its current version (the job's own `failed` is the record).
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
            let deleted = sqlx::query!(
                "UPDATE assets SET state = 'deleted'
                 WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND state = 'deleting'",
                job.project_id,
                job.environment_id,
                job.asset_id
            )
            .execute(&mut *transaction)
            .await
            .map_err(DbError::from_write)?;
            // Only the transition is ledgered: a job that finds the asset already `deleted`
            // (claimed again after a crash) changed nothing, and its row would duplicate the
            // first's. A restored backup resurrects the row live; the ledger's row (migration
            // 0048, D-124) is what marks it `deleted` again — the objects are already gone.
            if deleted.rows_affected() > 0 {
                crate::ledger::record_in(
                    &mut transaction,
                    crate::ledger::Kind::Asset,
                    job.project_id,
                    Some(job.environment_id),
                    &serde_json::json!({"asset": job.asset_id}).to_string(),
                )
                .await?;
            }
        }
        Outcome::Missing | Outcome::StillReferenced => {}
    }
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(landed)
}

/// [`finish`]'s `Outcome::Usable` half: swings the asset's current-version columns and records
/// the landing's version row when the asset still accepts a verdict — the row lock the UPDATE
/// takes serializes two landings on one asset, so the second sees the first's version when it
/// numbers its own. A verdict that finds no state left to land on supersedes the job and
/// reports [`Finish::Orphaned`], so the caller removes the objects it already promoted.
async fn land_usable(
    transaction: &mut Transaction<'static, Postgres>,
    job: &Claimed,
    outcome: &Outcome,
) -> Result<Finish, DbError> {
    let Outcome::Usable {
        detected_type,
        width,
        height,
        sha256,
        public_key,
        derivatives,
    } = outcome
    else {
        unreachable!("land_usable runs only for Outcome::Usable")
    };
    let swung = sqlx::query!(
        "UPDATE assets SET state = 'usable', detected_type = $4, width = $5, height = $6,
                           sha256 = $7, public_key = $8, derivatives = $9,
                           declared_type = $10, size_bytes = $11,
                           rejected_reason = NULL, verified_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND id = $3
           AND state IN ('quarantined', 'usable')",
        job.project_id,
        job.environment_id,
        job.asset_id,
        *detected_type,
        width,
        height,
        sha256,
        public_key,
        derivatives as &[String],
        job.declared_type,
        job.size_bytes
    )
    .execute(&mut **transaction)
    .await
    .map_err(DbError::from_write)?;
    if swung.rows_affected() != 1 {
        // The object writes already happened; only the record refuses them. The job ends
        // `superseded` — honest, and off the retry path — and the caller removes the objects.
        sqlx::query!(
            "UPDATE asset_jobs SET status = 'superseded',
                                   last_error = 'the asset no longer accepts a verdict',
                                   updated_at = now(), finished_at = now()
             WHERE project_id = $1 AND environment_id = $2 AND id = $3",
            job.project_id,
            job.environment_id,
            job.id
        )
        .execute(&mut **transaction)
        .await
        .map_err(DbError::from_write)?;
        return Ok(Finish::Orphaned);
    }
    // The version row is the landing's immutable record; a retry that promotes the same bytes
    // again meets the (asset, sha256) conflict and adds nothing (D-136).
    sqlx::query!(
        "INSERT INTO asset_versions (project_id, environment_id, asset_id, seq,
                                     declared_type, size_bytes, detected_type,
                                     width, height, sha256, public_key, derivatives)
         SELECT $1, $2, $3,
                coalesce(max(v.seq), 0) + 1, $4, $5, $6, $7, $8, $9, $10, $11
         FROM asset_versions v
         WHERE v.project_id = $1 AND v.environment_id = $2 AND v.asset_id = $3
         ON CONFLICT (project_id, environment_id, asset_id, sha256) DO NOTHING",
        job.project_id,
        job.environment_id,
        job.asset_id,
        job.declared_type,
        job.size_bytes,
        *detected_type,
        width,
        height,
        sha256,
        public_key,
        derivatives as &[String]
    )
    .execute(&mut **transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(Finish::Applied)
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

/// The retention sweep's first half (media retention, spec §18, D-099): one `expire` job for
/// every asset that has been collectable longer than `older_than` — `quarantined`, `rejected`
/// or `usable`, and in every case only while nothing names it: the delete request's keep-rule
/// applied to all three, so a draft naming even a rejected upload still pins it (M3), and a
/// stuck referrer never earns a failing job a pass. At most `limit` a pass, oldest first.
/// `expire` reuses the delete attempt end to end; [`mark_deleting`] rechecks the same
/// keep-rule under the row lock, so a name committed between this statement and the claim
/// fails the job and never loses an object. An asset already mid-job of any kind is left to
/// it: what is running decides what it becomes. Two passes racing the same asset can both
/// enqueue; the second job finds `deleted` and finishes having removed nothing. A project at
/// the work bound keeps its assets instead: one statement can't honestly hold one project's
/// admission lock, so each row counts the same open work [`crate::work::admit`] does, under
/// the statement's own snapshot — a pass may overshoot [`crate::work::MAX_OPEN`] by at most
/// its own `limit`, and the next pass sees the truth and enqueues nothing (D-131). The byte
/// bound is enforced the same way, from the caller's `room` — what
/// [`crate::work::install_payload`] left under [`crate::work::MAX_PENDING_PAYLOAD`]: a
/// running sum of each candidate's recorded size plus its key caps the pass's enqueue at
/// the room there was when it was computed, so the sweep itself can never push the
/// installation over (D-135). Both counts are of the statement's own snapshot; the gap a
/// concurrent admission leaves in them closes itself on the next pass.
pub async fn enqueue_expired(
    pool: &PgPool,
    older_than: OffsetDateTime,
    deadline_at: OffsetDateTime,
    limit: i64,
    room: i64,
) -> Result<u64, DbError> {
    let mut transaction = worker(pool).await?;
    let enqueued = sqlx::query!(
        "WITH collectable AS (
             SELECT a.project_id, a.environment_id, a.id, a.created_at, a.size_bytes,
                    'expire:' || a.id::text || ':' || gen_random_uuid()::text AS key
             FROM assets a
             WHERE a.created_at < $1 AND a.state IN ('quarantined', 'usable', 'rejected')
               AND NOT EXISTS (
                   SELECT 1 FROM asset_references r
                   JOIN entries e
                     ON e.project_id = r.project_id AND e.environment_id = r.environment_id
                    AND e.draft_revision_id = r.revision_id AND e.deleted_at IS NULL
                   WHERE r.project_id = a.project_id AND r.environment_id = a.environment_id
                     AND r.asset_id = a.id)
               AND NOT EXISTS (
                   SELECT 1 FROM asset_references r
                   JOIN publications p
                     ON p.project_id = r.project_id AND p.environment_id = r.environment_id
                    AND p.revision_id = r.revision_id
                   WHERE r.project_id = a.project_id AND r.environment_id = a.environment_id
                     AND r.asset_id = a.id)
               AND NOT EXISTS (
                   SELECT 1 FROM asset_jobs j
                   WHERE j.project_id = a.project_id AND j.environment_id = a.environment_id
                     AND j.asset_id = a.id
                     AND j.status IN ('pending', 'running', 'retrying'))
               AND (
                   SELECT count(*) FROM (
                       SELECT 1 FROM jobs j
                       WHERE j.project_id = a.project_id
                         AND j.status IN ('pending', 'retrying', 'running', 'held')
                       UNION ALL
                       SELECT 1 FROM asset_jobs j
                       WHERE j.project_id = a.project_id
                         AND j.status IN ('pending', 'running', 'retrying')
                       UNION ALL
                       SELECT 1 FROM bulk_jobs b
                       WHERE b.project_id = a.project_id AND b.state IN ('accepting', 'running')
                       UNION ALL
                       SELECT 1 FROM migration_plans m
                       WHERE m.project_id = a.project_id
                         AND m.state IN ('proposed', 'approved', 'running')
                   ) open_work
               ) < $4
             ORDER BY a.created_at
             LIMIT $3
         ), fitting AS (
             SELECT c.*, sum(c.size_bytes + octet_length(c.key))
                        OVER (ORDER BY c.created_at)::bigint AS running
             FROM collectable c
         )
         INSERT INTO asset_jobs (project_id, environment_id, id, asset_id, kind,
                                 idempotency_key, deadline_at)
         SELECT project_id, environment_id, gen_random_uuid(), id, 'expire', key, $2
         FROM fitting
         WHERE running <= $5",
        older_than,
        deadline_at,
        limit,
        crate::work::MAX_OPEN,
        room
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_write)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(enqueued.rows_affected())
}

/// Of `keys`, the ones something unfinished still needs: a `quarantined` asset's own
/// `quarantine_key`, or an open verify/reverify job's (a replacement's upload waits under a
/// key the asset row doesn't hold — D-136). The only objects the orphan sweep keeps: a
/// verified or rejected asset's leftover is dead weight, a superseded or failed job's too,
/// and a `deleting` one is already being removed, idempotently (D-099).
pub async fn quarantined_keys(pool: &PgPool, keys: &[String]) -> Result<Vec<String>, DbError> {
    let mut transaction = worker(pool).await?;
    let held = sqlx::query_scalar!(
        "SELECT quarantine_key FROM assets
         WHERE quarantine_key = ANY($1) AND state = 'quarantined'
         UNION
         SELECT quarantine_key FROM asset_jobs
         WHERE quarantine_key = ANY($1)
           AND status IN ('pending', 'retrying', 'running')",
        keys
    )
    .fetch_all(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(held.into_iter().flatten().collect())
}

/// Of `keys` found under `public/`, the ones no row serves from — the public-orphan sweep's
/// removal list (D-136). A key is kept while an asset's current `public_key` or any version's
/// equals it or prefixes it (a rendition sits under its original's key). Deleting an object
/// nobody recorded is what made a stable `public_key` dangerous; under content-addressed keys
/// the check is only ever a guard against an object that outlived its row — a restored backup
/// re-serves from the same keys, never a wrong one.
pub async fn unrecorded_public_keys(
    pool: &PgPool,
    keys: &[String],
) -> Result<Vec<String>, DbError> {
    let mut transaction = worker(pool).await?;
    let unrecorded = sqlx::query_scalar!(
        r#"SELECT v.key AS "key!" FROM (SELECT unnest($1::text[]) AS key) v
         WHERE NOT EXISTS (
             SELECT 1 FROM assets a
             WHERE v.key = a.public_key OR starts_with(v.key, a.public_key || '/'))
           AND NOT EXISTS (
             SELECT 1 FROM asset_versions ver
             WHERE v.key = ver.public_key OR starts_with(v.key, ver.public_key || '/'))"#,
        keys
    )
    .fetch_all(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(unrecorded)
}
