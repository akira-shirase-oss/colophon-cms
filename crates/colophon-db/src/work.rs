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
//! Lock order keeps admission safe. [`admission`] takes the project's advisory lock (class
//! "WA") before any other lock an admission takes, and every transaction that locks the
//! counter row holds its project's WA first. The counter row is the one lock shared across
//! projects, and an admission takes it last, at its post-write check: a project's admissions
//! are already in line behind WA, and across projects nothing else is shared, so no wait can
//! circle. Worker
//! transactions (claims, finishes, the sweeps, supersedes) never take the counter row: their
//! triggers only insert deltas, which nobody waits on. The worker's fold
//! ([`fold_payload`]) skips its turn rather than wait for an admission.

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

/// Takes the project's admission lock until the transaction commits or rolls back.
/// [`admission`] takes it itself; `publish` also takes it directly, at the top, so the lock
/// precedes the changeset's row lock and the content-type locks the commit reaches later —
/// the one ordering every admission needs, and enough: a transaction waiting here still
/// holds nothing another admission can wait on.
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

/// The project's admission lock, before any lock another admission may wait on. [`admit`]
/// calls it; a transaction that will grow payload without adding a job row (a bulk append
/// under [`crate::bulk::lock`], a plan's resolution under [`crate::migrations::lock`]) takes
/// it by the same order. The installation-wide byte lock it once took after this one is gone
/// (D-216): the counter row [`check_payload`] locks at the end replaces it.
pub async fn admission(tx: &mut ProjectTransaction) -> Result<(), DbError> {
    lock(tx).await
}

/// The unfinished work `project` retains, as [`admit`] counts it: delivery `jobs` pending,
/// retrying, running or held; `asset_jobs` pending, running or retrying; `bulk_jobs`
/// accepting or running; `migration_plans` proposed, approved or running. Job rows are the
/// bound's unit — a bulk manifest's items and a plan's entries ride inside their one row,
/// each bounded by its own manifest limit. `assets::enqueue_expired` counts the same set
/// per row: keep the two in step.
async fn open(tx: &mut ProjectTransaction) -> Result<i64, DbError> {
    sqlx::query_scalar!(
        r#"SELECT count(*) AS "open!" FROM (
               SELECT 1 FROM jobs
               WHERE project_id = $1 AND status IN ('pending', 'retrying', 'running', 'held')
               UNION ALL
               SELECT 1 FROM asset_jobs
               WHERE project_id = $1 AND status IN ('pending', 'running', 'retrying')
               UNION ALL
               SELECT 1 FROM bulk_jobs
               WHERE project_id = $1 AND state IN ('accepting', 'running')
               UNION ALL
               SELECT 1 FROM migration_plans
               WHERE project_id = $1 AND state IN ('proposed', 'approved', 'running')
           ) retained"#,
        tx.project
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
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
pub async fn fold_payload(
    pool: &sqlx::PgPool,
    stop: crate::Stop<'_>,
) -> Result<Option<i64>, DbError> {
    // A background unit: bounded server side, and never begun once told to stop.
    let mut transaction = crate::begin_background(pool, stop).await?;
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

/// Refuses with [`DbError::PayloadFull`] when the installation's pending payload passes
/// [`MAX_PENDING_PAYLOAD`], counted with this transaction's own uncommitted writes, so a write
/// that tips it over is refused before commit. Every site that stores payload calls it after
/// its write, last: it locks the counter row until commit (D-216), so the next admission
/// counts this one's bytes. The transaction then rolls back on a refusal: refused work is
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

/// Admits `adding` new job rows for the project, or refuses with [`DbError::WorkFull`] when
/// `open` plus `adding` would pass [`MAX_OPEN`], or [`DbError::PayloadFull`] when the
/// installation's pending payload is already over [`MAX_PENDING_PAYLOAD`]. The row count runs
/// under [`admission`], held until the transaction commits — a second admission of the
/// project can never slip between count and insert. The byte count here is the unlocked
/// total, an early refusal only: the check that decides is [`check_payload`] after the
/// caller's write, where a single bulk chunk or a fresh asset's recorded size, far larger than
/// the rows it rides in, is counted.
///
/// Call order is the caller's discipline, spelled out at each site: wherever a transaction
/// could take a lock another admission waits on (a content-type change lock, a token-limits
/// row), the admission locks must come first.
pub async fn admit(tx: &mut ProjectTransaction, adding: i64) -> Result<(), DbError> {
    // The early byte refusal reads the unlocked total, so it runs before the project's lock: a
    // round trip fewer under WA (on a remote database every one held under a lock costs its RTT
    // to every admission queued behind it).
    let pending = pending_payload(tx).await?;
    if pending > MAX_PENDING_PAYLOAD {
        return Err(DbError::PayloadFull {
            pending,
            limit: MAX_PENDING_PAYLOAD,
        });
    }
    admission(tx).await?;
    let open = open(tx).await?;
    if open.saturating_add(adding) > MAX_OPEN {
        return Err(DbError::WorkFull {
            open,
            limit: MAX_OPEN,
        });
    }
    Ok(())
}
