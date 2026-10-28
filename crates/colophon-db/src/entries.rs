//! Entries and their revisions (docs/modules/02-db.md, "Transactions"). Every function works
//! inside a [`ProjectTransaction`], so the caller is a member and row-level security confines
//! it to the project.

use colophon_domain::schema::ContentReference;
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
    /// The slug, copied out of the content: unique among the collection's drafts (D-054).
    pub slug: Option<&'a str>,
    /// The content type version the content was validated against.
    pub schema_version: i32,
    /// The entries its reference fields name (D-065).
    pub references: &'a [ContentReference],
}

/// The entry that has a slug, as a draft or as what readers get ([`slug_holder`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlugHolder {
    /// The entry.
    pub entry_id: Uuid,
    /// Its title, in the revision that has the slug.
    pub title: String,
    /// Whether that revision is the published one rather than the draft.
    pub published: bool,
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
    /// Who created it: the member whose own entry it is (D-058, step R.3).
    pub created_by: Uuid,
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
    /// Who created it.
    pub created_by: Uuid,
    /// When its draft last moved.
    pub updated_at: OffsetDateTime,
    /// The current draft revision.
    pub revision: Revision,
    /// The revision readers get now, and the release it went out in, if it is published.
    pub published: Option<(Uuid, i64)>,
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
) -> Result<Draft, DbError> {
    let entry = sqlx::query!(
        "INSERT INTO entries (project_id, environment_id, id, collection, draft_revision_id, draft_slug, draft_title, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING created_at, updated_at",
        tx.project,
        tx.environment,
        entry_id,
        collection,
        revision.id,
        revision.slug,
        revision.title,
        tx.identity
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    let created_at = insert_revision(tx, entry_id, collection, None, revision).await?;
    Ok(Draft {
        entry_id,
        collection: collection.to_owned(),
        created_at: entry.created_at,
        created_by: tx.identity,
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
        "SELECT draft_revision_id, collection, created_at, created_by FROM entries
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
        "UPDATE entries SET draft_revision_id = $4, draft_slug = $6, draft_title = $7, updated_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND draft_revision_id = $5
         RETURNING updated_at",
        tx.project,
        tx.environment,
        entry_id,
        revision.id,
        expected,
        revision.slug,
        revision.title
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    let Some(updated_at) = moved else {
        // Unreachable while the row lock is held; if it ever happens, the caller rolls back.
        return Ok(Saved::Stale {
            current: current.draft_revision_id,
        });
    };
    Ok(Saved::Saved(Box::new(Draft {
        entry_id,
        collection: current.collection,
        created_at: current.created_at,
        created_by: current.created_by,
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
    })))
}

async fn insert_revision(
    tx: &mut ProjectTransaction,
    entry_id: Uuid,
    collection: &str,
    base: Option<Uuid>,
    revision: NewRevision<'_>,
) -> Result<OffsetDateTime, DbError> {
    let created_at = sqlx::query_scalar!(
        "INSERT INTO revisions
           (project_id, environment_id, entry_id, id, base_revision_id, content, content_hash, title,
            author_id, collection, schema_version, slug)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
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
        revision.schema_version,
        revision.slug
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    insert_references(tx, revision.id, revision.references).await?;
    Ok(created_at)
}

/// Keeps a new revision's references (migration 0024, D-065). A precheck names the first one
/// that isn't an entry of its collection, for the refusal; the foreign key on (entry,
/// collection) is the rule, so a race can only end in an error, never a dangling reference.
async fn insert_references(
    tx: &mut ProjectTransaction,
    revision: Uuid,
    references: &[ContentReference],
) -> Result<(), DbError> {
    if references.is_empty() {
        return Ok(());
    }
    let mut entries = Vec::with_capacity(references.len());
    for reference in references {
        let Ok(entry) = Uuid::try_parse(&reference.entry) else {
            return Err(missing(reference));
        };
        entries.push(entry);
    }
    let found = sqlx::query!(
        "SELECT id, collection FROM entries
         WHERE project_id = $1 AND environment_id = $2 AND id = ANY($3)
         FOR KEY SHARE",
        tx.project,
        tx.environment,
        &entries
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    for (reference, entry) in references.iter().zip(&entries) {
        if !found
            .iter()
            .any(|row| row.id == *entry && row.collection == reference.collection)
        {
            return Err(missing(reference));
        }
    }
    let fields: Vec<&str> = references.iter().map(|r| r.field.as_str()).collect();
    let positions: Vec<i16> = references
        .iter()
        .map(|r| i16::try_from(r.position).unwrap_or(i16::MAX))
        .collect();
    let collections: Vec<&str> = references.iter().map(|r| r.collection.as_str()).collect();
    sqlx::query!(
        "INSERT INTO revision_references
           (project_id, environment_id, revision_id, field, position, collection, entry_id)
         SELECT $1, $2, $3, field, position, collection, entry_id
         FROM unnest($4::text[], $5::int2[], $6::text[], $7::uuid[])
              AS t (field, position, collection, entry_id)",
        tx.project,
        tx.environment,
        revision,
        &fields as &[&str],
        &positions,
        &collections as &[&str],
        &entries
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

fn missing(reference: &ContentReference) -> DbError {
    DbError::MissingReference {
        field: reference.field.clone(),
        position: reference.position,
        many: reference.many,
    }
}

/// Another entry of `collection` with `slug`, as its draft's slug or its published one: what a
/// write of that slug would run into. Only for the refusal's wording, and racy like any precheck:
/// the indexes of migration 0016 are the rule (D-054). `except` is the entry being saved, which
/// may keep its own slugs.
pub async fn slug_holder(
    tx: &mut ProjectTransaction,
    collection: &str,
    slug: &str,
    except: Option<Uuid>,
) -> Result<Option<SlugHolder>, DbError> {
    sqlx::query_as!(
        SlugHolder,
        r#"SELECT e.id AS "entry_id!", r.title AS "title!", false AS "published!"
           FROM entries e
           JOIN revisions r ON r.project_id = e.project_id AND r.environment_id = e.environment_id
                           AND r.id = e.draft_revision_id
           WHERE e.project_id = $1 AND e.environment_id = $2 AND e.collection = $3
             AND e.draft_slug = $4 AND e.id IS DISTINCT FROM $5
           UNION ALL
           SELECT p.entry_id, p.title, true
           FROM publications p
           WHERE p.project_id = $1 AND p.environment_id = $2 AND p.collection = $3
             AND p.slug = $4 AND p.valid_to IS NULL AND p.entry_id IS DISTINCT FROM $5
           LIMIT 1"#,
        tx.project,
        tx.environment,
        collection,
        slug,
        except
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// The entry with its current draft, if it exists in this environment and collection.
pub async fn get(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry_id: Uuid,
) -> Result<Option<Draft>, DbError> {
    Ok(get_with_live(tx, collection, entry_id)
        .await?
        .map(|(draft, _)| draft))
}

/// [`get`], and what readers get now if the entry is published: its live revision and the
/// release it went out in. One statement, so the draft and its publication are read at the
/// same snapshot.
pub async fn get_with_live(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry_id: Uuid,
) -> Result<Option<(Draft, Option<(Uuid, i64)>)>, DbError> {
    let row = sqlx::query!(
        r#"SELECT e.collection, e.created_at, e.created_by, e.updated_at,
                r.id AS revision_id, r.base_revision_id, r.author_id, r.created_at AS revision_created_at,
                r.content_hash, r.title, r.content,
                live.revision_id AS "published_revision_id?", live.valid_from AS "published_in?"
         FROM entries e
         JOIN revisions r ON r.project_id = e.project_id AND r.environment_id = e.environment_id
                         AND r.id = e.draft_revision_id
         LEFT JOIN LATERAL (
             SELECT p.revision_id, p.valid_from
             FROM publications p
             WHERE p.project_id = e.project_id AND p.environment_id = e.environment_id
               AND p.entry_id = e.id AND p.valid_to IS NULL
             LIMIT 1
         ) live ON true
         WHERE e.project_id = $1 AND e.environment_id = $2 AND e.id = $3 AND e.collection = $4"#,
        tx.project,
        tx.environment,
        entry_id,
        collection
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(row.map(|row| {
        let draft = Draft {
            entry_id,
            collection: row.collection,
            created_at: row.created_at,
            created_by: row.created_by,
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
        };
        (draft, row.published_revision_id.zip(row.published_in))
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
    created_by: Option<Uuid>,
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
        r#"SELECT e.id, e.created_by, e.updated_at,
                r.id AS revision_id, r.base_revision_id, r.author_id, r.created_at AS revision_created_at,
                r.content_hash, r.title, live.revision_id AS "published_revision_id?",
                live.valid_from AS "published_in?"
         FROM entries e
         CROSS JOIN LATERAL (
             SELECT r.id, r.base_revision_id, r.author_id, r.created_at, r.content_hash, r.title
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
           AND ($7::uuid IS NULL OR e.created_by = $7)
         ORDER BY e.id DESC
         LIMIT $5"#,
        tx.project,
        tx.environment,
        collection,
        before,
        limit.clamp(1, MAX_PAGE + 1),
        pattern,
        created_by
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(rows
        .into_iter()
        .map(|row| Summary {
            entry_id: row.id,
            created_by: row.created_by,
            updated_at: row.updated_at,
            revision: Revision {
                id: row.revision_id,
                base_revision_id: row.base_revision_id,
                author_id: row.author_id,
                created_at: row.revision_created_at,
                content_hash: row.content_hash,
                title: row.title,
            },
            published: row.published_revision_id.zip(row.published_in),
        })
        .collect())
}

/// An entry whose draft or published revision names another (D-065).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Referrer {
    /// The entry that names it.
    pub entry_id: Uuid,
    /// Its collection.
    pub collection: String,
    /// The reference field that names it.
    pub field: String,
    /// Whether its draft names it.
    pub in_draft: bool,
    /// Whether what readers get of it names it.
    pub published: bool,
}

/// The most referrers [`referrers`] returns.
pub const MAX_REFERRERS: i64 = 100;

/// The entries whose draft or published revision names `entry`, by field, at most
/// [`MAX_REFERRERS`]: what an editor should know before changing or taking it down.
pub async fn referrers(tx: &mut ProjectTransaction, entry: Uuid) -> Result<Vec<Referrer>, DbError> {
    let rows = sqlx::query!(
        r#"SELECT entry_id AS "entry_id!", collection AS "collection!", field AS "field!",
                  bool_or(draft) AS "in_draft!", bool_or(live) AS "published!"
           FROM (
               SELECT e.id AS entry_id, e.collection, r.field, true AS draft, false AS live
               FROM revision_references r
               JOIN entries e ON e.project_id = r.project_id AND e.environment_id = r.environment_id
                             AND e.draft_revision_id = r.revision_id
               WHERE r.project_id = $1 AND r.environment_id = $2 AND r.entry_id = $3
               UNION ALL
               SELECT p.entry_id, p.collection, r.field, false, true
               FROM revision_references r
               JOIN publications p ON p.project_id = r.project_id AND p.environment_id = r.environment_id
                                  AND p.revision_id = r.revision_id AND p.valid_to IS NULL
               WHERE r.project_id = $1 AND r.environment_id = $2 AND r.entry_id = $3
           ) named
           GROUP BY entry_id, collection, field
           ORDER BY collection, entry_id, field
           LIMIT $4"#,
        tx.project,
        tx.environment,
        entry,
        MAX_REFERRERS
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(rows
        .into_iter()
        .map(|row| Referrer {
            entry_id: row.entry_id,
            collection: row.collection,
            field: row.field,
            in_draft: row.in_draft,
            published: row.published,
        })
        .collect())
}

/// Who created each of these entries in this environment, and their collections; entries that
/// don't exist are missing (D-058, step R.3: the own condition).
pub async fn creators(
    tx: &mut ProjectTransaction,
    entries: &[Uuid],
) -> Result<Vec<(Uuid, String, Uuid)>, DbError> {
    let rows = sqlx::query!(
        "SELECT id, collection, created_by FROM entries
         WHERE project_id = $1 AND environment_id = $2 AND id = ANY($3)",
        tx.project,
        tx.environment,
        entries
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(rows
        .into_iter()
        .map(|row| (row.id, row.collection, row.created_by))
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
