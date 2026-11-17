//! Public delivery's reads (docs/modules/06-delivery.md; D-045). A [`DeliveryTransaction`] runs
//! as `colophon_delivery`, which may read published projections and nothing else: drafts,
//! revisions, members and sessions are out of reach by permission. Published content is public;
//! the project id in the URL sets the context row-level security reads in. A publication revoked
//! by an unpublish is never delivered, at any generation (D-049, DL5).

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::DbError;

/// The most entries one page of delivery returns.
pub const MAX_PAGE: i64 = 100;

/// A read-only transaction as `colophon_delivery`, in one project's context. The only way to get
/// one is [`begin`].
pub struct DeliveryTransaction {
    /// The transaction itself: `SET LOCAL ROLE colophon_delivery` with `app.project_id` set.
    /// `pub(crate)` so the query translator (`crate::query`) runs its page through it.
    pub(crate) transaction: Transaction<'static, Postgres>,
}

/// An environment as delivery sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    /// Its id.
    pub id: Uuid,
    /// Its epoch.
    pub epoch: i64,
    /// Its latest committed generation (0 before the first release).
    pub generation: i64,
    /// The generation of its latest release that unpublished something (0 if none has).
    pub last_unpublish: i64,
    /// Its locale policy version: bumped by every change to its languages (D-173).
    pub locale_policy: i64,
    /// The tags it lists, in display order (at most 50); empty when it lists none, and then any
    /// well-formed tag may be asked for.
    pub locales: Vec<String>,
    /// Its default language's tag, if it lists languages: what an untagged entry is written in.
    pub default_locale: Option<String>,
    /// The requested tag as the list spells it — `None` when nothing was requested or the list
    /// doesn't have it.
    pub listed_locale: Option<String>,
    /// The listed tag's fallbacks, in order (empty when it has none or isn't listed).
    pub chain: Vec<String>,
}

/// One published entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    /// The entry.
    pub entry_id: Uuid,
    /// The published revision.
    pub revision_id: Uuid,
    /// Its collection.
    pub collection: String,
    /// Its title.
    pub title: String,
    /// Its slug, if its content type has a slug field and it sets one (D-054).
    pub slug: Option<String>,
    /// Its canonical JSON.
    pub content: String,
    /// The content's hash.
    pub content_hash: Vec<u8>,
    /// The generation it was published in.
    pub valid_from: i64,
    /// Who may read it: `public`, `authenticated` or `role:<name>` — what the transaction's
    /// audience already proved it may see (D-137).
    pub visibility: String,
    /// The tag its revision was written in, `None` for the default language (D-173).
    pub locale: Option<String>,
    /// Its translation group: the entry's `localization_id`, or its own id when it has none.
    pub group_key: Uuid,
}

/// A published member of a translation group, without its content: what resolution and an
/// entry's `alternates` need (D-174).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sibling {
    /// The entry.
    pub entry_id: Uuid,
    /// Its tag, `None` for the default language.
    pub locale: Option<String>,
    /// Its slug.
    pub slug: Option<String>,
}

/// How a `?locale=` read picks rows (D-174).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// The tags to try, best first: the requested tag, then its fallbacks — as the
    /// environment's list spells them, which is how writes store them once there is a list.
    pub chain: Vec<String>,
    /// The default language's tag, which an untagged entry counts as; `None` when the
    /// environment has no default, and untagged entries match nothing.
    pub default: Option<String>,
}

/// The environment `slug` of `project` with its latest generation, in one statement: the delivery
/// cache asks this on every request (`delivery_environment`, migrations 0011, 0012 and 0066,
/// which runs as `colophon_delivery` in the project's context and returns nothing else).
pub async fn environment(
    pool: &PgPool,
    project: Uuid,
    slug: &str,
) -> Result<Option<Environment>, DbError> {
    environment_in(pool, project, slug, None).await
}

/// [`environment`], also answering the locale policy for `requested` (D-174): the tag as the
/// list spells it and its fallback chain — still one statement.
pub async fn environment_in(
    pool: &PgPool,
    project: Uuid,
    slug: &str,
    requested: Option<&str>,
) -> Result<Option<Environment>, DbError> {
    sqlx::query_as!(
        Environment,
        r#"SELECT id AS "id!", epoch AS "epoch!", generation AS "generation!",
                  last_unpublish AS "last_unpublish!", locale_policy AS "locale_policy!",
                  locales AS "locales!", default_locale, listed_locale,
                  coalesce(chain, '{}') AS "chain!"
           FROM delivery_environment($1, $2, $3)"#,
        project,
        slug,
        requested
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)
}

/// What [`media_object`] resolved for one media request (D-136).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaObject {
    /// The environment the asset lives in.
    pub environment_id: Uuid,
    /// The literal object key to serve — `None` when the environment exists but the asset or
    /// version does not (or isn't `usable`): the origin answers 404.
    pub public_key: Option<String>,
}

/// The media origin's whole read (`media_object`, migration 0051): resolve `slug` and pick the
/// object key in one call — the `usable` asset's current version, or the version `version_sha`
/// names (the `{sha}` in `/media/v1/{p}/{e}/{asset}/{sha}`). `None` when the environment itself
/// is unknown; a row with `public_key: None` when it exists but nothing deliverable does.
/// Quarantined, rejected, deleting and deleted assets answer `None` even while their objects
/// still sit in the store — the row, not the object, decides what is served.
pub async fn media_object(
    pool: &PgPool,
    project: Uuid,
    slug: &str,
    asset: Uuid,
    version_sha: Option<&str>,
) -> Result<Option<MediaObject>, DbError> {
    sqlx::query_as!(
        MediaObject,
        r#"SELECT environment_id AS "environment_id!", public_key
           FROM media_object($1, $2, $3, $4)"#,
        project,
        slug,
        asset,
        version_sha
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)
}

/// The audience a delivery transaction reads as (D-137): what `app.audience_role` is set to,
/// and so which `publications` rows the delivery role's policy lets through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Audience {
    /// No live site session: only `public` publications.
    Anonymous,
    /// A live site session whose user holds this audience role: `public`, `authenticated`
    /// and `role:<name>` for it.
    Member {
        /// The site user's current role, as the session lookup just read it.
        role: String,
    },
}

/// Starts a delivery transaction for `project` reading as `audience`: switches to
/// `colophon_delivery` for its duration and sets the project's id and the audience scope —
/// the role's name, or empty for anonymous — as transaction-local settings the policy reads.
/// The audience is only ever what the API proved a live session holds (or anonymous): the
/// setting is a claim, but one that expires with the transaction and gates nothing else.
pub async fn begin(
    pool: &PgPool,
    project: Uuid,
    audience: &Audience,
) -> Result<DeliveryTransaction, DbError> {
    let mut transaction = pool.begin().await.map_err(DbError::from_query)?;
    sqlx::query("SET LOCAL ROLE colophon_delivery")
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    let role = match audience {
        Audience::Anonymous => "",
        Audience::Member { role } => role.as_str(),
    };
    sqlx::query!(
        "SELECT set_config('app.project_id', $1, true) AS project_scope,
                set_config('app.audience_role', $2, true) AS audience_scope",
        project.to_string(),
        role
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(DeliveryTransaction { transaction })
}

impl DeliveryTransaction {
    /// Ends the transaction; it wrote nothing.
    pub async fn finish(self) -> Result<(), DbError> {
        self.transaction.commit().await.map_err(DbError::from_query)
    }

    /// For tests: the transaction itself, to show what the role can't do.
    #[doc(hidden)]
    pub fn raw(&mut self) -> &mut Transaction<'static, Postgres> {
        &mut self.transaction
    }

    /// The environment with this slug, with its latest generation.
    pub async fn environment(&mut self, slug: &str) -> Result<Option<Environment>, DbError> {
        sqlx::query_as!(
            Environment,
            r#"SELECT id, epoch, generation, last_unpublish, locale_policy_version AS locale_policy,
                      coalesce((SELECT array_agg(l.tag ORDER BY l.position) FROM environment_locales l
                                WHERE l.project_id = e.project_id AND l.environment_id = e.id), '{}') AS "locales!",
                      (SELECT l.tag FROM environment_locales l
                       WHERE l.project_id = e.project_id AND l.environment_id = e.id AND l.is_default) AS default_locale,
                      NULL::text AS listed_locale, '{}'::text[] AS "chain!"
               FROM environments e
               WHERE project_id = nullif(current_setting('app.project_id', true), '')::uuid AND slug = $1"#,
            slug
        )
        .fetch_optional(&mut *self.transaction)
        .await
        .map_err(DbError::from_query)
    }

    /// The collection's entries as published at `generation` and not since revoked, newest first,
    /// after `before` when given: at most `limit` (capped at one more than [`MAX_PAGE`]).
    pub async fn list(
        &mut self,
        environment: Uuid,
        collection: &str,
        generation: i64,
        before: Option<Uuid>,
        limit: i64,
    ) -> Result<Vec<Published>, DbError> {
        sqlx::query_as!(
            Published,
            "SELECT entry_id, revision_id, collection, title, slug, content, content_hash, valid_from, visibility,
                    locale, group_key
             FROM publications
             WHERE project_id = nullif(current_setting('app.project_id', true), '')::uuid
               AND environment_id = $1 AND collection = $2
               AND valid_from <= $3 AND (valid_to IS NULL OR valid_to > $3) AND revoked_in IS NULL
               AND ($4::uuid IS NULL OR entry_id < $4)
             ORDER BY entry_id DESC
             LIMIT $5",
            environment,
            collection,
            generation,
            before,
            limit.clamp(1, MAX_PAGE + 1)
        )
        .fetch_all(&mut *self.transaction)
        .await
        .map_err(DbError::from_query)
    }

    /// One entry as published at `generation`, unless it has since been revoked.
    pub async fn get(
        &mut self,
        environment: Uuid,
        collection: &str,
        entry: Uuid,
        generation: i64,
    ) -> Result<Option<Published>, DbError> {
        sqlx::query_as!(
            Published,
            "SELECT entry_id, revision_id, collection, title, slug, content, content_hash, valid_from, visibility,
                    locale, group_key
             FROM publications
             WHERE project_id = nullif(current_setting('app.project_id', true), '')::uuid
               AND environment_id = $1 AND collection = $2 AND entry_id = $3
               AND valid_from <= $4 AND (valid_to IS NULL OR valid_to > $4) AND revoked_in IS NULL",
            environment,
            collection,
            entry,
            generation
        )
        .fetch_optional(&mut *self.transaction)
        .await
        .map_err(DbError::from_query)
    }

    /// Whether the entry is gone as far as `generation` is concerned: nothing of it is delivered
    /// there, and either its publication at that generation was revoked or it was taken down at
    /// or before it. Asked when [`Self::get`] found nothing, to answer 410 rather than 404.
    pub async fn gone(
        &mut self,
        environment: Uuid,
        collection: &str,
        entry: Uuid,
        generation: i64,
    ) -> Result<bool, DbError> {
        sqlx::query_scalar!(
            r#"SELECT NOT EXISTS (
                   SELECT 1 FROM publications
                   WHERE project_id = nullif(current_setting('app.project_id', true), '')::uuid
                     AND environment_id = $1 AND collection = $2 AND entry_id = $3
                     AND valid_from <= $4 AND (valid_to IS NULL OR valid_to > $4) AND revoked_in IS NULL
               ) AND (EXISTS (
                   SELECT 1 FROM publications
                   WHERE project_id = nullif(current_setting('app.project_id', true), '')::uuid
                     AND environment_id = $1 AND collection = $2 AND entry_id = $3
                     AND valid_from <= $4 AND (valid_to IS NULL OR valid_to > $4) AND revoked_in IS NOT NULL
               ) OR EXISTS (
                   SELECT 1 FROM tombstones
                   WHERE project_id = nullif(current_setting('app.project_id', true), '')::uuid
                     AND environment_id = $1 AND collection = $2 AND entry_id = $3 AND generation <= $4
               )) AS "gone!""#,
            environment,
            collection,
            entry,
            generation
        )
        .fetch_one(&mut *self.transaction)
        .await
        .map_err(DbError::from_query)
    }

    /// A page of the collection as one row per translation group, at `generation` (D-174):
    /// each group's best published member for `resolution` — the earliest tag in its chain
    /// that the group publishes, an untagged member counting as the default — in group order,
    /// newest first, after the group `before` when given; at most `limit` (capped like
    /// [`Self::list`]). A group with no member in the chain is left out. Row-level security
    /// runs first, so a member the audience can't read is never anyone's fallback.
    ///
    /// The page reads at most `limit` rows per tag in the chain, each tag's straight off its
    /// own range of `publications_by_locale` (and the untagged range, when the default is in
    /// the chain): the first `limit` groups overall are among the first `limit` of each tag
    /// they publish, so the per-tag pages hold every group the page needs and every member
    /// those groups have in the chain. A page costs `chain × limit` index rows however large
    /// the collection is and however rare the tags. Tags compare exactly — with
    /// `colophon_delivery`'s row security in force only leakproof comparisons can use an index,
    /// and the list's spelling is what writes store and requests are respelled to.
    pub async fn list_in_locale(
        &mut self,
        environment: Uuid,
        collection: &str,
        generation: i64,
        resolution: &Resolution,
        before: Option<Uuid>,
        limit: i64,
    ) -> Result<Vec<Published>, DbError> {
        sqlx::query_as!(
            Published,
            r#"SELECT DISTINCT ON (c.group_key)
                      c.entry_id AS "entry_id!", c.revision_id AS "revision_id!",
                      c.collection AS "collection!", c.title AS "title!", c.slug,
                      c.content AS "content!", c.content_hash AS "content_hash!",
                      c.valid_from AS "valid_from!", c.visibility AS "visibility!", c.locale,
                      c.group_key AS "group_key!"
               FROM (
                   SELECT p.*, t.rank
                   FROM unnest($5::text[]) WITH ORDINALITY AS t (tag, rank)
                   CROSS JOIN LATERAL (
                       SELECT p.* FROM publications p
                       WHERE p.project_id = nullif(current_setting('app.project_id', true), '')::uuid
                         AND p.environment_id = $1 AND p.collection = $2 AND p.locale = t.tag
                         AND p.group_key < $6
                         AND p.valid_from <= $3 AND (p.valid_to IS NULL OR p.valid_to > $3)
                         AND p.revoked_in IS NULL
                       ORDER BY p.locale, p.group_key DESC
                       LIMIT $7
                   ) p
                   UNION ALL
                   SELECT p.*, array_position($5::text[], $4) AS rank
                   FROM (
                       SELECT p.* FROM publications p
                       WHERE p.project_id = nullif(current_setting('app.project_id', true), '')::uuid
                         AND p.environment_id = $1 AND p.collection = $2 AND p.locale IS NULL
                         AND $4 = ANY ($5::text[])
                         AND p.group_key < $6
                         AND p.valid_from <= $3 AND (p.valid_to IS NULL OR p.valid_to > $3)
                         AND p.revoked_in IS NULL
                       ORDER BY p.locale, p.group_key DESC
                       LIMIT $7
                   ) p
               ) c
               ORDER BY c.group_key DESC, c.rank, c.entry_id DESC
               LIMIT $7"#,
            environment,
            collection,
            generation,
            resolution.default.as_deref(),
            &resolution.chain,
            // The first page is "before" the greatest uuid: a bound the index can take, where
            // `$6 IS NULL OR …` would leave every page walking from the top.
            before.unwrap_or_else(Uuid::max),
            limit.clamp(1, MAX_PAGE + 1)
        )
        .fetch_all(&mut *self.transaction)
        .await
        .map_err(DbError::from_query)
    }

    /// The translation group of `entry` in `collection`, from any of its publications (an
    /// entry's group never changes) that the audience may read; `None` when it has none.
    pub async fn group_of(
        &mut self,
        environment: Uuid,
        collection: &str,
        entry: Uuid,
    ) -> Result<Option<Uuid>, DbError> {
        sqlx::query_scalar!(
            "SELECT group_key FROM publications
             WHERE project_id = nullif(current_setting('app.project_id', true), '')::uuid
               AND environment_id = $1 AND collection = $2 AND entry_id = $3
             LIMIT 1",
            environment,
            collection,
            entry
        )
        .fetch_optional(&mut *self.transaction)
        .await
        .map_err(DbError::from_query)
    }

    /// The members of translation group `group` published at `generation` and not revoked,
    /// that the audience may read — tag and slug only, by entry id — for resolution and an
    /// entry's `alternates`. A group holds one live entry per tag, so this is bounded by the
    /// environment's languages.
    pub async fn siblings(
        &mut self,
        environment: Uuid,
        collection: &str,
        group: Uuid,
        generation: i64,
    ) -> Result<Vec<Sibling>, DbError> {
        sqlx::query_as!(
            Sibling,
            "SELECT entry_id, locale, slug
             FROM publications
             WHERE project_id = nullif(current_setting('app.project_id', true), '')::uuid
               AND environment_id = $1 AND collection = $2 AND group_key = $3
               AND valid_from <= $4 AND (valid_to IS NULL OR valid_to > $4) AND revoked_in IS NULL
             ORDER BY entry_id DESC
             LIMIT 200",
            environment,
            collection,
            group,
            generation
        )
        .fetch_all(&mut *self.transaction)
        .await
        .map_err(DbError::from_query)
    }
}
