//! Content types: collections and their insert-only definition versions (docs/modules/02-db.md;
//! D-040). Like entries, every function runs inside a [`ProjectTransaction`]. Definitions are
//! validated by the domain (`colophon_domain::schema`) before they reach this module.

use sqlx::types::time::OffsetDateTime;
use uuid::Uuid;

use crate::{DbError, scope::ProjectTransaction};

/// The most drafts and publications one content type change checks (D-055). A collection with
/// more needs a migration, which doesn't exist yet.
pub const MAX_CHECKED: i64 = 10_000;

/// The first key of the advisory locks in [`lock_for_write`] and [`lock_for_change`]. PostgreSQL
/// keeps two-key advisory locks apart from one-key ones, such as the lock sqlx takes to migrate.
const LOCK_CLASS: i32 = 0x4354;

/// A draft or published revision a content type change has to keep valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    /// The entry.
    pub entry_id: Uuid,
    /// The revision.
    pub revision_id: Uuid,
    /// Its title.
    pub title: String,
    /// Its canonical content.
    pub content: String,
    /// Whether it is what readers get, rather than the draft.
    pub published: bool,
}

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

/// Holds `collection`'s content type still until the transaction ends. Entry writes and
/// publishes take this lock shared, before they read the content type or what they publish;
/// a change to the type takes it exclusively ([`lock_for_change`]). So a write is validated
/// against the version that is current when it commits, and a change sees every draft and
/// publication there will be until the next change (D-055). Waiting counts against the
/// statement timeout, like any query.
pub async fn lock_for_write(tx: &mut ProjectTransaction, collection: &str) -> Result<(), DbError> {
    lock(tx, collection, "pg_advisory_xact_lock_shared").await
}

/// The exclusive side of [`lock_for_write`], for a content type change.
pub async fn lock_for_change(tx: &mut ProjectTransaction, collection: &str) -> Result<(), DbError> {
    lock(tx, collection, "pg_advisory_xact_lock").await
}

async fn lock(
    tx: &mut ProjectTransaction,
    collection: &str,
    function: &'static str,
) -> Result<(), DbError> {
    // hashtext names the collection in 32 bits. A collision only makes two collections wait for
    // each other's changes; it never lets a change and a write overlap.
    sqlx::query(&format!(
        "SELECT 1 FROM {function}($1, hashtext($2::text || '/' || $3::text || '/' || $4))"
    ))
    .bind(LOCK_CLASS)
    .bind(tx.project)
    .bind(tx.environment)
    .bind(collection)
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(())
}

/// How many revisions a change to `collection` has to check: every entry's draft, and every
/// published revision that isn't also its entry's draft.
pub async fn stored_count(tx: &mut ProjectTransaction, collection: &str) -> Result<i64, DbError> {
    sqlx::query_scalar!(
        r#"SELECT (SELECT count(*) FROM entries
                   WHERE project_id = $1 AND environment_id = $2 AND collection = $3)
                + (SELECT count(*) FROM publications p
                   JOIN entries e ON e.project_id = p.project_id AND e.environment_id = p.environment_id
                                 AND e.id = p.entry_id
                   WHERE p.project_id = $1 AND p.environment_id = $2 AND p.collection = $3
                     AND p.valid_to IS NULL AND p.revision_id <> e.draft_revision_id) AS "count!""#,
        tx.project,
        tx.environment,
        collection
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Up to `limit` of the revisions [`stored_count`] counts, in entry order after `after`: the
/// drafts, or the published revisions that aren't drafts.
pub async fn stored_page(
    tx: &mut ProjectTransaction,
    collection: &str,
    published: bool,
    after: Uuid,
    limit: i64,
) -> Result<Vec<Stored>, DbError> {
    if published {
        sqlx::query_as!(
            Stored,
            r#"SELECT p.entry_id, p.revision_id, p.title, p.content, true AS "published!"
               FROM publications p
               JOIN entries e ON e.project_id = p.project_id AND e.environment_id = p.environment_id
                             AND e.id = p.entry_id
               WHERE p.project_id = $1 AND p.environment_id = $2 AND p.collection = $3
                 AND p.valid_to IS NULL AND p.revision_id <> e.draft_revision_id AND p.entry_id > $4
               ORDER BY p.entry_id
               LIMIT $5"#,
            tx.project,
            tx.environment,
            collection,
            after,
            limit
        )
        .fetch_all(&mut *tx.transaction)
        .await
        .map_err(DbError::from_query)
    } else {
        sqlx::query_as!(
            Stored,
            r#"SELECT e.id AS entry_id, r.id AS revision_id, r.title, r.content, false AS "published!"
               FROM entries e
               JOIN revisions r ON r.project_id = e.project_id AND r.environment_id = e.environment_id
                               AND r.id = e.draft_revision_id
               WHERE e.project_id = $1 AND e.environment_id = $2 AND e.collection = $3 AND e.id > $4
               ORDER BY e.id
               LIMIT $5"#,
            tx.project,
            tx.environment,
            collection,
            after,
            limit
        )
        .fetch_all(&mut *tx.transaction)
        .await
        .map_err(DbError::from_query)
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
