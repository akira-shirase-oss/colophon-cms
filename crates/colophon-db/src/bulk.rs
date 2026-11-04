//! Bounded bulk work on drafts (P5, D-116, migration 0046): a job is a manifest of exact
//! entries and base revisions, chunked at most 100 items a request; the worker drives it as
//! the identity that submitted it, one transaction per item, and every item answers its own
//! outcome — `applied`, `conflict`, `refused` or `skipped`. A multi-chunk job is never
//! globally atomic; its resume is the rows still `pending`.

use serde_json::Value;
use sqlx::{Connection as _, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{DbError, PgConnection, scope::ProjectTransaction};

/// The states a job moves through. `accepting`: chunks may still land and it hasn't been
/// sealed; `running`: the worker drives it; then exactly one terminal state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The manifest is still open to appends.
    Accepting,
    /// Sealed: the worker drives its items.
    Running,
    /// Every item terminal, none refused or conflicted.
    Done,
    /// Finished with at least one item to look at (refused or conflicted).
    Blocked,
    /// Cancelled before it finished.
    Cancelled,
}

impl State {
    /// The text the `state` column stores.
    pub fn as_str(self) -> &'static str {
        match self {
            State::Accepting => "accepting",
            State::Running => "running",
            State::Done => "done",
            State::Blocked => "blocked",
            State::Cancelled => "cancelled",
        }
    }

    /// Whether the job can never move again.
    pub fn is_terminal(self) -> bool {
        matches!(self, State::Done | State::Blocked | State::Cancelled)
    }
}

/// One bulk job, as a row.
#[derive(Debug, Clone)]
pub struct Job {
    /// The project.
    pub project: Uuid,
    /// The environment.
    pub environment: Uuid,
    /// The job.
    pub id: Uuid,
    /// Its family: 'entries'.
    pub kind: String,
    /// Its operation document: `{"collection": "<slug>"}` for 'entries'.
    pub operation: Value,
    /// SHA-256 rolled over the job head and each appended chunk.
    pub manifest_hash: Vec<u8>,
    /// Where it is.
    pub state: String,
    /// Who submitted it: the identity every item's transaction opens as.
    pub created_by: Uuid,
    /// `member` or `token`: which scope path [`crate::scope`] opens for it.
    pub principal: String,
    /// The manifest's size and progress.
    pub expected_items: i64,
    /// Items that reached a terminal state.
    pub done_items: i64,
    /// Items that finished `conflict` or `refused`.
    pub blocked_items: i64,
    /// When it was sealed (left `accepting`).
    pub started_at: Option<OffsetDateTime>,
    /// When it reached a terminal state.
    pub finished_at: Option<OffsetDateTime>,
    /// Why a blocked or cancelled job stopped.
    pub error: Option<String>,
    /// When it was accepted.
    pub created_at: OffsetDateTime,
    /// When it last changed.
    pub updated_at: OffsetDateTime,
}

impl Job {
    /// The collection an 'entries' job's items act on, from its operation document.
    pub fn collection(&self) -> Option<&str> {
        self.operation.get("collection").and_then(Value::as_str)
    }
}

/// The job head used to create one.
pub struct NewJob<'a> {
    /// Its id (`UUIDv7`).
    pub id: Uuid,
    /// 'entries'.
    pub kind: &'a str,
    /// `{"collection": "<slug>"}`.
    pub operation: Value,
    /// sha256 over the head and first chunk.
    pub manifest_hash: [u8; 32],
    /// Whether `created_by` acts as a member or a project API token.
    pub principal: &'a str,
    /// The first chunk's item count.
    pub expected_items: i64,
    /// Whether the submit sealed the manifest at once.
    pub seal: bool,
}

/// One manifest item.
#[derive(Debug, Clone)]
pub struct Item {
    /// The caller's stable name for it within the job.
    pub key: String,
    /// The entry it names.
    pub entry_id: Uuid,
    /// The base revision the write is bound to.
    pub expected_revision_id: Uuid,
    /// Its operation document.
    pub input: Value,
    /// `pending`, `applied`, `conflict`, `refused` or `skipped`.
    pub outcome: String,
    /// Why it didn't apply, when it didn't.
    pub problem: Option<Value>,
    /// The revision an applied write made.
    pub new_revision_id: Option<Uuid>,
    /// When it applied.
    pub applied_at: Option<OffsetDateTime>,
}

/// One item of a chunk being appended.
pub struct NewItem<'a> {
    /// Its client key.
    pub key: &'a str,
    /// The entry it names.
    pub entry_id: Uuid,
    /// Its base revision.
    pub expected_revision_id: Uuid,
    /// Its operation document.
    pub input: Value,
}

#[derive(sqlx::FromRow)]
struct JobRow {
    project_id: Uuid,
    environment_id: Uuid,
    id: Uuid,
    kind: String,
    operation: Value,
    manifest_hash: Vec<u8>,
    state: String,
    created_by: Uuid,
    principal: String,
    expected_items: i64,
    done_items: i64,
    blocked_items: i64,
    started_at: Option<OffsetDateTime>,
    finished_at: Option<OffsetDateTime>,
    error: Option<String>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

fn job_of(row: JobRow) -> Job {
    Job {
        project: row.project_id,
        environment: row.environment_id,
        id: row.id,
        kind: row.kind,
        operation: row.operation,
        manifest_hash: row.manifest_hash,
        state: row.state,
        created_by: row.created_by,
        principal: row.principal,
        expected_items: row.expected_items,
        done_items: row.done_items,
        blocked_items: row.blocked_items,
        started_at: row.started_at,
        finished_at: row.finished_at,
        error: row.error,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

#[derive(sqlx::FromRow)]
struct ItemRow {
    key: String,
    entry_id: Uuid,
    expected_revision_id: Uuid,
    input: Value,
    outcome: String,
    problem: Option<Value>,
    new_revision_id: Option<Uuid>,
    applied_at: Option<OffsetDateTime>,
}

fn item_of(row: ItemRow) -> Item {
    Item {
        key: row.key,
        entry_id: row.entry_id,
        expected_revision_id: row.expected_revision_id,
        input: row.input,
        outcome: row.outcome,
        problem: row.problem,
        new_revision_id: row.new_revision_id,
        applied_at: row.applied_at,
    }
}

/// Inserts the job (state `accepting` or `running` when sealed at submit).
pub async fn create(tx: &mut ProjectTransaction, new: &NewJob<'_>) -> Result<Job, DbError> {
    let state = if new.seal { "running" } else { "accepting" };
    let row = sqlx::query_as!(
        JobRow,
        "INSERT INTO bulk_jobs (project_id, environment_id, id, kind, operation, manifest_hash,
                                state, created_by, principal, expected_items, started_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                 CASE WHEN $7 = 'running' THEN now() ELSE NULL END)
         RETURNING project_id, environment_id, id, kind, operation, manifest_hash, state, created_by, principal,
                   expected_items, done_items, blocked_items, started_at, finished_at, error,
                   created_at, updated_at",
        tx.project,
        tx.environment,
        new.id,
        new.kind,
        new.operation,
        &new.manifest_hash[..],
        state,
        tx.identity,
        new.principal,
        new.expected_items
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(job_of(row))
}

/// Appends `items` to `job`, each landing once under its client key (`ON CONFLICT DO
/// NOTHING`: a retried append re-lands nothing, D-116). Returns how many rows were new and
/// which keys were already there — the per-item idempotency the manifest rests on.
pub async fn append(
    tx: &mut ProjectTransaction,
    job: Uuid,
    items: &[NewItem<'_>],
) -> Result<(i64, Vec<String>), DbError> {
    let keys: Vec<&str> = items.iter().map(|item| item.key).collect();
    let entries: Vec<Uuid> = items.iter().map(|item| item.entry_id).collect();
    let revisions: Vec<Uuid> = items.iter().map(|item| item.expected_revision_id).collect();
    let inputs: Vec<Value> = items.iter().map(|item| item.input.clone()).collect();
    let rows = sqlx::query!(
        "WITH source AS (
             SELECT key, entry, revision, input
             FROM unnest($4::text[], $5::uuid[], $6::uuid[], $7::jsonb[])
                  AS s (key, entry, revision, input)
         ), landed AS (
             INSERT INTO bulk_items (project_id, environment_id, job_id, key, entry_id,
                                     expected_revision_id, input)
             SELECT $1, $2, $3, key, entry, revision, input FROM source
             ON CONFLICT (project_id, environment_id, job_id, key) DO NOTHING
             RETURNING key
         )
         SELECT s.key, (l.key IS NOT NULL) AS \"landed!\" FROM source s
         LEFT JOIN landed l ON l.key = s.key",
        tx.project,
        tx.environment,
        job,
        &keys as &[&str],
        &entries,
        &revisions,
        &inputs as &[Value]
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    let mut landed = 0_i64;
    let mut duplicates = Vec::new();
    for row in rows {
        if row.landed {
            landed += 1;
        } else if let Some(key) = row.key {
            duplicates.push(key);
        }
    }
    // The count is part of the manifest's identity: expected_items rolls with the chunk;
    // the caller rolls the hash with [`set_hash`].
    if landed > 0 {
        sqlx::query!(
            "UPDATE bulk_jobs SET expected_items = expected_items + $4, updated_at = now()
             WHERE project_id = $1 AND environment_id = $2 AND id = $3
               AND state = 'accepting'",
            tx.project,
            tx.environment,
            job,
            landed
        )
        .execute(&mut *tx.transaction)
        .await
        .map_err(DbError::from_write)?;
    }
    Ok((landed, duplicates))
}

/// The job, if it exists in this environment.
pub async fn get(tx: &mut ProjectTransaction, id: Uuid) -> Result<Option<Job>, DbError> {
    let row = sqlx::query_as!(
        JobRow,
        "SELECT project_id, environment_id, id, kind, operation, manifest_hash, state, created_by, principal,
                expected_items, done_items, blocked_items, started_at, finished_at, error,
                created_at, updated_at
         FROM bulk_jobs WHERE project_id = $1 AND environment_id = $2 AND id = $3",
        tx.project,
        tx.environment,
        id
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(row.map(job_of))
}

/// The job, locked for a state change (seal, cancel). `None` while it doesn't exist.
pub async fn lock(tx: &mut ProjectTransaction, id: Uuid) -> Result<Option<Job>, DbError> {
    let row = sqlx::query_as!(
        JobRow,
        "SELECT project_id, environment_id, id, kind, operation, manifest_hash, state, created_by, principal,
                expected_items, done_items, blocked_items, started_at, finished_at, error,
                created_at, updated_at
         FROM bulk_jobs WHERE project_id = $1 AND environment_id = $2 AND id = $3
         FOR UPDATE",
        tx.project,
        tx.environment,
        id
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(row.map(job_of))
}

/// Seals the manifest: `accepting` → `running`, so the worker may drive it. The caller
/// holds the row lock ([`lock`]) and has already checked `If-Match`; an empty manifest is
/// refused at the API.
pub async fn seal(tx: &mut ProjectTransaction, id: Uuid) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE bulk_jobs SET state = 'running', started_at = now(), updated_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND state = 'accepting'",
        tx.project,
        tx.environment,
        id
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// Rolls `digest` into the job's manifest hash (the caller already folded the chunk in).
pub async fn set_hash(
    tx: &mut ProjectTransaction,
    id: Uuid,
    manifest_hash: &[u8; 32],
) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE bulk_jobs SET manifest_hash = $4, updated_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND id = $3",
        tx.project,
        tx.environment,
        id,
        &manifest_hash[..]
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// Cancels the job from `accepting` or `running`: its pending items become `skipped` — a
/// held item can never run now — and the job is terminal. The caller checked the row lock
/// and the permission; the running case is honest because an item already in its commit
/// still lands (`outcome` isn't `pending` where this looked).
pub async fn cancel(tx: &mut ProjectTransaction, id: Uuid) -> Result<bool, DbError> {
    let cancelled = sqlx::query!(
        "UPDATE bulk_jobs SET state = 'cancelled', finished_at = now(), updated_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND id = $3
           AND state IN ('accepting', 'running')",
        tx.project,
        tx.environment,
        id
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?
    .rows_affected();
    if cancelled == 0 {
        return Ok(false);
    }
    sqlx::query!(
        "UPDATE bulk_items SET outcome = 'skipped',
            problem = jsonb_build_object('reason', 'cancelled', 'pointer', '',
                                         'message', 'the job was cancelled before this item ran')
         WHERE project_id = $1 AND environment_id = $2 AND job_id = $3 AND outcome = 'pending'",
        tx.project,
        tx.environment,
        id
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(true)
}

/// The newest jobs first, `after`'s exclusive — at most `limit` (the caller caps it).
pub async fn list(
    tx: &mut ProjectTransaction,
    after: Option<Uuid>,
    limit: i64,
) -> Result<Vec<Job>, DbError> {
    let rows = sqlx::query_as!(
        JobRow,
        "SELECT project_id, environment_id, id, kind, operation, manifest_hash, state, created_by, principal,
                expected_items, done_items, blocked_items, started_at, finished_at, error,
                created_at, updated_at
         FROM bulk_jobs
         WHERE project_id = $1 AND environment_id = $2 AND ($4::uuid IS NULL OR id < $4)
         ORDER BY id DESC LIMIT $3",
        tx.project,
        tx.environment,
        limit,
        after
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(rows.into_iter().map(job_of).collect())
}

/// A job's items in key order (the resumable cursor's order), `after` the key it names —
/// at most `limit` (the caller caps it). `outcome` filters to one outcome.
pub async fn items(
    tx: &mut ProjectTransaction,
    job: Uuid,
    outcome: Option<&str>,
    after: Option<&str>,
    limit: i64,
) -> Result<Vec<Item>, DbError> {
    let rows = sqlx::query_as!(
        ItemRow,
        "SELECT key, entry_id, expected_revision_id, input, outcome, problem, new_revision_id,
                applied_at
         FROM bulk_items
         WHERE project_id = $1 AND environment_id = $2 AND job_id = $3
           AND ($5::text IS NULL OR outcome = $5)
           AND ($6::text IS NULL OR key > $6)
         ORDER BY key LIMIT $4",
        tx.project,
        tx.environment,
        job,
        limit,
        outcome,
        after
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(rows.into_iter().map(item_of).collect())
}

/// One item of the job, by client key.
pub async fn item(
    tx: &mut ProjectTransaction,
    job: Uuid,
    key: &str,
) -> Result<Option<Item>, DbError> {
    let row = sqlx::query_as!(
        ItemRow,
        "SELECT key, entry_id, expected_revision_id, input, outcome, problem, new_revision_id,
                applied_at
         FROM bulk_items
         WHERE project_id = $1 AND environment_id = $2 AND job_id = $3 AND key = $4",
        tx.project,
        tx.environment,
        job,
        key
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(row.map(item_of))
}

/// One item, locked in the item's own transaction so its outcome writes exactly once.
pub async fn lock_item(
    tx: &mut ProjectTransaction,
    job: Uuid,
    key: &str,
) -> Result<Option<Item>, DbError> {
    let row = sqlx::query_as!(
        ItemRow,
        "SELECT key, entry_id, expected_revision_id, input, outcome, problem, new_revision_id,
                applied_at
         FROM bulk_items
         WHERE project_id = $1 AND environment_id = $2 AND job_id = $3 AND key = $4
         FOR UPDATE",
        tx.project,
        tx.environment,
        job,
        key
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(row.map(item_of))
}

/// Writes the item's terminal outcome in its transaction — the same commit that carries
/// the revision it made (or proves it made none), D-116's exactly-once-per-item.
#[allow(clippy::too_many_arguments, reason = "one item row's worth of fields")]
pub async fn mark(
    transaction: &mut Transaction<'_, Postgres>,
    project: Uuid,
    environment: Uuid,
    job: Uuid,
    key: &str,
    outcome: &str,
    problem: Option<&Value>,
    new_revision_id: Option<Uuid>,
) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE bulk_items
         SET outcome = $5, problem = $6, new_revision_id = $7,
             applied_at = CASE WHEN $5 = 'applied' THEN now() ELSE NULL END
         WHERE project_id = $1 AND environment_id = $2 AND job_id = $3 AND key = $4
           AND outcome = 'pending'",
        project,
        environment,
        job,
        key,
        outcome,
        problem,
        new_revision_id
    )
    .execute(&mut **transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// Running jobs for the worker's scan, claimed oldest first (the in-flight index keeps
/// this cheap).
pub async fn running(pool: &sqlx::PgPool, limit: i64) -> Result<Vec<(Uuid, Uuid, Uuid)>, DbError> {
    let mut transaction = pool.begin().await.map_err(DbError::from_query)?;
    sqlx::query("SET LOCAL ROLE colophon_worker")
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    let rows = sqlx::query!(
        "SELECT project_id, environment_id, id FROM bulk_jobs
         WHERE state = 'running' ORDER BY started_at LIMIT $1",
        limit
    )
    .fetch_all(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.rollback().await.map_err(DbError::from_query)?;
    Ok(rows
        .into_iter()
        .map(|row| (row.project_id, row.environment_id, row.id))
        .collect())
}

/// Claims `job` for the session on `conn`: a `pg_try_advisory_lock` on a key unique to the
/// job, held until [`release`] or the connection drops — so one worker drives it, and a
/// crash frees it. The lock is a session lock, not a transaction's: it outlives each item's
/// commit.
pub async fn claim(
    conn: &mut crate::PgConnection,
    project: Uuid,
    environment: Uuid,
    job: Uuid,
) -> Result<bool, DbError> {
    sqlx::query_scalar!(
        r#"SELECT pg_try_advisory_lock(hashtext($1), hashtext($2)) AS "claimed!""#,
        format!("{project}/{environment}"),
        format!("bulk {job}")
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(DbError::from_query)
}

/// Releases the claim [`claim`] made; a dropped connection does it anyway.
pub async fn release(
    conn: &mut crate::PgConnection,
    project: Uuid,
    environment: Uuid,
    job: Uuid,
) -> Result<(), DbError> {
    sqlx::query_scalar!(
        r#"SELECT pg_advisory_unlock(hashtext($1), hashtext($2)) AS "released!""#,
        format!("{project}/{environment}"),
        format!("bulk {job}")
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(DbError::from_query)?;
    Ok(())
}

/// The job as the worker sees it — for its principal and collection — inside a
/// `colophon_worker` transaction (which sees every project's rows).
pub async fn read(
    tx: &mut Transaction<'_, Postgres>,
    project: Uuid,
    environment: Uuid,
    job: Uuid,
) -> Result<Option<Job>, DbError> {
    let row = sqlx::query_as!(
        JobRow,
        "SELECT project_id, environment_id, id, kind, operation, manifest_hash, state, created_by, principal,
                expected_items, done_items, blocked_items, started_at, finished_at, error,
                created_at, updated_at
         FROM bulk_jobs WHERE project_id = $1 AND environment_id = $2 AND id = $3",
        project,
        environment,
        job
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(DbError::from_query)?;
    Ok(row.map(job_of))
}

/// The drive's read of a job's head, as `colophon_worker` on its own connection.
pub async fn read_job(
    conn: &mut PgConnection,
    project: Uuid,
    environment: Uuid,
    job: Uuid,
) -> Result<Option<Job>, DbError> {
    let mut tx = conn.begin().await.map_err(DbError::from_query)?;
    sqlx::query("SET LOCAL ROLE colophon_worker")
        .execute(&mut *tx)
        .await
        .map_err(DbError::from_query)?;
    let job = read(&mut tx, project, environment, job).await?;
    tx.commit().await.map_err(DbError::from_query)?;
    Ok(job)
}

/// The job's still-pending items, at most `limit`, in manifest (key) order — the batch the
/// worker drives next, each in its own transaction.
pub async fn due_items(
    pool: &sqlx::PgPool,
    project: Uuid,
    environment: Uuid,
    job: Uuid,
    limit: i64,
) -> Result<Vec<Item>, DbError> {
    let mut transaction = pool.begin().await.map_err(DbError::from_query)?;
    sqlx::query("SET LOCAL ROLE colophon_worker")
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    let rows = sqlx::query_as!(
        ItemRow,
        "SELECT key, entry_id, expected_revision_id, input, outcome, problem, new_revision_id,
                applied_at
         FROM bulk_items
         WHERE project_id = $1 AND environment_id = $2 AND job_id = $3 AND outcome = 'pending'
         ORDER BY key LIMIT $4",
        project,
        environment,
        job,
        limit
    )
    .fetch_all(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.rollback().await.map_err(DbError::from_query)?;
    Ok(rows.into_iter().map(item_of).collect())
}

/// Recomputes a job's counters and terminal state from its items (like migration's
/// checkpoint): `done` when nothing is pending and nothing blocked the run; `blocked` when
/// at least one item finished `conflict` or `refused`. A cancelled job stays cancelled —
/// its `skipped` items were the cancellation's own write. Returns whether the job is
/// terminal now, and closes its budget reservations when it is (D-116).
pub async fn checkpoint(
    pool: &sqlx::PgPool,
    project: Uuid,
    environment: Uuid,
    job: Uuid,
) -> Result<bool, DbError> {
    let mut transaction = pool.begin().await.map_err(DbError::from_query)?;
    sqlx::query("SET LOCAL ROLE colophon_worker")
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    let closed = sqlx::query_scalar!(
        "WITH counts AS (
             SELECT count(*) FILTER (WHERE outcome = 'pending') AS pending,
                    count(*) FILTER (WHERE outcome IN ('applied', 'skipped')) AS done,
                    count(*) FILTER (WHERE outcome IN ('conflict', 'refused')) AS blocked
             FROM bulk_items
             WHERE project_id = $1 AND environment_id = $2 AND job_id = $3
         )
         UPDATE bulk_jobs j
         SET done_items = counts.done, blocked_items = counts.blocked,
             state = CASE WHEN j.state <> 'running' THEN j.state
                          WHEN counts.pending > 0 THEN 'running'
                          WHEN counts.blocked > 0 THEN 'blocked' ELSE 'done' END,
             finished_at = CASE WHEN counts.pending = 0 AND j.state = 'running'
                                THEN now() ELSE j.finished_at END,
             error = CASE WHEN counts.pending = 0 AND counts.blocked > 0 AND j.state = 'running'
                          THEN 'the run finished with items to look at' ELSE j.error END,
             updated_at = now()
         FROM counts
         WHERE j.project_id = $1 AND j.environment_id = $2 AND j.id = $3
         RETURNING (j.state IN ('done', 'blocked', 'cancelled'))",
        project,
        environment,
        job
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(DbError::from_write)?;
    let terminal = closed.unwrap_or(false);
    if terminal {
        // The reservations the outcome's transaction didn't already settle or release:
        // the jobs slot settles when the job ran, the unspent documents release.
        let ran = sqlx::query_scalar!(
            "SELECT started_at IS NOT NULL FROM bulk_jobs
             WHERE project_id = $1 AND environment_id = $2 AND id = $3",
            project,
            environment,
            job
        )
        .fetch_one(&mut *transaction)
        .await
        .map_err(DbError::from_query)?
        .unwrap_or(false);
        crate::budgets::close_bulk(&mut transaction, project, environment, job, ran).await?;
    }
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(terminal)
}

/// The entry fields a bulk item's write needs, locked `FOR UPDATE` in the item transaction
/// (the collection check and the base-revision compare run on this row, not a read taken
/// before the lock).
#[derive(Debug, Clone)]
pub struct LockedEntry {
    /// Its collection.
    pub collection: String,
    /// Its creator (the `own` half of a field grant).
    pub created_by: Uuid,
    /// The current head of its draft.
    pub draft_revision_id: Uuid,
    /// Whether it is a tombstone.
    pub gone: bool,
}

/// Locks the entry row `FOR UPDATE`; `None` when the entry does not exist at all.
pub async fn lock_entry(
    tx: &mut ProjectTransaction,
    entry: Uuid,
) -> Result<Option<LockedEntry>, DbError> {
    let row = sqlx::query!(
        "SELECT collection, created_by, draft_revision_id, deleted_at IS NOT NULL AS \"gone!\"
         FROM entries
         WHERE project_id = $1 AND environment_id = $2 AND id = $3 FOR UPDATE",
        tx.project,
        tx.environment,
        entry
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(row.map(|row| LockedEntry {
        collection: row.collection,
        created_by: row.created_by,
        draft_revision_id: row.draft_revision_id,
        gone: row.gone,
    }))
}

/// Marks a running job `blocked` with `error = 'corrupt_job'`, as `colophon_worker`: the
/// write the worker makes when the job's own record is unreadable — losing it leaves the
/// job running for the next drive to hit the same wall. Returns whether a job moved.
pub async fn mark_corrupt(
    pool: &sqlx::PgPool,
    project: Uuid,
    environment: Uuid,
    job: Uuid,
) -> Result<bool, DbError> {
    let mut transaction = pool.begin().await.map_err(DbError::from_query)?;
    sqlx::query("SET LOCAL ROLE colophon_worker")
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    let marked = sqlx::query!(
        "UPDATE bulk_jobs SET state = 'blocked', finished_at = now(),
                error = 'corrupt_job', updated_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND state = 'running'",
        project,
        environment,
        job
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_write)?
    .rows_affected();
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(marked > 0)
}
