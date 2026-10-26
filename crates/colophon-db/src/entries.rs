//! Entries and their revisions (docs/modules/02-db.md, "Transactions"). Every function works
//! inside a [`ProjectTransaction`], so the caller is a member and row-level security confines
//! it to the project.

use sqlx::types::time::OffsetDateTime;
use uuid::Uuid;

use crate::{DbError, scope::ProjectTransaction};

/// The most entries or revisions one call returns.
pub const MAX_PAGE: i64 = 100;

/// A revision to store. The content is the article's canonical JSON; the hash is its SHA-256.
#[derive(Debug, Clone, Copy)]
pub struct NewRevision<'a> {
    /// The new revision's id.
    pub id: Uuid,
    /// Canonical JSON.
    pub content: &'a str,
    /// SHA-256 of `content`.
    pub content_hash: &'a [u8; 32],
    /// The title, copied out of the content for lists.
    pub title: &'a str,
}

/// A revision's metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    /// Its id.
    pub id: Uuid,
    /// The revision it was saved on top of; `None` for an entry's first.
    pub base_revision_id: Option<Uuid>,
    /// Who saved it.
    pub author_id: Uuid,
    /// When.
    pub created_at: OffsetDateTime,
    /// SHA-256 of its content.
    pub content_hash: Vec<u8>,
    /// Its title.
    pub title: String,
}

/// An entry with its current draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// The entry's id.
    pub entry_id: Uuid,
    /// Its collection.
    pub collection: String,
    /// When the entry was created.
    pub created_at: OffsetDateTime,
    /// When its draft last moved.
    pub updated_at: OffsetDateTime,
    /// The current draft revision.
    pub revision: Revision,
    /// The draft's canonical JSON.
    pub content: String,
}

/// An entry in a list: its draft's metadata, without the content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// The entry's id.
    pub entry_id: Uuid,
    /// When its draft last moved.
    pub updated_at: OffsetDateTime,
    /// The current draft revision.
    pub revision: Revision,
}

/// What a save did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Saved {
    /// The draft now points at the new revision.
    Saved(Draft),
    /// The draft no longer points at the expected revision; nothing was written.
    Stale {
        /// The revision it points at now.
        current: Uuid,
    },
    /// No such entry in this environment.
    NotFound,
}

/// Creates an entry whose draft is its first revision, written by the transaction's identity.
pub async fn create(
    tx: &mut ProjectTransaction,
    entry_id: Uuid,
    collection: &str,
    revision: NewRevision<'_>,
) -> Result<Draft, DbError> {
    let entry = sqlx::query!(
        "INSERT INTO entries (project_id, environment_id, id, collection, draft_revision_id, created_by)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING created_at, updated_at",
        tx.project,
        tx.environment,
        entry_id,
        collection,
        revision.id,
        tx.identity
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    let created_at = insert_revision(tx, entry_id, None, revision).await?;
    Ok(Draft {
        entry_id,
        collection: collection.to_owned(),
        created_at: entry.created_at,
        updated_at: entry.updated_at,
        revision: Revision {
            id: revision.id,
            base_revision_id: None,
            author_id: tx.identity,
            created_at,
            content_hash: revision.content_hash.to_vec(),
            title: revision.title.to_owned(),
        },
        content: revision.content.to_owned(),
    })
}

/// Saves `revision` as the entry's new draft, but only if the draft is still `expected` (D2).
pub async fn save(
    tx: &mut ProjectTransaction,
    entry_id: Uuid,
    expected: Uuid,
    revision: NewRevision<'_>,
) -> Result<Saved, DbError> {
    // Lock the entry so concurrent saves queue here. This read only tells "stale" from
    // "missing" and names the current revision; the conditional UPDATE below is the rule.
    let current = sqlx::query!(
        "SELECT draft_revision_id, collection, created_at FROM entries
         WHERE project_id = $1 AND environment_id = $2 AND id = $3
         FOR UPDATE",
        tx.project,
        tx.environment,
        entry_id
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    let Some(current) = current else {
        return Ok(Saved::NotFound);
    };
    if current.draft_revision_id != expected {
        return Ok(Saved::Stale {
            current: current.draft_revision_id,
        });
    }
    let created_at = insert_revision(tx, entry_id, Some(expected), revision).await?;
    let moved = sqlx::query_scalar!(
        "UPDATE entries SET draft_revision_id = $4, updated_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND draft_revision_id = $5
         RETURNING updated_at",
        tx.project,
        tx.environment,
        entry_id,
        revision.id,
        expected
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    let Some(updated_at) = moved else {
        // Unreachable while the row lock is held; if it ever happens, the caller rolls back.
        return Ok(Saved::Stale {
            current: current.draft_revision_id,
        });
    };
    Ok(Saved::Saved(Draft {
        entry_id,
        collection: current.collection,
        created_at: current.created_at,
        updated_at,
        revision: Revision {
            id: revision.id,
            base_revision_id: Some(expected),
            author_id: tx.identity,
            created_at,
            content_hash: revision.content_hash.to_vec(),
            title: revision.title.to_owned(),
        },
        content: revision.content.to_owned(),
    }))
}

async fn insert_revision(
    tx: &mut ProjectTransaction,
    entry_id: Uuid,
    base: Option<Uuid>,
    revision: NewRevision<'_>,
) -> Result<OffsetDateTime, DbError> {
    sqlx::query_scalar!(
        "INSERT INTO revisions
           (project_id, environment_id, entry_id, id, base_revision_id, content, content_hash, title, author_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING created_at",
        tx.project,
        tx.environment,
        entry_id,
        revision.id,
        base,
        revision.content,
        &revision.content_hash[..],
        revision.title,
        tx.identity
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)
}

/// The entry with its current draft, if it exists in this environment.
pub async fn get(tx: &mut ProjectTransaction, entry_id: Uuid) -> Result<Option<Draft>, DbError> {
    let row = sqlx::query!(
        "SELECT e.collection, e.created_at, e.updated_at,
                r.id AS revision_id, r.base_revision_id, r.author_id, r.created_at AS revision_created_at,
                r.content_hash, r.title, r.content
         FROM entries e
         JOIN revisions r ON r.project_id = e.project_id AND r.environment_id = e.environment_id
                         AND r.id = e.draft_revision_id
         WHERE e.project_id = $1 AND e.environment_id = $2 AND e.id = $3",
        tx.project,
        tx.environment,
        entry_id
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(row.map(|row| Draft {
        entry_id,
        collection: row.collection,
        created_at: row.created_at,
        updated_at: row.updated_at,
        revision: Revision {
            id: row.revision_id,
            base_revision_id: row.base_revision_id,
            author_id: row.author_id,
            created_at: row.revision_created_at,
            content_hash: row.content_hash,
            title: row.title,
        },
        content: row.content,
    }))
}

/// A collection's entries, newest first, starting after `before` when given: at most `limit`,
/// capped at one more than [`MAX_PAGE`] so a caller can ask for an extra row to learn whether
/// another page follows.
pub async fn list(
    tx: &mut ProjectTransaction,
    collection: &str,
    before: Option<Uuid>,
    limit: i64,
) -> Result<Vec<Summary>, DbError> {
    let rows = sqlx::query!(
        "SELECT e.id, e.updated_at,
                r.id AS revision_id, r.base_revision_id, r.author_id, r.created_at AS revision_created_at,
                r.content_hash, r.title
         FROM entries e
         JOIN revisions r ON r.project_id = e.project_id AND r.environment_id = e.environment_id
                         AND r.id = e.draft_revision_id
         WHERE e.project_id = $1 AND e.environment_id = $2 AND e.collection = $3
           AND ($4::uuid IS NULL OR e.id < $4)
         ORDER BY e.id DESC
         LIMIT $5",
        tx.project,
        tx.environment,
        collection,
        before,
        limit.clamp(1, MAX_PAGE + 1)
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(rows
        .into_iter()
        .map(|row| Summary {
            entry_id: row.id,
            updated_at: row.updated_at,
            revision: Revision {
                id: row.revision_id,
                base_revision_id: row.base_revision_id,
                author_id: row.author_id,
                created_at: row.revision_created_at,
                content_hash: row.content_hash,
                title: row.title,
            },
        })
        .collect())
}

/// An entry's revisions, newest first, at most `limit` (capped at [`MAX_PAGE`]). Empty when
/// the entry doesn't exist.
pub async fn history(
    tx: &mut ProjectTransaction,
    entry_id: Uuid,
    limit: i64,
) -> Result<Vec<Revision>, DbError> {
    sqlx::query_as!(
        Revision,
        "SELECT id, base_revision_id, author_id, created_at, content_hash, title FROM revisions
         WHERE project_id = $1 AND environment_id = $2 AND entry_id = $3
         ORDER BY id DESC
         LIMIT $4",
        tx.project,
        tx.environment,
        entry_id,
        limit.clamp(1, MAX_PAGE)
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}
