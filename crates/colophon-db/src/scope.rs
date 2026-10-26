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
    let mut transaction = pool.begin().await.map_err(DbError::from_query)?;
    let identity = scope.identity.map(|id| id.to_string()).unwrap_or_default();
    let project = scope.project.map(|id| id.to_string()).unwrap_or_default();
    sqlx::query!(
        "SELECT set_config('app.identity_id', $1, true) AS identity_scope, set_config('app.project_id', $2, true) AS project_scope",
        identity,
        project
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(transaction)
}

/// A transaction inside a project the caller belongs to, in one of its environments. The only
/// way to get one is [`begin_in_project`], so code holding one has passed the membership check.
pub struct ProjectTransaction {
    /// The transaction, with both `app.identity_id` and `app.project_id` set.
    pub transaction: Transaction<'static, Postgres>,
    /// The signed-in identity.
    pub identity: Uuid,
    /// The project.
    pub project: Uuid,
    /// The environment's id.
    pub environment: Uuid,
    /// The identity's role in the project.
    pub role: String,
}

impl ProjectTransaction {
    /// Commits.
    pub async fn commit(self) -> Result<(), DbError> {
        self.transaction.commit().await.map_err(DbError::from_query)
    }
}

/// Begins a transaction as `identity`, checks that it is a member of `project`, and only then
/// enters the project's context and resolves the environment slug. `None` if either check fails:
/// to the caller, a project they aren't in and one that doesn't exist look the same (A6).
pub async fn begin_in_project(
    pool: &PgPool,
    identity: Uuid,
    project: Uuid,
    environment: &str,
) -> Result<Option<ProjectTransaction>, DbError> {
    let mut transaction = begin(
        pool,
        Scope {
            identity: Some(identity),
            project: None,
        },
    )
    .await?;
    let role = sqlx::query_scalar!(
        "SELECT role FROM memberships WHERE project_id = $1 AND identity_id = $2",
        project,
        identity
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    // Returning drops the transaction, which rolls it back.
    let Some(role) = role else { return Ok(None) };
    sqlx::query!(
        "SELECT set_config('app.project_id', $1, true) AS project_scope",
        project.to_string()
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    let environment = sqlx::query_scalar!(
        "SELECT id FROM environments WHERE project_id = $1 AND slug = $2",
        project,
        environment
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(environment.map(|environment| ProjectTransaction {
        transaction,
        identity,
        project,
        environment,
        role,
    }))
}
