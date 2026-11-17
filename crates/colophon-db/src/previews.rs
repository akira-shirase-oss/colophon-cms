//! Preview grants and the preview overlay (D-171, D-172; docs/modules/03-auth.md "Preview
//! grants", 06-delivery.md "Preview reads").
//!
//! A grant is a delegated, scoped, short-lived read: a person issues it for one entry (with its
//! translations) or one publish request bound to its hash; a one-time code is exchanged for a
//! bearer token. Both are stored only as SHA-256 hashes — the API hashes them, this module never
//! sees a secret. Management runs in the issuer's own [`ProjectTransaction`]; the exchange and
//! the token lookup run in the project's row-level-security context alone (no identity), since
//! the site that holds the code is nobody's session. What a read may show is then decided in a
//! transaction opened *as the issuer* ([`crate::scope::begin_in_project_isolated`]), so the
//! issuer's membership and role are checked again on every request.
//!
//! The overlay itself — published rows at the latest generation with the scope's rows laid
//! over them — is [`crate::query::overlay_page`] (the query language's third source) and
//! [`list`] / [`get`] here, all built on one statement head, [`crate::query::OVERLAY`].

use sqlx::{PgPool, Row, types::time::OffsetDateTime};
use uuid::Uuid;

use crate::{
    DbError,
    delivery::Published,
    query::{OVERLAY, OVERLAY_ROWS, Overlay},
    scope::{self, ProjectTransaction},
};

/// The most live grants one project holds at once; past it, issuing is refused.
pub const MAX_LIVE: i64 = 1_000;

/// How long a one-time code stays exchangeable, in seconds.
pub const CODE_SECONDS: i32 = 60;

/// The most rows a scope lays over a release: a publish request's own bound, and well past any
/// translation group.
pub const MAX_SCOPE: i64 = 500;

/// The site address a preview opens, for one environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The address with `{code}` and the other placeholders still in it.
    pub url_template: String,
    /// When it was last set.
    pub updated_at: OffsetDateTime,
}

/// The environment's preview address, if one is set.
pub async fn target(tx: &mut ProjectTransaction) -> Result<Option<Target>, DbError> {
    sqlx::query_as!(
        Target,
        "SELECT url_template, updated_at FROM preview_targets
         WHERE project_id = $1 AND environment_id = $2",
        tx.project,
        tx.environment
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Sets the environment's preview address (already validated), or clears it with `None`.
pub async fn set_target(
    tx: &mut ProjectTransaction,
    template: Option<&str>,
) -> Result<Option<Target>, DbError> {
    let Some(template) = template else {
        sqlx::query!(
            "DELETE FROM preview_targets WHERE project_id = $1 AND environment_id = $2",
            tx.project,
            tx.environment
        )
        .execute(&mut *tx.transaction)
        .await
        .map_err(DbError::from_write)?;
        return Ok(None);
    };
    sqlx::query_as!(
        Target,
        "INSERT INTO preview_targets (project_id, environment_id, url_template, updated_by)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (project_id, environment_id) DO UPDATE
             SET url_template = EXCLUDED.url_template, updated_by = EXCLUDED.updated_by,
                 updated_at = now()
         RETURNING url_template, updated_at",
        tx.project,
        tx.environment,
        template,
        tx.identity
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map(Some)
    .map_err(DbError::from_write)
}

/// What a grant lets its holder read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// One entry and its translation group, following their latest saved drafts.
    Entry(Uuid),
    /// One publish request's pinned revisions, while its hash is still this one.
    Changeset {
        /// The publish request.
        id: Uuid,
        /// Its hash when the grant was issued.
        hash: Vec<u8>,
    },
}

impl Scope {
    /// `entry` or `changeset`, as stored and as `Colophon-Preview` names it.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Entry(_) => "entry",
            Self::Changeset { .. } => "changeset",
        }
    }

    /// The scope a stored row describes.
    fn of(
        kind: &str,
        entry: Option<Uuid>,
        changeset: Option<Uuid>,
        hash: Option<Vec<u8>>,
    ) -> Result<Self, DbError> {
        match (kind, entry, changeset, hash) {
            ("entry", Some(entry), None, None) => Ok(Self::Entry(entry)),
            ("changeset", None, Some(id), Some(hash)) => Ok(Self::Changeset { id, hash }),
            _ => Err(DbError::Corrupt(
                "a preview grant's scope doesn't hold together",
            )),
        }
    }
}

/// A grant to store: the API made the id and hashed the code.
#[derive(Debug, Clone)]
pub struct NewGrant<'a> {
    /// Its id.
    pub id: Uuid,
    /// What it reads.
    pub scope: &'a Scope,
    /// How long its token lives, and how far each renewal moves it: 60 to 3,600 seconds.
    pub ttl_seconds: i32,
    /// SHA-256 of the one-time code.
    pub code_hash: &'a [u8; 32],
}

/// When a stored grant's code and token end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Issued {
    /// When it was made.
    pub created_at: OffsetDateTime,
    /// When its code stops being exchangeable.
    pub code_expires_at: OffsetDateTime,
    /// When its token stops reading.
    pub expires_at: OffsetDateTime,
}

/// How many of the project's grants are live (not revoked, not expired), counted up to
/// [`MAX_LIVE`].
pub async fn live(tx: &mut ProjectTransaction) -> Result<i64, DbError> {
    sqlx::query_scalar!(
        r#"SELECT count(*) AS "live!" FROM (
               SELECT 1 FROM preview_grants
               WHERE project_id = $1 AND revoked_at IS NULL AND expires_at > now()
               LIMIT $2) g"#,
        tx.project,
        MAX_LIVE
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Stores a grant issued by the transaction's identity, timed by the database's clock.
pub async fn issue(tx: &mut ProjectTransaction, grant: &NewGrant<'_>) -> Result<Issued, DbError> {
    let (entry, changeset, hash) = match grant.scope {
        Scope::Entry(entry) => (Some(*entry), None, None),
        Scope::Changeset { id, hash } => (None, Some(*id), Some(hash.as_slice())),
    };
    sqlx::query_as!(
        Issued,
        "INSERT INTO preview_grants
             (project_id, environment_id, id, issuer_id, scope_kind, entry_id, changeset_id,
              changeset_hash, ttl_seconds, code_hash, code_expires_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                 now() + make_interval(secs => $11), now() + make_interval(secs => $9::integer))
         RETURNING created_at, code_expires_at AS \"code_expires_at!\", expires_at",
        tx.project,
        tx.environment,
        grant.id,
        tx.identity,
        grant.scope.kind(),
        entry,
        changeset,
        hash,
        grant.ttl_seconds,
        &grant.code_hash[..],
        // `make_interval` takes double precision seconds.
        f64::from(CODE_SECONDS)
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)
}

/// A fresh one-time code for one of the identity's own live grants; `None` if it has none by
/// that id. The code never outlives the grant.
pub async fn new_code(
    tx: &mut ProjectTransaction,
    id: Uuid,
    code_hash: &[u8; 32],
) -> Result<Option<(OffsetDateTime, Scope)>, DbError> {
    let row = sqlx::query!(
        "UPDATE preview_grants
         SET code_hash = $5,
             code_expires_at = least(now() + make_interval(secs => $6), expires_at)
         WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND issuer_id = $4
           AND revoked_at IS NULL AND expires_at > now()
         RETURNING code_expires_at AS \"code_expires_at!\", scope_kind, entry_id, changeset_id,
                   changeset_hash",
        tx.project,
        tx.environment,
        id,
        tx.identity,
        &code_hash[..],
        f64::from(CODE_SECONDS)
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    row.map(|row| {
        Ok((
            row.code_expires_at,
            Scope::of(
                &row.scope_kind,
                row.entry_id,
                row.changeset_id,
                row.changeset_hash,
            )?,
        ))
    })
    .transpose()
}

/// Moves one of the identity's own live grants' end to now plus its time to live, never past
/// eight hours from when it was made. `None` if it has no live grant by that id.
pub async fn renew(
    tx: &mut ProjectTransaction,
    id: Uuid,
) -> Result<Option<OffsetDateTime>, DbError> {
    sqlx::query_scalar!(
        "UPDATE preview_grants
         SET expires_at = greatest(expires_at, least(created_at + interval '8 hours',
                                                     now() + make_interval(secs => ttl_seconds)))
         WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND issuer_id = $4
           AND revoked_at IS NULL AND expires_at > now()
         RETURNING expires_at",
        tx.project,
        tx.environment,
        id,
        tx.identity
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)
}

/// Ends one of the identity's own grants now; its code goes with it. `false` if it has no
/// unrevoked grant by that id.
pub async fn revoke(tx: &mut ProjectTransaction, id: Uuid) -> Result<bool, DbError> {
    let revoked = sqlx::query!(
        "UPDATE preview_grants SET revoked_at = now(), code_hash = NULL, code_expires_at = NULL
         WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND issuer_id = $4
           AND revoked_at IS NULL",
        tx.project,
        tx.environment,
        id,
        tx.identity
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?
    .rows_affected();
    Ok(revoked == 1)
}

/// One of the identity's own live grants, for the editor's "previews open" list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Live {
    /// Its id.
    pub id: Uuid,
    /// What it reads.
    pub scope: Scope,
    /// When it was made.
    pub created_at: OffsetDateTime,
    /// When it ends.
    pub expires_at: OffsetDateTime,
    /// Whether a site has exchanged its code.
    pub exchanged: bool,
}

/// The identity's own live grants in this environment, newest first, at most 100; only those
/// for `entry` when given.
pub async fn mine(tx: &mut ProjectTransaction, entry: Option<Uuid>) -> Result<Vec<Live>, DbError> {
    let rows = sqlx::query!(
        "SELECT id, scope_kind, entry_id, changeset_id, changeset_hash, created_at, expires_at,
                exchanged_at
         FROM preview_grants
         WHERE project_id = $1 AND environment_id = $2 AND issuer_id = $3
           AND revoked_at IS NULL AND expires_at > now()
           AND ($4::uuid IS NULL OR entry_id = $4)
         ORDER BY created_at DESC
         LIMIT 100",
        tx.project,
        tx.environment,
        tx.identity,
        entry
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    rows.into_iter()
        .map(|row| {
            Ok(Live {
                id: row.id,
                scope: Scope::of(
                    &row.scope_kind,
                    row.entry_id,
                    row.changeset_id,
                    row.changeset_hash,
                )?,
                created_at: row.created_at,
                expires_at: row.expires_at,
                exchanged: row.exchanged_at.is_some(),
            })
        })
        .collect()
}

/// A grant as a preview read finds it by its token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    /// Its id.
    pub id: Uuid,
    /// The environment it reads.
    pub environment_id: Uuid,
    /// That environment's slug, which the request's path must name.
    pub environment: String,
    /// Who issued it: every read runs as them.
    pub issuer: Uuid,
    /// What it reads.
    pub scope: Scope,
    /// When it ends.
    pub expires_at: OffsetDateTime,
    /// When it was revoked, if it was.
    pub revoked_at: Option<OffsetDateTime>,
    /// Whether it has ended, by the database's clock.
    pub expired: bool,
}

/// A transaction in `project`'s row-level-security context and nobody's identity: what the
/// exchange and the token lookup run in.
async fn in_project(
    pool: &PgPool,
    project: Uuid,
) -> Result<sqlx::Transaction<'static, sqlx::Postgres>, DbError> {
    scope::begin(
        pool,
        scope::Scope {
            identity: None,
            project: Some(project),
        },
    )
    .await
}

/// The grant whose token hashes to `token_hash`, in `project` only: a token presented on
/// another project's address finds nothing there.
pub async fn by_token(
    pool: &PgPool,
    project: Uuid,
    token_hash: &[u8; 32],
) -> Result<Option<Grant>, DbError> {
    let mut transaction = in_project(pool, project).await?;
    let row = sqlx::query!(
        r#"SELECT g.id, g.environment_id, e.slug AS environment, g.issuer_id, g.scope_kind,
                  g.entry_id, g.changeset_id, g.changeset_hash, g.expires_at, g.revoked_at,
                  g.expires_at <= now() AS "expired!"
           FROM preview_grants g
           JOIN environments e ON e.project_id = g.project_id AND e.id = g.environment_id
           WHERE g.project_id = $1 AND g.token_hash = $2"#,
        project,
        &token_hash[..]
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    row.map(|row| {
        Ok(Grant {
            id: row.id,
            environment_id: row.environment_id,
            environment: row.environment,
            issuer: row.issuer_id,
            scope: Scope::of(
                &row.scope_kind,
                row.entry_id,
                row.changeset_id,
                row.changeset_hash,
            )?,
            expires_at: row.expires_at,
            revoked_at: row.revoked_at,
            expired: row.expired,
        })
    })
    .transpose()
}

/// What a successful exchange hands the site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exchanged {
    /// The grant.
    pub id: Uuid,
    /// Who issued it, for the audit trail.
    pub issuer: Uuid,
    /// What it reads.
    pub scope: Scope,
    /// When its token ends.
    pub expires_at: OffsetDateTime,
}

/// Spends the one-time code hashing to `code_hash` on `project`'s environment `environment`,
/// storing `token_hash` as the grant's token. One `UPDATE` clears the code and sets the token,
/// so of two racing exchanges exactly one finds the code; an unknown, spent or expired code —
/// or a grant revoked or ended meanwhile — is `None`, all alike.
pub async fn exchange(
    pool: &PgPool,
    project: Uuid,
    environment: &str,
    code_hash: &[u8; 32],
    token_hash: &[u8; 32],
) -> Result<Option<Exchanged>, DbError> {
    let mut transaction = in_project(pool, project).await?;
    let row = sqlx::query!(
        "UPDATE preview_grants g
         SET code_hash = NULL, code_expires_at = NULL, token_hash = $3, exchanged_at = now()
         FROM environments e
         WHERE g.project_id = $1 AND e.project_id = g.project_id AND e.id = g.environment_id
           AND e.slug = $2 AND g.code_hash = $4 AND g.code_expires_at > now()
           AND g.revoked_at IS NULL AND g.expires_at > now()
         RETURNING g.id, g.issuer_id, g.scope_kind, g.entry_id, g.changeset_id,
                   g.changeset_hash, g.expires_at",
        project,
        environment,
        &token_hash[..],
        &code_hash[..]
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::from_write)?;
    let Some(row) = row else {
        return Ok(None);
    };
    crate::audit::record_in(
        &mut transaction,
        project,
        row.issuer_id,
        "preview.exchanged",
        &row.id.to_string(),
        "{}",
    )
    .await?;
    transaction.commit().await.map_err(DbError::from_write)?;
    Ok(Some(Exchanged {
        id: row.id,
        issuer: row.issuer_id,
        scope: Scope::of(
            &row.scope_kind,
            row.entry_id,
            row.changeset_id,
            row.changeset_hash,
        )?,
        expires_at: row.expires_at,
    }))
}

/// The environment's latest generation (0 before its first release), read in the preview's
/// own snapshot so the overlay and the generation it names agree.
pub async fn generation(tx: &mut ProjectTransaction) -> Result<i64, DbError> {
    sqlx::query_scalar!(
        "SELECT generation FROM environments WHERE project_id = $1 AND id = $2",
        tx.project,
        tx.environment
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// What one entry is to an issuer about to preview it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryFacts {
    /// Its collection.
    pub collection: String,
    /// Who created it (the `own` condition).
    pub created_by: Uuid,
    /// Its draft's slug, for the address's `{slug}`.
    pub slug: Option<String>,
    /// Its locale, for `{locale}`.
    pub locale: Option<String>,
}

/// The facts about a live entry of the environment the address template and the permission
/// check need; `None` if there is no such live entry.
pub async fn entry_facts(
    tx: &mut ProjectTransaction,
    entry: Uuid,
) -> Result<Option<EntryFacts>, DbError> {
    sqlx::query_as!(
        EntryFacts,
        "SELECT collection, created_by, draft_slug AS slug, locale FROM entries
         WHERE project_id = $1 AND environment_id = $2 AND id = $3",
        tx.project,
        tx.environment,
        entry
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// A publish request as a preview of it sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangesetState {
    /// Its current hash.
    pub hash: Vec<u8>,
    /// The release that published it, if one has.
    pub released_in: Option<i64>,
    /// Its first item, for the address's placeholders.
    pub first: Option<(Uuid, String)>,
}

/// The publish request's hash, whether it is published, and its first item; `None` if the
/// environment has no such request.
pub async fn changeset_state(
    tx: &mut ProjectTransaction,
    changeset: Uuid,
) -> Result<Option<ChangesetState>, DbError> {
    let row = sqlx::query!(
        r#"SELECT c.hash,
                  (SELECT r.generation FROM releases r
                   WHERE r.project_id = c.project_id AND r.environment_id = c.environment_id
                     AND r.changeset_id = c.id) AS released_in,
                  first.entry_id AS "first_entry?", first.collection AS "first_collection?"
           FROM changesets c
           LEFT JOIN LATERAL (
               SELECT i.entry_id, e.collection
               FROM changeset_items i
               JOIN entries e
                 ON e.project_id = i.project_id AND e.environment_id = i.environment_id
                AND e.id = i.entry_id
               WHERE i.project_id = c.project_id AND i.environment_id = c.environment_id
                 AND i.changeset_id = c.id
               ORDER BY i.entry_id
               LIMIT 1
           ) first ON true
           WHERE c.project_id = $1 AND c.environment_id = $2 AND c.id = $3"#,
        tx.project,
        tx.environment,
        changeset
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(row.map(|row| ChangesetState {
        hash: row.hash,
        released_in: row.released_in,
        first: row.first_entry.zip(row.first_collection),
    }))
}

/// The rows a scope lays over the release: each entry it names and the revision it shows —
/// `None` takes the entry out (a publish request's unpublish item). An entry scope follows the
/// entry's translation group's *current* drafts; a deleted entry is invisible here like
/// everywhere else, so it simply isn't in the scope.
pub async fn scope_rows(
    tx: &mut ProjectTransaction,
    scope: &Scope,
) -> Result<(Vec<Uuid>, Vec<Option<Uuid>>), DbError> {
    let rows: Vec<(Uuid, Option<Uuid>)> = match scope {
        Scope::Entry(entry) => sqlx::query!(
            r#"SELECT e.id, e.draft_revision_id AS "revision?"
               FROM entries e
               WHERE e.project_id = $1 AND e.environment_id = $2
                 AND (e.id = $3 OR e.localization_id = (
                         SELECT g.localization_id FROM entries g
                         WHERE g.project_id = $1 AND g.environment_id = $2 AND g.id = $3))
               ORDER BY e.id
               LIMIT $4"#,
            tx.project,
            tx.environment,
            entry,
            MAX_SCOPE
        )
        .fetch_all(&mut *tx.transaction)
        .await
        .map_err(DbError::from_query)?
        .into_iter()
        .map(|row| (row.id, row.revision))
        .collect(),
        Scope::Changeset { id, .. } => sqlx::query!(
            r#"SELECT entry_id,
                      CASE WHEN kind = 'publish' THEN revision_id END AS "revision?"
               FROM changeset_items
               WHERE project_id = $1 AND environment_id = $2 AND changeset_id = $3
               ORDER BY entry_id
               LIMIT $4"#,
            tx.project,
            tx.environment,
            id,
            MAX_SCOPE
        )
        .fetch_all(&mut *tx.transaction)
        .await
        .map_err(DbError::from_query)?
        .into_iter()
        .map(|row| (row.entry_id, row.revision))
        .collect(),
    };
    Ok(rows.into_iter().unzip())
}

/// One row of a preview read: the published view's own columns, where it came from, and who
/// created the entry (the issuer's `own` condition decides what of it they may read).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayRow {
    /// The row, in delivery's shape: for a scope row, `valid_from` is the generation the
    /// preview is laid over, since the draft is in no release yet.
    pub row: Published,
    /// `published`, `draft` or `changeset`.
    pub source: String,
    /// Who created the entry; `None` only if its entry is out of the issuer's sight.
    pub created_by: Option<Uuid>,
}

/// Reads one overlay row from a statement whose select list is [`OVERLAY_ROWS`]'.
pub(crate) fn overlay_row(row: &sqlx::postgres::PgRow) -> Result<OverlayRow, DbError> {
    Ok(OverlayRow {
        row: Published {
            entry_id: row.try_get("entry_id").map_err(DbError::Query)?,
            revision_id: row.try_get("revision_id").map_err(DbError::Query)?,
            collection: row.try_get("collection").map_err(DbError::Query)?,
            title: row.try_get("title").map_err(DbError::Query)?,
            slug: row.try_get("slug").map_err(DbError::Query)?,
            content: row.try_get("content").map_err(DbError::Query)?,
            content_hash: row.try_get("content_hash").map_err(DbError::Query)?,
            valid_from: row.try_get("valid_from").map_err(DbError::Query)?,
            visibility: row.try_get("visibility").map_err(DbError::Query)?,
            media: row.try_get("media").map_err(DbError::Query)?,
        },
        source: row.try_get("source").map_err(DbError::Query)?,
        created_by: row.try_get("created_by").map_err(DbError::Query)?,
    })
}

/// The overlay's head bound with its six parameters, ready for a tail.
fn head<'q>(
    text: String,
    tx: &ProjectTransaction,
    collection: &'q str,
    overlay: &'q Overlay<'q>,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    sqlx::query(sqlx::AssertSqlSafe(text))
        .bind(tx.environment)
        .bind(collection)
        .bind(overlay.generation)
        .bind(overlay.entries)
        .bind(overlay.revisions)
        .bind(overlay.source)
}

/// The collection as the preview sees it, newest entry first, after `before` when given: at
/// most `limit` rows (capped at one more than [`crate::delivery::MAX_PAGE`]).
pub async fn list(
    tx: &mut ProjectTransaction,
    collection: &str,
    overlay: &Overlay<'_>,
    before: Option<Uuid>,
    limit: i64,
) -> Result<Vec<OverlayRow>, DbError> {
    // The head is a fixed literal and the tail's two parameters follow its six.
    let text = format!(
        "{OVERLAY}{OVERLAY_ROWS} WHERE ($7::uuid IS NULL OR p.entry_id < $7) \
         ORDER BY p.entry_id DESC LIMIT $8"
    );
    let rows = head(text, tx, collection, overlay)
        .bind(before)
        .bind(limit.clamp(1, crate::delivery::MAX_PAGE + 1))
        .fetch_all(&mut *tx.transaction)
        .await
        .map_err(DbError::from_query)?;
    rows.iter().map(overlay_row).collect()
}

/// One entry as the preview sees it.
pub async fn get(
    tx: &mut ProjectTransaction,
    collection: &str,
    overlay: &Overlay<'_>,
    entry: Uuid,
) -> Result<Option<OverlayRow>, DbError> {
    let text = format!("{OVERLAY}{OVERLAY_ROWS} WHERE p.entry_id = $7");
    let row = head(text, tx, collection, overlay)
        .bind(entry)
        .fetch_optional(&mut *tx.transaction)
        .await
        .map_err(DbError::from_query)?;
    row.as_ref().map(overlay_row).transpose()
}
