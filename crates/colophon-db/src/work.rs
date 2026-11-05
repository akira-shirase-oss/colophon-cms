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
//! `colophon_pending_payload` sums every open job's idempotency key, every open asset job's
//! key plus its asset's recorded object size, open bulk jobs' operation documents and their
//! pending items' inputs, and open plans' definitions and stored resolutions — all as the
//! application role can't, as `colophon_worker`, the only role that sees every project's
//! rows. Large bodies live in object storage, not the database (spec §33): `size_bytes` is
//! the recorded, trusted count of those bytes, so an upload's true cost is what admission
//! weighs. A payload-admitting transaction checks once at admission and again after its
//! write — [`check_payload`] — so growth is refused with its own uncommitted bytes counted.
//!
//! Lock order keeps admission safe: [`admission`] takes the project's advisory lock (class
//! "WA"), then the installation's byte lock (class "WB", key 0 — taken inside
//! `colophon_pending_payload` for any count, and directly here for a lock-only caller).
//! Both precede every other lock an admission takes, so no cycle can form. Worker
//! transactions (claims, finishes, the sweeps) never hold one while taking another lock:
//! they only finish admitted work, so a wait can never circle back through them. The one
//! worker use of the byte lock is `colophon_pending_payload`'s own count, which holds it
//! alone for the single statement.

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

/// The byte-admission lock's class: "WB" — the second key of the pair `pg_advisory_xact_lock`
/// takes, key 0 because the bound is the installation's, not one project's.
const PAYLOAD_LOCK_CLASS: i32 = 0x5742;
const PAYLOAD_LOCK_KEY: i32 = 0;

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

/// The admission pair, in order — the project lock, then the installation's byte lock —
/// before any lock another admission may wait on. [`admit`] calls it; a transaction that
/// will grow payload without adding a job row (a bulk append under [`crate::bulk::lock`],
/// a plan's resolution under [`crate::migrations::lock`]) holds the same pair by the same
/// order, so the two locks and the row locks beneath them can never queue in a circle.
pub async fn admission(tx: &mut ProjectTransaction) -> Result<(), DbError> {
    lock(tx).await?;
    sqlx::query_scalar!(
        r#"SELECT 1 AS "held!" FROM pg_advisory_xact_lock($1, $2)"#,
        PAYLOAD_LOCK_CLASS,
        PAYLOAD_LOCK_KEY
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(())
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

/// The installation's pending payload bytes, as migration 0050's `colophon_pending_payload`
/// counts them — under the byte lock the function takes itself, so a count is never read
/// unlocked, and including this transaction's own uncommitted writes, so a post-write check
/// sees what this transaction is about to commit.
pub async fn pending_payload(tx: &mut ProjectTransaction) -> Result<i64, DbError> {
    sqlx::query_scalar!(r#"SELECT colophon_pending_payload() AS "pending!""#)
        .fetch_one(&mut *tx.transaction)
        .await
        .map_err(DbError::from_query)
}

/// [`pending_payload`] without a project: the install-wide count, for a worker sweep that
/// budgets what it enqueues against what room the bound leaves (the media sweep's
/// `room` in `assets::enqueue_expired`).
pub async fn install_payload(pool: &sqlx::PgPool) -> Result<i64, DbError> {
    sqlx::query_scalar!(r#"SELECT colophon_pending_payload() AS "pending!""#)
        .fetch_one(pool)
        .await
        .map_err(DbError::from_query)
}

/// Refuses with [`DbError::PayloadFull`] when the installation's pending payload already
/// passes [`MAX_PENDING_PAYLOAD`] — counting this transaction's own uncommitted writes, so
/// a write that tips it over is refused before commit. [`admit`] calls it before a caller's
/// write, and every site that stores payload calls it again after, inside the transaction
/// that then rolls back: refused work is never half-committed.
pub async fn check_payload(tx: &mut ProjectTransaction) -> Result<(), DbError> {
    let pending = pending_payload(tx).await?;
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
/// installation's pending payload is already over [`MAX_PENDING_PAYLOAD`]. The counts run
/// under [`admission`], held until the transaction commits — a second admission can never
/// slip between count and insert, and when the caller rolls back nothing was admitted at
/// all. A byte-count check after the caller's write (the `check_payload` calls at the
/// insert sites) is what keeps growth honest: a single bulk chunk or a fresh asset's
/// recorded size can be far larger than the rows it rides in.
///
/// Call order is the caller's discipline, spelled out at each site: wherever a transaction
/// could take a lock another admission waits on (a content-type change lock, a token-limits
/// row), the admission locks must come first.
pub async fn admit(tx: &mut ProjectTransaction, adding: i64) -> Result<(), DbError> {
    admission(tx).await?;
    let open = open(tx).await?;
    if open.saturating_add(adding) > MAX_OPEN {
        return Err(DbError::WorkFull {
            open,
            limit: MAX_OPEN,
        });
    }
    check_payload(tx).await
}
