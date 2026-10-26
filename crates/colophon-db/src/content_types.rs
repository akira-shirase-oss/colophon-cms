//! Content types: collections and their insert-only definition versions (docs/modules/02-db.md;
//! D-040). Like entries, every function runs inside a [`ProjectTransaction`]. Definitions are
//! validated by the domain (`colophon_domain::schema`) before they reach this module.

use sqlx::types::time::OffsetDateTime;

use crate::{DbError, scope::ProjectTransaction};

/// The built-in articles definition every environment starts with: exactly
/// `colophon_domain::schema::ContentType::articles().canonical_json()`, which a test checks, as
/// it checks migration 0003's copy.
pub const ARTICLES_DEFINITION: &str = r#"{"name":"Articles","title_field":"title","fields":[{"id":"title","label":"Title","type":"text","required":true,"max_chars":200,"multiline":false},{"id":"body","label":"Body","type":"rich_text","required":true}]}"#;

/// One version of a content type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// The collection's slug.
    pub collection: String,
    /// The version, from 1.
    pub version: i32,
    /// The canonical definition.
    pub definition: String,
    /// Its SHA-256.
    pub definition_hash: Vec<u8>,
    /// When this version was written.
    pub created_at: OffsetDateTime,
}

/// What an update did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Updated {
    /// The new version.
    Updated(Version),
    /// The current version isn't the one the update was based on; nothing was written.
    Stale {
        /// The current version.
        current: i32,
    },
    /// No such collection here.
    NotFound,
}

/// The current version of every content type, by slug.
pub async fn list(tx: &mut ProjectTransaction) -> Result<Vec<Version>, DbError> {
    sqlx::query_as!(
        Version,
        "SELECT DISTINCT ON (collection) collection, version, definition, definition_hash, created_at
         FROM content_type_versions
         WHERE project_id = $1 AND environment_id = $2
         ORDER BY collection, version DESC",
        tx.project,
        tx.environment
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// The current version of `collection`, if it exists here.
pub async fn current(
    tx: &mut ProjectTransaction,
    collection: &str,
) -> Result<Option<Version>, DbError> {
    sqlx::query_as!(
        Version,
        "SELECT collection, version, definition, definition_hash, created_at
         FROM content_type_versions
         WHERE project_id = $1 AND environment_id = $2 AND collection = $3
         ORDER BY version DESC
         LIMIT 1",
        tx.project,
        tx.environment,
        collection
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Creates a collection with its first definition. A taken slug is `DbError::Conflict`.
pub async fn create(
    tx: &mut ProjectTransaction,
    collection: &str,
    definition: &str,
    definition_hash: &[u8; 32],
) -> Result<Version, DbError> {
    sqlx::query!(
        "INSERT INTO collections (project_id, environment_id, slug) VALUES ($1, $2, $3)",
        tx.project,
        tx.environment,
        collection
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    insert(tx, collection, 1, None, definition, definition_hash).await
}

/// Writes version `expected + 1`, but only if `expected` is the current version.
pub async fn update(
    tx: &mut ProjectTransaction,
    collection: &str,
    expected: i32,
    definition: &str,
    definition_hash: &[u8; 32],
) -> Result<Updated, DbError> {
    let Some(current) = current(tx, collection).await? else {
        return Ok(Updated::NotFound);
    };
    if current.version != expected {
        return Ok(Updated::Stale {
            current: current.version,
        });
    }
    // A concurrent update that got there first makes this insert a duplicate key.
    match insert(
        tx,
        collection,
        expected + 1,
        Some(expected),
        definition,
        definition_hash,
    )
    .await
    {
        Ok(version) => Ok(Updated::Updated(version)),
        Err(DbError::Conflict) => Ok(Updated::Stale {
            current: expected + 1,
        }),
        Err(other) => Err(other),
    }
}

async fn insert(
    tx: &mut ProjectTransaction,
    collection: &str,
    version: i32,
    previous: Option<i32>,
    definition: &str,
    definition_hash: &[u8; 32],
) -> Result<Version, DbError> {
    sqlx::query_as!(
        Version,
        "INSERT INTO content_type_versions
           (project_id, environment_id, collection, version, previous_version, definition, definition_hash, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING collection, version, definition, definition_hash, created_at",
        tx.project,
        tx.environment,
        collection,
        version,
        previous,
        definition,
        &definition_hash[..],
        tx.identity
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)
}
