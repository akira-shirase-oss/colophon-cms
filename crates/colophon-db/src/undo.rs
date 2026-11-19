//! Undoing a takedown (D-184): what the API needs to know about a release before it may put back
//! what that release took down, and the mark that names an undo's changeset.
//!
//! The undo itself is an ordinary changeset published by [`crate::publish::publish`] — the same
//! locks, checks and outbox as any release — so this module only reads the facts the decision
//! needs; `changesets::create_undo` writes the changeset with `undo_of` (migration 0071).

use uuid::Uuid;

use crate::{DbError, scope::ProjectTransaction};

/// What deciding an undo of one release needs, read in one statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// The changeset the release published.
    pub changeset_id: Uuid,
    /// That changeset's title.
    pub title: String,
    /// Who published the release.
    pub published_by: Uuid,
    /// Whether the release is younger than the window, by the database's clock: its own
    /// `published_at` against this transaction's `now()`, never the application's clock.
    pub in_window: bool,
    /// How many of its changeset's items publish something; an undo reverses takedowns only.
    pub publishes: i64,
    /// How many of its items take an entry down.
    pub takes_down: i64,
}

/// The facts about release `generation`, or `None` when the environment has no such release.
/// `window_secs` is the undo window, in seconds.
pub async fn facts(
    tx: &mut ProjectTransaction,
    generation: i64,
    window_secs: i64,
) -> Result<Option<Facts>, DbError> {
    sqlx::query_as!(
        Facts,
        r#"SELECT r.changeset_id, c.title, r.published_by,
                  r.published_at > now() - make_interval(secs => $4::bigint::double precision)
                      AS "in_window!",
                  (SELECT count(*) FROM changeset_items i
                   WHERE i.project_id = r.project_id AND i.environment_id = r.environment_id
                     AND i.changeset_id = r.changeset_id AND i.kind <> 'unpublish') AS "publishes!",
                  (SELECT count(*) FROM changeset_items i
                   WHERE i.project_id = r.project_id AND i.environment_id = r.environment_id
                     AND i.changeset_id = r.changeset_id AND i.kind = 'unpublish') AS "takes_down!"
           FROM releases r
           JOIN changesets c ON c.project_id = r.project_id AND c.environment_id = r.environment_id
                            AND c.id = r.changeset_id
           WHERE r.project_id = $1 AND r.environment_id = $2 AND r.generation = $3"#,
        tx.project,
        tx.environment,
        generation,
        window_secs
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}
