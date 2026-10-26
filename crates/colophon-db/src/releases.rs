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
    /// Whether its changes have been purged from the environment's CDNs (D-050): `none` (no CDN
    /// job covers it), `accepted` (every CDN with one accepted a purge covering it), `pending`
    /// (one is still trying) or `failed`.
    pub purge: String,
    /// Each destination with a job at this generation or later (D-051), as three parallel lists:
    /// its kind, whether one of those jobs succeeded (covering this release), and whether one is
    /// still waiting or running.
    pub delivery_kinds: Vec<String>,
    /// See `delivery_kinds`.
    pub delivery_covered: Vec<bool>,
    /// See `delivery_kinds`.
    pub delivery_open: Vec<bool>,
    /// How many distinct locations a probe saw this release, or a later one, from.
    pub locations: i64,
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

/// The environment's releases, newest first, at most 100; only the one at `generation` when
/// given.
///
/// A destination counts as done for a release once one of its jobs at that release or later
/// succeeded (D-050, D-051), which is to say once its latest success is at that release or later.
/// So each destination's latest job, latest success and latest open job, read once by index,
/// answer for every release on the page. Asked per release instead, as a range of jobs, the list
/// read every later job for every release: 5.2 seconds for a project with 3,139 releases, timed
/// out at the end of a million-request load test. This way it takes about 7 ms.
pub async fn list(
    tx: &mut ProjectTransaction,
    generation: Option<i64>,
) -> Result<Vec<Release>, DbError> {
    sqlx::query_as!(
        Release,
        r#"WITH page AS MATERIALIZED (
               SELECT r.project_id, r.environment_id, r.generation, r.epoch, r.changeset_id, c.title,
                      r.changeset_hash, r.review_id, r.published_by, i.display_name AS published_by_name,
                      r.published_at
               FROM releases r
               JOIN changesets c ON c.project_id = r.project_id AND c.environment_id = r.environment_id
                                AND c.id = r.changeset_id
               JOIN identities i ON i.id = r.published_by
               WHERE r.project_id = $1 AND r.environment_id = $2 AND ($3::bigint IS NULL OR r.generation = $3)
               ORDER BY r.generation DESC
               LIMIT 100
           ), latest AS MATERIALIZED (
               SELECT d.id, d.kind,
                      (SELECT max(j.generation) FROM jobs j
                       WHERE j.project_id = d.project_id AND j.environment_id = d.environment_id
                         AND j.destination_id = d.id) AS last_job,
                      (SELECT max(j.generation) FROM jobs j
                       WHERE j.project_id = d.project_id AND j.environment_id = d.environment_id
                         AND j.destination_id = d.id AND j.status = 'succeeded') AS last_success,
                      (SELECT max(j.generation) FROM jobs j
                       WHERE j.project_id = d.project_id AND j.environment_id = d.environment_id
                         AND j.destination_id = d.id AND j.status IN ('pending', 'retrying', 'running')) AS last_open
               FROM destinations d
               WHERE d.project_id = $1 AND d.environment_id = $2
           ), seen AS MATERIALIZED (
               SELECT o.location, max(o.observed_generation) AS last_seen
               FROM observations o
               WHERE o.project_id = $1 AND o.environment_id = $2 AND o.http_status BETWEEN 200 AND 299
                 AND o.observed_generation >= (SELECT coalesce(min(generation), 0) FROM page)
               GROUP BY o.location
           )
           SELECT p.generation AS "generation!", p.epoch AS "epoch!", p.changeset_id AS "changeset_id!",
                  p.title AS "title!", p.changeset_hash AS "changeset_hash!", p.review_id AS "review_id!",
                  p.published_by AS "published_by!", p.published_by_name, p.published_at AS "published_at!",
                  (SELECT count(*) FROM publications x
                   WHERE x.project_id = p.project_id AND x.environment_id = p.environment_id
                     AND x.valid_from = p.generation) AS "entries!",
                  (SELECT count(*) FROM tombstones t
                   WHERE t.project_id = p.project_id AND t.environment_id = p.environment_id
                     AND t.generation = p.generation) AS "unpublished!",
                  (SELECT CASE WHEN count(*) = 0 THEN 'none'
                               WHEN bool_and(coalesce(l.last_success >= p.generation, false)) THEN 'accepted'
                               WHEN bool_or(coalesce(l.last_open >= p.generation, false)) THEN 'pending'
                               ELSE 'failed' END
                   FROM latest l WHERE l.kind = 'cdn' AND l.last_job >= p.generation) AS "purge!",
                  (SELECT coalesce(array_agg(l.kind ORDER BY l.id), '{}') FROM latest l
                   WHERE l.last_job >= p.generation) AS "delivery_kinds!",
                  (SELECT coalesce(array_agg(coalesce(l.last_success >= p.generation, false) ORDER BY l.id), '{}')
                   FROM latest l WHERE l.last_job >= p.generation) AS "delivery_covered!",
                  (SELECT coalesce(array_agg(coalesce(l.last_open >= p.generation, false) ORDER BY l.id), '{}')
                   FROM latest l WHERE l.last_job >= p.generation) AS "delivery_open!",
                  (SELECT count(*) FROM seen s WHERE s.last_seen >= p.generation) AS "locations!"
           FROM page p
           ORDER BY p.generation DESC"#,
        tx.project,
        tx.environment,
        generation
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// One destination's part in a release's receipt (D-051).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    /// The destination.
    pub destination_id: Uuid,
    /// Its name.
    pub name: String,
    /// `webhook`, `cdn` or `probe`.
    pub kind: String,
    /// For a probe, the location it is pinned to.
    pub location: Option<String>,
    /// Its job for this very release, if it had one: status, attempts, the last error, when it
    /// finished.
    pub status: Option<String>,
    /// See `status`.
    pub attempts: Option<i32>,
    /// See `status`.
    pub last_error: Option<String>,
    /// See `status`.
    pub finished_at: Option<OffsetDateTime>,
    /// The earliest release at or after this one whose job for the destination succeeded: this
    /// release's own, or a later one that covered it.
    pub covered_by: Option<i64>,
    /// Whether a job at this release or later is still waiting or running.
    pub open: bool,
}

/// One probe attempt for a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// The probe.
    pub destination_id: Uuid,
    /// Where from.
    pub location: String,
    /// The generation the answer named, if any.
    pub observed_generation: Option<i64>,
    /// The HTTP status, if there was an answer.
    pub http_status: Option<i16>,
    /// The body's SHA-256, if one was read.
    pub body_hash: Option<Vec<u8>>,
    /// What went wrong, if anything.
    pub error: Option<String>,
    /// When.
    pub observed_at: OffsetDateTime,
}

/// The most probe attempts a receipt lists.
pub const MAX_SEEN: i64 = 100;

/// The deliveries of the release at `generation`, one per destination with a job at that release
/// or later, oldest destination first, and the probe attempts made for it, in order.
pub async fn receipt(
    tx: &mut ProjectTransaction,
    generation: i64,
) -> Result<(Vec<Delivery>, Vec<Seen>), DbError> {
    let deliveries = sqlx::query_as!(
        Delivery,
        r#"SELECT d.id AS destination_id, d.name, d.kind, d.location,
                  own.status AS "status?", own.attempts AS "attempts?", own.last_error AS "last_error?",
                  own.finished_at AS "finished_at?",
                  (SELECT min(j.generation) FROM jobs j
                   WHERE j.project_id = d.project_id AND j.environment_id = d.environment_id
                     AND j.destination_id = d.id AND j.generation >= $3 AND j.status = 'succeeded') AS covered_by,
                  EXISTS (SELECT 1 FROM jobs j
                          WHERE j.project_id = d.project_id AND j.environment_id = d.environment_id
                            AND j.destination_id = d.id AND j.generation >= $3
                            AND j.status IN ('pending', 'retrying', 'running')) AS "open!"
           FROM destinations d
           LEFT JOIN jobs own ON own.project_id = d.project_id AND own.environment_id = d.environment_id
                             AND own.destination_id = d.id AND own.generation = $3
           WHERE d.project_id = $1 AND d.environment_id = $2
             AND EXISTS (SELECT 1 FROM jobs j
                         WHERE j.project_id = d.project_id AND j.environment_id = d.environment_id
                           AND j.destination_id = d.id AND j.generation >= $3)
           ORDER BY d.id"#,
        tx.project,
        tx.environment,
        generation
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    let seen = sqlx::query_as!(
        Seen,
        "SELECT destination_id, location, observed_generation, http_status, body_hash, error, observed_at
         FROM observations
         WHERE project_id = $1 AND environment_id = $2 AND generation = $3
         ORDER BY observed_at, id
         LIMIT $4",
        tx.project,
        tx.environment,
        generation,
        MAX_SEEN
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok((deliveries, seen))
}
