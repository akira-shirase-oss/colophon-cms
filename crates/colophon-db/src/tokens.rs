//! A project's API tokens (docs/modules/03-auth.md, "API tokens"; D-058, D-063, step R.4).
//!
//! A token holds permissions the way a role does and acts as an identity of its own, so what it
//! writes is attributed to it. Only its secret's SHA-256 hash is stored. Writes run after the API
//! has checked `tokens.manage` and that the caller holds every power the token gets
//! ([`Policy::covers`]). Creating one queues on the project's owner role row, like creating a
//! role, so the count of active tokens can't race past [`MAX_TOKENS`]. A token is revoked, never
//! deleted; revoking it waits for the requests it is serving (they hold a share lock on its row),
//! so none is still running under it once revocation commits.

use colophon_domain::budget::Limits;
use colophon_domain::policy::{Condition, Policy, TokenKind};
use sqlx::{PgPool, types::time::OffsetDateTime};
use uuid::Uuid;

use crate::{DbError, scope::MemberTransaction};

/// The issuer of a token's identity: no identity provider uses it, so a token's identity can
/// never sign in, and nobody who signs in can be a token.
pub const IDENTITY_ISSUER: &str = "urn:colophon:api-token";

/// The most active (not revoked, not expired) tokens a project may have.
pub const MAX_TOKENS: i64 = 100;

/// A token, as its project's members see it. Never its secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredToken {
    /// Its id.
    pub id: Uuid,
    /// The identity it acts as.
    pub identity_id: Uuid,
    /// Its name, for people.
    pub name: String,
    /// What it's for.
    pub description: String,
    /// `read-only`, `full-access` or `custom`.
    pub kind: TokenKind,
    /// What it may do.
    pub policy: Policy,
    /// Who made it, and their name if known.
    pub created_by: Uuid,
    /// Their display name.
    pub created_by_name: Option<String>,
    /// When.
    pub created_at: OffsetDateTime,
    /// When it stops working.
    pub expires_at: OffsetDateTime,
    /// When it was last used, to the minute.
    pub last_used_at: Option<OffsetDateTime>,
    /// When it was revoked, if it was.
    pub revoked_at: Option<OffsetDateTime>,
    /// Its budget ceilings (migration 0045, D-113); `None` when it has no limits row — it is
    /// unconstrained at the quota layer then.
    pub limits: Option<Limits>,
}

/// A token to create.
#[derive(Debug)]
pub struct NewToken<'a> {
    /// Its id.
    pub id: Uuid,
    /// The identity it will act as (a new one).
    pub identity_id: Uuid,
    /// Its name.
    pub name: &'a str,
    /// What it's for.
    pub description: &'a str,
    /// Its kind.
    pub kind: TokenKind,
    /// Its permissions (for `read-only` and `full-access`, the kind's fixed ones).
    pub policy: &'a Policy,
    /// The SHA-256 hash of its secret.
    pub hash: [u8; 32],
    /// When it stops working.
    pub expires_at: OffsetDateTime,
    /// Budget ceilings to store with it, checked already (D-113).
    pub limits: Option<&'a Limits>,
}

/// What creating a token did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Created {
    /// Done; the token as stored.
    Done(Box<StoredToken>),
    /// The project has [`MAX_TOKENS`] active tokens already.
    TooMany,
}

/// What revoking a token did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revoked {
    /// Done; the token as stored now.
    Done(Box<StoredToken>),
    /// No such token in this project.
    NotFound,
    /// It was revoked already; nothing changed.
    Already(Box<StoredToken>),
}

/// A token found by its secret's hash, before its project's context is entered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    /// Its id.
    pub id: Uuid,
    /// Its project.
    pub project_id: Uuid,
    /// The identity it acts as.
    pub identity_id: Uuid,
    /// When it stops working.
    pub expires_at: OffsetDateTime,
    /// When it was revoked, if it was.
    pub revoked_at: Option<OffsetDateTime>,
    /// When it was last used, to the minute.
    pub last_used_at: Option<OffsetDateTime>,
}

/// The token whose secret hashes to `hash`, revoked and expired ones included (the caller
/// refuses those). Through `colophon_api_token`, the one lookup that runs outside a project's
/// context (migration 0023).
pub async fn find(pool: &PgPool, hash: &[u8; 32]) -> Result<Option<Found>, DbError> {
    let row = sqlx::query!(
        r#"SELECT id AS "id!", project_id AS "project_id!", identity_id AS "identity_id!",
                  expires_at AS "expires_at!", revoked_at, last_used_at
           FROM colophon_api_token($1)"#,
        hash.as_slice()
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)?;
    Ok(row.map(|row| Found {
        id: row.id,
        project_id: row.project_id,
        identity_id: row.identity_id,
        expires_at: row.expires_at,
        revoked_at: row.revoked_at,
        last_used_at: row.last_used_at,
    }))
}

/// Records that `token` was used now, if it wasn't in the last minute. In a transaction of its
/// own, never a request's: the update locks the token's row, which requests hold a share lock
/// on while they run.
pub async fn touch(pool: &PgPool, token: &Found) -> Result<(), DbError> {
    let mut transaction = crate::scope::begin(
        pool,
        crate::scope::Scope {
            identity: Some(token.identity_id),
            project: Some(token.project_id),
        },
    )
    .await?;
    sqlx::query!(
        "UPDATE api_tokens SET last_used_at = now()
         WHERE project_id = $1 AND id = $2
           AND (last_used_at IS NULL OR last_used_at < now() - interval '1 minute')",
        token.project_id,
        token.id
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_write)?;
    transaction.commit().await.map_err(DbError::from_query)
}

/// The project's tokens, newest first (at most 200), or the one with `id`.
async fn read(tx: &mut MemberTransaction, id: Option<Uuid>) -> Result<Vec<StoredToken>, DbError> {
    let rows = sqlx::query!(
        r#"SELECT t.id, t.identity_id, t.name, t.description, t.kind, t.created_by,
                  n.display_name AS created_by_name, t.created_at, t.expires_at, t.last_used_at,
                  t.revoked_at, l.token_id AS "limited?",
                  l.documents_per_day AS "documents_per_day?",
                  l.max_concurrent_jobs AS "max_concurrent_jobs?",
                  l.requests_per_minute AS "requests_per_minute?",
                  l.tokens_per_day AS "tokens_per_day?",
                  l.cost_per_day_micros AS "cost_per_day_micros?",
                  coalesce(p.actions, '{}') AS "actions!",
                  coalesce(p.subjects, '{}') AS "subjects!",
                  coalesce(p.fields, '{}') AS "fields!: Vec<Option<String>>",
                  coalesce(p.conditions, '{}') AS "conditions!: Vec<Option<String>>"
           FROM api_tokens t
           LEFT JOIN identities n ON n.id = t.created_by
           LEFT JOIN api_token_limits l
                  ON l.project_id = t.project_id AND l.token_id = t.id
           LEFT JOIN LATERAL (
               SELECT array_agg(action ORDER BY action, subject) AS actions,
                      array_agg(subject ORDER BY action, subject) AS subjects,
                      array_agg(array_to_string(fields, ',') ORDER BY action, subject) AS fields,
                      array_agg(condition ORDER BY action, subject) AS conditions
               FROM api_token_permissions
               WHERE project_id = t.project_id AND token_id = t.id
           ) p ON true
           WHERE t.project_id = $1 AND ($2::uuid IS NULL OR t.id = $2)
           ORDER BY t.created_at DESC, t.id DESC
           LIMIT 200"#,
        tx.project,
        id
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    rows.into_iter()
        .map(|row| {
            Ok(StoredToken {
                kind: TokenKind::parse(&row.kind)
                    .ok_or(DbError::Corrupt("a token has an unknown kind"))?,
                policy: crate::scope::policy(
                    row.actions,
                    row.subjects,
                    row.fields,
                    row.conditions,
                )?,
                id: row.id,
                identity_id: row.identity_id,
                name: row.name,
                description: row.description,
                created_by: row.created_by,
                created_by_name: row.created_by_name,
                created_at: row.created_at,
                expires_at: row.expires_at,
                last_used_at: row.last_used_at,
                revoked_at: row.revoked_at,
                limits: row.limited.map(|_| Limits {
                    documents_per_day: row.documents_per_day,
                    max_concurrent_jobs: row.max_concurrent_jobs,
                    requests_per_minute: row.requests_per_minute,
                    tokens_per_day: row.tokens_per_day,
                    cost_per_day_micros: row.cost_per_day_micros,
                }),
            })
        })
        .collect()
}

/// The project's tokens, newest first.
pub async fn list(tx: &mut MemberTransaction) -> Result<Vec<StoredToken>, DbError> {
    read(tx, None).await
}

/// The token with this id.
pub async fn get(tx: &mut MemberTransaction, id: Uuid) -> Result<Option<StoredToken>, DbError> {
    Ok(read(tx, Some(id)).await?.pop())
}

/// Creates a token, its identity and its permissions. The caller checked who may.
///
/// The project's owner-role row is the creation lock: tokens (and roles) are made one at a time
/// per project, so the count of active tokens never races past [`MAX_TOKENS`]. The lock is
/// held until commit, so everything under it is kept to two statements: the lock itself, alone
/// — so the next statement's snapshot is taken after it is held, and sees the token the
/// previous holder committed — then the count check and every insert in one statement. The
/// answer is built from what was written, not read back. Before, eight round trips under the
/// lock let creators queue, each holding a pooled connection, until reads starved.
#[expect(
    clippy::too_many_lines,
    reason = "the lock, then one statement holding every write, kept together on purpose"
)]
pub async fn create(tx: &mut MemberTransaction, token: &NewToken<'_>) -> Result<Created, DbError> {
    // Statement 1: queue behind any other creation of a role or token in the project.
    sqlx::query_scalar!(
        "SELECT 1 AS locked FROM roles WHERE project_id = $1 AND key = 'owner' FOR NO KEY UPDATE",
        tx.project
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?
    .ok_or(DbError::Corrupt("a project has no owner role"))?;
    let permissions = token.policy.permissions();
    let actions: Vec<&str> = permissions.iter().map(|p| p.action.as_str()).collect();
    let subjects: Vec<&str> = permissions.iter().map(|p| p.subject.as_str()).collect();
    // Field ids never contain commas (schema::is_field_id), so a list travels joined.
    let fields: Vec<Option<String>> = permissions
        .iter()
        .map(|p| p.fields.as_ref().map(|list| list.join(",")))
        .collect();
    let conditions: Vec<Option<&str>> = permissions
        .iter()
        .map(|p| p.condition.map(Condition::as_str))
        .collect();
    let subject = token.id.to_string();
    // Statement 2: the cap and every insert, in one snapshot taken after the lock is held.
    // Nothing is inserted when the project is at the cap: `ident` selects from `room`, and
    // each later insert from the one before it.
    let made = sqlx::query!(
        r#"WITH active AS (
               SELECT count(*) AS n FROM api_tokens
               WHERE project_id = $1 AND revoked_at IS NULL AND expires_at > now()
           ), room AS (
               SELECT 1 FROM active WHERE n < $2
           ), ident AS (
               INSERT INTO identities (id, issuer, subject, display_name)
               SELECT $3, $4, $5, $6 FROM room
               RETURNING id
           ), made AS (
               INSERT INTO api_tokens (id, project_id, identity_id, name, description, kind,
                                       token_hash, created_by, expires_at)
               SELECT $7, $1, ident.id, $6, $8, $9, $10, $11, $12 FROM ident
               RETURNING id, created_at, expires_at
           ), granted AS (
               INSERT INTO api_token_permissions (project_id, token_id, action, subject, fields,
                                                  condition)
               SELECT $1, made.id, p.action, p.subject, string_to_array(p.fields, ','),
                      p.condition
               FROM made, unnest($13::text[], $14::text[], $15::text[], $16::text[])
                    AS p (action, subject, fields, condition)
               RETURNING action, subject, fields, condition
           )
           SELECT (SELECT created_at FROM made) AS created_at,
                  (SELECT expires_at FROM made) AS expires_at,
                  (SELECT display_name FROM identities WHERE id = $11) AS created_by_name,
                  (SELECT count(*) FROM granted) AS "granted!",
                  -- The permissions as stored, aggregated exactly as `read` aggregates them, so
                  -- the answer is what a later read of the token returns.
                  (SELECT coalesce(array_agg(action ORDER BY action, subject), '{}')
                   FROM granted) AS "actions!",
                  (SELECT coalesce(array_agg(subject ORDER BY action, subject), '{}')
                   FROM granted) AS "subjects!",
                  (SELECT coalesce(array_agg(array_to_string(fields, ',')
                                             ORDER BY action, subject), '{}')
                   FROM granted) AS "fields!: Vec<Option<String>>",
                  (SELECT coalesce(array_agg(condition ORDER BY action, subject), '{}')
                   FROM granted) AS "conditions!: Vec<Option<String>>""#,
        tx.project,
        MAX_TOKENS,
        token.identity_id,
        IDENTITY_ISSUER,
        subject,
        token.name,
        token.id,
        token.description,
        token.kind.as_str(),
        token.hash.as_slice(),
        tx.identity,
        token.expires_at,
        &actions as &[&str],
        &subjects as &[&str],
        &fields as &[Option<String>],
        &conditions as &[Option<&str>]
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    let (Some(created_at), Some(expires_at)) = (made.created_at, made.expires_at) else {
        return Ok(Created::TooMany);
    };
    if usize::try_from(made.granted).ok() != Some(permissions.len()) {
        return Err(DbError::Corrupt(
            "a token's permissions were not all stored",
        ));
    }
    if let Some(limits) = token.limits {
        crate::budgets::set_limits(tx, token.id, limits).await?;
    }
    // Every stored field as the database wrote it (`RETURNING`), so the answer is what a later
    // read returns; the rest as given.
    Ok(Created::Done(Box::new(StoredToken {
        id: token.id,
        identity_id: token.identity_id,
        name: token.name.to_owned(),
        description: token.description.to_owned(),
        kind: token.kind,
        policy: crate::scope::policy(made.actions, made.subjects, made.fields, made.conditions)?,
        created_by: tx.identity,
        created_by_name: made.created_by_name,
        created_at,
        expires_at,
        last_used_at: None,
        revoked_at: None,
        limits: token.limits.copied(),
    })))
}

/// Revokes a token, by the transaction's identity. Waits for requests it is serving.
pub async fn revoke(tx: &mut MemberTransaction, id: Uuid) -> Result<Revoked, DbError> {
    let changed = sqlx::query!(
        "UPDATE api_tokens SET revoked_at = now(), revoked_by = $3
         WHERE project_id = $1 AND id = $2 AND revoked_at IS NULL",
        tx.project,
        id,
        tx.identity
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?
    .rows_affected();
    let Some(token) = get(tx, id).await? else {
        return Ok(Revoked::NotFound);
    };
    Ok(if changed == 1 {
        Revoked::Done(Box::new(token))
    } else {
        Revoked::Already(Box::new(token))
    })
}
