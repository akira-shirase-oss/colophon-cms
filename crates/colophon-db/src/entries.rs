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
    /// The content type version the content was validated against.
    pub schema_version: i32,
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
    /// The entry's locale tag, when it has one.
    pub locale: Option<String>,
}

/// An entry in a list: its draft's metadata, without the content beyond the asked-for fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// The entry's id.
    pub entry_id: Uuid,
    /// When its draft last moved.
    pub updated_at: OffsetDateTime,
    /// The current draft revision.
    pub revision: Revision,
    /// The revision readers get now, and the release it went out in, if it is published.
    pub published: Option<(Uuid, i64)>,
    /// The draft's values of the `columns` fields `list` was asked for, as a JSON object; a
    /// field the content doesn't have is `null`.
    pub fields: String,
    /// The entry's locale tag, when it has one.
    pub locale: Option<String>,
}

/// The longest title search accepted.
pub const MAX_SEARCH: usize = 100;

/// What a save did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Saved {
    /// The draft now points at the new revision.
    Saved(Box<Draft>),
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
    uniques: &[(String, [u8; 32])],
    locale: Option<&str>,
) -> Result<Draft, DbError> {
    let entry = sqlx::query!(
        "INSERT INTO entries (project_id, environment_id, id, collection, draft_revision_id, draft_title, created_by, locale)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING created_at, updated_at",
        tx.project,
        tx.environment,
        entry_id,
        collection,
        revision.id,
        revision.title,
        tx.identity,
        locale
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    let created_at = insert_revision(tx, entry_id, collection, None, revision).await?;
    set_uniques(tx, entry_id, collection, uniques).await?;
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
        locale: locale.map(str::to_owned),
    })
}

/// Saves `revision` as the entry's new draft, but only if the draft is still `expected` (D2).
pub async fn save(
    tx: &mut ProjectTransaction,
    entry_id: Uuid,
    expected: Uuid,
    revision: NewRevision<'_>,
    uniques: &[(String, [u8; 32])],
) -> Result<Saved, DbError> {
    // Lock the entry so concurrent saves queue here. This read only tells "stale" from
    // "missing" and names the current revision; the conditional UPDATE below is the rule.
    let current = sqlx::query!(
        "SELECT draft_revision_id, collection, created_at, locale FROM entries
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
    let created_at =
        insert_revision(tx, entry_id, &current.collection, Some(expected), revision).await?;
    let moved = sqlx::query_scalar!(
        "UPDATE entries SET draft_revision_id = $4, draft_title = $6, updated_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND draft_revision_id = $5
         RETURNING updated_at",
        tx.project,
        tx.environment,
        entry_id,
        revision.id,
        expected,
        revision.title
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
    set_uniques(tx, entry_id, &current.collection, uniques).await?;
    Ok(Saved::Saved(Box::new(Draft {
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
        locale: current.locale,
    })))
}

/// The answer to a delete: gone, never there, or held in place by something that references it
/// (a changeset item or a published copy — Postgres refuses, and that's the rule).
pub enum Removed {
    Removed,
    NotFound,
    Referenced,
}

/// Every draft's canonical content, newest first — the lines of a collection's export.
/// Capped at 10,000 rows so an export stays a bounded read.
pub async fn export(tx: &mut ProjectTransaction, collection: &str) -> Result<Vec<String>, DbError> {
    let rows = sqlx::query!(
        r#"SELECT r.content
           FROM entries e
           CROSS JOIN LATERAL (
               SELECT r.content
               FROM revisions r
               WHERE r.project_id = e.project_id AND r.environment_id = e.environment_id
                 AND r.id = e.draft_revision_id
               LIMIT 1
           ) r
           WHERE e.project_id = $1 AND e.environment_id = $2 AND e.collection = $3
           ORDER BY e.created_at DESC, e.id DESC
           LIMIT 10000"#,
        tx.project,
        tx.environment,
        collection
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(rows.into_iter().map(|row| row.content).collect())
}

/// Deletes an entry and its revisions outright. Only a draft-entry is really removable: a
/// revision pinned by a changeset item or carried by a release fails the foreign keys.
pub async fn remove(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry_id: Uuid,
) -> Result<Removed, DbError> {
    let revisions = sqlx::query!(
        "DELETE FROM revisions WHERE project_id = $1 AND environment_id = $2 AND entry_id = $3",
        tx.project,
        tx.environment,
        entry_id
    )
    .execute(&mut *tx.transaction)
    .await;
    if let Err(error) = &revisions
        && matches!(error, sqlx::Error::Database(database) if database.is_foreign_key_violation())
    {
        return Ok(Removed::Referenced);
    }
    revisions.map_err(DbError::from_write)?;
    let entries = sqlx::query!(
        "DELETE FROM entries WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND collection = $4",
        tx.project,
        tx.environment,
        entry_id,
        collection
    )
    .execute(&mut *tx.transaction)
    .await;
    match entries {
        Ok(done) if done.rows_affected() > 0 => Ok(Removed::Removed),
        Ok(_) => Ok(Removed::NotFound),
        Err(sqlx::Error::Database(database)) if database.is_foreign_key_violation() => {
            Ok(Removed::Referenced)
        }
        Err(error) => Err(DbError::from_write(error)),
    }
}

/// Replaces the entry's `unique_values` rows with its new draft's: `uniques` is each unique
/// field's id with the sha256 of its canonical value. A value another entry already holds trips
/// the table's primary key, surfaced as [`DbError::Conflict`].
async fn set_uniques(
    tx: &mut ProjectTransaction,
    entry_id: Uuid,
    collection: &str,
    uniques: &[(String, [u8; 32])],
) -> Result<(), DbError> {
    sqlx::query!(
        "DELETE FROM unique_values
         WHERE project_id = $1 AND environment_id = $2 AND collection = $3 AND entry_id = $4",
        tx.project,
        tx.environment,
        collection,
        entry_id
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    for (field_id, hash) in uniques {
        sqlx::query!(
            "INSERT INTO unique_values (project_id, environment_id, collection, field_id, entry_id, value_hash)
             VALUES ($1, $2, $3, $4, $5, $6)",
            tx.project,
            tx.environment,
            collection,
            field_id,
            entry_id,
            &hash[..]
        )
        .execute(&mut *tx.transaction)
        .await
        .map_err(DbError::from_write)?;
    }
    Ok(())
}

async fn insert_revision(
    tx: &mut ProjectTransaction,
    entry_id: Uuid,
    collection: &str,
    base: Option<Uuid>,
    revision: NewRevision<'_>,
) -> Result<OffsetDateTime, DbError> {
    sqlx::query_scalar!(
        "INSERT INTO revisions
           (project_id, environment_id, entry_id, id, base_revision_id, content, content_hash, title,
            author_id, collection, schema_version)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         RETURNING created_at",
        tx.project,
        tx.environment,
        entry_id,
        revision.id,
        base,
        revision.content,
        &revision.content_hash[..],
        revision.title,
        tx.identity,
        collection,
        revision.schema_version
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)
}

/// The entry with its current draft, if it exists in this environment and collection.
pub async fn get(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry_id: Uuid,
) -> Result<Option<Draft>, DbError> {
    let row = sqlx::query!(
        "SELECT e.collection, e.created_at, e.updated_at, e.locale,
                r.id AS revision_id, r.base_revision_id, r.author_id, r.created_at AS revision_created_at,
                r.content_hash, r.title, r.content
         FROM entries e
         JOIN revisions r ON r.project_id = e.project_id AND r.environment_id = e.environment_id
                         AND r.id = e.draft_revision_id
         WHERE e.project_id = $1 AND e.environment_id = $2 AND e.id = $3 AND e.collection = $4",
        tx.project,
        tx.environment,
        entry_id,
        collection
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
        locale: row.locale,
    }))
}

/// A collection's entries, newest first, starting after `before` when given: at most `limit`,
/// capped at one more than [`MAX_PAGE`] so a caller can ask for an extra row to learn whether
/// another page follows.
///
/// Each entry's draft is looked up on its own (`LATERAL … LIMIT 1`, which Postgres can't fold
/// into a join and reorder), so the only plan walks the collection's entries newest first and
/// stops at `limit`, with one key lookup each. As a plain join, statistics gathered while the
/// tables were nearly empty let Postgres scan every revision for every entry instead: in a CI
/// load test that grew them to 512 entries in seconds, one page took 288 ms to throw away 523,776
/// rows.
pub async fn list(
    tx: &mut ProjectTransaction,
    collection: &str,
    before: Option<Uuid>,
    limit: i64,
    search: Option<&str>,
    columns: &[String],
    locale: Option<&str>,
) -> Result<Vec<Summary>, DbError> {
    // A title search matches anywhere in the draft's title, case-insensitively; its own `%`, `_`
    // and `\` match themselves. It filters on the entry's copy of the title (migration 0015), so
    // only matching entries look up their draft revision.
    let pattern = search.map(|text| {
        let escaped = text
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        format!("%{escaped}%")
    });
    let rows = sqlx::query!(
        r#"SELECT e.id, e.updated_at, e.locale,
                r.id AS revision_id, r.base_revision_id, r.author_id, r.created_at AS revision_created_at,
                r.content_hash, r.title, live.revision_id AS "published_revision_id?",
                live.valid_from AS "published_in?",
                COALESCE(
                    (SELECT jsonb_object_agg(k, r.content::jsonb -> k) FROM unnest($7::text[]) AS u(k)),
                    '{}'::jsonb
                )::text AS "fields!"
         FROM entries e
         CROSS JOIN LATERAL (
             SELECT r.id, r.base_revision_id, r.author_id, r.created_at, r.content_hash, r.title,
                    r.content
             FROM revisions r
             WHERE r.project_id = e.project_id AND r.environment_id = e.environment_id
               AND r.id = e.draft_revision_id
             LIMIT 1
         ) r
         LEFT JOIN LATERAL (
             SELECT p.revision_id, p.valid_from
             FROM publications p
             WHERE p.project_id = e.project_id AND p.environment_id = e.environment_id
               AND p.entry_id = e.id AND p.valid_to IS NULL
             LIMIT 1
         ) live ON true
         WHERE e.project_id = $1 AND e.environment_id = $2 AND e.collection = $3
           AND ($4::uuid IS NULL OR e.id < $4)
           AND ($6::text IS NULL OR e.draft_title ILIKE $6)
           AND ($8::text IS NULL OR e.locale = $8)
         ORDER BY e.id DESC
         LIMIT $5"#,
        tx.project,
        tx.environment,
        collection,
        before,
        limit.clamp(1, MAX_PAGE + 1),
        pattern,
        columns,
        locale
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(rows
        .into_iter()
        .map(|row| Summary {
            entry_id: row.id,
            updated_at: row.updated_at,
            locale: row.locale,
            revision: Revision {
                id: row.revision_id,
                base_revision_id: row.base_revision_id,
                author_id: row.author_id,
                created_at: row.revision_created_at,
                content_hash: row.content_hash,
                title: row.title,
            },
            published: row.published_revision_id.zip(row.published_in),
            fields: row.fields,
        })
        .collect())
}

/// An entry's revisions, newest first, at most `limit` (capped at [`MAX_PAGE`]). Empty when
/// the entry doesn't exist in this collection.
pub async fn history(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry_id: Uuid,
    limit: i64,
) -> Result<Vec<Revision>, DbError> {
    sqlx::query_as!(
        Revision,
        "SELECT id, base_revision_id, author_id, created_at, content_hash, title FROM revisions
         WHERE project_id = $1 AND environment_id = $2 AND entry_id = $3 AND collection = $5
         ORDER BY id DESC
         LIMIT $4",
        tx.project,
        tx.environment,
        entry_id,
        limit.clamp(1, MAX_PAGE),
        collection
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// One revision of `entry` in `collection`, with its canonical content, if it exists here: what a
/// review shows side by side.
pub async fn revision(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry_id: Uuid,
    revision_id: Uuid,
) -> Result<Option<(Revision, String)>, DbError> {
    let row = sqlx::query!(
        "SELECT id, base_revision_id, author_id, created_at, content_hash, title, content FROM revisions
         WHERE project_id = $1 AND environment_id = $2 AND entry_id = $3 AND id = $4 AND collection = $5",
        tx.project,
        tx.environment,
        entry_id,
        revision_id,
        collection
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(row.map(|row| {
        (
            Revision {
                id: row.id,
                base_revision_id: row.base_revision_id,
                author_id: row.author_id,
                created_at: row.created_at,
                content_hash: row.content_hash,
                title: row.title,
            },
            row.content,
        )
    }))
}

/// A draft that mentions an entry id anywhere in its canonical content: the coarse screen
/// before the caller checks which `reference` field, if any, actually holds it.
pub struct Referring {
    /// The entry the draft belongs to.
    pub id: Uuid,
    /// The draft's title.
    pub title: String,
    /// The draft's canonical content.
    pub content: String,
}

/// The newest drafts in `collection` whose content mentions `id`, latest change first,
/// capped at fifty. `LIKE` only screens: a text field can hold the same id, so a row is a
/// candidate, not a proof.
pub async fn referencing(
    tx: &mut ProjectTransaction,
    collection: &str,
    id: Uuid,
) -> Result<Vec<Referring>, DbError> {
    let rows = sqlx::query!(
        r#"SELECT e.id, r.title, r.content
           FROM entries e
           JOIN revisions r ON r.id = e.draft_revision_id
           WHERE e.collection = $1 AND r.content LIKE '%' || $2 || '%'
           ORDER BY e.updated_at DESC, e.id
           LIMIT 50"#,
        collection,
        id.to_string()
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(rows
        .into_iter()
        .map(|row| Referring {
            id: row.id,
            title: row.title,
            content: row.content,
        })
        .collect())
}

/// Sets an entry's locale tag, or clears it for `None`. The draft pointer doesn't move — a
/// locale is a label on the entry, not content. Returns whether the entry was there.
pub async fn set_locale(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry_id: Uuid,
    locale: Option<&str>,
) -> Result<bool, DbError> {
    let done = sqlx::query!(
        "UPDATE entries SET locale = $5
         WHERE project_id = $1 AND environment_id = $2 AND collection = $3 AND id = $4",
        tx.project,
        tx.environment,
        collection,
        entry_id,
        locale
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(done.rows_affected() == 1)
}

/// One entry in a localization group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Localization {
    /// The entry.
    pub id: Uuid,
    /// Its locale tag.
    pub locale: String,
}

/// The siblings of `entry` — its whole localization group, itself included, ordered by
/// locale. An unlocalized entry has no group and returns nothing.
pub async fn localizations(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry_id: Uuid,
) -> Result<Vec<Localization>, DbError> {
    sqlx::query_as!(
        Localization,
        r#"SELECT s.id, s.locale AS "locale!" FROM entries s
           JOIN entries e ON e.localization_id = s.localization_id
           WHERE e.project_id = $1 AND e.environment_id = $2 AND e.id = $3 AND e.collection = $4
             AND s.project_id = $1 AND s.environment_id = $2
           ORDER BY s.locale"#,
        tx.project,
        tx.environment,
        entry_id,
        collection
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// What making a localized sibling did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Localized {
    /// The new entry's draft: a copy of the source's, in the asked-for locale.
    Made(Box<Draft>),
    /// The group already has this locale — the sibling that does.
    Occupied(Uuid),
    /// No such source entry in this environment and collection.
    Missing,
}

/// The source half of [`create_localization`]: what its draft revision holds and where it
/// sits in a localization group.
struct SourceRow {
    localization_id: Option<Uuid>,
    content: String,
    title: String,
    content_hash: Vec<u8>,
    schema_version: i32,
}

/// Creates `entry_id` as `source`'s sibling in `locale`, copying the source's draft. A source
/// with no group joins one first: after this, both are localized. All in this transaction, so
/// the group never sits half-made.
pub async fn create_localization(
    tx: &mut ProjectTransaction,
    entry_id: Uuid,
    source_id: Uuid,
    collection: &str,
    locale: &str,
) -> Result<Localized, DbError> {
    let source = sqlx::query_as!(
        SourceRow,
        "SELECT e.localization_id,
                r.content, r.title, r.content_hash, r.schema_version
         FROM entries e
         JOIN revisions r ON r.project_id = e.project_id AND r.environment_id = e.environment_id
                         AND r.id = e.draft_revision_id
         WHERE e.project_id = $1 AND e.environment_id = $2 AND e.id = $3 AND e.collection = $4
         FOR UPDATE OF e",
        tx.project,
        tx.environment,
        source_id,
        collection
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    let Some(source) = source else {
        return Ok(Localized::Missing);
    };
    let group = if let Some(group) = source.localization_id {
        group
    } else {
        let group = Uuid::now_v7();
        sqlx::query!(
            "UPDATE entries SET localization_id = $4
             WHERE project_id = $1 AND environment_id = $2 AND id = $3",
            tx.project,
            tx.environment,
            source_id,
            group
        )
        .execute(&mut *tx.transaction)
        .await
        .map_err(DbError::from_write)?;
        group
    };
    let occupied = sqlx::query_scalar!(
        "SELECT id FROM entries
         WHERE project_id = $1 AND environment_id = $2 AND localization_id = $3 AND locale = $4",
        tx.project,
        tx.environment,
        group,
        locale
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    if let Some(id) = occupied {
        return Ok(Localized::Occupied(id));
    }
    insert_sibling(tx, entry_id, collection, locale, group, source).await
}

/// The insert half of [`create_localization`]: the new entry, and its first revision copied
/// verbatim from the source's draft — same content, same hash.
async fn insert_sibling(
    tx: &mut ProjectTransaction,
    entry_id: Uuid,
    collection: &str,
    locale: &str,
    group: Uuid,
    source: SourceRow,
) -> Result<Localized, DbError> {
    let revision_id = Uuid::now_v7();
    let entry = sqlx::query!(
        "INSERT INTO entries (project_id, environment_id, id, collection, draft_revision_id,
                              draft_title, created_by, locale, localization_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING created_at, updated_at",
        tx.project,
        tx.environment,
        entry_id,
        collection,
        revision_id,
        source.title,
        tx.identity,
        locale,
        group
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    let mut content_hash = [0_u8; 32];
    content_hash.copy_from_slice(&source.content_hash[..32.min(source.content_hash.len())]);
    let created_at = insert_revision(
        tx,
        entry_id,
        collection,
        None,
        NewRevision {
            id: revision_id,
            content: &source.content,
            content_hash: &content_hash,
            title: &source.title,
            schema_version: source.schema_version,
        },
    )
    .await?;
    Ok(Localized::Made(Box::new(Draft {
        entry_id,
        collection: collection.to_owned(),
        created_at: entry.created_at,
        updated_at: entry.updated_at,
        revision: Revision {
            id: revision_id,
            base_revision_id: None,
            author_id: tx.identity,
            created_at,
            content_hash: source.content_hash.clone(),
            title: source.title.clone(),
        },
        content: source.content,
        locale: Some(locale.to_owned()),
    })))
}
