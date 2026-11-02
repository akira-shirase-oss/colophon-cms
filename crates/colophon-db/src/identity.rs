//! Identities, sessions and sign-in attempts (docs/modules/03-auth.md). None of these tables is
//! project-scoped; the application role reaches them through these functions only.

use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    DbError,
    projects::{self, Project},
    scope::{self, Scope},
};

/// How stale `last_seen_at` may get before a request records the session's use. Callers check
/// it before calling [`touch_session`], so most requests don't write at all.
pub const TOUCH_INTERVAL: Duration = Duration::MINUTE;

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
    .map_err(DbError::from_query)?;
    Ok(row.id)
}

/// The `issuer` for email-and-password accounts (docs/modules/03-auth.md, D-084).
pub const PASSWORD_ISSUER: &str = "password";

/// Consecutive misses before the account pauses — bounded, so guessing costs time and shows.
pub const LOCK_AFTER: i16 = 8;
/// How long a locked account stays locked.
pub const LOCK_FOR: Duration = Duration::minutes(15);

/// What signing up did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignedUp {
    /// A new identity with a password credential.
    Made(Uuid),
    /// That email already has an account.
    Taken,
}

/// Creates an email-and-password account: `('password', email)` with the Argon2id hash and a
/// display name. `Taken` if the email already has one.
pub async fn sign_up(
    pool: &PgPool,
    email: &str,
    password_hash: &str,
    display_name: Option<&str>,
) -> Result<SignedUp, DbError> {
    let row = sqlx::query_scalar!(
        "INSERT INTO identities (id, issuer, subject, display_name, email, password_hash)
         VALUES ($1, $2, $3, $4, $3, $5)
         ON CONFLICT DO NOTHING
         RETURNING id",
        Uuid::now_v7(),
        PASSWORD_ISSUER,
        email,
        display_name,
        password_hash
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)?;
    Ok(match row {
        Some(id) => SignedUp::Made(id),
        None => SignedUp::Taken,
    })
}

/// The answer to an email-and-password sign-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordSignIn {
    /// Right password; the counter resets and the session may start.
    SignedIn(Uuid),
    /// Wrong password, or no such account — the same answer, after the same work.
    Refused,
    /// Too many misses; the account pauses until this time.
    Locked(OffsetDateTime),
}

/// Signs in with an email and a password: the lock check, the verify and the counter all run
/// under one row lock, so a locked account can't be hammered concurrently.
pub async fn password_sign_in(
    pool: &PgPool,
    email: &str,
    password: &str,
) -> Result<PasswordSignIn, DbError> {
    let mut transaction = pool.begin().await.map_err(DbError::from_query)?;
    let row = sqlx::query!(
        "SELECT id, password_hash, locked_until FROM identities
         WHERE issuer = $1 AND subject = $2
         FOR UPDATE",
        PASSWORD_ISSUER,
        email
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    let Some(row) = row else {
        // No such account: the same verify work runs anyway, so the answer can't be told apart.
        let _ = crate::passwords::verify_in(password, None).await;
        return Ok(PasswordSignIn::Refused);
    };
    if row
        .locked_until
        .is_some_and(|until| until > OffsetDateTime::now_utc())
    {
        return Ok(PasswordSignIn::Locked(
            row.locked_until.unwrap_or_else(OffsetDateTime::now_utc),
        ));
    }
    let right = crate::passwords::verify_in(password, row.password_hash.as_deref()).await;
    if right {
        sqlx::query!(
            "UPDATE identities SET failed_sign_ins = 0, locked_until = NULL, last_sign_in_at = now()
             WHERE id = $1",
            row.id
        )
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
        transaction.commit().await.map_err(DbError::from_query)?;
        return Ok(PasswordSignIn::SignedIn(row.id));
    }
    let outcome = sqlx::query_scalar!(
        "UPDATE identities
         SET failed_sign_ins = least(failed_sign_ins + 1, 32767),
             locked_until = CASE WHEN failed_sign_ins + 1 >= $2 THEN now() + make_interval(secs => $3) END
         WHERE id = $1
         RETURNING locked_until",
        row.id,
        i32::from(LOCK_AFTER),
        f64::from(u32::try_from(LOCK_FOR.whole_seconds()).unwrap_or(u32::MAX))
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(match outcome {
        Some(until) => PasswordSignIn::Locked(until),
        None => PasswordSignIn::Refused,
    })
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
    .map_err(DbError::from_query)?;
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
    .map_err(DbError::from_query)?;
    Ok(row)
}

/// Marks a session as used now. The statement itself also skips sessions touched within
/// [`TOUCH_INTERVAL`], so concurrent requests on one session write once.
pub async fn touch_session(pool: &PgPool, id: Uuid) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE sessions SET last_seen_at = now() WHERE id = $1 AND last_seen_at < now() - interval '1 minute'",
        id
    )
    .execute(pool)
    .await
    .map_err(DbError::from_query)?;
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
    .map_err(DbError::from_query)?;
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
    .map_err(DbError::from_query)?;
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
    .map_err(DbError::from_query)?;
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
    .map_err(DbError::from_query)?;
    Ok(row)
}

/// An identity, as `/me` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// Its id.
    pub id: Uuid,
    /// The provider that vouches for it.
    pub issuer: String,
    /// Its id at that provider: what `colophon admin bootstrap` binds to.
    pub subject: String,
    /// A name to show, if the provider gave one.
    pub display_name: Option<String>,
    /// Whether it is one of the installation's administrators (D-043).
    pub administrator: bool,
    /// Whether the installation has an administrator at all: until it does, a setup code from the
    /// server's terminal makes the first one (D-061).
    pub installation_set_up: bool,
}

/// An identity and the projects it is a member of: everything `/me` shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// The identity.
    pub identity: Identity,
    /// Its projects, as [`projects::list`] returns them.
    pub projects: Vec<Project>,
}

/// The identity with this id and its projects, read in one transaction (one pooled connection).
pub async fn profile(pool: &PgPool, id: Uuid) -> Result<Option<Profile>, DbError> {
    let mut transaction = scope::begin(
        pool,
        Scope {
            identity: Some(id),
            project: None,
        },
    )
    .await?;
    let identity = sqlx::query_as!(
        Identity,
        r#"SELECT i.id, i.issuer, i.subject, i.display_name,
                  EXISTS (SELECT 1 FROM administrators a WHERE a.identity_id = i.id) AS "administrator!",
                  colophon_installation_set_up() AS "installation_set_up!"
           FROM identities i WHERE i.id = $1"#,
        id
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    let profile = match identity {
        Some(identity) => Some(Profile {
            identity,
            projects: projects::list_in(&mut transaction, id).await?,
        }),
        None => None,
    };
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(profile)
}
