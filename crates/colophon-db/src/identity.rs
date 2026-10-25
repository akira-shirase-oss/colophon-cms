//! Identities, sessions and sign-in attempts (docs/modules/03-auth.md). None of these tables is
//! project-scoped; the application role reaches them through these functions only.

use sqlx::{PgPool, types::time::OffsetDateTime};
use uuid::Uuid;

use crate::DbError;

/// Records a sign-in by `(issuer, subject)`: creates the identity on first sight, refreshes its
/// display name and sign-in time after that. Returns its id.
pub async fn record_sign_in(
    pool: &PgPool,
    issuer: &str,
    subject: &str,
    display_name: Option<&str>,
) -> Result<Uuid, DbError> {
    let row = sqlx::query!(
        "INSERT INTO identities (id, issuer, subject, display_name)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (issuer, subject)
         DO UPDATE SET display_name = EXCLUDED.display_name, last_sign_in_at = now()
         RETURNING id",
        Uuid::now_v7(),
        issuer,
        subject,
        display_name
    )
    .fetch_one(pool)
    .await
    .map_err(DbError::Query)?;
    Ok(row.id)
}

/// A live session, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The session's id (not the token).
    pub id: Uuid,
    /// Who it belongs to.
    pub identity_id: Uuid,
    /// SHA-256 of the session's CSRF token.
    pub csrf_hash: Vec<u8>,
    /// When it was last used, for the idle timeout.
    pub last_seen_at: OffsetDateTime,
    /// When it ends regardless of use.
    pub expires_at: OffsetDateTime,
}

/// Stores a new session. Only hashes are stored.
pub async fn create_session(
    pool: &PgPool,
    identity_id: Uuid,
    token_hash: &[u8; 32],
    csrf_hash: &[u8; 32],
    expires_at: OffsetDateTime,
) -> Result<Uuid, DbError> {
    let id = Uuid::now_v7();
    sqlx::query!(
        "INSERT INTO sessions (id, token_hash, csrf_hash, identity_id, expires_at) VALUES ($1, $2, $3, $4, $5)",
        id,
        &token_hash[..],
        &csrf_hash[..],
        identity_id,
        expires_at
    )
    .execute(pool)
    .await
    .map_err(DbError::Query)?;
    Ok(id)
}

/// The session with this token hash, if it exists, isn't revoked and hasn't expired.
pub async fn find_session(
    pool: &PgPool,
    token_hash: &[u8; 32],
) -> Result<Option<Session>, DbError> {
    let row = sqlx::query_as!(
        Session,
        "SELECT id, identity_id, csrf_hash, last_seen_at, expires_at FROM sessions
         WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now()",
        &token_hash[..]
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::Query)?;
    Ok(row)
}

/// Marks a session as used now, at most once a minute, so busy sessions don't write on every
/// request.
pub async fn touch_session(pool: &PgPool, id: Uuid) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE sessions SET last_seen_at = now() WHERE id = $1 AND last_seen_at < now() - interval '1 minute'",
        id
    )
    .execute(pool)
    .await
    .map_err(DbError::Query)?;
    Ok(())
}

/// Revokes a session. Revoked sessions are kept (not deleted) for the audit trail.
pub async fn revoke_session(pool: &PgPool, id: Uuid) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE sessions SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL",
        id
    )
    .execute(pool)
    .await
    .map_err(DbError::Query)?;
    Ok(())
}

/// The state kept between leaving for the identity provider and coming back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignInAttempt {
    /// The nonce the ID token must carry.
    pub nonce: String,
    /// The PKCE code verifier for the token exchange.
    pub pkce_verifier: String,
    /// Where to send the person afterwards (a path on this site).
    pub return_to: String,
}

/// Stores a sign-in attempt, and clears out a bounded number of expired ones.
pub async fn start_sign_in(
    pool: &PgPool,
    state_hash: &[u8; 32],
    attempt: &SignInAttempt,
    expires_at: OffsetDateTime,
) -> Result<(), DbError> {
    sqlx::query!(
        "DELETE FROM sign_in_attempts WHERE state_hash IN
           (SELECT state_hash FROM sign_in_attempts WHERE expires_at < now() LIMIT 100)"
    )
    .execute(pool)
    .await
    .map_err(DbError::Query)?;
    sqlx::query!(
        "INSERT INTO sign_in_attempts (state_hash, nonce, pkce_verifier, return_to, expires_at)
         VALUES ($1, $2, $3, $4, $5)",
        &state_hash[..],
        attempt.nonce,
        attempt.pkce_verifier,
        attempt.return_to,
        expires_at
    )
    .execute(pool)
    .await
    .map_err(DbError::Query)?;
    Ok(())
}

/// Takes the attempt for this state: returns it once, and never again (it is deleted), and only
/// if it hasn't expired.
pub async fn finish_sign_in(
    pool: &PgPool,
    state_hash: &[u8; 32],
) -> Result<Option<SignInAttempt>, DbError> {
    let row = sqlx::query_as!(
        SignInAttempt,
        "DELETE FROM sign_in_attempts WHERE state_hash = $1 AND expires_at > now()
         RETURNING nonce, pkce_verifier, return_to",
        &state_hash[..]
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::Query)?;
    Ok(row)
}

/// An identity, as `/me` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// Its id.
    pub id: Uuid,
    /// The provider that vouches for it.
    pub issuer: String,
    /// A name to show, if the provider gave one.
    pub display_name: Option<String>,
}

/// The identity with this id.
pub async fn find_identity(pool: &PgPool, id: Uuid) -> Result<Option<Identity>, DbError> {
    let row = sqlx::query_as!(
        Identity,
        "SELECT id, issuer, display_name FROM identities WHERE id = $1",
        id
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::Query)?;
    Ok(row)
}
