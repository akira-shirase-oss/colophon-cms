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
    transaction: Transaction<'static, Postgres>,
}

/// An environment as delivery sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Environment {
    /// Its id.
    pub id: Uuid,
    /// Its epoch.
    pub epoch: i64,
    /// Its latest committed generation (0 before the first release).
    pub generation: i64,
    /// The generation of its latest release that unpublished something (0 if none has).
    pub last_unpublish: i64,
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
    /// Its canonical JSON.
    pub content: String,
    /// The content's hash.
    pub content_hash: Vec<u8>,
    /// The generation it was published in.
    pub valid_from: i64,
}

/// The environment `slug` of `project` with its latest generation, in one statement: the delivery
/// cache asks this on every request (`delivery_environment`, migrations 0011 and 0012, which runs
/// as `colophon_delivery` in the project's context and returns nothing else).
pub async fn environment(
    pool: &PgPool,
    project: Uuid,
    slug: &str,
) -> Result<Option<Environment>, DbError> {
    sqlx::query_as!(
        Environment,
        r#"SELECT id AS "id!", epoch AS "epoch!", generation AS "generation!",
                  last_unpublish AS "last_unpublish!"
           FROM delivery_environment($1, $2)"#,
        project,
        slug
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)
}

/// Starts a delivery transaction for `project`: switches to `colophon_delivery` for its duration.
pub async fn begin(pool: &PgPool, project: Uuid) -> Result<DeliveryTransaction, DbError> {
    let mut transaction = pool.begin().await.map_err(DbError::from_query)?;
    sqlx::query("SET LOCAL ROLE colophon_delivery")
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    sqlx::query!(
        "SELECT set_config('app.project_id', $1, true) AS project_scope",
        project.to_string()
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
            "SELECT id, epoch, generation, last_unpublish FROM environments WHERE project_id = nullif(current_setting('app.project_id', true), '')::uuid AND slug = $1",
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
            "SELECT entry_id, revision_id, collection, title, content, content_hash, valid_from
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
            "SELECT entry_id, revision_id, collection, title, content, content_hash, valid_from
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
}
