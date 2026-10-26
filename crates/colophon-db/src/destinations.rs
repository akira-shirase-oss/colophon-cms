//! Where a project's releases are delivered (docs/modules/05-worker.md; D-046, D-050, D-051): a
//! webhook hears each release; a CDN is told what to purge; a probe looks for the release. Owners and developers configure them; the API
//! checks that and the address before calling these.

use sqlx::types::time::OffsetDateTime;
use uuid::Uuid;

use crate::{DbError, scope::ProjectTransaction};

/// A configured destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    /// Its id.
    pub id: Uuid,
    /// `webhook`, `cdn` or `probe`.
    pub kind: String,
    /// A name for people.
    pub name: String,
    /// Where it posts.
    pub url: String,
    /// Whether new releases are announced to it.
    pub enabled: bool,
    /// For a probe: the only location whose workers may run it, if pinned.
    pub location: Option<String>,
    /// When it was added.
    pub created_at: OffsetDateTime,
}

/// Adds a destination of `kind` (`webhook`, `cdn` or `probe`), a probe optionally pinned to
/// `location`.
pub async fn create(
    tx: &mut ProjectTransaction,
    kind: &str,
    name: &str,
    url: &str,
    location: Option<&str>,
) -> Result<Destination, DbError> {
    sqlx::query_as!(
        Destination,
        "INSERT INTO destinations (project_id, environment_id, id, kind, name, url, location, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING id, kind, name, url, enabled, location, created_at",
        tx.project,
        tx.environment,
        Uuid::now_v7(),
        kind,
        name,
        url,
        location,
        tx.identity
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)
}

/// The environment's destinations, oldest first (at most 100).
pub async fn list(tx: &mut ProjectTransaction) -> Result<Vec<Destination>, DbError> {
    sqlx::query_as!(
        Destination,
        "SELECT id, kind, name, url, enabled, location, created_at FROM destinations
         WHERE project_id = $1 AND environment_id = $2 ORDER BY id LIMIT 100",
        tx.project,
        tx.environment
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Turns announcements to a destination on or off; false if there's no such destination.
pub async fn set_enabled(
    tx: &mut ProjectTransaction,
    id: Uuid,
    enabled: bool,
) -> Result<bool, DbError> {
    let changed = sqlx::query!(
        "UPDATE destinations SET enabled = $4 WHERE project_id = $1 AND environment_id = $2 AND id = $3",
        tx.project,
        tx.environment,
        id,
        enabled
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(changed.rows_affected() == 1)
}
