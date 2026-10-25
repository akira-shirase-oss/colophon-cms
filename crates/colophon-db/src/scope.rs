//! Scoped transactions: the row-level-security context Postgres enforces (docs/modules/02-db.md,
//! "Row-level security and roles").
//!
//! Every project-scoped table denies everything unless the transaction says who is asking
//! (`app.identity_id`) and, for work inside a project, which project (`app.project_id`). The
//! settings are transaction-local (`set_config(..., true)`), so they end with the transaction and
//! can't leak into the next request that borrows the same pooled connection.

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::DbError;

/// Who is asking, and in which project.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Scope {
    /// The signed-in identity, if any.
    pub identity: Option<Uuid>,
    /// The project being worked in, if any. Set only after the caller is authorized for it.
    pub project: Option<Uuid>,
}

/// Begins a transaction with `scope` as its row-level-security context.
pub async fn begin(pool: &PgPool, scope: Scope) -> Result<Transaction<'static, Postgres>, DbError> {
    let mut transaction = pool.begin().await.map_err(DbError::Query)?;
    let identity = scope.identity.map(|id| id.to_string()).unwrap_or_default();
    let project = scope.project.map(|id| id.to_string()).unwrap_or_default();
    sqlx::query!(
        "SELECT set_config('app.identity_id', $1, true) AS identity_scope, set_config('app.project_id', $2, true) AS project_scope",
        identity,
        project
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(DbError::Query)?;
    Ok(transaction)
}
