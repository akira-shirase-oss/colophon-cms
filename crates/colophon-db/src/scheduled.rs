//! Publishes set for the future (`scheduled_publishes`, migration 0020): the row pins the
//! changeset hash, the approval, the base generation and who asked for it. The API inserts
//! one when a publisher schedules; the server's claim loop takes due rows and runs the
//! normal publish — a schedule commits or it records why it could not, it never guesses.

use sqlx::{PgPool, types::time::OffsetDateTime};
use uuid::Uuid;

use crate::{DbError, scope::ProjectTransaction};

/// A scheduled publish: what it will publish, when, and how it ended (or hasn't).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheduled {
    /// Its id.
    pub id: Uuid,
    /// The changeset it will publish.
    pub changeset_id: Uuid,
    /// The hash it was scheduled under; if the head moved, the publish refuses.
    pub changeset_hash: Vec<u8>,
    /// The approval it publishes under.
    pub review_id: Uuid,
    /// The generation the publisher stood on; `releases::commit` refuses if it moved.
    pub base_generation: i64,
    /// The member who scheduled it; the commit runs under their identity and role.
    pub requested_by: Uuid,
    /// Not before.
    pub run_at: OffsetDateTime,
    /// `pending`, `claimed`, `cancelled`, `committed` or `failed`.
    pub status: String,
    /// How it ended: the release it committed, or the error it recorded.
    pub outcome: Option<ScheduledOutcome>,
    /// When it was scheduled.
    pub created_at: OffsetDateTime,
}

/// How a schedule ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledOutcome {
    /// It committed as this generation.
    Committed(i64),
    /// It was cancelled before it ran.
    Cancelled,
    /// The publish refused or failed; the message says why.
    Failed(String),
}

/// The answer to a cancel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cancelled {
    /// It is now cancelled; it will not run.
    Cancelled,
    /// It already left `pending` — claimed, committed, failed or cancelled earlier.
    TooLate,
    /// No such schedule in this environment.
    NotFound,
}

fn map(row: ScheduledRow) -> Scheduled {
    let outcome = match row.status.as_str() {
        "committed" => row.generation.map(ScheduledOutcome::Committed),
        "cancelled" => Some(ScheduledOutcome::Cancelled),
        "failed" => row.error.map(ScheduledOutcome::Failed),
        _ => None,
    };
    Scheduled {
        id: row.id,
        changeset_id: row.changeset_id,
        changeset_hash: row.changeset_hash,
        review_id: row.review_id,
        base_generation: row.base_generation,
        requested_by: row.requested_by,
        run_at: row.run_at,
        status: row.status,
        outcome,
        created_at: row.created_at,
    }
}

struct ScheduledRow {
    id: Uuid,
    changeset_id: Uuid,
    changeset_hash: Vec<u8>,
    review_id: Uuid,
    base_generation: i64,
    requested_by: Uuid,
    run_at: OffsetDateTime,
    status: String,
    generation: Option<i64>,
    error: Option<String>,
    created_at: OffsetDateTime,
}

/// Schedules the publish of a changeset: its hash, the approval it publishes under, the base
/// generation the publisher stood on, and the requester the commit will run as.
pub async fn schedule(
    tx: &mut ProjectTransaction,
    id: Uuid,
    changeset: Uuid,
    hash: &[u8],
    review: Uuid,
    base: i64,
    run_at: OffsetDateTime,
) -> Result<Scheduled, DbError> {
    let row = sqlx::query_as!(
        ScheduledRow,
        "INSERT INTO scheduled_publishes
             (project_id, environment_id, id, changeset_id, changeset_hash, review_id,
              base_generation, requested_by, run_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING id, changeset_id, changeset_hash, review_id, base_generation,
                   requested_by, run_at, status, generation, error, created_at",
        tx.project,
        tx.environment,
        id,
        changeset,
        hash,
        review,
        base,
        tx.identity,
        run_at
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(map(row))
}

/// The schedules on one changeset, pending first then newest finished — the changeset page
/// shows whether a publish is set and how the last one ended.
pub async fn for_changeset(
    tx: &mut ProjectTransaction,
    changeset: Uuid,
) -> Result<Vec<Scheduled>, DbError> {
    let rows = sqlx::query_as!(
        ScheduledRow,
        "SELECT id, changeset_id, changeset_hash, review_id, base_generation,
                requested_by, run_at, status, generation, error, created_at
         FROM scheduled_publishes
         WHERE project_id = $1 AND environment_id = $2 AND changeset_id = $3
         ORDER BY (status = 'pending' OR status = 'claimed') DESC, created_at DESC
         LIMIT 10",
        tx.project,
        tx.environment,
        changeset
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(rows.into_iter().map(map).collect())
}

/// Cancels a schedule while it is still pending: `TooLate` if it already ran, failed,
/// committed or was cancelled; `NotFound` if it never was.
pub async fn cancel(tx: &mut ProjectTransaction, id: Uuid) -> Result<Cancelled, DbError> {
    let current = sqlx::query_scalar!(
        "SELECT status FROM scheduled_publishes
         WHERE project_id = $1 AND environment_id = $2 AND id = $3",
        tx.project,
        tx.environment,
        id
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    match current.map(|status: String| status.as_str() == "pending") {
        None => Ok(Cancelled::NotFound),
        Some(false) => Ok(Cancelled::TooLate),
        Some(true) => {
            sqlx::query!(
                "UPDATE scheduled_publishes SET status = 'cancelled', finished_at = now()
                 WHERE project_id = $1 AND environment_id = $2 AND id = $3",
                tx.project,
                tx.environment,
                id
            )
            .execute(&mut *tx.transaction)
            .await
            .map_err(DbError::from_write)?;
            Ok(Cancelled::Cancelled)
        }
    }
}

/// A due row claimed by the server's loop: the pins the publish replays under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due {
    /// The schedule's id.
    pub id: Uuid,
    /// Its project and environment.
    pub project: Uuid,
    /// The environment it commits in.
    pub environment: Uuid,
    /// The changeset it will publish.
    pub changeset_id: Uuid,
    /// The hash it was scheduled under.
    pub changeset_hash: Vec<u8>,
    /// The approval it publishes under.
    pub review_id: Uuid,
    /// The generation the publisher stood on.
    pub base_generation: i64,
    /// The member the commit runs as.
    pub requested_by: Uuid,
}

/// Opens a transaction with `app.claim` set: the cross-project policy the claim loop runs
/// under, and nothing a member's request can reach.
async fn claiming(pool: &PgPool) -> Result<sqlx::Transaction<'static, sqlx::Postgres>, DbError> {
    let mut tx = pool.begin().await.map_err(DbError::from_query)?;
    sqlx::query!("SELECT set_config('app.claim', 'on', true) AS claiming")
        .fetch_one(&mut *tx)
        .await
        .map_err(DbError::from_query)?;
    Ok(tx)
}

/// Claims up to `limit` due rows (`FOR UPDATE SKIP LOCKED`): concurrent servers split them,
/// and a claim after the deadline is impossible — a schedule never runs twice.
pub async fn claim_due(pool: &PgPool, limit: i64) -> Result<Vec<Due>, DbError> {
    let mut tx = claiming(pool).await?;
    let rows = sqlx::query_as!(
        Due,
        "UPDATE scheduled_publishes s SET status = 'claimed', claimed_at = now()
         WHERE (s.project_id, s.environment_id, s.id) IN (
             SELECT t.project_id, t.environment_id, t.id
             FROM scheduled_publishes t
             WHERE t.status = 'pending' AND t.run_at <= now()
             ORDER BY t.run_at
             FOR UPDATE SKIP LOCKED
             LIMIT $1
         )
         RETURNING s.id, s.project_id AS project, s.environment_id AS environment,
                   s.changeset_id, s.changeset_hash, s.review_id, s.base_generation,
                   s.requested_by",
        limit.clamp(1, 50)
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(DbError::from_query)?;
    tx.commit().await.map_err(DbError::from_query)?;
    Ok(rows)
}

/// Marks a claimed schedule committed, with the generation the release landed as.
pub async fn finish(pool: &PgPool, id: Uuid, generation: i64) -> Result<(), DbError> {
    let mut tx = claiming(pool).await?;
    sqlx::query!(
        "UPDATE scheduled_publishes SET status = 'committed', generation = $2, finished_at = now()
         WHERE id = $1 AND status = 'claimed'",
        id,
        generation
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_write)?;
    tx.commit().await.map_err(DbError::from_query)
}

/// Marks a claimed schedule failed, with the refusal's message as the record.
pub async fn fail(pool: &PgPool, id: Uuid, error: &str) -> Result<(), DbError> {
    let mut tx = claiming(pool).await?;
    sqlx::query!(
        "UPDATE scheduled_publishes SET status = 'failed', error = $2, finished_at = now()
         WHERE id = $1 AND status = 'claimed'",
        id,
        error.chars().take(500).collect::<String>()
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_write)?;
    tx.commit().await.map_err(DbError::from_query)
}

/// Fails rows whose claim outlived the server that took it: a crash mid-run would otherwise
/// leave a schedule `claimed` forever. It never re-runs — the release commit is atomic, so a
/// stale claim means it did not publish; the error points at the releases to prove it.
pub async fn expire(pool: &PgPool, claimed_for: std::time::Duration) -> Result<u64, DbError> {
    let mut tx = claiming(pool).await?;
    let done = sqlx::query!(
        "UPDATE scheduled_publishes
         SET status = 'failed', finished_at = now(),
             error = 'the server stopped while it ran; the releases show whether it published'
         WHERE status = 'claimed' AND claimed_at < now() - make_interval(secs => $1)",
        f64::from(u32::try_from(claimed_for.as_secs()).unwrap_or(u32::MAX))
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_write)?
    .rows_affected();
    tx.commit().await.map_err(DbError::from_query)?;
    Ok(done)
}
