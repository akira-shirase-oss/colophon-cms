//! Scoped transactions: the row-level-security context Postgres enforces (docs/modules/02-db.md,
//! "Row-level security and roles").
//!
//! Every project-scoped table denies everything unless the transaction says who is asking
//! (`app.identity_id`) and, for work inside a project, which project (`app.project_id`). The
//! settings are transaction-local (`set_config(..., true)`), so they end with the transaction and
//! can't leak into the next request that borrows the same pooled connection.

use colophon_domain::policy::{Action, Condition, Permission, Policy, Subject};
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
    /// What that role may do (D-058).
    pub policy: Policy,
}

impl ProjectTransaction {
    /// Commits.
    pub async fn commit(self) -> Result<(), DbError> {
        self.transaction.commit().await.map_err(DbError::from_query)
    }
}

/// A transaction inside a project the caller belongs to, without an environment: for
/// project-level work such as members. The only way to get one is [`begin_as_member`].
pub struct MemberTransaction {
    /// The transaction, with both `app.identity_id` and `app.project_id` set.
    pub transaction: Transaction<'static, Postgres>,
    /// The signed-in identity.
    pub identity: Uuid,
    /// The project.
    pub project: Uuid,
    /// The identity's role in the project.
    pub role: String,
    /// What that role may do (D-058).
    pub policy: Policy,
}

impl MemberTransaction {
    /// Commits.
    pub async fn commit(self) -> Result<(), DbError> {
        self.transaction.commit().await.map_err(DbError::from_query)
    }
}

/// Begins a transaction as `identity`, checks that it is a member of `project`, and only then
/// enters the project's context. `None` if it isn't: to the caller, a project they aren't in and
/// one that doesn't exist look the same (A6).
pub async fn begin_as_member(
    pool: &PgPool,
    identity: Uuid,
    project: Uuid,
) -> Result<Option<MemberTransaction>, DbError> {
    let mut transaction = begin(
        pool,
        Scope {
            identity: Some(identity),
            project: None,
        },
    )
    .await?;
    // The role and its permissions in one statement and one pass over them, as parallel arrays
    // in one order (D-058).
    let member = sqlx::query!(
        r#"SELECT m.role,
                  coalesce(p.actions, '{}') AS "actions!",
                  coalesce(p.subjects, '{}') AS "subjects!",
                  coalesce(p.fields, '{}') AS "fields!: Vec<Option<String>>",
                  coalesce(p.conditions, '{}') AS "conditions!: Vec<Option<String>>"
           FROM memberships m
           LEFT JOIN LATERAL (
               SELECT array_agg(action ORDER BY action, subject) AS actions,
                      array_agg(subject ORDER BY action, subject) AS subjects,
                      array_agg(array_to_string(fields, ',') ORDER BY action, subject) AS fields,
                      array_agg(condition ORDER BY action, subject) AS conditions
               FROM role_permissions
               WHERE project_id = m.project_id AND role_key = m.role
           ) p ON true
           WHERE m.project_id = $1 AND m.identity_id = $2"#,
        project,
        identity
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    // Returning drops the transaction, which rolls it back.
    let Some(member) = member else {
        return Ok(None);
    };
    let policy = policy(
        member.actions,
        member.subjects,
        member.fields,
        member.conditions,
    )?;
    let role = member.role;
    sqlx::query!(
        "SELECT set_config('app.project_id', $1, true) AS project_scope",
        project.to_string()
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(Some(MemberTransaction {
        transaction,
        identity,
        project,
        role,
        policy,
    }))
}

/// A role's permissions from the arrays `begin_as_member` reads. A stored action or condition the
/// code doesn't know is corrupt data, not a permission to skip.
pub(crate) fn policy(
    actions: Vec<String>,
    subjects: Vec<String>,
    fields: Vec<Option<String>>,
    conditions: Vec<Option<String>>,
) -> Result<Policy, DbError> {
    let permissions = actions
        .into_iter()
        .zip(subjects)
        .zip(fields.into_iter().zip(conditions))
        .map(|((action, subject), (fields, condition))| {
            Ok(Permission {
                action: Action::parse(&action)
                    .ok_or(DbError::Corrupt("a role permission has an unknown action"))?,
                subject: Subject::parse(&subject),
                fields: fields.map(|list| list.split(',').map(str::to_owned).collect()),
                condition: match condition {
                    None => None,
                    Some(name) => Some(Condition::parse(&name).ok_or(DbError::Corrupt(
                        "a role permission has an unknown condition",
                    ))?),
                },
            })
        })
        .collect::<Result<Vec<_>, DbError>>()?;
    Ok(Policy::new(permissions))
}

/// [`begin_as_member`], then resolves the environment slug. `None` if either check fails.
pub async fn begin_in_project(
    pool: &PgPool,
    identity: Uuid,
    project: Uuid,
    environment: &str,
) -> Result<Option<ProjectTransaction>, DbError> {
    let Some(mut member) = begin_as_member(pool, identity, project).await? else {
        return Ok(None);
    };
    let environment = sqlx::query_scalar!(
        "SELECT id FROM environments WHERE project_id = $1 AND slug = $2",
        project,
        environment
    )
    .fetch_optional(&mut *member.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(environment.map(|environment| ProjectTransaction {
        transaction: member.transaction,
        identity,
        project,
        environment,
        role: member.role,
        policy: member.policy,
    }))
}

/// [`begin_as_member`], in an environment named by id (a schedule falling due knows its
/// environment's id, not the slug a request uses). `None` if either check fails.
pub async fn begin_in_environment(
    pool: &PgPool,
    identity: Uuid,
    project: Uuid,
    environment: Uuid,
) -> Result<Option<ProjectTransaction>, DbError> {
    let Some(mut member) = begin_as_member(pool, identity, project).await? else {
        return Ok(None);
    };
    let found = sqlx::query_scalar!(
        "SELECT id FROM environments WHERE project_id = $1 AND id = $2",
        project,
        environment
    )
    .fetch_optional(&mut *member.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(found.map(|environment| ProjectTransaction {
        transaction: member.transaction,
        identity,
        project,
        environment,
        role: member.role,
        policy: member.policy,
    }))
}
