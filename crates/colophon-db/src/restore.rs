//! The restore procedure's half inside the database (docs/modules/18-operations.md "Restore
//! procedure"; spec §33; F13, F14; invariants O2 and O3; D-126; migration 0049).
//!
//! `scripts/restore.sh` orders these calls: the schema check refuses a backup this binary is
//! too old for, [`advance_epoch`] registers the recovery epoch before anything serves (O2),
//! [`hold_jobs`] freezes the restored outbox so no stale announcement or purge can dispatch
//! before external effects are reconciled (O3), [`outbox`] is what the operator reconciles
//! against, and [`resume_jobs`] is the explicit enable of spec step 6.
//!
//! Every function runs on the pool it is given — the migrator's connection to the *restored*
//! database, never the application's: row-level security does not force on the table owner,
//! and these statements name no project context.

use sqlx::{PgPool, types::time::OffsetDateTime};
use uuid::Uuid;

use crate::DbError;

/// One applied `_sqlx_migrations` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The migration's version.
    pub version: i64,
    /// Its recorded description.
    pub description: String,
}

/// The migrations the restored database applied, version-ordered.
pub async fn applied_migrations(pool: &PgPool) -> Result<Vec<Applied>, DbError> {
    sqlx::query_as!(
        Applied,
        "SELECT version, description FROM _sqlx_migrations WHERE success ORDER BY version"
    )
    .fetch_all(pool)
    .await
    .map_err(DbError::from_query)
}

/// What [`check_schema`] found comparing a restored schema with this binary's migrations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaCheck {
    /// The same set.
    Current,
    /// The binary knows migrations the restored database hasn't applied yet — an older backup
    /// on newer code. Listed so the operator applies them deliberately (`admin migrate`), the
    /// upgrade runbook's call; restore step 2 doesn't silently change a schema.
    Pending(Vec<String>),
    /// The restored database applied migrations this binary doesn't know — the backup is newer
    /// than the code, and serving it here would be guessing: restore refused (F10's territory).
    Unknown(Vec<String>),
}

/// Compares `applied` (from [`applied_migrations`]) with the binary's embedded [`crate::MIGRATOR`]:
/// `Unknown` for a backup newer than the code, `Pending` for code newer than the backup.
pub fn check_schema(applied: &[Applied]) -> SchemaCheck {
    let known: std::collections::BTreeMap<i64, String> = crate::MIGRATOR
        .iter()
        .map(|migration| (migration.version, migration.description.to_string()))
        .collect();
    let unknown: Vec<String> = applied
        .iter()
        .filter(|row| !known.contains_key(&row.version))
        .map(|row| format!("{}-{}", row.version, row.description))
        .collect();
    if !unknown.is_empty() {
        return SchemaCheck::Unknown(unknown);
    }
    let have: std::collections::BTreeSet<i64> = applied.iter().map(|row| row.version).collect();
    let pending: Vec<String> = known
        .iter()
        .filter(|(version, _)| !have.contains(version))
        .map(|(version, description)| format!("{version}-{description}"))
        .collect();
    if pending.is_empty() {
        SchemaCheck::Current
    } else {
        SchemaCheck::Pending(pending)
    }
}

/// One environment's new epoch, as [`advance_epoch`] returned it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpochAdvanced {
    /// The environment's project.
    pub project_id: Uuid,
    /// The environment.
    pub environment_id: Uuid,
    /// Its slug (`production`, …), for the restore log.
    pub slug: String,
    /// The epoch it now holds.
    pub epoch: i64,
}

/// Registers the recovery epoch on every environment (spec step 3): one more than whatever the
/// backup held, so no cached generation, delivery `Scope` key or worker lease from the old
/// timeline can ever collide with the new one (D4, DL4). Runs before serving (O2); the
/// procedure calls it exactly once — its stage marker is the gate, and the result is written
/// into the restore manifest as the record.
pub async fn advance_epoch(pool: &PgPool) -> Result<Vec<EpochAdvanced>, DbError> {
    sqlx::query_as!(
        EpochAdvanced,
        "UPDATE environments SET epoch = epoch + 1
         RETURNING project_id, id AS environment_id, slug, epoch"
    )
    .fetch_all(pool)
    .await
    .map_err(DbError::from_query)
}

/// One outbox job the hold froze, or that waits frozen for the resume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldJob {
    /// The job's project.
    pub project_id: Uuid,
    /// Its environment.
    pub environment_id: Uuid,
    /// The job.
    pub id: Uuid,
    /// Its destination.
    pub destination_id: Uuid,
    /// The destination's name, for the reconciliation listing.
    pub destination_name: String,
    /// The release it announces.
    pub generation: i64,
    /// Where it stood when the hold took it — `running` means its attempt may already have
    /// landed on the old timeline: the job F13 refuses to resend blindly. `held` when listing
    /// already-held jobs (the database keeps no record of the pre-hold state; the restore
    /// manifest's hold report is that record).
    pub was: String,
    /// Attempts so far, kept: they really happened.
    pub attempts: i32,
    /// When its lifetime ends.
    pub deadline_at: OffsetDateTime,
}

/// Freezes every unfinished delivery job — `pending`, `retrying` or `running` at backup time —
/// as `held`, so nothing the old timeline dispatched can fire before reconciliation (spec step
/// 5, O3). The claim query never reads `held`, the lease fields clear (a dead worker's lease is
/// nobody's now), and each row is reported with the state it was taken from: `was = 'running'`
/// marks the in-flight attempts whose external effect is *unknown* — never assume the resend
/// is safe. Final states (`succeeded`, `failed`, `superseded`) stay as the backup recorded them.
pub async fn hold_jobs(pool: &PgPool) -> Result<Vec<HeldJob>, DbError> {
    let mut transaction = pool.begin().await.map_err(DbError::from_query)?;
    let taken = sqlx::query_as!(
        HeldJob,
        "SELECT j.project_id, j.environment_id, j.id, j.destination_id, d.name AS destination_name,
                j.generation, j.status AS was, j.attempts, j.deadline_at
         FROM jobs j
         JOIN destinations d ON d.project_id = j.project_id AND d.environment_id = j.environment_id
                            AND d.id = j.destination_id
         WHERE j.status IN ('pending', 'retrying', 'running')
         ORDER BY j.created_at
         FOR UPDATE OF j"
    )
    .fetch_all(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    sqlx::query!(
        "UPDATE jobs SET status = 'held', lease_owner = NULL, lease_expires_at = NULL, updated_at = now()
         WHERE status IN ('pending', 'retrying', 'running')"
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(taken)
}

/// The jobs still frozen: the reconciliation list the operator works through before resume.
pub async fn held_jobs(pool: &PgPool) -> Result<Vec<HeldJob>, DbError> {
    sqlx::query_as!(
        HeldJob,
        "SELECT j.project_id, j.environment_id, j.id, j.destination_id, d.name AS destination_name,
                j.generation, j.status AS was, j.attempts, j.deadline_at
         FROM jobs j
         JOIN destinations d ON d.project_id = j.project_id AND d.environment_id = j.environment_id
                            AND d.id = j.destination_id
         WHERE j.status = 'held'
         ORDER BY j.created_at"
    )
    .fetch_all(pool)
    .await
    .map_err(DbError::from_query)
}

/// Everything that could still act on the restored database: what the operator reconciles
/// (spec step 5). Counts only — reconciliation needs shapes, not content. The `Option`s are
/// `None` when the table doesn't exist on this backup (its schema predates the table), so an
/// older backup reports honestly instead of failing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outbox {
    /// Delivery jobs by status, including `held`.
    pub jobs: Vec<(String, i64)>,
    /// The jobs frozen by [`hold_jobs`], oldest first.
    pub held: Vec<HeldJob>,
    /// `scheduled` schedules waiting (`run_at` any time), if the table exists.
    pub schedules_waiting: Option<i64>,
    /// Of those, already due — a worker would fire them.
    pub schedules_due: Option<i64>,
    /// Unfinished `asset_jobs`, if the table exists.
    pub asset_jobs_open: Option<i64>,
    /// `bulk_jobs` still `accepting` or `running`, if the table exists.
    pub bulk_jobs_open: Option<i64>,
    /// `migration_plans` still `proposed`, `approved` or `running`, if the table exists.
    pub migration_runs_open: Option<i64>,
}

/// The restore's work inventory (spec step 5): the held outbox plus every other surface that
/// could act once a server points at this database — due schedules, in-flight asset, bulk and
/// content-migration work — so the operator's reconciliation sees all of it, not just jobs.
pub async fn outbox(pool: &PgPool) -> Result<Outbox, DbError> {
    let jobs = sqlx::query!(
        "SELECT status AS \"status!\", count(*) AS \"n!\" FROM jobs GROUP BY status ORDER BY status"
    )
    .fetch_all(pool)
    .await
    .map_err(DbError::from_query)?
    .into_iter()
    .map(|row| (row.status, row.n))
    .collect();
    let held = held_jobs(pool).await?;
    // to_regclass in a query of its own: a CASE branch still names the table at parse time.
    let schedules = exists(pool, "schedules").await?;
    let (schedules_waiting, schedules_due) = if schedules {
        let row = sqlx::query!(
            "SELECT count(*) AS \"waiting!\", count(*) FILTER (WHERE run_at <= now()) AS \"due!\"
             FROM schedules WHERE state = 'scheduled'"
        )
        .fetch_one(pool)
        .await
        .map_err(DbError::from_query)?;
        (Some(row.waiting), Some(row.due))
    } else {
        (None, None)
    };
    let asset_jobs_open = if exists(pool, "asset_jobs").await? {
        Some(
            sqlx::query_scalar!(
                "SELECT count(*) AS \"n!\" FROM asset_jobs WHERE status IN ('pending', 'running', 'retrying')"
            )
            .fetch_one(pool)
            .await
            .map_err(DbError::from_query)?,
        )
    } else {
        None
    };
    let bulk_jobs_open = if exists(pool, "bulk_jobs").await? {
        Some(
            sqlx::query_scalar!(
                "SELECT count(*) AS \"n!\" FROM bulk_jobs WHERE state IN ('accepting', 'running')"
            )
            .fetch_one(pool)
            .await
            .map_err(DbError::from_query)?,
        )
    } else {
        None
    };
    let migration_runs_open = if exists(pool, "migration_plans").await? {
        Some(
            sqlx::query_scalar!(
                "SELECT count(*) AS \"n!\" FROM migration_plans WHERE state IN ('proposed', 'approved', 'running')"
            )
            .fetch_one(pool)
            .await
            .map_err(DbError::from_query)?,
        )
    } else {
        None
    };
    Ok(Outbox {
        jobs,
        held,
        schedules_waiting,
        schedules_due,
        asset_jobs_open,
        bulk_jobs_open,
        migration_runs_open,
    })
}

/// Whether `table` exists on this (possibly old-schema) database.
async fn exists(pool: &PgPool, table: &str) -> Result<bool, DbError> {
    sqlx::query_scalar!("SELECT to_regclass($1) IS NOT NULL AS \"there!\"", table)
        .fetch_one(pool)
        .await
        .map_err(DbError::from_query)
}

/// Returns every held job to `pending` with `next_attempt_at = now()` — the explicit enable of
/// spec step 6, run only after reconciliation (O3). The deadline floor moves too: a backup is
/// usually older than a job's one-hour lifetime, and without `greatest(deadline_at, now() +
/// lifetime)` a resumed job would be unclaimable and die on the next expiry sweep — held for
/// nothing. Jobs still inside their lifetime keep it; the clock only ever moves forward for
/// ones whose lifetime already ran out. Ids of the resumed jobs, for the restore manifest.
pub async fn resume_jobs(pool: &PgPool, lifetime_secs: i32) -> Result<Vec<Uuid>, DbError> {
    sqlx::query_scalar!(
        "UPDATE jobs SET status = 'pending', next_attempt_at = now(),
                        deadline_at = greatest(deadline_at, now() + $1::int * interval '1 second'),
                        updated_at = now()
         WHERE status = 'held'
         RETURNING id AS \"id!\"",
        lifetime_secs
    )
    .fetch_all(pool)
    .await
    .map_err(DbError::from_query)
}

#[cfg(test)]
mod tests {
    use super::{Applied, SchemaCheck, check_schema};

    fn applied(versions: &[i64]) -> Vec<Applied> {
        versions
            .iter()
            .map(|version| Applied {
                version: *version,
                description: format!("migration {version}"),
            })
            .collect()
    }

    #[test]
    fn the_schema_check_reads_the_binarys_known_migrations() {
        let known: Vec<i64> = crate::MIGRATOR
            .iter()
            .map(|migration| migration.version)
            .collect();
        assert_ne!(known, [] as [i64; 0]);
        // The embedded set exactly: current.
        let exact: Vec<Applied> = crate::MIGRATOR
            .iter()
            .map(|migration| Applied {
                version: migration.version,
                description: migration.description.to_string(),
            })
            .collect();
        assert_eq!(check_schema(&exact), SchemaCheck::Current);
        // A prefix of it: pending, named.
        let cut = &exact[..exact.len() - 1];
        match check_schema(cut) {
            SchemaCheck::Pending(pending) => assert_eq!(pending.len(), 1),
            other => panic!("a prefix must be pending, not {other:?}"),
        }
        // Anything beyond it: refused as unknown.
        let mut foreign = exact.clone();
        foreign.push(Applied {
            version: i64::MAX,
            description: "from the future".to_owned(),
        });
        match check_schema(&foreign) {
            SchemaCheck::Unknown(unknown) => {
                assert_eq!(unknown, ["9223372036854775807-from the future"]);
            }
            other => panic!("a migration the binary lacks must refuse, not {other:?}"),
        }
        // An empty applied list (a database that never migrated) is pending, not current.
        assert!(matches!(
            check_schema(&applied(&[])),
            SchemaCheck::Pending(_)
        ));
    }
}
