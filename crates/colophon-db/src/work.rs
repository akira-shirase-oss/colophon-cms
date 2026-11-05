//! The bound on work a project retains (docs/modules/05-worker.md "Admission"; spec §33;
//! D-131, F09): at most [`colophon_domain::job::MAX_PENDING`] unfinished jobs at once, counted
//! across the four tables workers draw from — `jobs`, `asset_jobs`, `bulk_jobs` and
//! `migration_plans`, the same "open work" a restore's inventory counts (O3). Every
//! transaction that inserts such a row calls [`admit`] first; over the bound it answers
//! [`DbError::WorkFull`], the transaction aborts and the API refuses with 503 `unavailable`
//! / `work_full` — new expensive work is refused before it is accepted, never half-committed.
//! Open work still drains: the bound refuses admissions, not the work already kept, so a
//! project at the bound waits out its workers rather than stalling.
//!
//! [`admit`] takes the project's admission lock, a transaction-scoped advisory lock, then
//! counts: no two admissions can both see room that isn't there. The lock is any admitting
//! transaction's first — before the content-type locks publishing and plan creation take,
//! before the token-limits row a budget reservation takes, before the job-table writes of the
//! commit itself — so an admission never waits on a lock another admission holds, and no cycle
//! can form. Worker transactions (claims, finishes, the sweeps) never take it: they only finish
//! admitted work, so a wait can never circle back through them.

use crate::{DbError, scope::ProjectTransaction};

/// The most unfinished jobs a project may retain (spec §33): the domain's `MAX_PENDING`.
pub const MAX_OPEN: i64 = colophon_domain::job::MAX_PENDING;

/// The first key of the advisory locks [`lock`] takes — a class of its own, like the
/// content-type (`0x4354`), component (`0x434D`) and role (`0x524C`) classes: "WA", work
/// admission.
const LOCK_CLASS: i32 = 0x5741;

/// Takes the project's admission lock until the transaction commits or rolls back.
/// [`admit`] takes it itself; `publish` also takes it directly, at the top, so the lock
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

/// Admits `adding` new job rows for the project, or refuses with [`DbError::WorkFull`] when
/// `open` plus `adding` would pass [`MAX_OPEN`]. The count runs under [`lock`], held until
/// the transaction commits — a second admission can never slip between count and insert, and
/// when the caller rolls back nothing was admitted at all.
///
/// Call order is the caller's discipline, spelled out at each site: wherever a transaction
/// could take a lock another admission waits on (a content-type change lock, a token-limits
/// row), the admission lock must come first.
pub async fn admit(tx: &mut ProjectTransaction, adding: i64) -> Result<(), DbError> {
    lock(tx).await?;
    let open = open(tx).await?;
    if open.saturating_add(adding) > MAX_OPEN {
        return Err(DbError::WorkFull {
            open,
            limit: MAX_OPEN,
        });
    }
    Ok(())
}
