//! Changesets and reviews (docs/modules/01-domain.md, "Editorial workflow"; D-044). Everything
//! runs inside a [`ProjectTransaction`]; the API computes hashes (with
//! `colophon_domain::changeset::canonical`) and checks the policy. [`lock`] takes the changeset
//! row `FOR UPDATE`: refreshes and reviews both take it, so a review is checked against the hash
//! the changeset has when the review is stored.

use sqlx::types::time::OffsetDateTime;
use uuid::Uuid;

use crate::{DbError, scope::ProjectTransaction};

/// The most entries one changeset may hold (spec §14: a first release batch holds at most 100).
pub const MAX_ITEMS: usize = 100;

/// The most reviews a changeset view shows, newest first.
pub const MAX_REVIEWS: i64 = 100;

/// An entry's current revision, with everything a changeset pins about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    /// The entry.
    pub entry_id: Uuid,
    /// Its revision.
    pub revision_id: Uuid,
    /// Its collection.
    pub collection: String,
    /// The revision's content hash.
    pub content_hash: Vec<u8>,
    /// The content-type version the revision was validated against.
    pub schema_version: i32,
    /// That version's definition hash.
    pub definition_hash: Vec<u8>,
}

/// A changeset's own fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    /// Its id.
    pub id: Uuid,
    /// Its title.
    pub title: String,
    /// Its current hash.
    pub hash: Vec<u8>,
    /// Who made it.
    pub created_by: Uuid,
    /// When.
    pub created_at: OffsetDateTime,
    /// Who submitted it for review, if anyone has.
    pub submitted_by: Option<Uuid>,
    /// When.
    pub submitted_at: Option<OffsetDateTime>,
    /// When it last changed.
    pub updated_at: OffsetDateTime,
    /// The release it was published as, if it has been (D-045).
    pub published: Option<i64>,
}

/// One pinned entry, as a changeset view shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// What is pinned.
    pub pin: Pin,
    /// The pinned revision's title.
    pub title: String,
    /// The entry's current revision: different from the pinned one once someone edits it.
    pub current_revision_id: Uuid,
}

/// A recorded review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    /// Its id.
    pub id: Uuid,
    /// Who reviewed.
    pub reviewer_id: Uuid,
    /// Their name, if known.
    pub reviewer_name: Option<String>,
    /// `approve` or `request_changes`.
    pub decision: String,
    /// The changeset hash it was made on.
    pub changeset_hash: Vec<u8>,
    /// Its place in the changeset's decisions: 1 for the first, increasing by one.
    pub seq: i32,
    /// What the reviewer wrote.
    pub comment: String,
    /// When.
    pub created_at: OffsetDateTime,
}

/// A changeset in a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// Its fields.
    pub head: Head,
    /// How many entries it pins.
    pub items: i64,
    /// How many of them have been edited since they were pinned.
    pub stale: i64,
    /// The newest review on its current hash.
    pub latest: Option<Review>,
}

/// The current revisions of `entries` in the transaction's environment, with what a changeset
/// pins about them. Entries that don't exist here are left out.
pub async fn pins(tx: &mut ProjectTransaction, entries: &[Uuid]) -> Result<Vec<Pin>, DbError> {
    sqlx::query_as!(
        Pin,
        "SELECT e.id AS entry_id, r.id AS revision_id, r.collection, r.content_hash, r.schema_version,
                v.definition_hash
         FROM entries e
         JOIN revisions r ON r.project_id = e.project_id AND r.environment_id = e.environment_id
                         AND r.id = e.draft_revision_id
         JOIN content_type_versions v ON v.project_id = r.project_id AND v.environment_id = r.environment_id
                                     AND v.collection = r.collection AND v.version = r.schema_version
         WHERE e.project_id = $1 AND e.environment_id = $2 AND e.id = ANY($3)",
        tx.project,
        tx.environment,
        entries
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Stores a new changeset pinning `pins`, with the hash the caller computed from them.
pub async fn create(
    tx: &mut ProjectTransaction,
    id: Uuid,
    title: &str,
    pins: &[Pin],
    hash: &[u8; 32],
) -> Result<(), DbError> {
    sqlx::query!(
        "INSERT INTO changesets (project_id, environment_id, id, title, hash, created_by)
         VALUES ($1, $2, $3, $4, $5, $6)",
        tx.project,
        tx.environment,
        id,
        title,
        &hash[..],
        tx.identity
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    let entries: Vec<Uuid> = pins.iter().map(|pin| pin.entry_id).collect();
    let revisions: Vec<Uuid> = pins.iter().map(|pin| pin.revision_id).collect();
    sqlx::query!(
        "INSERT INTO changeset_items (project_id, environment_id, changeset_id, entry_id, revision_id)
         SELECT $1, $2, $3, entry, revision FROM unnest($4::uuid[], $5::uuid[]) AS pinned (entry, revision)",
        tx.project,
        tx.environment,
        id,
        &entries,
        &revisions
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

fn head(
    id: Uuid,
    title: String,
    hash: Vec<u8>,
    created_by: Uuid,
    created_at: OffsetDateTime,
    submitted: (Option<Uuid>, Option<OffsetDateTime>),
    changed: (OffsetDateTime, Option<i64>),
) -> Head {
    Head {
        id,
        title,
        hash,
        created_by,
        created_at,
        submitted_by: submitted.0,
        submitted_at: submitted.1,
        updated_at: changed.0,
        published: changed.1,
    }
}

/// The changeset, read without a lock.
pub async fn get(tx: &mut ProjectTransaction, id: Uuid) -> Result<Option<Head>, DbError> {
    let row = sqlx::query!(
        r#"SELECT c.title, c.hash, c.created_by, c.created_at, c.submitted_by, c.submitted_at, c.updated_at,
                  r.generation AS "published?"
           FROM changesets c
           LEFT JOIN releases r ON r.project_id = c.project_id AND r.environment_id = c.environment_id
                               AND r.changeset_id = c.id
           WHERE c.project_id = $1 AND c.environment_id = $2 AND c.id = $3"#,
        tx.project,
        tx.environment,
        id
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(row.map(|row| {
        head(
            id,
            row.title,
            row.hash,
            row.created_by,
            row.created_at,
            (row.submitted_by, row.submitted_at),
            (row.updated_at, row.published),
        )
    }))
}

/// The changeset, locked until the transaction ends: refreshes and reviews queue on it.
pub async fn lock(tx: &mut ProjectTransaction, id: Uuid) -> Result<Option<Head>, DbError> {
    let row = sqlx::query!(
        r#"SELECT c.title, c.hash, c.created_by, c.created_at, c.submitted_by, c.submitted_at, c.updated_at,
                  r.generation AS "published?"
           FROM changesets c
           LEFT JOIN releases r ON r.project_id = c.project_id AND r.environment_id = c.environment_id
                               AND r.changeset_id = c.id
           WHERE c.project_id = $1 AND c.environment_id = $2 AND c.id = $3
           FOR UPDATE OF c"#,
        tx.project,
        tx.environment,
        id
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(row.map(|row| {
        head(
            id,
            row.title,
            row.hash,
            row.created_by,
            row.created_at,
            (row.submitted_by, row.submitted_at),
            (row.updated_at, row.published),
        )
    }))
}

/// The changeset's items, each with its entry's current revision, ordered by entry id.
pub async fn items(tx: &mut ProjectTransaction, id: Uuid) -> Result<Vec<Item>, DbError> {
    let rows = sqlx::query!(
        "SELECT i.entry_id, i.revision_id, r.collection, r.content_hash, r.schema_version,
                v.definition_hash, r.title, e.draft_revision_id
         FROM changeset_items i
         JOIN revisions r ON r.project_id = i.project_id AND r.environment_id = i.environment_id
                         AND r.id = i.revision_id
         JOIN content_type_versions v ON v.project_id = r.project_id AND v.environment_id = r.environment_id
                                     AND v.collection = r.collection AND v.version = r.schema_version
         JOIN entries e ON e.project_id = i.project_id AND e.environment_id = i.environment_id
                       AND e.id = i.entry_id
         WHERE i.project_id = $1 AND i.environment_id = $2 AND i.changeset_id = $3
         ORDER BY i.entry_id",
        tx.project,
        tx.environment,
        id
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(rows
        .into_iter()
        .map(|row| Item {
            pin: Pin {
                entry_id: row.entry_id,
                revision_id: row.revision_id,
                collection: row.collection,
                content_hash: row.content_hash,
                schema_version: row.schema_version,
                definition_hash: row.definition_hash,
            },
            title: row.title,
            current_revision_id: row.draft_revision_id,
        })
        .collect())
}

/// Re-pins the changeset's entries to `pins` (their current revisions) and stores the new hash.
/// The caller holds [`lock`].
pub async fn repin(
    tx: &mut ProjectTransaction,
    id: Uuid,
    pins: &[Pin],
    hash: &[u8; 32],
) -> Result<(), DbError> {
    let entries: Vec<Uuid> = pins.iter().map(|pin| pin.entry_id).collect();
    let revisions: Vec<Uuid> = pins.iter().map(|pin| pin.revision_id).collect();
    sqlx::query!(
        "UPDATE changeset_items i SET revision_id = pinned.revision
         FROM unnest($4::uuid[], $5::uuid[]) AS pinned (entry, revision)
         WHERE i.project_id = $1 AND i.environment_id = $2 AND i.changeset_id = $3
           AND i.entry_id = pinned.entry AND i.revision_id <> pinned.revision",
        tx.project,
        tx.environment,
        id,
        &entries,
        &revisions
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    sqlx::query!(
        "UPDATE changesets SET hash = $4, updated_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND id = $3",
        tx.project,
        tx.environment,
        id,
        &hash[..]
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(())
}

/// Marks the changeset submitted by the transaction's identity; a no-op if it already is.
pub async fn submit(tx: &mut ProjectTransaction, id: Uuid) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE changesets SET submitted_by = $4, submitted_at = now(), updated_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND submitted_by IS NULL",
        tx.project,
        tx.environment,
        id,
        tx.identity
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(())
}

/// Records the transaction's identity's decision on the changeset with `hash`, as its next
/// decision in sequence. The caller holds [`lock`] and has checked that `hash` is current.
pub async fn review(
    tx: &mut ProjectTransaction,
    id: Uuid,
    hash: &[u8],
    decision: &str,
    comment: &str,
) -> Result<(), DbError> {
    sqlx::query!(
        "INSERT INTO reviews (project_id, environment_id, id, changeset_id, changeset_hash, seq, reviewer_id, decision, comment)
         SELECT $1, $2, $3, $4, $5, coalesce(max(seq), 0) + 1, $6, $7, $8
         FROM reviews WHERE project_id = $1 AND environment_id = $2 AND changeset_id = $4",
        tx.project,
        tx.environment,
        Uuid::now_v7(),
        id,
        hash,
        tx.identity,
        decision,
        comment
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// The changeset's reviews, newest first, at most [`MAX_REVIEWS`]; only those on `hash` when
/// given.
pub async fn reviews(
    tx: &mut ProjectTransaction,
    id: Uuid,
    hash: Option<&[u8]>,
) -> Result<Vec<Review>, DbError> {
    sqlx::query_as!(
        Review,
        r#"SELECT v.id, v.reviewer_id, i.display_name AS reviewer_name, v.decision,
                  v.changeset_hash, v.seq, v.comment, v.created_at
           FROM reviews v JOIN identities i ON i.id = v.reviewer_id
           WHERE v.project_id = $1 AND v.environment_id = $2 AND v.changeset_id = $3
             AND ($4::bytea IS NULL OR v.changeset_hash = $4)
           ORDER BY v.seq DESC
           LIMIT $5"#,
        tx.project,
        tx.environment,
        id,
        hash,
        MAX_REVIEWS
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// The environment's changesets, newest first (at most 100), each with its counts and the newest
/// review on its current hash.
///
/// Planned afresh on every run (`plan_cache_mode`, for this transaction only): its lookups per
/// changeset are only fast with indexes, and a generic plan Postgres settles on while the tables
/// are nearly empty (a full scan of `reviews` for each changeset) would be kept as they grow,
/// until the next `ANALYZE`. The first CI runs of step 2.4 measured exactly that: the list slowed
/// to 200 ms within a 40-second load test as reviews grew from a few to 3,000.
pub async fn list(tx: &mut ProjectTransaction) -> Result<Vec<Summary>, DbError> {
    sqlx::query("SET LOCAL plan_cache_mode = force_custom_plan")
        .execute(&mut *tx.transaction)
        .await
        .map_err(DbError::from_query)?;
    let rows = sqlx::query!(
        r#"SELECT c.id, c.title, c.hash, c.created_by, c.created_at, c.submitted_by, c.submitted_at,
                  c.updated_at, rel.generation AS "published?",
                  counts.items AS "items!", counts.stale AS "stale!",
                  latest.id AS "review_id?", latest.reviewer_id AS "reviewer_id?",
                  latest.reviewer_name, latest.decision AS "decision?", latest.seq AS "seq?",
                  latest.comment AS "comment?", latest.created_at AS "reviewed_at?"
           FROM changesets c
           CROSS JOIN LATERAL (
               SELECT count(*) AS items, count(*) FILTER (WHERE e.draft_revision_id <> i.revision_id) AS stale
               FROM changeset_items i
               JOIN entries e ON e.project_id = i.project_id AND e.environment_id = i.environment_id
                             AND e.id = i.entry_id
               WHERE i.project_id = c.project_id AND i.environment_id = c.environment_id
                 AND i.changeset_id = c.id
           ) counts
           LEFT JOIN LATERAL (
               SELECT v.id, v.reviewer_id, n.display_name AS reviewer_name, v.decision, v.seq, v.comment,
                      v.created_at
               FROM reviews v JOIN identities n ON n.id = v.reviewer_id
               WHERE v.project_id = c.project_id AND v.environment_id = c.environment_id
                 AND v.changeset_id = c.id AND v.changeset_hash = c.hash
               ORDER BY v.seq DESC
               LIMIT 1
           ) latest ON true
           LEFT JOIN releases rel ON rel.project_id = c.project_id AND rel.environment_id = c.environment_id
                                 AND rel.changeset_id = c.id
           WHERE c.project_id = $1 AND c.environment_id = $2
           ORDER BY c.id DESC
           LIMIT 100"#,
        tx.project,
        tx.environment
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let latest = match (
                row.review_id,
                row.reviewer_id,
                row.decision,
                row.seq,
                row.comment,
                row.reviewed_at,
            ) {
                (
                    Some(id),
                    Some(reviewer_id),
                    Some(decision),
                    Some(seq),
                    Some(comment),
                    Some(created_at),
                ) => Some(Review {
                    id,
                    reviewer_id,
                    reviewer_name: row.reviewer_name,
                    decision,
                    changeset_hash: row.hash.clone(),
                    seq,
                    comment,
                    created_at,
                }),
                _ => None,
            };
            Summary {
                head: head(
                    row.id,
                    row.title,
                    row.hash,
                    row.created_by,
                    row.created_at,
                    (row.submitted_by, row.submitted_at),
                    (row.updated_at, row.published),
                ),
                items: row.items,
                stale: row.stale,
                latest,
            }
        })
        .collect())
}
