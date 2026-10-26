//! Signing in from a terminal (docs/modules/03-auth.md; D-041): device logins approved in a
//! browser, and the personal access tokens they turn into. Neither table is project-scoped; the
//! application reaches them only through these functions. Only hashes are stored.

use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::DbError;

/// How stale a token's `last_used_at` may get before a request records its use.
pub const TOUCH_INTERVAL: Duration = Duration::MINUTE;

/// A device login, as the approval page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceLogin {
    /// What the client called itself, shown to the person approving.
    pub client_name: String,
    /// When the code stops working.
    pub expires_at: OffsetDateTime,
    /// Whether someone already approved it.
    pub approved: bool,
}

/// What polling a device login found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poll {
    /// Not approved yet.
    Pending,
    /// Approved, and now used up: issue a token for this identity.
    Approved {
        /// Who approved it.
        identity: Uuid,
        /// What the client called itself.
        client_name: String,
    },
    /// Unknown, expired, or already exchanged.
    Gone,
}

/// A live access token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessToken {
    /// The token's id (not the token).
    pub id: Uuid,
    /// Whose it is.
    pub identity_id: Uuid,
    /// When it was last used.
    pub last_used_at: OffsetDateTime,
}

/// Stores a device login, after clearing out a bounded number of expired ones. A user code
/// that is already taken is `DbError::Conflict`: draw another.
pub async fn start_device_login(
    pool: &PgPool,
    device_hash: &[u8; 32],
    user_code: &str,
    client_name: &str,
    expires_at: OffsetDateTime,
) -> Result<(), DbError> {
    sqlx::query!(
        "DELETE FROM device_logins WHERE device_hash IN
           (SELECT device_hash FROM device_logins WHERE expires_at < now() LIMIT 100)"
    )
    .execute(pool)
    .await
    .map_err(DbError::from_query)?;
    sqlx::query!(
        "INSERT INTO device_logins (device_hash, user_code, client_name, expires_at) VALUES ($1, $2, $3, $4)",
        &device_hash[..],
        user_code,
        client_name,
        expires_at
    )
    .execute(pool)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// The live device login with this user code.
pub async fn device_login(pool: &PgPool, user_code: &str) -> Result<Option<DeviceLogin>, DbError> {
    sqlx::query_as!(
        DeviceLogin,
        r#"SELECT client_name, expires_at, approved_by IS NOT NULL AS "approved!" FROM device_logins
           WHERE user_code = $1 AND expires_at > now()"#,
        user_code
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)
}

/// Approves a live, not yet approved device login as `identity`; false if there is none.
pub async fn approve_device_login(
    pool: &PgPool,
    user_code: &str,
    identity: Uuid,
) -> Result<bool, DbError> {
    let approved = sqlx::query_scalar!(
        r#"UPDATE device_logins SET approved_by = $2, approved_at = now()
           WHERE user_code = $1 AND approved_by IS NULL AND expires_at > now()
           RETURNING true AS "approved!""#,
        user_code,
        identity
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)?;
    Ok(approved.is_some())
}

/// Polls a device login by its secret's hash. An approved one is deleted as it is read, so it
/// turns into a token exactly once.
pub async fn poll_device_login(pool: &PgPool, device_hash: &[u8; 32]) -> Result<Poll, DbError> {
    let taken = sqlx::query!(
        r#"DELETE FROM device_logins
           WHERE device_hash = $1 AND approved_by IS NOT NULL AND expires_at > now()
           RETURNING approved_by AS "approved_by!", client_name"#,
        &device_hash[..]
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)?;
    if let Some(taken) = taken {
        return Ok(Poll::Approved {
            identity: taken.approved_by,
            client_name: taken.client_name,
        });
    }
    let pending = sqlx::query_scalar!(
        r#"SELECT true AS "pending!" FROM device_logins WHERE device_hash = $1 AND expires_at > now()"#,
        &device_hash[..]
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)?;
    Ok(if pending.is_some() {
        Poll::Pending
    } else {
        Poll::Gone
    })
}

/// Stores a new access token; only its hash.
pub async fn create_access_token(
    pool: &PgPool,
    identity: Uuid,
    token_hash: &[u8; 32],
    name: &str,
    expires_at: OffsetDateTime,
) -> Result<Uuid, DbError> {
    let id = Uuid::now_v7();
    sqlx::query!(
        "INSERT INTO access_tokens (id, token_hash, identity_id, name, expires_at) VALUES ($1, $2, $3, $4, $5)",
        id,
        &token_hash[..],
        identity,
        name,
        expires_at
    )
    .execute(pool)
    .await
    .map_err(DbError::from_write)?;
    Ok(id)
}

/// The access token with this hash, if it isn't revoked or expired.
pub async fn find_access_token(
    pool: &PgPool,
    token_hash: &[u8; 32],
) -> Result<Option<AccessToken>, DbError> {
    sqlx::query_as!(
        AccessToken,
        "SELECT id, identity_id, last_used_at FROM access_tokens
         WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now()",
        &token_hash[..]
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)
}

/// Records a token's use; the statement also skips tokens touched within [`TOUCH_INTERVAL`].
pub async fn touch_access_token(pool: &PgPool, id: Uuid) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE access_tokens SET last_used_at = now() WHERE id = $1 AND last_used_at < now() - interval '1 minute'",
        id
    )
    .execute(pool)
    .await
    .map_err(DbError::from_query)?;
    Ok(())
}

/// Revokes a token. Revoked tokens are kept for the audit trail.
pub async fn revoke_access_token(pool: &PgPool, id: Uuid) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE access_tokens SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL",
        id
    )
    .execute(pool)
    .await
    .map_err(DbError::from_query)?;
    Ok(())
}
