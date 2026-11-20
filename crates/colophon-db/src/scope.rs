//! Scoped transactions: the row-level-security context Postgres enforces (docs/modules/02-db.md,
//! "Row-level security and roles").
//!
//! Every project-scoped table denies everything unless the transaction says who is asking
//! (`app.identity_id`) and, for work inside a project, which project (`app.project_id`). The
//! settings are transaction-local (`set_config(..., true)`), so they end with the transaction and
//! can't leak into the next request that borrows the same pooled connection.

use colophon_domain::policy::{Action, Condition, Permission, Policy, Subject};
use sqlx::{AssertSqlSafe, PgPool, Postgres, Transaction};
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

/// How a transaction's reads see the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Isolation {
    /// Postgres's default: each statement sees what was committed when it began.
    #[default]
    PerStatement,
    /// One snapshot for the whole transaction (`REPEATABLE READ`): for a read made of several
    /// statements that must agree — a release receipt's deliveries and the locations that saw
    /// them, say — so a commit landing between two of them can't show half of itself.
    Snapshot,
}

/// Begins a transaction with `scope` as its row-level-security context.
pub async fn begin(pool: &PgPool, scope: Scope) -> Result<Transaction<'static, Postgres>, DbError> {
    begin_isolated(pool, scope, Isolation::PerStatement).await
}

/// [`begin`], with the reads seeing the database as `isolation` says.
pub async fn begin_isolated(
    pool: &PgPool,
    scope: Scope,
    isolation: Isolation,
) -> Result<Transaction<'static, Postgres>, DbError> {
    let mut transaction = match isolation {
        Isolation::PerStatement => crate::begin_tx(pool).await,
        Isolation::Snapshot => crate::begin_tx_snapshot(pool).await,
    }
    .map_err(DbError::from_query)?;
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
    /// The API token this transaction runs as, when it does (D-118); `None` for a person.
    pub token: Option<TokenScope>,
}

/// The API token a transaction runs as (D-118): what the request-time quota needs — the
/// token's id, for the audit log to name, and its `requests_per_minute` ceiling, read with the
/// same `FOR SHARE` row the token check already took, so the limit costs nothing extra.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenScope {
    /// The token's id.
    pub id: Uuid,
    /// Its `api_token_limits.requests_per_minute`; `None` — no limits row, or the field
    /// NULL — is unconstrained at the request-quota layer.
    pub requests_per_minute: Option<i64>,
}

impl ProjectTransaction {
    /// Commits.
    pub async fn commit(self) -> Result<(), DbError> {
        self.transaction.commit().await.map_err(DbError::from_query)
    }
}

/// A named savepoint inside `tx`, so a failing step rolls back to it without losing the
/// transaction's earlier work (an import dry-run's per-item isolation, D-114). `name` is a
/// call-site literal: savepoint names can't be bound parameters, which is what the
/// `AssertSqlSafe` wrap records — there is nothing else to inject.
pub async fn savepoint(tx: &mut ProjectTransaction, name: &'static str) -> Result<(), DbError> {
    sqlx::query(AssertSqlSafe(format!("SAVEPOINT {name}")))
        .execute(&mut *tx.transaction)
        .await
        .map_err(DbError::from_write)?;
    Ok(())
}

/// Releases `name`: the work since [`savepoint`] is part of the transaction now.
pub async fn release(tx: &mut ProjectTransaction, name: &'static str) -> Result<(), DbError> {
    sqlx::query(AssertSqlSafe(format!("RELEASE SAVEPOINT {name}")))
        .execute(&mut *tx.transaction)
        .await
        .map_err(DbError::from_write)?;
    Ok(())
}

/// Undoes everything since `name` and leaves the transaction usable.
pub async fn rollback_to(tx: &mut ProjectTransaction, name: &'static str) -> Result<(), DbError> {
    sqlx::query(AssertSqlSafe(format!("ROLLBACK TO SAVEPOINT {name}")))
        .execute(&mut *tx.transaction)
        .await
        .map_err(DbError::from_write)?;
    Ok(())
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
    /// The API token this transaction runs as, when it does (D-118); `None` for a person.
    pub token: Option<TokenScope>,
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
    begin_as_member_isolated(pool, identity, project, Isolation::PerStatement).await
}

/// [`begin_as_member`], with the reads seeing the database as `isolation` says.
pub async fn begin_as_member_isolated(
    pool: &PgPool,
    identity: Uuid,
    project: Uuid,
    isolation: Isolation,
) -> Result<Option<MemberTransaction>, DbError> {
    Ok(enter_as_member(pool, identity, project, None, isolation)
        .await?
        .map(|(member, _)| member))
}

/// Begins a transaction and enters `project` as `identity` in one round trip after BEGIN (D-227):
/// `colophon_enter_project` (migration 0079) sets the identity alone, reads the membership and its
/// role's permissions, and only for a member sets the project and resolves `environment`. The same
/// statements in the same order as when they were separate round trips, so the project is set only
/// once the membership is proven. `None` if `identity` isn't a member: returning drops the
/// transaction, which rolls it back.
async fn enter_as_member(
    pool: &PgPool,
    identity: Uuid,
    project: Uuid,
    environment: Option<&str>,
    isolation: Isolation,
) -> Result<Option<(MemberTransaction, Option<Uuid>)>, DbError> {
    let mut transaction = begin_bare(pool, isolation).await?;
    let row = sqlx::query(
        "SELECT role, actions, subjects, fields, conditions, environment_id
         FROM colophon_enter_project($1, $2, $3)",
    )
    .bind(identity)
    .bind(project)
    .bind(environment)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let decoded = decode_context(&row, true)?;
    Ok(Some((
        MemberTransaction {
            transaction,
            identity,
            project,
            role: decoded.role.unwrap_or_default(),
            policy: decoded.policy,
            token: None,
        },
        decoded.environment,
    )))
}

/// BEGIN at `isolation`, with no context set yet: the context functions set it.
async fn begin_bare(
    pool: &PgPool,
    isolation: Isolation,
) -> Result<Transaction<'static, Postgres>, DbError> {
    match isolation {
        Isolation::PerStatement => crate::begin_tx(pool).await,
        Isolation::Snapshot => crate::begin_tx_snapshot(pool).await,
    }
    .map_err(DbError::from_query)
}

/// One context function's row, decoded.
struct Context {
    role: Option<String>,
    policy: Policy,
    environment: Option<Uuid>,
}

fn decode_context(row: &sqlx::postgres::PgRow, with_role: bool) -> Result<Context, DbError> {
    use sqlx::Row as _;
    let get = |error: sqlx::Error| DbError::from_query(error);
    let role = if with_role {
        Some(row.try_get::<String, _>("role").map_err(get)?)
    } else {
        None
    };
    let policy = policy(
        row.try_get::<Vec<String>, _>("actions").map_err(get)?,
        row.try_get::<Vec<String>, _>("subjects").map_err(get)?,
        row.try_get::<Vec<Option<String>>, _>("fields")
            .map_err(get)?,
        row.try_get::<Vec<Option<String>>, _>("conditions")
            .map_err(get)?,
    )?;
    let environment = row
        .try_get::<Option<Uuid>, _>("environment_id")
        .map_err(get)?;
    Ok(Context {
        role,
        policy,
        environment,
    })
}

/// Begins a transaction as the API token that acts as `identity`, in `project` (step R.4,
/// D-063): checks, inside the project's context, that the token belongs to the project and is
/// neither revoked nor expired, and reads its permissions — and, with the same row, its
/// `requests_per_minute` ceiling for the request-time quota (D-118). The check holds a share
/// lock on the token's row until the transaction ends, so revoking the token waits for this
/// request, and no request runs under a token once its revocation has committed. `None` if any
/// check fails.
pub async fn begin_as_token(
    pool: &PgPool,
    identity: Uuid,
    project: Uuid,
) -> Result<Option<MemberTransaction>, DbError> {
    begin_as_token_isolated(pool, identity, project, Isolation::PerStatement).await
}

/// [`begin_as_token`], with the reads seeing the database as `isolation` says.
pub async fn begin_as_token_isolated(
    pool: &PgPool,
    identity: Uuid,
    project: Uuid,
    isolation: Isolation,
) -> Result<Option<MemberTransaction>, DbError> {
    Ok(enter_as_token(pool, identity, project, None, isolation)
        .await?
        .map(|(token, _)| token))
}

/// Begins a transaction and enters `project` as the API token that acts as `identity`, in one round
/// trip after BEGIN (D-227): `colophon_enter_project_as_token` (migration 0079) sets both settings,
/// checks the token with the share lock that makes its revocation wait for this request (D-063),
/// reads its permissions and resolves `environment`: today's statements, in today's order. `None` if
/// there is no live token for `identity` in `project`.
async fn enter_as_token(
    pool: &PgPool,
    identity: Uuid,
    project: Uuid,
    environment: Option<&str>,
    isolation: Isolation,
) -> Result<Option<(MemberTransaction, Option<Uuid>)>, DbError> {
    use sqlx::Row as _;
    let mut transaction = begin_bare(pool, isolation).await?;
    let row = sqlx::query(
        "SELECT token_id, requests_per_minute, actions, subjects, fields, conditions, environment_id
         FROM colophon_enter_project_as_token($1, $2, $3)",
    )
    .bind(identity)
    .bind(project)
    .bind(environment)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let decoded = decode_context(&row, false)?;
    let scope = TokenScope {
        id: row.try_get("token_id").map_err(DbError::from_query)?,
        requests_per_minute: row
            .try_get("requests_per_minute")
            .map_err(DbError::from_query)?,
    };
    Ok(Some((
        MemberTransaction {
            transaction,
            identity,
            project,
            role: TOKEN_ROLE.to_owned(),
            policy: decoded.policy,
            token: Some(scope),
        },
        decoded.environment,
    )))
}

/// What a token's transaction reports as its role: no role has this key (role keys are
/// lowercase letters, digits and hyphens), so nothing that looks a role up finds it.
pub const TOKEN_ROLE: &str = "API token";

/// [`begin_as_token`], then resolves the environment slug. `None` if either check fails.
pub async fn begin_in_project_as_token(
    pool: &PgPool,
    identity: Uuid,
    project: Uuid,
    environment: &str,
) -> Result<Option<ProjectTransaction>, DbError> {
    begin_in_project_as_token_isolated(
        pool,
        identity,
        project,
        environment,
        Isolation::PerStatement,
    )
    .await
}

/// [`begin_in_project_as_token`], with the reads seeing the database as `isolation` says.
pub async fn begin_in_project_as_token_isolated(
    pool: &PgPool,
    identity: Uuid,
    project: Uuid,
    environment: &str,
    isolation: Isolation,
) -> Result<Option<ProjectTransaction>, DbError> {
    let Some((token, found)) =
        enter_as_token(pool, identity, project, Some(environment), isolation).await?
    else {
        return Ok(None);
    };
    Ok(found.map(|environment| narrowed(token, environment)))
}

/// A member's (or a token's) transaction, narrowed to `environment`, which the context function
/// resolved in the same round trip (D-227). `None` upstream if the project has no such environment.
fn narrowed(member: MemberTransaction, environment: Uuid) -> ProjectTransaction {
    ProjectTransaction {
        transaction: member.transaction,
        identity: member.identity,
        project: member.project,
        environment,
        role: member.role,
        policy: member.policy,
        token: member.token,
    }
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
    begin_in_project_isolated(
        pool,
        identity,
        project,
        environment,
        Isolation::PerStatement,
    )
    .await
}

/// [`begin_in_project`], with the reads seeing the database as `isolation` says.
pub async fn begin_in_project_isolated(
    pool: &PgPool,
    identity: Uuid,
    project: Uuid,
    environment: &str,
    isolation: Isolation,
) -> Result<Option<ProjectTransaction>, DbError> {
    let Some((member, found)) =
        enter_as_member(pool, identity, project, Some(environment), isolation).await?
    else {
        return Ok(None);
    };
    Ok(found.map(|environment| narrowed(member, environment)))
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
        token: member.token,
    }))
}

/// [`begin_as_token`], in an environment named by id: the transaction a bulk job's worker
/// opens for each item when its submitter was a project API token (D-116). The same
/// `FOR SHARE` token row check stands, so a token revoked since the job was accepted gets
/// `None` — the item it couldn't open is recorded, and the run stops.
pub async fn begin_in_environment_as_token(
    pool: &PgPool,
    identity: Uuid,
    project: Uuid,
    environment: Uuid,
) -> Result<Option<ProjectTransaction>, DbError> {
    let Some(mut member) = begin_as_token(pool, identity, project).await? else {
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
        token: member.token,
    }))
}
