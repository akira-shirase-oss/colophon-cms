//! Per-account usage and quotas (D-235; migration 0088). Statement triggers on entries, assets
//! and asset versions keep each owning account's totals and refuse a write that takes one past
//! its limit (`CQ001`, [`DbError::QuotaExceeded`]); uploads also reserve their bytes here
//! before any reaches the object store. Administrators' projects are counted, never refused.

use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    DbError,
    scope::{self, ProjectTransaction, Scope},
};

/// How long an upload's reservation counts against its account if the upload is never
/// recorded (a failed or abandoned upload): longer than any upload may take (the body's
/// overall bound, then the object store write, within the request deadline).
pub const RESERVATION_SECS: i32 = 3600;

/// The limits, 0 meaning no bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Entries across an account's projects.
    pub entries: u64,
    /// Assets across an account's projects.
    pub assets: u64,
    /// Bytes stored, quarantined and being uploaded.
    pub storage_bytes: u64,
}

/// What the signed-in account holds and may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    /// Entries held.
    pub entries: u64,
    /// Assets held.
    pub assets: u64,
    /// Bytes of verified asset versions.
    pub stored_bytes: u64,
    /// Bytes of uploads still quarantined.
    pub pending_bytes: u64,
    /// The limits.
    pub limits: Limits,
    /// An administrator: counted, never refused.
    pub exempt: bool,
    /// Projects this account owns.
    pub projects: u64,
}

/// One drifted total `reconcile` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drift {
    /// The account.
    pub account: Uuid,
    /// `entries`, `assets`, `stored_bytes` or `pending_bytes`.
    pub field: String,
    /// What `account_usage` held.
    pub stored: i64,
    /// What the rows add up to.
    pub counted: i64,
}

fn as_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn as_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

/// Writes the limits the server was configured with (at boot: the last boot wins).
///
/// # Errors
///
/// The database's.
pub async fn set_limits(pool: &PgPool, limits: Limits) -> Result<(), DbError> {
    sqlx::query("SELECT colophon_usage_set_quotas($1, $2, $3)")
        .bind(as_i64(limits.entries))
        .bind(as_i64(limits.assets))
        .bind(as_i64(limits.storage_bytes))
        .execute(pool)
        .await
        .map_err(DbError::from_query)?;
    Ok(())
}

/// Reserves `bytes` of an upload against the entered project's account, before they reach the
/// object store; `reservation` is the asset's id.
///
/// # Errors
///
/// [`DbError::QuotaExceeded`] when stored, quarantined and reserved bytes plus these would
/// pass the account's storage limit; the database's otherwise.
pub async fn reserve(
    tx: &mut ProjectTransaction,
    reservation: Uuid,
    bytes: u64,
) -> Result<(), DbError> {
    sqlx::query("SELECT colophon_usage_reserve($1, $2, $3)")
        .bind(reservation)
        .bind(as_i64(bytes))
        .bind(RESERVATION_SECS)
        .execute(&mut *tx.transaction)
        .await
        .map_err(DbError::from_query)?;
    Ok(())
}

/// Removes an upload's reservation as it is recorded (its bytes then count as quarantined).
///
/// # Errors
///
/// The database's.
pub async fn release(tx: &mut ProjectTransaction, reservation: Uuid) -> Result<(), DbError> {
    sqlx::query("SELECT colophon_usage_release($1)")
        .bind(reservation)
        .execute(&mut *tx.transaction)
        .await
        .map_err(DbError::from_query)?;
    Ok(())
}

/// The signed-in account's usage and limits, in an identity-only transaction (no project).
///
/// # Errors
///
/// The database's.
pub async fn of_identity(pool: &PgPool, identity: Uuid) -> Result<Usage, DbError> {
    let mut tx = scope::begin(
        pool,
        Scope {
            identity: Some(identity),
            project: None,
        },
    )
    .await?;
    let row: (i64, i64, i64, i64, i64, i64, i64, bool) =
        sqlx::query_as("SELECT * FROM colophon_account_usage()")
            .fetch_one(&mut *tx)
            .await
            .map_err(DbError::from_query)?;
    let projects: i32 = sqlx::query_scalar("SELECT colophon_projects_owned($1)")
        .bind(identity)
        .fetch_one(&mut *tx)
        .await
        .map_err(DbError::from_query)?;
    tx.commit().await.map_err(DbError::from_query)?;
    Ok(Usage {
        entries: as_u64(row.0),
        assets: as_u64(row.1),
        stored_bytes: as_u64(row.2),
        pending_bytes: as_u64(row.3),
        limits: Limits {
            entries: as_u64(row.4),
            assets: as_u64(row.5),
            storage_bytes: as_u64(row.6),
        },
        exempt: row.7,
        projects: u64::try_from(projects).unwrap_or(0),
    })
}

/// Recounts every account's usage from the rows; with `repair`, writes the recount. Needs the
/// migration role (the owner), never the API's.
///
/// # Errors
///
/// The database's.
pub async fn reconcile(owner: &PgPool, repair: bool) -> Result<Vec<Drift>, DbError> {
    let mut tx = crate::begin_tx(owner).await.map_err(DbError::from_query)?;
    let rows: Vec<(Uuid, String, i64, i64)> =
        sqlx::query_as("SELECT * FROM colophon_usage_reconcile($1)")
            .bind(repair)
            .fetch_all(&mut *tx)
            .await
            .map_err(DbError::from_query)?;
    tx.commit().await.map_err(DbError::from_query)?;
    Ok(rows
        .into_iter()
        .map(|(account, field, stored, counted)| Drift {
            account,
            field,
            stored,
            counted,
        })
        .collect())
}
