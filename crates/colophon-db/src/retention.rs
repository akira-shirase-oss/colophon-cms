//! Table retention (spec §27, D-135; docs/modules/02-db.md "Retention",
//! docs/modules/05-worker.md "Retention"): the bounded deletes that keep durable history
//! bounded. Media's own sweep ([`crate::assets::enqueue_expired`] and the orphan pass) lives
//! in `assets`; this is everything else, run by the worker beside it on
//! `COLOPHON_TABLE_SWEEP_MS`.
//!
//! Ages, per table, and why (D-135):
//!
//! - `work` (30 days, spec §27's "terminal ordinary jobs"): `jobs`, `asset_jobs`,
//!   `bulk_jobs`, `migration_plans`, `schedules` once finished, and `observations` —
//!   the receipts a job's receipt assembles — past the same age. A finished job stays while
//!   a `budget_reservations` row still names it (the ledger is its evidence until the ledger
//!   itself ages out); a finished bulk job or plan stays while its items remain, which the
//!   sweep drains first — a parent never outlives the rows that explain it, and never loses
//!   them to a delete while it could still be read.
//! - `ledger` (90 days): `budget_reservations` `settled` or `released`. `held` and
//!   `uncertain` rows are never deleted — an uncertain row is the truth "nobody knows what
//!   happened"; the reconciler owns it, not retention.
//! - `audit` (90 days, spec §27): `audit_events`, through `colophon_audit_sweep` — the only
//!   delete the append-only trigger allows, run as `colophon_retention`, a role that can do
//!   nothing else (migration 0050).
//! - `idempotency` (30 days; the spec floor is 7): `idempotency_records`. Past the age a
//!   replayed key is a fresh write, which the spec's minimum makes honest.
//! - `session` (90 days past `expires_at`): `sessions`, `access_tokens` — dead state, not
//!   audit. The audit trail of revocations, where one is kept, is `audit_events`.
//! - `flow` (90 days): `invitations` never accepted that lapsed or were revoked, and
//!   `setup_codes` past expiry — used or not; the administrator a used code made is the
//!   record that outlives it.
//! - `attempt` (15 minutes): `setup_attempts`, matching the throttle's own window in
//!   `colophon_setup_attempt`. `sign_in_attempts` and `device_logins` go at `expires_at`:
//!   dead rows their writers already prune on a busy day, swept here so a quiet one
//!   doesn't keep them forever either.
//!
//! What this never touches, deliberately: `deletion_ledger` (append-only by design, D-125);
//! `revisions`, `publications`, `tombstones`, `comments`, `tasks` — content history the
//! product itself keeps, pinned by everything that names it; and every table whose rows
//! are bounded by construction (a changeset pins to live entries; a tombstone refuses
//! while anything depends on it). A bound here is a delete of rows whose business meaning
//! has expired, never of content.
//!
//! One transaction per call, every statement bounded by `batch`, in an order that leaves
//! no row briefly orphaned of what explains it: the ledger, then receipts and their jobs,
//! then manifests and their parents, then audit, then auth and flow state. A statement
//! that finds nothing still costs almost nothing; a backlog drains `batch` a call, oldest
//! first — the sweep is resumable because every delete is unconditional.
//!
//! The pass runs as `colophon_worker`, which gains DELETE on the work tables and nothing
//! more: the audit log, the session tables and the sign-in and setup flows stay readable
//! only through their owners, so their deletes execute `colophon_retention`'s definer
//! functions — the worker removes rows it can never enumerate (D-135).

use sqlx::{AssertSqlSafe, PgPool};
use time::OffsetDateTime;

use crate::{DbError, Postgres, Transaction};

/// How long each class of row is kept. The production values are [`Retention::DEFAULT`];
/// a test passes smaller ones instead of backdating a database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retention {
    /// Terminal `jobs`, `asset_jobs`, `bulk_jobs`, `migration_plans`, `schedules` and
    /// `observations`: spec §27's 30 days for ordinary jobs.
    pub work: time::Duration,
    /// `audit_events`: spec §27's 90 days.
    pub audit: time::Duration,
    /// Settled and released `budget_reservations`.
    pub ledger: time::Duration,
    /// `idempotency_records` (spec's floor is 7 days).
    pub idempotency: time::Duration,
    /// `sessions` and `access_tokens`, past `expires_at`.
    pub session: time::Duration,
    /// `invitations` and `setup_codes`, past expiry or revocation.
    pub flow: time::Duration,
    /// `setup_attempts`: the throttle's own 15-minute window.
    pub attempt: time::Duration,
}

impl Retention {
    /// The spec's ages (D-135).
    pub const DEFAULT: Self = Self {
        work: time::Duration::days(30),
        audit: time::Duration::days(90),
        ledger: time::Duration::days(90),
        idempotency: time::Duration::days(30),
        session: time::Duration::days(90),
        flow: time::Duration::days(90),
        attempt: time::Duration::minutes(15),
    };
}

impl Default for Retention {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The most rows one statement — so one sweep — touches per table. Small enough that a
/// pass over a busy install never holds a worker's connection long; a backlog drains at
/// this rate a call, oldest first.
pub const BATCH: i64 = 500;

/// What one [`sweep`] removed, per table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Swept {
    /// `budget_reservations` settled or released past the ledger age.
    pub reservations: u64,
    /// `observations` past the work age, or of jobs deleted this pass.
    pub observations: u64,
    /// `jobs` finished past the work age with nothing left naming them.
    pub jobs: u64,
    /// `asset_jobs` finished past the work age.
    pub asset_jobs: u64,
    /// `bulk_items` of bulk jobs doomed this pass.
    pub bulk_items: u64,
    /// `bulk_jobs` finished past the work age, items drained, no reservation naming them.
    pub bulk_jobs: u64,
    /// `migration_items` of plans doomed this pass.
    pub migration_items: u64,
    /// `migration_plans` finished past the work age, items drained.
    pub migration_plans: u64,
    /// `schedules` finished past the work age.
    pub schedules: u64,
    /// `audit_events` past the audit age.
    pub audit: u64,
    /// `idempotency_records` past the idempotency age.
    pub idempotency: u64,
    /// `sessions` past `expires_at` plus the session age.
    pub sessions: u64,
    /// `access_tokens` past `expires_at` plus the session age.
    pub access_tokens: u64,
    /// `invitations` never accepted, lapsed or revoked past the flow age.
    pub invitations: u64,
    /// `device_logins` past `expires_at`.
    pub device_logins: u64,
    /// `sign_in_attempts` past `expires_at`.
    pub sign_in_attempts: u64,
    /// `setup_attempts` outside the throttle's window.
    pub setup_attempts: u64,
    /// `setup_codes` past `expires_at` plus the flow age.
    pub setup_codes: u64,
}

impl Swept {
    /// Every row this pass removed; zero means the next call may stop early.
    pub fn total(&self) -> u64 {
        self.reservations
            + self.observations
            + self.jobs
            + self.asset_jobs
            + self.bulk_items
            + self.bulk_jobs
            + self.migration_items
            + self.migration_plans
            + self.schedules
            + self.audit
            + self.idempotency
            + self.sessions
            + self.access_tokens
            + self.invitations
            + self.device_logins
            + self.sign_in_attempts
            + self.setup_attempts
            + self.setup_codes
    }
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

/// Calls one of migration 0050's `colophon_sweep_*` functions — a bounded delete owned by
/// `colophon_retention` that a worker may execute but never see past — and answers how many
/// rows it removed.
async fn retained(
    tx: &mut Transaction<'_, Postgres>,
    function: &'static str,
    cutoff: OffsetDateTime,
    batch: i64,
) -> Result<u64, DbError> {
    // `function` is a literal at every call site, never input.
    let removed =
        sqlx::query_scalar::<Postgres, i64>(AssertSqlSafe(format!("SELECT {function}($1, $2)")))
            .bind(cutoff)
            .bind(batch)
            .fetch_one(&mut **tx)
            .await
            .map_err(DbError::from_query)?;
    Ok(u64::try_from(removed).unwrap_or_default())
}

/// Runs one bounded pass over every retained table, as `colophon_worker` — at most `batch`
/// rows a table, oldest first. `ages` says how long each class is kept; `BATCH` is the
/// production batch. Returns what it deleted.
#[allow(
    clippy::too_many_lines,
    reason = "one transaction's deletes, in an order that leaves no row briefly orphaned \
              of what explains it — splitting the sequence would hide that order"
)]
pub async fn sweep(pool: &PgPool, ages: &Retention, batch: i64) -> Result<Swept, DbError> {
    let mut tx = worker(pool).await?;
    let work_cut = OffsetDateTime::now_utc() - ages.work;
    let audit_cut = OffsetDateTime::now_utc() - ages.audit;
    let ledger_cut = OffsetDateTime::now_utc() - ages.ledger;
    let idempotency_cut = OffsetDateTime::now_utc() - ages.idempotency;
    let session_cut = OffsetDateTime::now_utc() - ages.session;
    let flow_cut = OffsetDateTime::now_utc() - ages.flow;
    let attempt_cut = OffsetDateTime::now_utc() - ages.attempt;
    // The ledger first: a settled or released reservation is the one thing that keeps a
    // finished job reading live, so the ledger's own age must free it before the job's can.
    let reservations = sqlx::query!(
        "WITH doomed AS (
             SELECT project_id, id FROM budget_reservations
             WHERE state IN ('settled', 'released') AND settled_at < $1
             ORDER BY settled_at
             LIMIT $2)
         DELETE FROM budget_reservations r USING doomed d
         WHERE r.project_id = d.project_id AND r.id = d.id",
        ledger_cut,
        batch
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_write)?
    .rows_affected();

    // Receipts past the age, and receipts of jobs that are about to go — a job never
    // leaves an observation dangling to a row that no longer exists.
    let observations = sqlx::query!(
        "WITH doomed AS (
             SELECT o.project_id, o.environment_id, o.id FROM observations o
             LEFT JOIN jobs j
               ON j.project_id = o.project_id AND j.environment_id = o.environment_id
              AND j.id = o.job_id
             WHERE o.observed_at < $1
                OR (j.finished_at < $1 AND NOT EXISTS (
                        SELECT 1 FROM budget_reservations r
                        WHERE r.project_id = j.project_id
                          AND r.environment_id = j.environment_id AND r.job_id = j.id))
             ORDER BY o.observed_at
             LIMIT $2)
         DELETE FROM observations o USING doomed d
         WHERE o.project_id = d.project_id AND o.environment_id = d.environment_id
           AND o.id = d.id",
        work_cut,
        batch
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_write)?
    .rows_affected();

    // Finished jobs old enough, with no reservation still naming them and no observation
    // left: one racing receipt keeps its job for another pass, never loses the pair.
    let jobs = sqlx::query!(
        "WITH doomed AS (
             SELECT project_id, environment_id, id FROM jobs j
             WHERE j.finished_at < $1
               AND NOT EXISTS (
                   SELECT 1 FROM budget_reservations r
                   WHERE r.project_id = j.project_id AND r.environment_id = j.environment_id
                     AND r.job_id = j.id)
               AND NOT EXISTS (
                   SELECT 1 FROM observations o
                   WHERE o.project_id = j.project_id AND o.environment_id = j.environment_id
                     AND o.job_id = j.id)
             ORDER BY j.finished_at
             LIMIT $2)
         DELETE FROM jobs j USING doomed d
         WHERE j.project_id = d.project_id AND j.environment_id = d.environment_id
           AND j.id = d.id",
        work_cut,
        batch
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_write)?
    .rows_affected();

    let asset_jobs = sqlx::query!(
        "WITH doomed AS (
             SELECT project_id, environment_id, id FROM asset_jobs
             WHERE finished_at < $1
             ORDER BY finished_at
             LIMIT $2)
         DELETE FROM asset_jobs j USING doomed d
         WHERE j.project_id = d.project_id AND j.environment_id = d.environment_id
           AND j.id = d.id",
        work_cut,
        batch
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_write)?
    .rows_affected();

    // A bulk manifest's items go before their job: the job's doomed set asks for none left,
    // so a parent only outlives its items by a pass, never by a delete.
    let bulk_items = sqlx::query!(
        "WITH doomed AS (
             SELECT i.project_id, i.environment_id, i.job_id, i.key FROM bulk_items i
             JOIN bulk_jobs b
               ON b.project_id = i.project_id AND b.environment_id = i.environment_id
              AND b.id = i.job_id
             WHERE b.finished_at < $1
               AND NOT EXISTS (
                   SELECT 1 FROM budget_reservations r
                   WHERE r.bulk_job_id = b.id)
             ORDER BY i.created_at
             LIMIT $2)
         DELETE FROM bulk_items i USING doomed d
         WHERE i.project_id = d.project_id AND i.environment_id = d.environment_id
           AND i.job_id = d.job_id AND i.key = d.key",
        work_cut,
        batch
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_write)?
    .rows_affected();

    let bulk_jobs = sqlx::query!(
        "WITH doomed AS (
             SELECT project_id, environment_id, id FROM bulk_jobs b
             WHERE b.finished_at < $1
               AND NOT EXISTS (
                   SELECT 1 FROM budget_reservations r
                   WHERE r.bulk_job_id = b.id)
               AND NOT EXISTS (
                   SELECT 1 FROM bulk_items i
                   WHERE i.project_id = b.project_id AND i.environment_id = b.environment_id
                     AND i.job_id = b.id)
             ORDER BY b.finished_at
             LIMIT $2)
         DELETE FROM bulk_jobs b USING doomed d
         WHERE b.project_id = d.project_id AND b.environment_id = d.environment_id
           AND b.id = d.id",
        work_cut,
        batch
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_write)?
    .rows_affected();

    let migration_items = sqlx::query!(
        "WITH doomed AS (
             SELECT i.project_id, i.environment_id, i.plan_id, i.entry_id
             FROM migration_items i
             JOIN migration_plans p
               ON p.project_id = i.project_id AND p.environment_id = i.environment_id
              AND p.id = i.plan_id
             WHERE p.finished_at < $1
             ORDER BY i.plan_id, i.entry_id
             LIMIT $2)
         DELETE FROM migration_items i USING doomed d
         WHERE i.project_id = d.project_id AND i.environment_id = d.environment_id
           AND i.plan_id = d.plan_id AND i.entry_id = d.entry_id",
        work_cut,
        batch
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_write)?
    .rows_affected();

    let migration_plans = sqlx::query!(
        "WITH doomed AS (
             SELECT project_id, environment_id, id FROM migration_plans p
             WHERE p.finished_at < $1
               AND NOT EXISTS (
                   SELECT 1 FROM migration_items i
                   WHERE i.project_id = p.project_id AND i.environment_id = p.environment_id
                     AND i.plan_id = p.id)
             ORDER BY p.finished_at
             LIMIT $2)
         DELETE FROM migration_plans p USING doomed d
         WHERE p.project_id = d.project_id AND p.environment_id = d.environment_id
           AND p.id = d.id",
        work_cut,
        batch
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_write)?
    .rows_affected();

    let schedules = sqlx::query!(
        "WITH doomed AS (
             SELECT project_id, environment_id, id FROM schedules
             WHERE finished_at < $1
             ORDER BY finished_at
             LIMIT $2)
         DELETE FROM schedules s USING doomed d
         WHERE s.project_id = d.project_id AND s.environment_id = d.environment_id
           AND s.id = d.id",
        work_cut,
        batch
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_write)?
    .rows_affected();

    // The audit log, through the one door migration 0050 leaves: colophon_audit_sweep runs
    // as colophon_retention, the only role the append-only trigger lets delete.
    let audit_rows = sqlx::query_scalar!(
        "SELECT colophon_audit_sweep($1, $2) AS \"swept!\"",
        audit_cut,
        batch
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(DbError::from_query)?;
    let audit = u64::try_from(audit_rows).unwrap_or_default();

    // The rest of the pass runs behind colophon_retention's definer functions: the session
    // and flow tables are private to the sign-in and invite flows, the worker has never
    // been able to read them and that stays true — it can only execute the bounded deletes
    // migration 0050 owns (D-135).
    let idempotency = retained(
        &mut tx,
        "colophon_sweep_idempotency",
        idempotency_cut,
        batch,
    )
    .await?;

    // Sessions and tokens are dead state once expired — a revocation a person asked for is
    // an audit event, not a row to keep.
    let sessions = retained(&mut tx, "colophon_sweep_sessions", session_cut, batch).await?;
    let access_tokens =
        retained(&mut tx, "colophon_sweep_access_tokens", session_cut, batch).await?;
    let invitations = retained(&mut tx, "colophon_sweep_invitations", flow_cut, batch).await?;

    // Dead device and sign-in flows: their writers prune the same rows on a busy day; the
    // sweep keeps a quiet one bounded too.
    let device_logins = retained(
        &mut tx,
        "colophon_sweep_device_logins",
        OffsetDateTime::now_utc(),
        batch,
    )
    .await?;
    let sign_in_attempts = retained(
        &mut tx,
        "colophon_sweep_sign_in_attempts",
        OffsetDateTime::now_utc(),
        batch,
    )
    .await?;
    let setup_attempts =
        retained(&mut tx, "colophon_sweep_setup_attempts", attempt_cut, batch).await?;
    let setup_codes = retained(&mut tx, "colophon_sweep_setup_codes", flow_cut, batch).await?;

    tx.commit().await.map_err(DbError::from_query)?;
    Ok(Swept {
        reservations,
        observations,
        jobs,
        asset_jobs,
        bulk_items,
        bulk_jobs,
        migration_items,
        migration_plans,
        schedules,
        audit,
        idempotency,
        sessions,
        access_tokens,
        invitations,
        device_logins,
        sign_in_attempts,
        setup_attempts,
        setup_codes,
    })
}
