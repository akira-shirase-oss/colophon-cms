//! The bound on work an installation retains (docs/modules/05-worker.md "Admission"; spec §33;
//! D-131, D-135, F09): at most [`colophon_domain::job::MAX_PENDING`] unfinished jobs per
//! project, counted across the four tables workers draw from — `jobs`, `asset_jobs`,
//! `bulk_jobs` and `migration_plans`, the same "open work" a restore's inventory counts (O3) —
//! and at most [`colophon_domain::job::MAX_PENDING_PAYLOAD`] bytes of pending payload metadata
//! across the whole installation. Every transaction that inserts such a row calls [`admit`]
//! first; over either bound it answers [`DbError::WorkFull`] or [`DbError::PayloadFull`], the
//! transaction aborts and the API refuses with 503 `unavailable` — new expensive work is
//! refused before it is accepted, never half-committed. Open work still drains: the bounds
//! refuse admissions, not the work already kept.
//!
//! Payload metadata is the stored, variable-size bytes of pending work (D-135):
//! the total weighs every open job's idempotency key, every open asset job's
//! key plus its asset's recorded object size, open bulk jobs' operation documents and their
//! pending items' inputs, and open plans' definitions and stored resolutions — all as the
//! application role can't, as `colophon_worker`, the only role that sees every project's
//! rows. Large bodies live in object storage, not the database (spec §33): `size_bytes` is
//! the recorded, trusted count of those bytes, so an upload's true cost is what admission
//! weighs.
//!
//! The installation's total is a running count (D-216, migration 0078), not a SUM per
//! admission: triggers on every table it weighs append each statement's change to
//! `payload_deltas`, and `payload_counter`'s one row holds what has been folded. A
//! payload-admitting transaction refuses early on the unlocked total ([`admit`]), writes, and
//! then checks at its END ([`check_payload`]): `colophon_payload_admit` locks the counter row
//! until commit, folds every delta it can see (its own uncommitted ones included) and returns
//! the total. The next admission waits on that row and then sees this one's committed fold, so
//! two admissions can never both see room that isn't there. Paths that grow the total without
//! admitting (a worker recording an asset's size) aren't checked against the bound, exactly as
//! they weren't under the old byte lock: their bytes count once they commit.
//!
//! A project's open work is a running count the same way (D-216 phase 2, migration 0084):
//! triggers on the four tables append each statement's change in open rows to `open_deltas`, and
//! `work_open` holds one row per project, made with the project. [`admit`] refuses early on the
//! unlocked count, the caller writes, and [`check`] decides at the END: `colophon_work_admit`
//! locks the project's row until commit, folds the project's visible deltas (its own included)
//! and returns the count. Exactness is the payload counter's argument, per project: the next
//! admission of the project waits on that row and then sees this one's committed fold. A
//! project's admissions no longer queue behind each other for their whole transaction, only
//! for the check and the commit.
//!
//! Lock order keeps admission safe: the counters are taken last, the project's row and then the
//! installation's counter, both in [`check`]'s one statement (a payload-only write, a bulk
//! append or a plan's resolution, takes the installation's counter alone, through
//! [`check_payload`]). After them a transaction takes at most its token's limits row
//! (`budgets::reserve_jobs`, the bulk reservations), whose holders never go on to take a counter,
//! so no wait can circle. Worker transactions (claims, finishes, the sweeps, supersedes) never
//! take a counter: their triggers only insert deltas, which nobody waits on. The worker's folds
//! ([`fold_payload`], [`fold_open`]) skip what an admission holds rather than wait for it.
//!
//! The project's advisory lock ([`lock`], class "WA") is no longer part of admission. A publish
//! and an undo still take it at the top, which keeps a project's publishes one at a time as
//! before; a bulk append and a plan's resolution take it through their own locks.

use crate::{DbError, scope::ProjectTransaction};

/// The most unfinished jobs a project may retain (spec §33): the domain's `MAX_PENDING`.
pub const MAX_OPEN: i64 = colophon_domain::job::MAX_PENDING;

/// The most pending payload metadata an installation may retain (spec §33): the domain's
/// `MAX_PENDING_PAYLOAD`.
pub const MAX_PENDING_PAYLOAD: i64 = colophon_domain::job::MAX_PENDING_PAYLOAD;

/// The first key of the advisory locks [`lock`] takes — a class of its own, like the
/// content-type (`0x4354`), component (`0x434D`) and role (`0x524C`) classes: "WA", work
/// admission.
const LOCK_CLASS: i32 = 0x5741;

/// Takes the project's advisory lock ("WA") until the transaction commits or rolls back.
/// `publish` takes it at the top, through [`admission`], so a project's publishes run one at a
/// time; admission itself no longer takes it (D-216 phase 2: [`check`]'s counter row decides).
pub async fn lock(tx: &mut ProjectTransaction) -> Result<(), DbError> {
    sqlx::query_scalar!(
        r#"SELECT 1 AS "held!" FROM pg_advisory_xact_lock($1, hashtext($2::text))"#,
        LOCK_CLASS,
        tx.project.to_string()
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(())
}

/// The project's advisory lock, as the paths that keep it take it: a publish and an undo at the
/// top, a bulk append under [`crate::bulk::lock`] and a plan's resolution under
/// [`crate::migrations::lock`]. Nothing waits on a counter while holding a lock taken after
/// it, so taking WA first keeps the order counters-last.
pub async fn admission(tx: &mut ProjectTransaction) -> Result<(), DbError> {
    lock(tx).await
}

/// The installation's pending payload bytes without a lock: the folded count plus every
/// delta this transaction can see (its own uncommitted writes included). Other transactions'
/// uncommitted work isn't in it; [`check_payload`]'s locked count is the one that decides.
pub async fn pending_payload(tx: &mut ProjectTransaction) -> Result<i64, DbError> {
    sqlx::query_scalar!(r#"SELECT colophon_payload_total() AS "pending!""#)
        .fetch_one(&mut *tx.transaction)
        .await
        .map_err(DbError::from_query)
}

/// [`pending_payload`] without a project: the install-wide count, for a worker sweep that
/// budgets what it enqueues against what room the bound leaves (the media sweep's
/// `room` in `assets::enqueue_expired`).
pub async fn install_payload(pool: &sqlx::PgPool) -> Result<i64, DbError> {
    sqlx::query_scalar!(r#"SELECT colophon_payload_total() AS "pending!""#)
        .fetch_one(pool)
        .await
        .map_err(DbError::from_query)
}

/// The running total against the SUM it stands for (D-216): `(counted, actual, drift)`.
/// Drift is reported, never repaired: a non-zero drift is a bug in a trigger.
pub async fn reconcile_payload(pool: &sqlx::PgPool) -> Result<(i64, i64, i64), DbError> {
    let row = sqlx::query!(
        r#"SELECT counted AS "counted!", actual AS "actual!", drift AS "drift!"
           FROM colophon_payload_reconcile()"#
    )
    .fetch_one(pool)
    .await
    .map_err(DbError::from_query)?;
    Ok((row.counted, row.actual, row.drift))
}

/// The worker's fold (D-216): the deltas logged since the last fold go into the counter,
/// unless an admission holds it (`None`: this turn is skipped, the admission folds them).
/// `Some(n)`: `n` deltas folded.
/// It runs as `colophon_worker`, the role the fold belongs to.
pub async fn fold_payload(pool: &sqlx::PgPool) -> Result<Option<i64>, DbError> {
    let mut transaction = crate::begin_tx(pool).await.map_err(DbError::from_query)?;
    sqlx::query("SET LOCAL ROLE colophon_worker")
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    let folded = sqlx::query_scalar!(r#"SELECT colophon_payload_tick() AS "folded!""#)
        .fetch_one(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok((folded >= 0).then_some(folded))
}

/// The delta log's length and the age of its oldest delta in seconds, for the metrics
/// surface: a log that keeps growing means nothing is folding it.
pub async fn payload_log(pool: &sqlx::PgPool) -> Result<(i64, f64), DbError> {
    let row = sqlx::query!(
        r#"SELECT deltas AS "deltas!", oldest_seconds AS "oldest_seconds!"
           FROM colophon_payload_log()"#
    )
    .fetch_one(pool)
    .await
    .map_err(DbError::from_query)?;
    Ok((row.deltas, row.oldest_seconds))
}

/// The worker's fold of open counts (D-216 phase 2): every project's logged deltas go into its
/// `work_open` row, skipping a project whose row an admission holds (it folds them itself).
/// Returns how many deltas it folded. It runs as `colophon_worker`.
pub async fn fold_open(pool: &sqlx::PgPool) -> Result<i64, DbError> {
    let mut transaction = crate::begin_tx(pool).await.map_err(DbError::from_query)?;
    sqlx::query("SET LOCAL ROLE colophon_worker")
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    let folded = sqlx::query_scalar!(r#"SELECT colophon_work_tick() AS "folded!""#)
        .fetch_one(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(folded)
}

/// Every project's running open count against the rows it stands for: `(projects, drift)`,
/// how many projects drift and by how much in all. Reported, never repaired: a non-zero drift
/// is a bug in a trigger. As `colophon_worker`.
pub async fn reconcile_open(pool: &sqlx::PgPool) -> Result<(i64, i64), DbError> {
    let mut transaction = crate::begin_tx(pool).await.map_err(DbError::from_query)?;
    sqlx::query("SET LOCAL ROLE colophon_worker")
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    let row = sqlx::query!(
        r#"SELECT projects AS "projects!", drift AS "drift!" FROM colophon_work_reconcile()"#
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok((row.projects, row.drift))
}

/// The open-delta log's length and the age of its oldest delta in seconds, for the metrics
/// surface. As `colophon_worker`.
pub async fn open_log(pool: &sqlx::PgPool) -> Result<(i64, f64), DbError> {
    let mut transaction = crate::begin_tx(pool).await.map_err(DbError::from_query)?;
    sqlx::query("SET LOCAL ROLE colophon_worker")
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    let row = sqlx::query!(
        r#"SELECT deltas AS "deltas!", oldest_seconds AS "oldest_seconds!"
           FROM colophon_work_log()"#
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok((row.deltas, row.oldest_seconds))
}

/// The admission's decision, at the END of a job-admitting transaction (D-216 phase 2), after
/// every write: refuses with [`DbError::WorkFull`] when the project's open work, its own new
/// rows counted, passes [`MAX_OPEN`], or with [`DbError::PayloadFull`] when the installation's
/// pending payload passes [`MAX_PENDING_PAYLOAD`]. One statement takes the project's counter
/// row and then the installation's, both held to commit (the module's lock order); every site
/// that called [`admit`] calls this after its writes, last but for its token's limits row. The
/// transaction rolls back on a refusal: refused work is never half-committed.
pub async fn check(tx: &mut ProjectTransaction) -> Result<(), DbError> {
    let row =
        sqlx::query!(r#"SELECT open AS "open!", pending AS "pending!" FROM colophon_work_check()"#)
            .fetch_one(&mut *tx.transaction)
            .await
            .map_err(DbError::from_query)?;
    if row.open > MAX_OPEN {
        return Err(DbError::WorkFull {
            open: row.open,
            limit: MAX_OPEN,
        });
    }
    if row.pending > MAX_PENDING_PAYLOAD {
        return Err(DbError::PayloadFull {
            pending: row.pending,
            limit: MAX_PENDING_PAYLOAD,
        });
    }
    Ok(())
}

/// Refuses with [`DbError::PayloadFull`] when the installation's pending payload passes
/// [`MAX_PENDING_PAYLOAD`], counted with this transaction's own uncommitted writes, so a write
/// that tips it over is refused before commit. Every site that stores payload calls it after
/// its write, last: it locks the counter row until commit (D-216), so the next admission
/// counts this one's bytes. A site that also admitted job rows calls [`check`] instead, which
/// makes this check too. The transaction then rolls back on a refusal: refused work is
/// never half-committed.
pub async fn check_payload(tx: &mut ProjectTransaction) -> Result<(), DbError> {
    let pending = sqlx::query_scalar!(r#"SELECT colophon_payload_admit() AS "pending!""#)
        .fetch_one(&mut *tx.transaction)
        .await
        .map_err(DbError::from_query)?;
    if pending > MAX_PENDING_PAYLOAD {
        return Err(DbError::PayloadFull {
            pending,
            limit: MAX_PENDING_PAYLOAD,
        });
    }
    Ok(())
}

/// The early refusal of `adding` new job rows for the project, before the caller writes: with
/// [`DbError::WorkFull`] when the project's open work plus `adding` would pass [`MAX_OPEN`], or
/// [`DbError::PayloadFull`] when the installation's pending payload is already over
/// [`MAX_PENDING_PAYLOAD`]. Both counts are the unlocked ones (one round trip, no lock), so a
/// refusal here saves the writes; the decision is [`check`]'s, after them. Every caller calls
/// [`check`] after its writes.
pub async fn admit(tx: &mut ProjectTransaction, adding: i64) -> Result<(), DbError> {
    let row = sqlx::query!(
        r#"SELECT colophon_payload_total() AS "pending!", colophon_work_open() AS "open!""#
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    if row.pending > MAX_PENDING_PAYLOAD {
        return Err(DbError::PayloadFull {
            pending: row.pending,
            limit: MAX_PENDING_PAYLOAD,
        });
    }
    if row.open.saturating_add(adding) > MAX_OPEN {
        return Err(DbError::WorkFull {
            open: row.open,
            limit: MAX_OPEN,
        });
    }
    Ok(())
}
