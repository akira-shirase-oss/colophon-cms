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

/// What a changeset pins about one entry: the revision, with everything that identifies it, and
/// what to do with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    /// `publish` (the pinned revision is the entry's current one) or `unpublish` (it is the
    /// published one, to be taken down; D-049).
    pub kind: String,
    /// The entry.
    pub entry_id: Uuid,
    /// The pinned revision.
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
    /// The generation whose publication set it restores, when a rollback built it (D-097).
    pub rollback: Option<i64>,
    /// The release it undoes, when it is an undo of a takedown (D-184); `rollback` is then the
    /// release just before it.
    pub undo_of: Option<i64>,
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
    /// The revision the pin must still match: for `publish`, the entry's current revision
    /// (different once someone edits it); for `unpublish`, the published one (none once nothing
    /// is).
    pub current_revision_id: Option<Uuid>,
    /// The revision readers get now, if the entry is published: what the item changes from.
    pub published_revision_id: Option<Uuid>,
    /// Who created the entry: for roles that may change only their own (D-058). `None` once the
    /// entry is deleted (D-080), which only a published changeset can still show.
    pub created_by: Option<Uuid>,
}

impl Item {
    /// Whether the entry moved on since it was pinned, so the changeset must be refreshed.
    pub fn moved(&self) -> bool {
        self.current_revision_id != Some(self.pin.revision_id)
    }
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
    /// The review stage it decided for — `None` for the implicit review of an environment
    /// without stages (D-104).
    pub stage: Option<String>,
    /// The changeset hash it was made on.
    pub changeset_hash: Vec<u8>,
    /// Its place in the changeset's decisions: 1 for the first, increasing by one.
    pub seq: i32,
    /// What the reviewer wrote.
    pub comment: String,
    /// When.
    pub created_at: OffsetDateTime,
}

/// A review stage of the environment, as stored (D-104): one of an ordered list of gates a
/// submitted changeset needs an approval on before it may publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stage {
    /// Its key in the list — what a review names it by.
    pub key: String,
    /// Its name, as reviewers see it.
    pub name: String,
    /// Its place in the list, from 0: display order only, never a gate.
    pub position: i32,
    /// Whether its approval must come from someone other than the submitter.
    pub different_reviewer: bool,
}

/// A changeset in a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// Its fields.
    pub head: Head,
    /// How many entries it pins.
    pub items: i64,
    /// How many of those it unpublishes.
    pub unpublish: i64,
    /// How many of them moved on since they were pinned.
    pub stale: i64,
    /// The newest review on its current hash.
    pub latest: Option<Review>,
}

/// The current revisions of `entries` in the transaction's environment, with what a changeset
/// pins about them. Entries that don't exist here are left out.
pub async fn pins(tx: &mut ProjectTransaction, entries: &[Uuid]) -> Result<Vec<Pin>, DbError> {
    sqlx::query_as!(
        Pin,
        r#"SELECT 'publish' AS "kind!", e.id AS entry_id, r.id AS revision_id, r.collection, r.content_hash,
                  r.schema_version, v.definition_hash
         FROM entries e
         JOIN revisions r ON r.project_id = e.project_id AND r.environment_id = e.environment_id
                         AND r.id = e.draft_revision_id
         JOIN content_type_versions v ON v.project_id = r.project_id AND v.environment_id = r.environment_id
                                     AND v.collection = r.collection AND v.version = r.schema_version
         WHERE e.project_id = $1 AND e.environment_id = $2 AND e.id = ANY($3)"#,
        tx.project,
        tx.environment,
        entries
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// The published revisions of `entries`, pinned to be unpublished. Entries that aren't published
/// here are left out.
pub async fn published_pins(
    tx: &mut ProjectTransaction,
    entries: &[Uuid],
) -> Result<Vec<Pin>, DbError> {
    sqlx::query_as!(
        Pin,
        r#"SELECT 'unpublish' AS "kind!", p.entry_id, r.id AS revision_id, r.collection, r.content_hash,
                  r.schema_version, v.definition_hash
           FROM publications p
           JOIN revisions r ON r.project_id = p.project_id AND r.environment_id = p.environment_id
                           AND r.id = p.revision_id
           JOIN content_type_versions v ON v.project_id = r.project_id AND v.environment_id = r.environment_id
                                       AND v.collection = r.collection AND v.version = r.schema_version
           WHERE p.project_id = $1 AND p.environment_id = $2 AND p.entry_id = ANY($3) AND p.valid_to IS NULL"#,
        tx.project,
        tx.environment,
        entries
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Plans this transaction's statements for the values they run with (D-185): "live at a
/// generation" is two index ranges whose better half depends on the generation — an old one is
/// found by `valid_from`, a recent one by `valid_to` — and a cached generic plan, which Postgres
/// switches to after five runs of a prepared statement, picks one half for every generation and
/// scans half the history for the rest (measured: 8 ms custom, 126–260 ms generic, at 100k
/// publications). A release's changes page the same way: its cursor's `OR` folds away only in a
/// plan for the values (0.9 ms custom, 300 ms generic). Transaction-local, so it ends with the
/// transaction.
pub(crate) async fn plan_for_values(tx: &mut ProjectTransaction) -> Result<(), DbError> {
    sqlx::query!("SET LOCAL plan_cache_mode = force_custom_plan")
        .execute(&mut *tx.transaction)
        .await
        .map_err(DbError::from_query)?;
    Ok(())
}

/// The most pins one rollback changeset may carry (D-185). A release changes at most
/// [`MAX_ITEMS`] entries, but a rollback restores a whole publication set, so it may change more;
/// past this many it is refused as `rollback_too_large`, and a nearer release is the way back.
/// Sized from the idempotency record its answer must fit ([`crate::idempotency::MAX_RESPONSE_BYTES`]):
/// the worst-case item serialises to 1,342 bytes and the rest of the answer to 39,280, so 194 fit;
/// 190 leaves room (`colophon-api`'s `the_largest_rollback_answer_fits_its_idempotency_record`).
/// Before this cap a rollback of a few hundred entries failed with a 500 (D-185).
pub const MAX_ROLLBACK_ITEMS: usize = 190;

/// What [`rollback_pins`] reads at most: one past [`MAX_ROLLBACK_ITEMS`], so a caller can tell a
/// set that fits from one that doesn't without reading the rest.
const ROLLBACK_PINS_READ: i64 = 191;
const _: () = assert!(
    ROLLBACK_PINS_READ == 191 && MAX_ROLLBACK_ITEMS == 190,
    "ROLLBACK_PINS_READ is one past MAX_ROLLBACK_ITEMS"
);

/// The publication set as it stood at `generation`, as the pins a rollback changeset would carry:
/// a publish pin for every entry published then (matching the delivery read at that generation,
/// which leaves out anything since revoked), and an unpublish pin for every entry published now
/// that wasn't published then. An entry publishing the same revision already is not in the set —
/// the changeset that follows restores the past as a new release; the generation still moves
/// forward (module 01, D4; docs/modules/06, "rollback is a new generation"). Publish pins
/// first, then unpublish pins, each by entry id; at most one more than [`MAX_ROLLBACK_ITEMS`].
pub async fn rollback_pins(
    tx: &mut ProjectTransaction,
    generation: i64,
) -> Result<Vec<Pin>, DbError> {
    plan_for_values(tx).await?;
    sqlx::query_as!(
        Pin,
        r#"WITH at AS (
               -- Live at $3: still open, or closed after it. Two halves, each on its own
               -- index (open rows; closed by release), so the read follows what was live
               -- then, not the whole history (D-185).
               SELECT p.entry_id, p.revision_id
               FROM publications p
               WHERE p.project_id = $1 AND p.environment_id = $2
                 AND p.valid_to IS NULL AND p.valid_from <= $3 AND p.revoked_in IS NULL
               UNION ALL
               SELECT p.entry_id, p.revision_id
               FROM publications p
               WHERE p.project_id = $1 AND p.environment_id = $2
                 AND p.valid_to > $3 AND p.valid_from <= $3 AND p.revoked_in IS NULL
           ), now AS (
               SELECT p.entry_id, p.revision_id
               FROM publications p
               WHERE p.project_id = $1 AND p.environment_id = $2 AND p.valid_to IS NULL
           ), diff AS MATERIALIZED (
               -- What differs, cut to the page first: only these rows meet the revisions
               -- (D-185; joining first fetched every revision live then, at 48k live entries
               -- most of half a second).
               SELECT 'publish' AS kind, at.entry_id, at.revision_id
               FROM at
               WHERE NOT EXISTS (
                   SELECT 1 FROM now WHERE now.entry_id = at.entry_id AND now.revision_id = at.revision_id
               )
               UNION ALL
               SELECT 'unpublish', now.entry_id, now.revision_id
               FROM now
               WHERE NOT EXISTS (SELECT 1 FROM at WHERE at.entry_id = now.entry_id)
               ORDER BY 1, 2
               LIMIT $4
           )
           SELECT d.kind AS "kind!", d.entry_id AS "entry_id!", r.id AS "revision_id!",
                  r.collection AS "collection!", r.content_hash AS "content_hash!",
                  r.schema_version AS "schema_version!", v.definition_hash AS "definition_hash!"
           FROM diff d
           JOIN revisions r ON r.project_id = $1 AND r.environment_id = $2
                           AND r.id = d.revision_id
           JOIN content_type_versions v ON v.project_id = r.project_id AND v.environment_id = r.environment_id
                                       AND v.collection = r.collection AND v.version = r.schema_version
           ORDER BY 1, 2"#,
        tx.project,
        tx.environment,
        generation,
        ROLLBACK_PINS_READ
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// How many entries a rollback to `generation` would put back and take down — [`rollback_pins`]'
/// set, counted without reading it, so a preview can say how far past
/// [`MAX_ROLLBACK_ITEMS`] a rollback is.
pub async fn rollback_counts(
    tx: &mut ProjectTransaction,
    generation: i64,
) -> Result<(i64, i64), DbError> {
    plan_for_values(tx).await?;
    let row = sqlx::query!(
        r#"WITH at AS (
               -- Live at $3: still open, or closed after it. Two halves, each on its own
               -- index (open rows; closed by release), so the read follows what was live
               -- then, not the whole history (D-185).
               SELECT p.entry_id, p.revision_id
               FROM publications p
               WHERE p.project_id = $1 AND p.environment_id = $2
                 AND p.valid_to IS NULL AND p.valid_from <= $3 AND p.revoked_in IS NULL
               UNION ALL
               SELECT p.entry_id, p.revision_id
               FROM publications p
               WHERE p.project_id = $1 AND p.environment_id = $2
                 AND p.valid_to > $3 AND p.valid_from <= $3 AND p.revoked_in IS NULL
           ), now AS (
               SELECT p.entry_id, p.revision_id
               FROM publications p
               WHERE p.project_id = $1 AND p.environment_id = $2 AND p.valid_to IS NULL
           )
           SELECT (SELECT count(*) FROM at
                   WHERE NOT EXISTS (SELECT 1 FROM now
                                     WHERE now.entry_id = at.entry_id AND now.revision_id = at.revision_id)
                  ) AS "publish!",
                  (SELECT count(*) FROM now
                   WHERE NOT EXISTS (SELECT 1 FROM at WHERE at.entry_id = now.entry_id)
                  ) AS "unpublish!""#,
        tx.project,
        tx.environment,
        generation
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok((row.publish, row.unpublish))
}

/// What a person reads about one pin before a rollback is made: the title of the version it
/// names, and the version readers get of its entry now, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinTitle {
    /// The pinned revision.
    pub revision_id: Uuid,
    /// Its title.
    pub title: String,
    /// The entry's open publication now, if it is published.
    pub live_revision_id: Option<Uuid>,
}

/// The titles of `revisions` and what each one's entry publishes now: the rollback preview's
/// words for [`rollback_pins`].
pub async fn pin_titles(
    tx: &mut ProjectTransaction,
    revisions: &[Uuid],
) -> Result<Vec<PinTitle>, DbError> {
    sqlx::query_as!(
        PinTitle,
        r#"SELECT r.id AS "revision_id!", r.title AS "title!", now.revision_id AS "live_revision_id?"
           FROM revisions r
           LEFT JOIN publications now ON now.project_id = r.project_id AND now.environment_id = r.environment_id
                                     AND now.entry_id = r.entry_id AND now.valid_to IS NULL
           WHERE r.project_id = $1 AND r.environment_id = $2 AND r.id = ANY($3)"#,
        tx.project,
        tx.environment,
        revisions
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// An entry readers had at a generation that a rollback to it can't bring back: it was taken
/// down since, which revoked that publication (D-049), so the generation's set no longer holds
/// it (D-097). Only entries not published now: one published again is taken down by the
/// rollback, and is among its pins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptDown {
    /// The entry.
    pub entry_id: Uuid,
    /// Its collection.
    pub collection: String,
    /// The title readers had at the generation.
    pub title: String,
    /// The version readers had at the generation.
    pub revision_id: Uuid,
    /// The release that took it down (and revoked that publication).
    pub taken_down_in: i64,
    /// Who created the entry; `None` once it was deleted (D-080), when it can't be published
    /// again. What a role reads of it follows from this (D-058).
    pub created_by: Option<Uuid>,
    /// How many entries a rollback to the generation keeps down, however many are listed.
    pub total: i64,
}

/// The most kept-down entries a rollback preview lists.
pub const MAX_KEPT_DOWN: i64 = 100;

/// The entries a rollback to `generation` leaves down although readers had them then: published
/// at `generation`, revoked by a takedown since, and not published now. By title, at most
/// [`MAX_KEPT_DOWN`], each row carrying the total.
pub async fn kept_down(
    tx: &mut ProjectTransaction,
    generation: i64,
) -> Result<Vec<KeptDown>, DbError> {
    plan_for_values(tx).await?;
    sqlx::query_as!(
        KeptDown,
        r#"SELECT p.entry_id, p.collection, p.title, p.revision_id,
                  p.revoked_in AS "taken_down_in!", e.created_by AS "created_by?",
                  count(*) OVER () AS "total!"
           FROM publications p
           LEFT JOIN entries e ON e.project_id = p.project_id AND e.environment_id = p.environment_id
                              AND e.id = p.entry_id
           WHERE p.project_id = $1 AND p.environment_id = $2
             -- A revoked publication is always closed (its takedown closed it): live at $3
             -- is closed after it, which the closed-by-release index finds (D-185).
             AND p.valid_to > $3 AND p.valid_from <= $3
             AND p.revoked_in IS NOT NULL
             AND NOT EXISTS (SELECT 1 FROM publications now
                             WHERE now.project_id = p.project_id AND now.environment_id = p.environment_id
                               AND now.entry_id = p.entry_id AND now.valid_to IS NULL)
           ORDER BY p.title, p.entry_id
           LIMIT $4"#,
        tx.project,
        tx.environment,
        generation,
        MAX_KEPT_DOWN
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// A pinned `publish` revision written before its collection's current version, with the
/// definition in force: a type change can move on after the revision's publication closed, so
/// publish checks it again (D-055). A rollback pins history on purpose (D-097), which is how
/// such a pin usually gets here.
pub struct OutdatedPin {
    /// The entry the revision belongs to.
    pub entry_id: Uuid,
    /// The pinned revision's title.
    pub title: String,
    /// The pinned revision's stored content.
    pub content: String,
    /// The collection's definition now.
    pub definition: String,
}

/// The `publish` pins of `id` whose revisions predate their collection's current version.
/// Called under `content_types::lock_for_write`, so the versions it reads cannot move.
pub async fn outdated_pins(
    tx: &mut ProjectTransaction,
    id: Uuid,
) -> Result<Vec<OutdatedPin>, DbError> {
    sqlx::query_as!(
        OutdatedPin,
        r#"SELECT i.entry_id AS "entry_id!", r.title AS "title!", r.content AS "content!",
                  cur.definition AS "definition!"
           FROM changeset_items i
           JOIN revisions r ON r.project_id = i.project_id AND r.environment_id = i.environment_id
                           AND r.id = i.revision_id
           JOIN (SELECT DISTINCT ON (collection) collection, version, definition
                 FROM content_type_versions
                 WHERE project_id = $1 AND environment_id = $2
                 ORDER BY collection, version DESC) cur
             ON cur.collection = r.collection AND cur.version > r.schema_version
           WHERE i.project_id = $1 AND i.environment_id = $2 AND i.changeset_id = $3
             AND i.kind = 'publish'"#,
        tx.project,
        tx.environment,
        id
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Stores a new changeset pinning `pins`, with the hash the caller computed from them. `rollback`
/// is the generation whose publication set it restores (D-097), so its publish pins are never
/// stale: they name older revisions on purpose, and a pinned revision cannot drift.
pub async fn create(
    tx: &mut ProjectTransaction,
    id: Uuid,
    title: &str,
    pins: &[Pin],
    hash: &[u8; 32],
    rollback: Option<i64>,
) -> Result<(), DbError> {
    insert(tx, id, title, pins, hash, rollback, None).await
}

/// [`create`] for the undo of release `generation` (D-184, migration 0071): its pins put back
/// what that release took down, so it restores the release before (`rollback_generation =
/// generation - 1`) and names the one it undoes. The mark is written with the row and never
/// changed: the application may insert changesets but update only their hash and submission.
pub async fn create_undo(
    tx: &mut ProjectTransaction,
    id: Uuid,
    title: &str,
    pins: &[Pin],
    hash: &[u8; 32],
    generation: i64,
) -> Result<(), DbError> {
    insert(
        tx,
        id,
        title,
        pins,
        hash,
        Some(generation - 1),
        Some(generation),
    )
    .await
}

/// The row and its items, as [`create`] and [`create_undo`] write them.
async fn insert(
    tx: &mut ProjectTransaction,
    id: Uuid,
    title: &str,
    pins: &[Pin],
    hash: &[u8; 32],
    rollback: Option<i64>,
    undo_of: Option<i64>,
) -> Result<(), DbError> {
    sqlx::query!(
        "INSERT INTO changesets (project_id, environment_id, id, title, hash, created_by, rollback_generation, undo_of)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        tx.project,
        tx.environment,
        id,
        title,
        &hash[..],
        tx.identity,
        rollback,
        undo_of
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    let entries: Vec<Uuid> = pins.iter().map(|pin| pin.entry_id).collect();
    let revisions: Vec<Uuid> = pins.iter().map(|pin| pin.revision_id).collect();
    let kinds: Vec<String> = pins.iter().map(|pin| pin.kind.clone()).collect();
    sqlx::query!(
        "INSERT INTO changeset_items (project_id, environment_id, changeset_id, entry_id, revision_id, kind)
         SELECT $1, $2, $3, entry, revision, kind
         FROM unnest($4::uuid[], $5::uuid[], $6::text[]) AS pinned (entry, revision, kind)",
        tx.project,
        tx.environment,
        id,
        &entries,
        &revisions,
        &kinds
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "the row's own fields, grouped where they can be: id, title, hash, who, when"
)]
fn head(
    id: Uuid,
    title: String,
    hash: Vec<u8>,
    created_by: Uuid,
    created_at: OffsetDateTime,
    submitted: (Option<Uuid>, Option<OffsetDateTime>),
    changed: (OffsetDateTime, Option<i64>),
    restores: (Option<i64>, Option<i64>),
) -> Head {
    Head {
        id,
        title,
        hash,
        created_by,
        created_at,
        submitted_by: submitted.0,
        submitted_at: submitted.1,
        rollback: restores.0,
        undo_of: restores.1,
        updated_at: changed.0,
        published: changed.1,
    }
}

/// The changeset, read without a lock.
pub async fn get(tx: &mut ProjectTransaction, id: Uuid) -> Result<Option<Head>, DbError> {
    let row = sqlx::query!(
        r#"SELECT c.title, c.hash, c.created_by, c.created_at, c.submitted_by, c.submitted_at, c.updated_at,
                  c.rollback_generation, c.undo_of,
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
            (row.rollback_generation, row.undo_of),
        )
    }))
}

/// The changeset, locked until the transaction ends: refreshes and reviews queue on it.
pub async fn lock(tx: &mut ProjectTransaction, id: Uuid) -> Result<Option<Head>, DbError> {
    let row = sqlx::query!(
        r#"SELECT c.title, c.hash, c.created_by, c.created_at, c.submitted_by, c.submitted_at, c.updated_at,
                  c.rollback_generation, c.undo_of,
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
            (row.rollback_generation, row.undo_of),
        )
    }))
}

/// The collections the changeset's items belong to, in order: what a publish locks with
/// `content_types::lock_for_write` before it reads the items (D-055).
pub async fn collections(tx: &mut ProjectTransaction, id: Uuid) -> Result<Vec<String>, DbError> {
    sqlx::query_scalar!(
        "SELECT DISTINCT e.collection
         FROM changeset_items i
         JOIN entries e ON e.project_id = i.project_id AND e.environment_id = i.environment_id
                       AND e.id = i.entry_id
         WHERE i.project_id = $1 AND i.environment_id = $2 AND i.changeset_id = $3
         ORDER BY e.collection",
        tx.project,
        tx.environment,
        id
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// The changeset's items, each with the revision its pin must still match, ordered by entry id.
/// Each item's revision and entry are looked up on their own, so stale statistics can't turn the
/// join into scans of every revision (see `entries::list`).
pub async fn items(tx: &mut ProjectTransaction, id: Uuid) -> Result<Vec<Item>, DbError> {
    let rows = sqlx::query!(
        r#"SELECT i.kind, i.entry_id, i.revision_id, r.collection, r.content_hash, r.schema_version,
                  v.definition_hash, r.title,
                  CASE WHEN i.kind = 'publish' AND c.rollback_generation IS NULL THEN e.draft_revision_id
                       WHEN i.kind = 'publish' THEN i.revision_id
                       ELSE (SELECT p.revision_id FROM publications p
                             WHERE p.project_id = i.project_id AND p.environment_id = i.environment_id
                               AND p.entry_id = i.entry_id AND p.valid_to IS NULL)
                  END AS current_revision_id,
                  (SELECT p.revision_id FROM publications p
                   WHERE p.project_id = i.project_id AND p.environment_id = i.environment_id
                     AND p.entry_id = i.entry_id AND p.valid_to IS NULL) AS published_revision_id,
                  e.created_by AS "created_by?"
         FROM changeset_items i
         JOIN changesets c ON c.project_id = i.project_id AND c.environment_id = i.environment_id
                          AND c.id = i.changeset_id
         CROSS JOIN LATERAL (
             SELECT r.collection, r.content_hash, r.schema_version, r.title
             FROM revisions r
             WHERE r.project_id = i.project_id AND r.environment_id = i.environment_id AND r.id = i.revision_id
             LIMIT 1
         ) r
         JOIN content_type_versions v ON v.project_id = i.project_id AND v.environment_id = i.environment_id
                                     AND v.collection = r.collection AND v.version = r.schema_version
         -- A deleted entry (D-080) is hidden from this role, but a published changeset keeps
         -- listing what it published: its item stays, without a draft or a creator.
         LEFT JOIN LATERAL (
             SELECT e.draft_revision_id, e.created_by
             FROM entries e
             WHERE e.project_id = i.project_id AND e.environment_id = i.environment_id AND e.id = i.entry_id
             LIMIT 1
         ) e ON true
         WHERE i.project_id = $1 AND i.environment_id = $2 AND i.changeset_id = $3
         ORDER BY i.entry_id"#,
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
                kind: row.kind,
                entry_id: row.entry_id,
                revision_id: row.revision_id,
                collection: row.collection,
                content_hash: row.content_hash,
                schema_version: row.schema_version,
                definition_hash: row.definition_hash,
            },
            title: row.title,
            current_revision_id: row.current_revision_id,
            published_revision_id: row.published_revision_id,
            created_by: row.created_by,
        })
        .collect())
}

/// Re-pins the changeset's entries to `pins` (for each, the revision its kind pins now) and stores
/// the new hash. The caller holds [`lock`].
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
    .map_err(DbError::from_write)?;
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

/// Whether `identity` is an API token's own (D-111), read where the caller already looks —
/// inside a transaction holding [`lock`], so the answer can't move under the check it feeds.
pub async fn is_token_identity(
    tx: &mut ProjectTransaction,
    identity: Uuid,
) -> Result<bool, DbError> {
    sqlx::query_scalar!(
        r#"SELECT issuer = $2 AS "token!" FROM identities WHERE id = $1"#,
        identity,
        crate::tokens::IDENTITY_ISSUER
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map(|issuer| issuer.unwrap_or(false))
    .map_err(DbError::from_query)
}

/// Records the transaction's identity's decision on the changeset with `hash`, as its next
/// decision in sequence — `stage` names the review stage it decides for, `None` the implicit
/// review (D-104). The caller holds [`lock`] and has checked that `hash` is current.
pub async fn review(
    tx: &mut ProjectTransaction,
    id: Uuid,
    hash: &[u8],
    decision: &str,
    comment: &str,
    stage: Option<&str>,
) -> Result<(), DbError> {
    sqlx::query!(
        "INSERT INTO reviews (project_id, environment_id, id, changeset_id, changeset_hash, seq, reviewer_id, decision, comment, stage)
         SELECT $1, $2, $3, $4, $5, coalesce(max(seq), 0) + 1, $6, $7, $8, $9
         FROM reviews WHERE project_id = $1 AND environment_id = $2 AND changeset_id = $4",
        tx.project,
        tx.environment,
        Uuid::now_v7(),
        id,
        hash,
        tx.identity,
        decision,
        comment,
        stage
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
        r#"SELECT v.id, v.reviewer_id, i.display_name AS reviewer_name, v.decision, v.stage,
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

/// The newest [`MAX_REVIEWS`] reviews on each of `changesets`' current hash, paired with the
/// changeset's id, newest first within each — what an environment with stages needs to decide
/// every listed changeset, which the one `latest` review per changeset can't do (D-104).
pub async fn current_reviews(
    tx: &mut ProjectTransaction,
    changesets: &[Uuid],
) -> Result<Vec<(Uuid, Review)>, DbError> {
    let rows = sqlx::query!(
        r#"SELECT v.changeset_id, v.id, v.reviewer_id, i.display_name AS reviewer_name,
                  v.decision, v.stage, v.changeset_hash, v.seq, v.comment, v.created_at
           FROM (
               SELECT v.*, row_number() OVER (PARTITION BY v.changeset_id
                                              ORDER BY v.seq DESC) AS n
               FROM reviews v
               JOIN changesets c ON c.project_id = v.project_id
                                AND c.environment_id = v.environment_id
                                AND c.id = v.changeset_id AND v.changeset_hash = c.hash
               WHERE v.project_id = $1 AND v.environment_id = $2 AND v.changeset_id = ANY($3)
           ) v JOIN identities i ON i.id = v.reviewer_id
           WHERE v.n <= $4
           ORDER BY v.changeset_id, v.seq DESC"#,
        tx.project,
        tx.environment,
        changesets,
        MAX_REVIEWS
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.changeset_id,
                Review {
                    id: row.id,
                    reviewer_id: row.reviewer_id,
                    reviewer_name: row.reviewer_name,
                    decision: row.decision,
                    stage: row.stage,
                    changeset_hash: row.changeset_hash,
                    seq: row.seq,
                    comment: row.comment,
                    created_at: row.created_at,
                },
            )
        })
        .collect())
}

/// The environment's review stages, in their order (D-104).
pub async fn stages(tx: &mut ProjectTransaction) -> Result<Vec<Stage>, DbError> {
    sqlx::query_as!(
        Stage,
        "SELECT key, name, position, different_reviewer FROM review_stages
         WHERE project_id = $1 AND environment_id = $2 ORDER BY position",
        tx.project,
        tx.environment
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// [`stages`], with the environment row locked in share mode first: a stage-list replace takes
/// [`lock_environment`], so it waits for every in-flight review, publish or schedule check that
/// is deciding against the list — while share locks don't exclude one another, so those
/// decisions don't serialize against each other. The environment row is the lock target rather
/// than the stage rows: `colophon_api` deliberately has no `UPDATE` on `review_stages` (its rows
/// are never rewritten, only deleted and reinserted by a replace), and Postgres requires an
/// updatable column for any `FOR` lock — `environments` has one (`generation`, `last_unpublish`).
pub async fn stages_locked(tx: &mut ProjectTransaction) -> Result<Vec<Stage>, DbError> {
    sqlx::query!(
        "SELECT id FROM environments WHERE project_id = $1 AND id = $2 FOR SHARE",
        tx.project,
        tx.environment
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    stages(tx).await
}

/// [`stages_locked`], in no-key-update mode for a publish that goes on to rewrite the
/// environment row: it excludes other publishes and stage-list replaces exactly like the
/// `UPDATE environments` the caller later makes — so no shared read has to upgrade into the
/// write, which is where two publishes deadlock — while tolerating the `FOR KEY SHARE` lock
/// every writer in this environment already holds from claiming its idempotency record
/// (`FOR UPDATE` would deadlock against those instead).
pub async fn stages_locked_for_write(tx: &mut ProjectTransaction) -> Result<Vec<Stage>, DbError> {
    sqlx::query!(
        "SELECT id FROM environments WHERE project_id = $1 AND id = $2 FOR NO KEY UPDATE",
        tx.project,
        tx.environment
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    stages(tx).await
}

/// The environment row, locked until the transaction ends: a stage-list replace takes it, so
/// two replaces can't interleave a delete with an insert at the same position, and readers of
/// the list (who take it in share mode through [`stages_locked`]) finish first.
pub async fn lock_environment(tx: &mut ProjectTransaction) -> Result<(), DbError> {
    sqlx::query!(
        "SELECT id FROM environments WHERE project_id = $1 AND id = $2 FOR UPDATE",
        tx.project,
        tx.environment
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(())
}

/// Replaces the environment's stage list with `stages`, in their order: a stage that keeps its
/// key keeps the reviews recorded for it; a removed one's become history (D-104). The caller
/// holds [`lock_environment`].
pub async fn set_stages(tx: &mut ProjectTransaction, stages: &[Stage]) -> Result<(), DbError> {
    sqlx::query!(
        "DELETE FROM review_stages WHERE project_id = $1 AND environment_id = $2",
        tx.project,
        tx.environment
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    let keys: Vec<&str> = stages.iter().map(|stage| stage.key.as_str()).collect();
    let names: Vec<&str> = stages.iter().map(|stage| stage.name.as_str()).collect();
    let positions: Vec<i32> = stages.iter().map(|stage| stage.position).collect();
    let flags: Vec<bool> = stages
        .iter()
        .map(|stage| stage.different_reviewer)
        .collect();
    sqlx::query!(
        "INSERT INTO review_stages (project_id, environment_id, key, name, position, different_reviewer)
         SELECT $1, $2, key, name, position, different_reviewer
         FROM unnest($3::text[], $4::text[], $5::integer[], $6::boolean[])
              AS stage (key, name, position, different_reviewer)",
        tx.project,
        tx.environment,
        &keys as &[&str],
        &names as &[&str],
        &positions,
        &flags
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
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
                  c.updated_at, c.rollback_generation, c.undo_of, rel.generation AS "published?",
                  counts.items AS "items!", counts.unpublish AS "unpublish!", counts.stale AS "stale!",
                  latest.id AS "review_id?", latest.reviewer_id AS "reviewer_id?",
                  latest.reviewer_name, latest.decision AS "decision?", latest.seq AS "seq?",
                  latest.stage AS "stage?",
                  latest.comment AS "comment?", latest.created_at AS "reviewed_at?"
           FROM changesets c
           CROSS JOIN LATERAL (
               SELECT count(*) AS items, count(*) FILTER (WHERE i.kind = 'unpublish') AS unpublish,
                      count(*) FILTER (
                          WHERE CASE WHEN i.kind = 'publish' AND c.rollback_generation IS NOT NULL THEN false
                                     WHEN i.kind = 'publish' THEN e.draft_revision_id <> i.revision_id
                                     ELSE NOT EXISTS (
                                         SELECT 1 FROM publications p
                                         WHERE p.project_id = i.project_id AND p.environment_id = i.environment_id
                                           AND p.entry_id = i.entry_id AND p.valid_to IS NULL
                                           AND p.revision_id = i.revision_id)
                                END
                      ) AS stale
               FROM changeset_items i
               -- Items of deleted entries still count (D-080); only published changesets have them.
               LEFT JOIN entries e ON e.project_id = i.project_id AND e.environment_id = i.environment_id
                                  AND e.id = i.entry_id
               WHERE i.project_id = c.project_id AND i.environment_id = c.environment_id
                 AND i.changeset_id = c.id
           ) counts
           LEFT JOIN LATERAL (
               SELECT v.id, v.reviewer_id, n.display_name AS reviewer_name, v.decision, v.seq, v.stage,
                      v.comment, v.created_at
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
            let latest = latest_review(
                &row.hash,
                (
                    row.review_id,
                    row.reviewer_id,
                    row.reviewer_name,
                    row.decision,
                    row.stage,
                    row.seq,
                    row.comment,
                    row.reviewed_at,
                ),
            );
            Summary {
                head: head(
                    row.id,
                    row.title,
                    row.hash,
                    row.created_by,
                    row.created_at,
                    (row.submitted_by, row.submitted_at),
                    (row.updated_at, row.published),
                    (row.rollback_generation, row.undo_of),
                ),
                items: row.items,
                unpublish: row.unpublish,
                stale: row.stale,
                latest,
            }
        })
        .collect())
}

/// The newest review on the changeset's current hash, when the lateral join found every field of one.
#[expect(
    clippy::type_complexity,
    reason = "the query! row's optional fields as the lateral join returns them"
)]
fn latest_review(
    hash: &[u8],
    parts: (
        Option<Uuid>,
        Option<Uuid>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i32>,
        Option<String>,
        Option<OffsetDateTime>,
    ),
) -> Option<Review> {
    let (id, reviewer_id, reviewer_name, decision, stage, seq, comment, created_at) = parts;
    match (id, reviewer_id, decision, seq, comment, created_at) {
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
            reviewer_name,
            decision,
            stage,
            changeset_hash: hash.to_vec(),
            seq,
            comment,
            created_at,
        }),
        _ => None,
    }
}
