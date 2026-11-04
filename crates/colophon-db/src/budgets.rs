//! Budget reservations (docs/modules/09-automation.md, "Budgets"; spec §17, §33; D-113,
//! invariant AU3): the ledger that makes a principal's work bounded at the database boundary.
//!
//! `api_token_limits` names the ceilings. The accepting transaction locks that row
//! (`SELECT … FOR UPDATE`), counts what the principal already holds or spent, and either takes
//! a `held` reservation or refuses — so two concurrent requests of one token can never both
//! see room that isn't there. A token without a limits row is unconstrained and leaves no
//! rows at all; a person is never the `api_token_limits` row's subject, so member writes pass
//! the same call untouched.
//!
//! `budget_reservations` holds the allowance: `held` while work is in flight, `settled` with
//! what the outcome consumed in its own commit, `released` when the work can never happen, and
//! `uncertain` when the reconciler finds a held row that outlived anything that could still be
//! running — kept, counted conservatively, and announced in the audit log as
//! `budget.discrepancy`. The worker's [`reconcile`] frees reservations of jobs that ended
//! (their own commit normally already did) and flags the stuck ones.

use colophon_domain::budget::{self, Close, Kind, Limits};
use sqlx::{
    PgPool, Postgres, Transaction, postgres::types::PgInterval, types::time::OffsetDateTime,
};
use uuid::Uuid;

use crate::{
    DbError,
    scope::{MemberTransaction, ProjectTransaction},
};

/// The identity `budget.discrepancy` audit events name: the system actor, inserted by
/// migration 0045 (`urn:colophon:system` / `worker`). No person can be it.
pub const WORKER_IDENTITY: Uuid = Uuid::from_u128(0x029cfcbb_de9b_48c9_94fb_8bf6b5a3502a);

/// A held reservation on a job outlives the job's own lifetime plus the sweeps' slack: the
/// expire and release paths have had every chance, so still-held means they missed it.
const STALE_JOB_SECS: i64 = colophon_domain::job::LIFETIME_SECS + 600;
/// A held reservation on no job — a request-scoped write — outlives any request. One can only
/// exist because a transaction committed it `held`, which no writer does: an anomaly.
const STALE_LONE_SECS: i64 = 900;
/// The most rows one sweep frees or flags; anything more waits for the next pass.
const MAX_SWEEP: i64 = 200;

/// What a principal already holds or spent: the counts [`reserve`] and the token view's
/// `usage` share.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    /// Documents written in the last day (settled, held and uncertain reservations).
    pub documents_last_day: i64,
    /// Jobs reservations still open (held or uncertain).
    pub jobs_open: i64,
    /// Requests in the last minute. The request quota counts them in the limiter, not this
    /// ledger (D-118), so this stays 0 unless a reservation records one; the token view
    /// overlays the limiter's own window.
    pub requests_last_minute: i64,
}

/// The counts, in one read. Under the limits lock inside [`reserve`] this is the number the
/// check refuses on.
async fn used(
    transaction: &mut Transaction<'_, Postgres>,
    project: Uuid,
    principal: Uuid,
) -> Result<Usage, DbError> {
    let row = sqlx::query!(
        r#"SELECT coalesce(sum(reserved) FILTER (WHERE kind = 'documents' AND state <> 'released'
                        AND created_at > now() - interval '1 day'), 0)::bigint AS "documents!",
                  coalesce(sum(reserved) FILTER (WHERE kind = 'jobs'
                        AND state IN ('held', 'uncertain')), 0)::bigint AS "jobs!",
                  coalesce(sum(reserved) FILTER (WHERE kind = 'requests' AND state <> 'released'
                        AND created_at > now() - interval '1 minute'), 0)::bigint AS "requests!"
           FROM budget_reservations
           WHERE project_id = $1 AND principal_id = $2"#,
        project,
        principal
    )
    .fetch_one(&mut **transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(Usage {
        documents_last_day: row.documents,
        jobs_open: row.jobs,
        requests_last_minute: row.requests,
    })
}

/// The token's usage for `GET …/tokens/{token}`.
pub async fn usage(tx: &mut MemberTransaction, principal: Uuid) -> Result<Usage, DbError> {
    used(&mut tx.transaction, tx.project, principal).await
}

/// The calling token's limits row, locked `FOR UPDATE` so the count-and-insert that follows
/// can't race a sibling request of the same token. `None` — for a person, or a token with no
/// limits — is the unconstrained case: nothing is counted, nothing is written.
async fn lock_limits(tx: &mut ProjectTransaction) -> Result<Option<Limits>, DbError> {
    let row = sqlx::query!(
        "SELECT l.documents_per_day, l.max_concurrent_jobs, l.requests_per_minute
         FROM api_tokens t
         JOIN api_token_limits l ON l.project_id = t.project_id AND l.token_id = t.id
         WHERE t.project_id = $1 AND t.identity_id = $2
         FOR UPDATE OF l",
        tx.project,
        tx.identity
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(row.map(|row| Limits {
        documents_per_day: row.documents_per_day,
        max_concurrent_jobs: row.max_concurrent_jobs,
        requests_per_minute: row.requests_per_minute,
    }))
}

/// Takes `amount` of `kind` for the transaction's caller — the reservation that must exist
/// before the work runs (AU3). `Ok(Some)`: the `held` row's id, for [`settle`] or [`release`];
/// `Ok(None)`: the caller is unconstrained and nothing was written. `Err(BudgetExceeded)`: the
/// limit refuses it — the transaction is aborted, so nothing it wrote stands.
pub async fn reserve(
    tx: &mut ProjectTransaction,
    kind: Kind,
    amount: i64,
) -> Result<Option<Uuid>, DbError> {
    let Some(limits) = lock_limits(tx).await? else {
        return Ok(None);
    };
    let used = used(&mut tx.transaction, tx.project, tx.identity).await?;
    let have = match kind {
        Kind::Documents => used.documents_last_day,
        Kind::Jobs => used.jobs_open,
        Kind::Requests => used.requests_last_minute,
        Kind::Tokens | Kind::Cost => 0,
    };
    let limit = limits.ceiling(kind);
    if !budget::fits(have, amount, limit) {
        return Err(DbError::BudgetExceeded {
            kind,
            used: have,
            limit: limit.unwrap_or_default(),
        });
    }
    let id = sqlx::query_scalar!(
        "INSERT INTO budget_reservations (project_id, environment_id, id, principal_id, kind, reserved, state)
         VALUES ($1, $2, uuidv7(), $3, $4, $5, 'held') RETURNING id",
        tx.project,
        tx.environment,
        tx.identity,
        kind.as_str(),
        amount
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(Some(id))
}

/// One `jobs` reservation per job `job_ids` names, each `held` — the release's commit reserves
/// its dispatch (AU3). Same lock-count-refuse shape as [`reserve`]; the whole insert is refused
/// or committed together.
pub async fn reserve_jobs(tx: &mut ProjectTransaction, job_ids: &[Uuid]) -> Result<(), DbError> {
    if job_ids.is_empty() {
        return Ok(());
    }
    let Some(limits) = lock_limits(tx).await? else {
        return Ok(());
    };
    let used = used(&mut tx.transaction, tx.project, tx.identity).await?;
    let amount = i64::try_from(job_ids.len()).unwrap_or(i64::MAX);
    if !budget::fits(used.jobs_open, amount, limits.max_concurrent_jobs) {
        return Err(DbError::BudgetExceeded {
            kind: Kind::Jobs,
            used: used.jobs_open,
            limit: limits.max_concurrent_jobs.unwrap_or_default(),
        });
    }
    sqlx::query!(
        "INSERT INTO budget_reservations (project_id, environment_id, id, principal_id, kind, reserved, state, job_id)
         SELECT $1, $2, uuidv7(), $3, 'jobs', 1, 'held', job
         FROM unnest($4::uuid[]) AS job",
        tx.project,
        tx.environment,
        tx.identity,
        job_ids
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// Reserves the `jobs` slot a bulk job holds (D-116): one `held` row linked by
/// `bulk_job_id`, so the ledger tells it from delivery dispatch. Called once, in the
/// transaction that accepts the job — like [`reserve_jobs`] for a release's dispatch.
pub async fn reserve_bulk_job(tx: &mut ProjectTransaction, job: Uuid) -> Result<(), DbError> {
    let Some(limits) = lock_limits(tx).await? else {
        return Ok(());
    };
    let used = used(&mut tx.transaction, tx.project, tx.identity).await?;
    if !budget::fits(used.jobs_open, 1, limits.max_concurrent_jobs) {
        return Err(DbError::BudgetExceeded {
            kind: Kind::Jobs,
            used: used.jobs_open,
            limit: limits.max_concurrent_jobs.unwrap_or_default(),
        });
    }
    sqlx::query!(
        "INSERT INTO budget_reservations
             (project_id, environment_id, id, principal_id, kind, reserved, state, bulk_job_id)
         VALUES ($1, $2, uuidv7(), $3, 'jobs', 1, 'held', $4)",
        tx.project,
        tx.environment,
        tx.identity,
        job
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// Reserves one `documents` unit per item a bulk chunk adds (D-116): `count` `held` rows of
/// `reserved` 1 each, linked to the job, so every item's own commit can settle exactly the
/// unit it consumed and a never-run item can release its own. Called in each accepting
/// transaction, after [`reserve_bulk_job`] for the job's first chunk.
pub async fn reserve_bulk_documents(
    tx: &mut ProjectTransaction,
    job: Uuid,
    count: i64,
) -> Result<(), DbError> {
    if count <= 0 {
        return Ok(());
    }
    let Some(limits) = lock_limits(tx).await? else {
        return Ok(());
    };
    let used = used(&mut tx.transaction, tx.project, tx.identity).await?;
    if !budget::fits(used.documents_last_day, count, limits.documents_per_day) {
        return Err(DbError::BudgetExceeded {
            kind: Kind::Documents,
            used: used.documents_last_day,
            limit: limits.documents_per_day.unwrap_or_default(),
        });
    }
    sqlx::query!(
        "INSERT INTO budget_reservations
             (project_id, environment_id, id, principal_id, kind, reserved, state, bulk_job_id)
         SELECT $1, $2, uuidv7(), $3, 'documents', 1, 'held', $4
         FROM generate_series(1, $5::bigint)",
        tx.project,
        tx.environment,
        tx.identity,
        job,
        count
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// Marks one of `job`'s held `documents` units consumed, in the item transaction that spent
/// it (D-116): the applied write and its settlement land or roll back together. A job whose
/// caller has no limits row holds nothing, so this is a no-op for it.
pub async fn settle_bulk_document(
    transaction: &mut Transaction<'_, Postgres>,
    project: Uuid,
    job: Uuid,
) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE budget_reservations SET state = 'settled', settled = 1, settled_at = now()
         WHERE id = (SELECT id FROM budget_reservations
                     WHERE project_id = $1 AND bulk_job_id = $2
                       AND kind = 'documents' AND state = 'held'
                     ORDER BY created_at LIMIT 1)",
        project,
        job
    )
    .execute(&mut **transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// Releases one of `job`'s held `documents` units, in the item transaction that proved the
/// item can never consume it (D-116): a refused, conflicted or skipped item frees what it
/// held, so the allowance is honest rather than conservative.
pub async fn release_bulk_document(
    transaction: &mut Transaction<'_, Postgres>,
    project: Uuid,
    job: Uuid,
) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE budget_reservations SET state = 'released', settled_at = now()
         WHERE id = (SELECT id FROM budget_reservations
                     WHERE project_id = $1 AND bulk_job_id = $2
                       AND kind = 'documents' AND state = 'held'
                     ORDER BY created_at LIMIT 1)",
        project,
        job
    )
    .execute(&mut **transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// Closes every reservation `job` still holds when it reaches a terminal state (D-116):
/// the `jobs` slot settles when the job ran (it consumed a slot of the caller's
/// concurrency) and releases when it never started; every `documents` unit an item didn't
/// already settle is released — what was never written costs nothing. In the transaction
/// that records the terminal state, like [`close_job`] for a delivery job.
pub async fn close_bulk(
    transaction: &mut Transaction<'_, Postgres>,
    project: Uuid,
    environment: Uuid,
    job: Uuid,
    ran: bool,
) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE budget_reservations
         SET state = CASE WHEN kind = 'jobs' AND $4 THEN 'settled' ELSE 'released' END,
             settled = CASE WHEN kind = 'jobs' AND $4 THEN reserved ELSE NULL END,
             settled_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND bulk_job_id = $3
           AND state IN ('held', 'uncertain')",
        project,
        environment,
        job,
        ran
    )
    .execute(&mut **transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// The outcome's commit tells the reservation what the work consumed (D-113). For a
/// reservation the same transaction holds, like an entry write's: the write and its settlement
/// land or roll back together.
pub async fn settle(
    tx: &mut ProjectTransaction,
    reservation: Option<Uuid>,
    consumed: i64,
) -> Result<(), DbError> {
    let Some(reservation) = reservation else {
        return Ok(());
    };
    let updated = sqlx::query!(
        "UPDATE budget_reservations SET state = 'settled', settled = $3, settled_at = now()
         WHERE project_id = $1 AND id = $2 AND state = 'held'",
        tx.project,
        reservation,
        consumed
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?
    .rows_affected();
    if updated != 1 {
        return Err(DbError::Corrupt(
            "a reservation held by this transaction isn't held",
        ));
    }
    Ok(())
}

/// The work can't happen: frees `reservation`'s allowance (cancellation, a refused commit that
/// still wants its ledger whole).
pub async fn release(tx: &mut ProjectTransaction, reservation: Uuid) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE budget_reservations SET state = 'released', settled_at = now()
         WHERE project_id = $1 AND id = $2 AND state = 'held'",
        tx.project,
        reservation
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// Frees the reservations `jobs` hold: the release that supersedes their jobs says they can
/// never run — `uncertain` ones too, since the job's answer is certain now.
pub async fn release_jobs(
    transaction: &mut Transaction<'_, Postgres>,
    project: Uuid,
    jobs: &[Uuid],
) -> Result<(), DbError> {
    if jobs.is_empty() {
        return Ok(());
    }
    sqlx::query!(
        "UPDATE budget_reservations SET state = 'released', settled_at = now()
         WHERE project_id = $1 AND job_id = ANY($2::uuid[]) AND state IN ('held', 'uncertain')",
        project,
        jobs
    )
    .execute(&mut **transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// Closes `job`'s reservation the way its outcome says (D-113): `settled` with the slot it
/// held when the job ran its last attempt, `released` when a newer release superseded it.
/// In the `jobs::finish` transaction, so the outcome and its ledger entry land together.
pub async fn close_job(
    transaction: &mut Transaction<'_, Postgres>,
    project: Uuid,
    environment: Uuid,
    job: Uuid,
    close: Close,
) -> Result<(), DbError> {
    match close {
        Close::Settle => {
            sqlx::query!(
                "UPDATE budget_reservations SET state = 'settled', settled = reserved, settled_at = now()
                 WHERE project_id = $1 AND environment_id = $2 AND job_id = $3 AND state = 'held'",
                project,
                environment,
                job
            )
            .execute(&mut **transaction)
            .await
            .map_err(DbError::from_write)?;
        }
        Close::Release => {
            sqlx::query!(
                "UPDATE budget_reservations SET state = 'released', settled_at = now()
                 WHERE project_id = $1 AND environment_id = $2 AND job_id = $3
                   AND state IN ('held', 'uncertain')",
                project,
                environment,
                job
            )
            .execute(&mut **transaction)
            .await
            .map_err(DbError::from_write)?;
        }
    }
    Ok(())
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

/// What one [`reconcile`] pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reconciled {
    /// Reservations closed because their job's outcome is on record (`settled` if it ran,
    /// `released` if it was superseded).
    pub released: u64,
    /// Reservations flagged `uncertain`: held past anything that could still be running.
    pub uncertain: u64,
}

/// The sweep beside `jobs::expire` (bounded at `MAX_SWEEP` rows a pass): reservations of
/// jobs whose outcome is recorded are closed the way it says — `settled` when the job ran,
/// `released` when it was superseded, `uncertain` rows too, the job's answer being certain —
/// and held rows that outlived their work are flagged `uncertain`, each announced
/// in the project's audit log as `budget.discrepancy` by the system identity (migration 0045).
/// Nothing silently drops an ambiguous reservation: it keeps counting until it is resolved.
pub async fn reconcile(pool: &PgPool) -> Result<Reconciled, DbError> {
    let mut tx = worker(pool).await?;
    let released = sqlx::query!(
        "WITH closing AS (
             SELECT r.project_id, r.id, j.status
             FROM budget_reservations r
             JOIN jobs j ON j.project_id = r.project_id AND j.environment_id = r.environment_id
                        AND j.id = r.job_id
             WHERE r.state IN ('held', 'uncertain')
               AND j.status IN ('succeeded', 'failed', 'superseded')
             LIMIT $1
         )
         UPDATE budget_reservations r
         SET state = CASE WHEN closing.status = 'superseded' THEN 'released' ELSE 'settled' END,
             settled = CASE WHEN closing.status = 'superseded' THEN NULL ELSE r.reserved END,
             settled_at = now()
         FROM closing WHERE r.project_id = closing.project_id AND r.id = closing.id",
        MAX_SWEEP
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_query)?
    .rows_affected();
    let stale = sqlx::query!(
        "WITH stale AS (
             SELECT project_id, id FROM budget_reservations
             WHERE state = 'held'
               AND ((job_id IS NULL AND bulk_job_id IS NULL AND created_at < now() - $2::interval)
                    OR ((job_id IS NOT NULL OR bulk_job_id IS NOT NULL)
                        AND created_at < now() - $1::interval))
             ORDER BY created_at
             LIMIT $3 FOR UPDATE SKIP LOCKED
         )
         UPDATE budget_reservations r SET state = 'uncertain'
         FROM stale WHERE r.project_id = stale.project_id AND r.id = stale.id
         RETURNING r.project_id, r.id",
        PgInterval {
            months: 0,
            days: 0,
            microseconds: STALE_JOB_SECS * 1_000_000,
        },
        PgInterval {
            months: 0,
            days: 0,
            microseconds: STALE_LONE_SECS * 1_000_000,
        },
        MAX_SWEEP
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(DbError::from_query)?;
    if !stale.is_empty() {
        let projects: Vec<Uuid> = stale.iter().map(|row| row.project_id).collect();
        let ids: Vec<Uuid> = stale.iter().map(|row| row.id).collect();
        sqlx::query!(
            "INSERT INTO audit_events (project_id, id, actor_id, action, subject, detail)
             SELECT r.project_id, uuidv7(), $3, 'budget.discrepancy', r.id::text,
                    jsonb_build_object('kind', r.kind, 'reserved', r.reserved,
                                       'principal_id', r.principal_id, 'job_id', r.job_id,
                                       'held_at', r.created_at)
             FROM budget_reservations r
             JOIN unnest($1::uuid[], $2::uuid[]) AS s (project_id, id)
               ON s.project_id = r.project_id AND s.id = r.id",
            &projects,
            &ids,
            WORKER_IDENTITY
        )
        .execute(&mut *tx)
        .await
        .map_err(DbError::from_write)?;
    }
    tx.commit().await.map_err(DbError::from_query)?;
    Ok(Reconciled {
        released,
        uncertain: u64::try_from(stale.len()).unwrap_or(u64::MAX),
    })
}

/// The bulk half of [`reconcile`]'s closing pass (D-116): reservations a terminal
/// `bulk_jobs` row still holds — crashed before its close could commit — are closed the way
/// its outcome says: the `jobs` slot `settled` when the job ran, `released` when it never
/// started; `documents` units an item never spent are `released`. The item-level settle and
/// release already cover the common path; this is the crash backstop, like the `jobs` join
/// above.
pub async fn reconcile_bulk(pool: &PgPool) -> Result<u64, DbError> {
    let mut tx = worker(pool).await?;
    let closed = sqlx::query!(
        "WITH closing AS (
             SELECT r.project_id, r.id, r.kind, (b.started_at IS NOT NULL) AS ran
             FROM budget_reservations r
             JOIN bulk_jobs b ON b.project_id = r.project_id AND b.environment_id = r.environment_id
                            AND b.id = r.bulk_job_id
             WHERE r.state IN ('held', 'uncertain')
               AND b.state IN ('done', 'blocked', 'cancelled')
             LIMIT $1
         )
         UPDATE budget_reservations r
         SET state = CASE WHEN closing.kind = 'jobs' AND closing.ran THEN 'settled' ELSE 'released' END,
             settled = CASE WHEN closing.kind = 'jobs' AND closing.ran THEN r.reserved ELSE NULL END,
             settled_at = now()
         FROM closing WHERE r.project_id = closing.project_id AND r.id = closing.id",
        MAX_SWEEP
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_query)?
    .rows_affected();
    tx.commit().await.map_err(DbError::from_query)?;
    Ok(closed)
}

/// Replaces a token's limits row (at least one bound, checked by [`Limits::check`] first).
pub async fn set_limits(
    tx: &mut MemberTransaction,
    token: Uuid,
    limits: &Limits,
) -> Result<(), DbError> {
    sqlx::query!(
        "INSERT INTO api_token_limits (project_id, token_id, documents_per_day, max_concurrent_jobs,
                                       requests_per_minute, updated_by, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, now())
         ON CONFLICT (project_id, token_id) DO UPDATE
         SET documents_per_day = $3, max_concurrent_jobs = $4, requests_per_minute = $5,
             updated_by = $6, updated_at = now()",
        tx.project,
        token,
        limits.documents_per_day,
        limits.max_concurrent_jobs,
        limits.requests_per_minute,
        tx.identity
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// Removes a token's limits: the token is unconstrained at the quota layer again.
pub async fn remove_limits(tx: &mut MemberTransaction, token: Uuid) -> Result<(), DbError> {
    sqlx::query!(
        "DELETE FROM api_token_limits WHERE project_id = $1 AND token_id = $2",
        tx.project,
        token
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// A reservation row as the project's members see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredReservation {
    /// Its id.
    pub id: Uuid,
    /// Whose work it's for.
    pub principal_id: Uuid,
    /// What it bounds.
    pub kind: String,
    /// The allowance taken.
    pub reserved: i64,
    /// What the outcome consumed, if it landed.
    pub settled: Option<i64>,
    /// `held`, `settled`, `released` or `uncertain`.
    pub state: String,
    /// The job it's for, if any.
    pub job_id: Option<Uuid>,
    /// When it was taken.
    pub created_at: OffsetDateTime,
    /// When it closed, if it has.
    pub settled_at: Option<OffsetDateTime>,
}

/// A principal's reservations, newest first (at most 200) — for the tests that read the ledger
/// and whoever audits a discrepancy.
pub async fn of_principal(
    tx: &mut MemberTransaction,
    principal: Uuid,
) -> Result<Vec<StoredReservation>, DbError> {
    sqlx::query_as!(
        StoredReservation,
        "SELECT id, principal_id, kind, reserved, settled, state, job_id, created_at, settled_at
         FROM budget_reservations
         WHERE project_id = $1 AND principal_id = $2
         ORDER BY created_at DESC, id DESC
         LIMIT 200",
        tx.project,
        principal
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}
