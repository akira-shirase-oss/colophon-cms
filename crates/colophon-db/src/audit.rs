//! A project's audit log (docs/modules/03-auth.md, "Audit log"; D-058, D-063, step R.4).
//!
//! Every change to roles, memberships, invitations and API tokens is recorded in the transaction
//! that makes it, so a change and its record commit together or not at all. The log is
//! append-only: the API's database role may insert and read, and a trigger refuses updates,
//! deletes and truncation from anyone (migration 0023). Details are JSON the API builds, carried
//! as text so this crate needs no JSON library; they never hold a secret.

use sqlx::{Postgres, Transaction, types::time::OffsetDateTime};
use uuid::Uuid;

use crate::{DbError, scope::MemberTransaction, tokens::IDENTITY_ISSUER};

/// The most events one page returns.
pub const MAX_PAGE: i64 = 100;

/// One recorded change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Its id (time-ordered).
    pub id: Uuid,
    /// When.
    pub at: OffsetDateTime,
    /// Who: a person or an API token's identity.
    pub actor_id: Uuid,
    /// Their display name, or the token's name.
    pub actor_name: Option<String>,
    /// Whether the actor is an API token.
    pub actor_is_token: bool,
    /// `role.created`, `member.role_changed`, `token.revoked`, …
    pub action: String,
    /// What it was done to.
    pub subject: String,
    /// Before and after, as JSON text.
    pub detail: String,
}

/// Records `action` on `subject` by the transaction's identity, in its project and
/// transaction. `detail` is a JSON object as text.
pub async fn record(
    tx: &mut MemberTransaction,
    action: &str,
    subject: &str,
    detail: &str,
) -> Result<(), DbError> {
    record_in(
        &mut tx.transaction,
        tx.project,
        tx.identity,
        action,
        subject,
        detail,
    )
    .await
}

/// [`record`], for a transaction that is in the project's context without being a
/// [`MemberTransaction`] (accepting an invitation: the caller isn't a member until it commits).
pub async fn record_in(
    transaction: &mut Transaction<'static, Postgres>,
    project: Uuid,
    actor: Uuid,
    action: &str,
    subject: &str,
    detail: &str,
) -> Result<(), DbError> {
    sqlx::query!(
        "INSERT INTO audit_events (project_id, id, actor_id, action, subject, detail)
         VALUES ($1, $2, $3, $4, $5, $6::text::jsonb)",
        project,
        Uuid::now_v7(),
        actor,
        action,
        subject,
        detail
    )
    .execute(&mut **transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// The project's events, newest first, before the event `before` when given, at most `limit`
/// (capped at one more than [`MAX_PAGE`], so a caller can learn whether another page follows);
/// only `action`'s, or only `actor`'s, when given.
pub async fn list(
    tx: &mut MemberTransaction,
    before: Option<Uuid>,
    limit: i64,
    action: Option<&str>,
    actor: Option<Uuid>,
) -> Result<Vec<Event>, DbError> {
    let rows = sqlx::query!(
        r#"SELECT e.id, e.at, e.actor_id, n.display_name AS actor_name,
                  coalesce(n.issuer = $6, false) AS "actor_is_token!", e.action, e.subject,
                  e.detail::text AS "detail!"
           FROM audit_events e
           LEFT JOIN identities n ON n.id = e.actor_id
           WHERE e.project_id = $1
             AND ($2::uuid IS NULL OR e.id < $2)
             AND ($4::text IS NULL OR e.action = $4)
             AND ($5::uuid IS NULL OR e.actor_id = $5)
           ORDER BY e.id DESC
           LIMIT $3"#,
        tx.project,
        before,
        limit.clamp(1, MAX_PAGE + 1),
        action,
        actor,
        IDENTITY_ISSUER
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(rows
        .into_iter()
        .map(|row| Event {
            id: row.id,
            at: row.at,
            actor_id: row.actor_id,
            actor_name: row.actor_name,
            actor_is_token: row.actor_is_token,
            action: row.action,
            subject: row.subject,
            detail: row.detail,
        })
        .collect())
}
