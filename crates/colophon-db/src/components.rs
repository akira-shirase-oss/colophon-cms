//! Shared components: named groups of fields many content types use, with insert-only
//! definition versions (migration 0029, D-073). Like content types, every function runs inside a
//! [`ProjectTransaction`], and definitions are validated by the domain before they reach here.
//!
//! Locks: a content type write that uses components takes [`lock_for_use`] on them before its
//! own content-type lock, and a component change takes [`lock_for_change`] before the locks of
//! the content types it rewrites. Components always come first, so the two never wait on each
//! other in a circle.

use sqlx::types::time::OffsetDateTime;

use crate::{DbError, content_types, scope::ProjectTransaction};

/// The first key of the advisory locks in [`lock_for_use`] and [`lock_for_change`], apart from the
/// content types' class.
const LOCK_CLASS: i32 = 0x434d;

/// One version of a component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// The component's slug.
    pub component: String,
    /// The version, from 1.
    pub version: i32,
    /// The canonical definition.
    pub definition: String,
    /// Its SHA-256.
    pub definition_hash: Vec<u8>,
    /// When this version was written.
    pub created_at: OffsetDateTime,
}

/// The current version of every component, by slug.
pub async fn list(tx: &mut ProjectTransaction) -> Result<Vec<Version>, DbError> {
    sqlx::query_as!(
        Version,
        "SELECT DISTINCT ON (component) component, version, definition, definition_hash, created_at
         FROM component_versions
         WHERE project_id = $1 AND environment_id = $2
         ORDER BY component, version DESC",
        tx.project,
        tx.environment
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// The current versions of the components in `slugs` that exist here.
pub async fn current_of(
    tx: &mut ProjectTransaction,
    slugs: &[String],
) -> Result<Vec<Version>, DbError> {
    if slugs.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_as!(
        Version,
        "SELECT DISTINCT ON (component) component, version, definition, definition_hash, created_at
         FROM component_versions
         WHERE project_id = $1 AND environment_id = $2 AND component = ANY($3)
         ORDER BY component, version DESC",
        tx.project,
        tx.environment,
        slugs
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// The current version of `component`, if it exists here.
pub async fn current(
    tx: &mut ProjectTransaction,
    component: &str,
) -> Result<Option<Version>, DbError> {
    Ok(current_of(tx, &[component.to_owned()]).await?.pop())
}

/// Creates a component with its first definition. A taken slug is `DbError::Conflict`.
pub async fn create(
    tx: &mut ProjectTransaction,
    component: &str,
    definition: &str,
    definition_hash: &[u8; 32],
) -> Result<Version, DbError> {
    sqlx::query!(
        "INSERT INTO components (project_id, environment_id, slug) VALUES ($1, $2, $3)",
        tx.project,
        tx.environment,
        component
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    insert(tx, component, 1, None, definition, definition_hash).await
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
    /// No such component here.
    NotFound,
}

/// Writes version `expected + 1`, but only if `expected` is the current version. The caller
/// holds [`lock_for_change`].
pub async fn update(
    tx: &mut ProjectTransaction,
    component: &str,
    expected: i32,
    definition: &str,
    definition_hash: &[u8; 32],
) -> Result<Updated, DbError> {
    let Some(current) = current(tx, component).await? else {
        return Ok(Updated::NotFound);
    };
    if current.version != expected {
        return Ok(Updated::Stale {
            current: current.version,
        });
    }
    match insert(
        tx,
        component,
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

/// The current versions of the content types that use `component`, in slug order: the order
/// their locks are taken in. Canonical definitions write `"component":"<slug>"` only for the
/// marker (a quote inside a string is escaped), so the text finds them exactly.
pub async fn users(
    tx: &mut ProjectTransaction,
    component: &str,
) -> Result<Vec<content_types::Version>, DbError> {
    let marker = format!(r#""component":"{component}""#);
    Ok(content_types::list(tx)
        .await?
        .into_iter()
        .filter(|version| version.definition.contains(&marker))
        .collect())
}

/// Holds the components in `slugs` still until the transaction ends, so a content type that uses
/// them is written with their current fields. Takes them in sorted order.
pub async fn lock_for_use(tx: &mut ProjectTransaction, slugs: &[String]) -> Result<(), DbError> {
    let mut sorted: Vec<&String> = slugs.iter().collect();
    sorted.sort();
    sorted.dedup();
    for slug in sorted {
        lock(
            tx,
            slug,
            "SELECT 1 FROM pg_advisory_xact_lock_shared($1, hashtext($2::text || '/' || $3::text || '/' || $4))",
        )
        .await?;
    }
    Ok(())
}

/// The exclusive side of [`lock_for_use`], for a change to the component.
pub async fn lock_for_change(tx: &mut ProjectTransaction, component: &str) -> Result<(), DbError> {
    lock(
        tx,
        component,
        "SELECT 1 FROM pg_advisory_xact_lock($1, hashtext($2::text || '/' || $3::text || '/' || $4))",
    )
    .await
}

async fn lock(
    tx: &mut ProjectTransaction,
    component: &str,
    statement: &'static str,
) -> Result<(), DbError> {
    sqlx::query(statement)
        .bind(LOCK_CLASS)
        .bind(tx.project)
        .bind(tx.environment)
        .bind(component)
        .execute(&mut *tx.transaction)
        .await
        .map_err(DbError::from_query)?;
    Ok(())
}

async fn insert(
    tx: &mut ProjectTransaction,
    component: &str,
    version: i32,
    previous: Option<i32>,
    definition: &str,
    definition_hash: &[u8; 32],
) -> Result<Version, DbError> {
    sqlx::query_as!(
        Version,
        "INSERT INTO component_versions
           (project_id, environment_id, component, version, previous_version, definition, definition_hash, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING component, version, definition, definition_hash, created_at",
        tx.project,
        tx.environment,
        component,
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
