//! Releases (docs/modules/01-domain.md, "Releases"; D-045). [`commit`] is the whole release in
//! one transaction: the conditional generation bump, the release record, the publication
//! history, tombstones and revocations for what it unpublishes (D-049), and the outbox of
//! delivery jobs (D-046). The API checks approval and policy first, under the changeset's lock.

use sqlx::types::time::OffsetDateTime;
use uuid::Uuid;

use crate::{DbError, scope::ProjectTransaction};

/// A committed release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Its number in the environment: 1 for the first, increasing by exactly one.
    pub generation: i64,
    /// The environment's epoch when it committed.
    pub epoch: i64,
    /// The changeset it published.
    pub changeset_id: Uuid,
    /// That changeset's title.
    pub title: String,
    /// The changeset hash it published: the approved one.
    pub changeset_hash: Vec<u8>,
    /// The approval it was published under.
    pub review_id: Uuid,
    /// Who published it.
    pub published_by: Uuid,
    /// Their name, if known.
    pub published_by_name: Option<String>,
    /// When.
    pub published_at: OffsetDateTime,
    /// How many entries it changed.
    pub entries: i64,
    /// How many entries it unpublished.
    pub unpublished: i64,
}

/// What [`commit`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Committed {
    /// The release committed as `generation`.
    Done {
        /// Its generation.
        generation: i64,
        /// The environment's epoch.
        epoch: i64,
        /// How many entries now publish a different revision.
        changed: u64,
        /// How many entries it unpublished.
        unpublished: u64,
    },
    /// The environment is no longer at the base generation: another release committed first.
    Moved {
        /// The generation now.
        current: i64,
    },
    /// An entry to unpublish no longer publishes the pinned revision. The caller checks this
    /// under the changeset's lock first, so it means a bug, and nothing is committed.
    Unmatched,
}

/// The environment's epoch and generation.
pub async fn generation(tx: &mut ProjectTransaction) -> Result<(i64, i64), DbError> {
    let row = sqlx::query!(
        "SELECT epoch, generation FROM environments WHERE project_id = $1 AND id = $2",
        tx.project,
        tx.environment
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok((row.epoch, row.generation))
}

/// Commits the release of `changeset` (whose items pin what to publish) on `base`: the next
/// generation, if the environment is still at `base`. Each publish item whose entry doesn't
/// already publish the pinned revision closes its open interval at the new generation and opens
/// one with a copy of the revision. Each unpublish item closes its entry's open interval, leaves
/// a tombstone, and revokes every earlier publication of the entry, so the origin delivers none
/// of them at any generation (D-049). The caller holds the changeset's lock and has checked the
/// approval `review`. Each enabled destination gets a delivery job with `job_lifetime` to run.
pub async fn commit(
    tx: &mut ProjectTransaction,
    base: i64,
    changeset: Uuid,
    hash: &[u8],
    review: Uuid,
    job_lifetime: std::time::Duration,
) -> Result<Committed, DbError> {
    let bumped = sqlx::query!(
        "UPDATE environments SET generation = generation + 1
         WHERE project_id = $1 AND id = $2 AND generation = $3
         RETURNING epoch, generation",
        tx.project,
        tx.environment,
        base
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    let Some(bumped) = bumped else {
        let (_, current) = generation(tx).await?;
        return Ok(Committed::Moved { current });
    };
    sqlx::query!(
        "INSERT INTO releases (project_id, environment_id, generation, epoch, changeset_id, changeset_hash, review_id, published_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        tx.project,
        tx.environment,
        bumped.generation,
        bumped.epoch,
        changeset,
        hash,
        review,
        tx.identity
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    sqlx::query!(
        "UPDATE publications p SET valid_to = $4
         FROM changeset_items i
         WHERE i.project_id = $1 AND i.environment_id = $2 AND i.changeset_id = $3
           AND p.project_id = i.project_id AND p.environment_id = i.environment_id
           AND i.kind = 'publish' AND p.entry_id = i.entry_id AND p.valid_to IS NULL
           AND p.revision_id <> i.revision_id",
        tx.project,
        tx.environment,
        changeset,
        bumped.generation
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    let opened = sqlx::query!(
        "INSERT INTO publications (project_id, environment_id, entry_id, revision_id, collection, title, content, content_hash, valid_from)
         SELECT i.project_id, i.environment_id, i.entry_id, r.id, r.collection, r.title, r.content, r.content_hash, $4
         FROM changeset_items i
         JOIN revisions r ON r.project_id = i.project_id AND r.environment_id = i.environment_id
                         AND r.id = i.revision_id
         WHERE i.project_id = $1 AND i.environment_id = $2 AND i.changeset_id = $3 AND i.kind = 'publish'
           AND NOT EXISTS (
               SELECT 1 FROM publications p
               WHERE p.project_id = i.project_id AND p.environment_id = i.environment_id
                 AND p.entry_id = i.entry_id AND p.valid_to IS NULL
           )",
        tx.project,
        tx.environment,
        changeset,
        bumped.generation
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    let unpublished = unpublish(tx, changeset, bumped.generation).await?;
    let Some(unpublished) = unpublished else {
        return Ok(Committed::Unmatched);
    };
    outbox(tx, bumped.generation, job_lifetime).await?;
    Ok(Committed::Done {
        generation: bumped.generation,
        epoch: bumped.epoch,
        changed: opened.rows_affected(),
        unpublished,
    })
}

/// The outbox: one job per enabled destination for the release at `generation`, committed with
/// the release or not at all. Older announcements that haven't started are obsolete now.
async fn outbox(
    tx: &mut ProjectTransaction,
    generation: i64,
    job_lifetime: std::time::Duration,
) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE jobs SET status = 'superseded', updated_at = now(), finished_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND generation < $3 AND status IN ('pending', 'retrying')",
        tx.project,
        tx.environment,
        generation
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    sqlx::query!(
        "INSERT INTO jobs (project_id, environment_id, id, destination_id, generation, idempotency_key, deadline_at)
         SELECT project_id, environment_id, uuidv7(), id, $3,
                format('colophon:%s:%s:%s:%s', project_id, environment_id, $3::bigint, id),
                now() + make_interval(secs => $4::bigint)
         FROM destinations
         WHERE project_id = $1 AND environment_id = $2 AND enabled",
        tx.project,
        tx.environment,
        generation,
        i64::try_from(job_lifetime.as_secs()).unwrap_or(i64::MAX)
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// The unpublish items of `changeset`, at `generation`: closes each entry's open publication if
/// it is the pinned revision, records a tombstone, revokes every earlier publication of the entry
/// and notes the environment's latest unpublish. How many, or `None` if an item's entry no longer
/// publishes its pinned revision (then the caller commits nothing).
async fn unpublish(
    tx: &mut ProjectTransaction,
    changeset: Uuid,
    generation: i64,
) -> Result<Option<u64>, DbError> {
    let expected = sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM changeset_items
           WHERE project_id = $1 AND environment_id = $2 AND changeset_id = $3 AND kind = 'unpublish'"#,
        tx.project,
        tx.environment,
        changeset
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    if expected == 0 {
        return Ok(Some(0));
    }
    let taken = sqlx::query!(
        "WITH taken AS (
             UPDATE publications p SET valid_to = $4
             FROM changeset_items i
             WHERE i.project_id = $1 AND i.environment_id = $2 AND i.changeset_id = $3 AND i.kind = 'unpublish'
               AND p.project_id = i.project_id AND p.environment_id = i.environment_id
               AND p.entry_id = i.entry_id AND p.valid_to IS NULL AND p.revision_id = i.revision_id
             RETURNING p.entry_id, p.revision_id, p.collection
         )
         INSERT INTO tombstones (project_id, environment_id, entry_id, generation, collection, revision_id)
         SELECT $1, $2, entry_id, $4, collection, revision_id FROM taken",
        tx.project,
        tx.environment,
        changeset,
        generation
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    if i64::try_from(taken.rows_affected()).ok() != Some(expected) {
        return Ok(None);
    }
    sqlx::query!(
        "UPDATE publications p SET revoked_in = $3
         FROM tombstones t
         WHERE t.project_id = $1 AND t.environment_id = $2 AND t.generation = $3
           AND p.project_id = t.project_id AND p.environment_id = t.environment_id
           AND p.entry_id = t.entry_id AND p.valid_from < $3 AND p.revoked_in IS NULL",
        tx.project,
        tx.environment,
        generation
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    sqlx::query!(
        "UPDATE environments SET last_unpublish = $3 WHERE project_id = $1 AND id = $2",
        tx.project,
        tx.environment,
        generation
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(Some(taken.rows_affected()))
}

/// The generation `changeset` was published as, if it has been.
pub async fn of_changeset(
    tx: &mut ProjectTransaction,
    changeset: Uuid,
) -> Result<Option<i64>, DbError> {
    sqlx::query_scalar!(
        "SELECT generation FROM releases WHERE project_id = $1 AND environment_id = $2 AND changeset_id = $3",
        tx.project,
        tx.environment,
        changeset
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// The environment's releases, newest first, at most 100.
pub async fn list(tx: &mut ProjectTransaction) -> Result<Vec<Release>, DbError> {
    sqlx::query_as!(
        Release,
        r#"SELECT r.generation, r.epoch, r.changeset_id, c.title, r.changeset_hash, r.review_id,
                  r.published_by, i.display_name AS published_by_name, r.published_at,
                  (SELECT count(*) FROM publications p
                   WHERE p.project_id = r.project_id AND p.environment_id = r.environment_id
                     AND p.valid_from = r.generation) AS "entries!",
                  (SELECT count(*) FROM tombstones t
                   WHERE t.project_id = r.project_id AND t.environment_id = r.environment_id
                     AND t.generation = r.generation) AS "unpublished!"
           FROM releases r
           JOIN changesets c ON c.project_id = r.project_id AND c.environment_id = r.environment_id
                            AND c.id = r.changeset_id
           JOIN identities i ON i.id = r.published_by
           WHERE r.project_id = $1 AND r.environment_id = $2
           ORDER BY r.generation DESC
           LIMIT 100"#,
        tx.project,
        tx.environment
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}
