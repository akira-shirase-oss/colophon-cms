//! Entries and their revisions (docs/modules/02-db.md, "Transactions"). Every function works
//! inside a [`ProjectTransaction`], so the caller is a member and row-level security confines
//! it to the project.

use colophon_domain::schema::{ContentAsset, ContentReference};
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
    /// The uploads its asset fields name (migration 0036, media M1).
    pub assets: &'a [ContentAsset],
    /// The ids of its content type's unique fields (D-070): the entry claims the values the
    /// content sets in them. The caller's shared lock on the content type keeps them current.
    pub unique: &'a [String],
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
    /// Who created the entry: whether a refusal may name it depends on it (step R.3).
    pub created_by: Uuid,
}

/// The entry that holds a value of a unique field, as a draft or as what readers get
/// ([`unique_holder`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueHolder {
    /// The field.
    pub field: String,
    /// The entry.
    pub entry_id: Uuid,
    /// Its title, in the revision that holds the value.
    pub title: String,
    /// Whether that revision is the published one rather than the draft.
    pub published: bool,
    /// Who created the entry: whether a refusal may name it depends on it (step R.3).
    pub created_by: Uuid,
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
    /// The entry's locale tag, or `None` for the project's default (D-087).
    pub locale: Option<String>,
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
    /// The entry's locale tag, or `None` for the project's default (D-087).
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
    locale: Option<&str>,
) -> Result<Draft, DbError> {
    let entry = sqlx::query!(
        "INSERT INTO entries (project_id, environment_id, id, collection, draft_revision_id, draft_slug, draft_title, created_by, single, locale)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8,
                 -- Single types (D-067): the current version's flag, held still by the caller's
                 -- shared lock on the content type. `entries_single` refuses a second entry.
                 coalesce((SELECT (v.definition::jsonb ->> 'single')::boolean
                           FROM content_type_versions v
                           WHERE v.project_id = $1 AND v.environment_id = $2 AND v.collection = $4
                           ORDER BY v.version DESC LIMIT 1), false),
                 $9)
         RETURNING created_at, updated_at",
        tx.project,
        tx.environment,
        entry_id,
        collection,
        revision.id,
        revision.slug,
        revision.title,
        tx.identity,
        locale
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    let created_at = insert_revision(tx, entry_id, collection, None, revision).await?;
    claim_draft(tx, entry_id, collection, revision).await?;
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
        locale: locale.map(str::to_owned),
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
        "SELECT draft_revision_id, collection, created_at, created_by, locale FROM entries
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
    claim_draft(tx, entry_id, &current.collection, revision).await?;
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
        locale: current.locale,
    })))
}

/// What a deletion did (D-080). Every refusal writes nothing; the caller rolls back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Deleted {
    /// The entry is a tombstone now.
    Deleted {
        /// The draft it had: the revision the caller confirmed.
        revision: Uuid,
        /// When.
        at: OffsetDateTime,
    },
    /// No such live entry in this environment and collection.
    NotFound,
    /// The draft is no longer the expected revision.
    Stale {
        /// The revision it points at now.
        current: Uuid,
    },
    /// Readers get it: it must be unpublished first.
    Published,
    /// Another live entry's draft, or a publication, still names it: these, at most
    /// [`MAX_REFERRERS`] (possibly none, if they changed since the deletion looked).
    Referenced(Vec<Referrer>),
    /// Changesets that haven't been published pin it: these, at most [`MAX_PENDING`].
    InChangeset(Vec<Pending>),
}

/// A changeset that hasn't been published yet and pins an entry ([`Deleted::InChangeset`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// The changeset.
    pub id: Uuid,
    /// Its title.
    pub title: String,
    /// Whether a schedule will publish it.
    pub scheduled: bool,
}

/// The most changesets a refused deletion names.
pub const MAX_PENDING: i64 = 20;

/// Deletes `entry` of `collection` as the transaction's identity, if its draft is still
/// `expected` and nothing depends on it (migration 0030, D-080): it becomes a tombstone that the
/// application's role no longer sees, its slug, unique values and single-type place come free,
/// and the audit log records it. Revisions, history and releases stay as they were. The caller has
/// checked that its role may delete this entry.
///
/// Holds the collection's content-type lock shared first, as saves do (D-055), so a content type
/// change that claims unique values never claims one for an entry being deleted. The checks and
/// the tombstone are one call to `colophon_delete_entry`, under a row lock on the entry; the names
/// in a refusal are read after it, in the same transaction.
pub async fn delete(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry: Uuid,
    expected: Uuid,
) -> Result<Deleted, DbError> {
    crate::content_types::lock_for_write(tx, collection).await?;
    let row = sqlx::query!(
        r#"SELECT outcome AS "outcome!", draft_revision, deleted
           FROM colophon_delete_entry($1, $2, $3, $4, $5, $6)"#,
        tx.project,
        tx.environment,
        entry,
        collection,
        expected,
        Uuid::now_v7()
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    let corrupt = DbError::Corrupt("colophon_delete_entry answered without its draft");
    Ok(match row.outcome.as_str() {
        "deleted" => Deleted::Deleted {
            revision: row.draft_revision.ok_or(corrupt)?,
            at: row
                .deleted
                .ok_or(DbError::Corrupt("a deletion answered without its time"))?,
        },
        "not_found" => Deleted::NotFound,
        "stale" => Deleted::Stale {
            current: row.draft_revision.ok_or(corrupt)?,
        },
        "published" => Deleted::Published,
        "referenced" => Deleted::Referenced(
            referrers(tx, entry)
                .await?
                .into_iter()
                .filter(|referrer| referrer.entry_id != entry)
                .collect(),
        ),
        "in_changeset" => Deleted::InChangeset(pending(tx, entry).await?),
        _ => {
            return Err(DbError::Corrupt(
                "colophon_delete_entry gave an unknown outcome",
            ));
        }
    })
}

/// The changesets that pin `entry` and haven't been published, newest first, at most
/// [`MAX_PENDING`].
async fn pending(tx: &mut ProjectTransaction, entry: Uuid) -> Result<Vec<Pending>, DbError> {
    sqlx::query_as!(
        Pending,
        r#"SELECT c.id, c.title,
                  EXISTS (SELECT 1 FROM schedules s
                          WHERE s.project_id = c.project_id AND s.environment_id = c.environment_id
                            AND s.changeset_id = c.id AND s.state = 'scheduled') AS "scheduled!"
           FROM changeset_items i
           JOIN changesets c ON c.project_id = i.project_id AND c.environment_id = i.environment_id
                            AND c.id = i.changeset_id
           WHERE i.project_id = $1 AND i.environment_id = $2 AND i.entry_id = $3
             AND NOT EXISTS (SELECT 1 FROM releases r
                             WHERE r.project_id = c.project_id AND r.environment_id = c.environment_id
                               AND r.changeset_id = c.id)
           ORDER BY c.id DESC
           LIMIT $4"#,
        tx.project,
        tx.environment,
        entry,
        MAX_PENDING
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
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
    insert_assets(tx, revision.id, revision.assets).await?;
    Ok(created_at)
}

/// Makes the entry's draft claims the values `revision` sets in its unique fields (migration
/// 0027, D-070), replacing the old draft's. A value another entry's draft holds fails
/// `unique_claims_value`: `ValueTaken`, and the caller rolls back. Claims exist only for the
/// fields the current version marks unique, so a type without any has none to replace.
async fn claim_draft(
    tx: &mut ProjectTransaction,
    entry_id: Uuid,
    collection: &str,
    revision: NewRevision<'_>,
) -> Result<(), DbError> {
    if revision.unique.is_empty() {
        return Ok(());
    }
    sqlx::query!(
        "DELETE FROM unique_claims
         WHERE project_id = $1 AND environment_id = $2 AND entry_id = $3 AND NOT published",
        tx.project,
        tx.environment,
        entry_id
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    sqlx::query!(
        "INSERT INTO unique_claims
           (project_id, environment_id, collection, field, published, value_hash, entry_id)
         SELECT $1, $2, $3, field, false, value_hash, $4
         FROM colophon_unique_values($5, $6)",
        tx.project,
        tx.environment,
        collection,
        entry_id,
        revision.unique,
        revision.content
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
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
    let inserted = sqlx::query!(
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
    .await;
    match inserted {
        Ok(_) => Ok(()),
        // An entry deleted after the precheck read it (migration 0030's trigger, D-080): refused
        // like any reference to an entry that isn't there, at the first place that names it.
        Err(error) => match crate::deleted_entry(&error) {
            Some(("revision_references", gone)) => references
                .iter()
                .zip(&entries)
                .find(|(_, entry)| gone.is_none_or(|gone| **entry == gone))
                .map(|(reference, _)| reference)
                .or_else(|| references.first())
                .map_or_else(
                    || Err(DbError::from_write(error)),
                    |named| Err(missing(named)),
                ),
            _ => Err(DbError::from_write(error)),
        },
    }
}

fn missing(reference: &ContentReference) -> DbError {
    DbError::MissingReference {
        field: reference.field.clone(),
        position: reference.position,
        many: reference.many,
    }
}

fn missing_asset(asset: &ContentAsset) -> DbError {
    DbError::MissingAsset {
        field: asset.field.clone(),
        position: asset.position,
        many: asset.many,
    }
}

/// Keeps a new revision's asset references (migration 0036, media M1), as [`insert_references`]
/// keeps its entries: a precheck names the first id that isn't an asset of this environment for
/// the refusal, and the foreign key is the rule — a draft may hold a quarantined or rejected
/// asset, but never one that doesn't exist. No `FOR KEY SHARE` here as on `entries`: the API
/// role may not update `assets` (state changes are the worker's, migration 0035) so it cannot
/// take the lock itself — migration 0039's `asset_references_asset_live` trigger takes it for
/// it instead, refusing a name for an asset that is `deleting`, `deleted` or on a delete job's
/// way the same way `ON DELETE RESTRICT` would.
async fn insert_assets(
    tx: &mut ProjectTransaction,
    revision: Uuid,
    assets: &[ContentAsset],
) -> Result<(), DbError> {
    if assets.is_empty() {
        return Ok(());
    }
    let mut ids = Vec::with_capacity(assets.len());
    for asset in assets {
        let Ok(id) = Uuid::try_parse(&asset.asset) else {
            return Err(missing_asset(asset));
        };
        ids.push(id);
    }
    let found = sqlx::query!(
        "SELECT id FROM assets
         WHERE project_id = $1 AND environment_id = $2 AND id = ANY($3)",
        tx.project,
        tx.environment,
        &ids
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    for (asset, id) in assets.iter().zip(&ids) {
        if !found.iter().any(|row| row.id == *id) {
            return Err(missing_asset(asset));
        }
    }
    let fields: Vec<&str> = assets.iter().map(|a| a.field.as_str()).collect();
    let positions: Vec<i16> = assets
        .iter()
        .map(|a| i16::try_from(a.position).unwrap_or(i16::MAX))
        .collect();
    sqlx::query!(
        "INSERT INTO asset_references
           (project_id, environment_id, revision_id, field, position, asset_id)
         SELECT $1, $2, $3, field, position, asset_id
         FROM unnest($4::text[], $5::int2[], $6::uuid[])
              AS t (field, position, asset_id)",
        tx.project,
        tx.environment,
        revision,
        &fields as &[&str],
        &positions,
        &ids
    )
    .execute(&mut *tx.transaction)
    .await
    // An upload deleted between the precheck and the insert fails the foreign key, as does
    // one whose deletion is committed or on the way (migration 0039's trigger): refused like
    // any name for an asset that isn't there, at the place that names it.
    .map_err(|error| match crate::deleted_asset(&error) {
        Some(gone) => assets
            .iter()
            .zip(&ids)
            .find(|(_, id)| **id == gone)
            .map(|(asset, _)| asset)
            .or_else(|| assets.first())
            .map_or_else(|| DbError::from_write(error), missing_asset),
        None => match error.as_database_error() {
            Some(db) if db.is_foreign_key_violation() => assets
                .first()
                .map_or_else(|| DbError::from_write(error), missing_asset),
            _ => DbError::from_write(error),
        },
    })?;
    Ok(())
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
        r#"SELECT e.id AS "entry_id!", r.title AS "title!", false AS "published!",
                  e.created_by AS "created_by!"
           FROM entries e
           JOIN revisions r ON r.project_id = e.project_id AND r.environment_id = e.environment_id
                           AND r.id = e.draft_revision_id
           WHERE e.project_id = $1 AND e.environment_id = $2 AND e.collection = $3
             AND e.draft_slug = $4 AND e.id IS DISTINCT FROM $5
           UNION ALL
           SELECT p.entry_id, p.title, true, e.created_by
           FROM publications p
           JOIN entries e ON e.project_id = p.project_id AND e.environment_id = p.environment_id
                         AND e.id = p.entry_id
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

/// Another entry of `collection` holding a value `content` sets in one of `fields`, as its
/// draft or published: what a write of `content` would run into, the first field first. Only
/// for the refusal's wording, and racy like any precheck: `unique_claims_value` is the rule
/// (D-070). `except` is the entry being saved, which may keep its own values.
pub async fn unique_holder(
    tx: &mut ProjectTransaction,
    collection: &str,
    fields: &[String],
    content: &str,
    except: Option<Uuid>,
) -> Result<Option<UniqueHolder>, DbError> {
    if fields.is_empty() {
        return Ok(None);
    }
    sqlx::query_as!(
        UniqueHolder,
        r#"SELECT v.field AS "field!", c.entry_id AS "entry_id!", c.published AS "published!",
                  coalesce(CASE WHEN c.published
                               THEN (SELECT p.title FROM publications p
                                     WHERE p.project_id = c.project_id
                                       AND p.environment_id = c.environment_id
                                       AND p.entry_id = c.entry_id AND p.valid_to IS NULL)
                               ELSE e.draft_title END, '') AS "title!",
                  e.created_by AS "created_by!"
           FROM colophon_unique_values($4, $5) v
           JOIN unique_claims c
             ON c.project_id = $1 AND c.environment_id = $2 AND c.collection = $3
            AND c.field = v.field AND c.value_hash = v.value_hash
            AND c.entry_id IS DISTINCT FROM $6
           JOIN entries e
             ON e.project_id = c.project_id AND e.environment_id = c.environment_id
            AND e.id = c.entry_id
           ORDER BY array_position($4, v.field), c.published
           LIMIT 1"#,
        tx.project,
        tx.environment,
        collection,
        fields,
        content,
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
        r#"SELECT e.collection, e.created_at, e.created_by, e.updated_at, e.locale,
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
            locale: row.locale,
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
        r#"SELECT e.id, e.created_by, e.updated_at, e.locale,
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
           AND ($8::text IS NULL OR e.locale = $8)
         ORDER BY e.id DESC
         LIMIT $5"#,
        tx.project,
        tx.environment,
        collection,
        before,
        limit.clamp(1, MAX_PAGE + 1),
        pattern,
        created_by,
        locale
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
            locale: row.locale,
        })
        .collect())
}

/// Every draft's canonical content, newest first — the lines of a collection's export (D-086),
/// of one creator's entries when `created_by` says whose. Capped at 10,000 rows so an export
/// stays a bounded read.
pub async fn export(
    tx: &mut ProjectTransaction,
    collection: &str,
    created_by: Option<Uuid>,
) -> Result<Vec<String>, DbError> {
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
             AND ($4::uuid IS NULL OR e.created_by = $4)
           ORDER BY e.created_at DESC, e.id DESC
           LIMIT 10000"#,
        tx.project,
        tx.environment,
        collection,
        created_by
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(rows.into_iter().map(|row| row.content).collect())
}

/// What setting an entry's locale did (D-087). Every refusal writes nothing; the caller rolls
/// back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetLocale {
    /// The locale was stored.
    Set,
    /// Another member of the entry's group already has that locale: its id.
    Occupied(Uuid),
    /// The entry is in a group; a grouped entry can't lose its locale (the CHECK on
    /// `localization_id` requires one). It leaves the default only by deletion.
    Grouped,
    /// No live entry has that id in the collection.
    Missing,
}

/// Stores an entry's locale label — `None` clears it back to the project's default. A grouped
/// entry keeps a locale (the migration's CHECK), and one it changes to must be free in its
/// group; both rules hold in the same transaction a save made its revision in.
pub async fn set_locale(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry_id: Uuid,
    locale: Option<&str>,
) -> Result<SetLocale, DbError> {
    // Lock the entry like a save does, so a localize or another locale write queues here.
    let group = sqlx::query!(
        "SELECT localization_id FROM entries
         WHERE project_id = $1 AND environment_id = $2 AND collection = $3
           AND id = $4 AND deleted_at IS NULL
         FOR UPDATE",
        tx.project,
        tx.environment,
        collection,
        entry_id
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?
    .map(|row| row.localization_id);
    let Some(group) = group else {
        return Ok(SetLocale::Missing);
    };
    if locale.is_none() && group.is_some() {
        return Ok(SetLocale::Grouped);
    }
    if let (Some(group), Some(locale)) = (group, locale) {
        let occupied = sqlx::query!(
            "SELECT id FROM entries
             WHERE project_id = $1 AND environment_id = $2 AND localization_id = $3
               AND locale = $4 AND id <> $5 AND deleted_at IS NULL",
            tx.project,
            tx.environment,
            group,
            locale,
            entry_id
        )
        .fetch_optional(&mut *tx.transaction)
        .await
        .map_err(DbError::from_query)?;
        if let Some(row) = occupied {
            return Ok(SetLocale::Occupied(row.id));
        }
    }
    sqlx::query!(
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
    Ok(SetLocale::Set)
}

/// A sibling entry in a localization group (D-087).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Localization {
    /// The sibling's entry id.
    pub id: Uuid,
    /// Its locale tag — never `None` (the group's CHECK requires one).
    pub locale: Option<String>,
}

/// Every live entry in an entry's localization group, including the entry itself, ordered by
/// locale: what an editor sees as the document's translations. Empty when it is unlocalized —
/// an unlocalized entry is its own version, not a group of one.
pub async fn localizations(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry_id: Uuid,
) -> Result<Vec<Localization>, DbError> {
    let rows = sqlx::query!(
        "SELECT s.id, s.locale FROM entries s
         JOIN entries e ON e.project_id = s.project_id AND e.environment_id = s.environment_id
                       AND e.localization_id = s.localization_id
         WHERE e.project_id = $1 AND e.environment_id = $2 AND e.collection = $3
           AND e.id = $4 AND e.deleted_at IS NULL AND s.deleted_at IS NULL
         ORDER BY s.locale",
        tx.project,
        tx.environment,
        collection,
        entry_id
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(rows
        .into_iter()
        .map(|row| Localization {
            id: row.id,
            locale: row.locale,
        })
        .collect())
}

/// What a localization write found (D-087). The refusals write nothing; the caller rolls back.
#[derive(Debug)]
pub enum Localized {
    /// The sibling was made and the source's group holds them both.
    Made(Box<Draft>),
    /// The locale is already held by the named group member.
    Occupied(Uuid),
    /// The source is missing or tombstoned.
    Missing,
}

/// The state a sibling copy needs from its source (D-087).
#[derive(Debug, Clone)]
pub struct SourceRow {
    /// The source's group, if it already has one — `None` means the localize creates it.
    pub localization_id: Option<Uuid>,
    /// The source's locale tag: `None` means it can't join a group until one is named — the
    /// CHECK on `localization_id` requires every member tagged.
    pub locale: Option<String>,
    /// The source draft's canonical content: the sibling's seed, before the caller's rewrite.
    pub content: String,
}

/// Reads and locks the source of a localization: its group, its tag and its draft's content.
/// The lock makes the group's first member queue here rather than race the unique index.
pub async fn source_for_localization(
    tx: &mut ProjectTransaction,
    collection: &str,
    source_id: Uuid,
) -> Result<Option<SourceRow>, DbError> {
    sqlx::query_as!(
        SourceRow,
        "SELECT e.localization_id, e.locale, r.content
         FROM entries e
         JOIN revisions r ON r.project_id = e.project_id AND r.environment_id = e.environment_id
                         AND r.id = e.draft_revision_id
         WHERE e.project_id = $1 AND e.environment_id = $2 AND e.collection = $3
           AND e.id = $4 AND e.deleted_at IS NULL
         FOR UPDATE OF e",
        tx.project,
        tx.environment,
        collection,
        source_id
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Inserts the sibling a localize prepared: gives the source a group if it has none, checks the
/// locale is free in it, then writes the entry and its first revision — the same statements
/// [`create`] runs, with the group attached.
pub async fn create_localization(
    tx: &mut ProjectTransaction,
    entry_id: Uuid,
    source_id: Uuid,
    collection: &str,
    locale: &str,
    revision: NewRevision<'_>,
) -> Result<Localized, DbError> {
    // The source was locked by `source_for_localization`; COALESCE keeps a group a concurrent
    // localize won while this one prepared its copy.
    let candidate = Uuid::now_v7();
    let group = sqlx::query!(
        "UPDATE entries SET localization_id = coalesce(localization_id, $5)
         WHERE project_id = $1 AND environment_id = $2 AND collection = $3
           AND id = $4 AND deleted_at IS NULL
         RETURNING localization_id",
        tx.project,
        tx.environment,
        collection,
        source_id,
        candidate
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    let Some(assigned) = group else {
        return Ok(Localized::Missing);
    };
    let group = assigned.localization_id.unwrap_or(candidate);
    let occupied = sqlx::query!(
        "SELECT id FROM entries
         WHERE project_id = $1 AND environment_id = $2 AND localization_id = $3
           AND locale = $4 AND deleted_at IS NULL",
        tx.project,
        tx.environment,
        group,
        locale
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    if let Some(row) = occupied {
        return Ok(Localized::Occupied(row.id));
    }
    let entry = sqlx::query!(
        "INSERT INTO entries (project_id, environment_id, id, collection, draft_revision_id, draft_slug, draft_title, created_by, single, locale, localization_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8,
                 coalesce((SELECT (v.definition::jsonb ->> 'single')::boolean
                           FROM content_type_versions v
                           WHERE v.project_id = $1 AND v.environment_id = $2 AND v.collection = $4
                           ORDER BY v.version DESC LIMIT 1), false),
                 $9, $10)
         RETURNING created_at, updated_at",
        tx.project,
        tx.environment,
        entry_id,
        collection,
        revision.id,
        revision.slug,
        revision.title,
        tx.identity,
        Some(locale),
        group
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    let created_at = insert_revision(tx, entry_id, collection, None, revision).await?;
    claim_draft(tx, entry_id, collection, revision).await?;
    Ok(Localized::Made(Box::new(Draft {
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
        locale: Some(locale.to_owned()),
    })))
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
/// [`MAX_REFERRERS`]: what an editor should know before changing or taking it down. Each
/// reference finds its entry and its open publication by revision (migration 0028), one lookup
/// apiece: a revision is at most one entry's draft and has at most one open publication, and
/// the `LIMIT 1` keeps a generic plan from scanning the environment's entries per reference.
pub async fn referrers(tx: &mut ProjectTransaction, entry: Uuid) -> Result<Vec<Referrer>, DbError> {
    let rows = sqlx::query!(
        r#"SELECT entry_id AS "entry_id!", collection AS "collection!", field AS "field!",
                  bool_or(draft) AS "in_draft!", bool_or(live) AS "published!"
           FROM (
               SELECT e.id AS entry_id, e.collection, r.field, true AS draft, false AS live
               FROM revision_references r
               CROSS JOIN LATERAL (
                   SELECT e.id, e.collection FROM entries e
                   WHERE e.project_id = r.project_id AND e.environment_id = r.environment_id
                     AND e.draft_revision_id = r.revision_id
                   LIMIT 1
               ) e
               WHERE r.project_id = $1 AND r.environment_id = $2 AND r.entry_id = $3
               UNION ALL
               SELECT p.entry_id, p.collection, r.field, false, true
               FROM revision_references r
               CROSS JOIN LATERAL (
                   SELECT p.entry_id, p.collection FROM publications p
                   WHERE p.project_id = r.project_id AND p.environment_id = r.environment_id
                     AND p.revision_id = r.revision_id AND p.valid_to IS NULL
                   LIMIT 1
               ) p
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
