//! Public delivery's reads (docs/modules/06-delivery.md; D-045). A [`DeliveryTransaction`] runs
//! as `colophon_delivery`, which may read published projections and nothing else: drafts,
//! revisions, members and sessions are out of reach by permission. Published content is public;
//! the project id in the URL sets the context row-level security reads in.

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
            "SELECT id, epoch, generation FROM environments WHERE project_id = nullif(current_setting('app.project_id', true), '')::uuid AND slug = $1",
            slug
        )
        .fetch_optional(&mut *self.transaction)
        .await
        .map_err(DbError::from_query)
    }

    /// The collection's entries as published at `generation`, newest first, after `before` when
    /// given: at most `limit` (capped at one more than [`MAX_PAGE`]).
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
               AND valid_from <= $3 AND (valid_to IS NULL OR valid_to > $3)
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

    /// One entry as published at `generation`.
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
               AND valid_from <= $4 AND (valid_to IS NULL OR valid_to > $4)",
            environment,
            collection,
            entry,
            generation
        )
        .fetch_optional(&mut *self.transaction)
        .await
        .map_err(DbError::from_query)
    }
}
