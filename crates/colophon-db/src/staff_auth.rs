//! The staff auth log and the sign-up cap (D-235, migration 0095).
//!
//! `staff_auth_events` records who signed up, signed in, failed to, signed out or had a session
//! rotated on the staff side, append-only, kept for the audit age. It never holds a password, a
//! token or a raw email: an event with no account to name carries the email's keyed digest
//! (HMAC-SHA256 under the installation's key, made by the caller) or nothing. The application
//! role may only insert.
//!
//! `signup_throttle` counts sign-up attempts per client address an hour, refused ones included,
//! through `colophon_signup_attempt`.

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::DbError;

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// An account was made.
    SignedUp,
    /// A sign-up was refused after it was counted (bot check, registration, a taken email).
    SignUpRefused,
    /// A sign-in succeeded.
    SignedIn,
    /// A sign-in was refused.
    SignInFailed,
    /// A session was ended by its holder.
    SignedOut,
    /// A session was replaced by a new one (sign-in over an old one, a change of standing).
    SessionRotated,
    /// An administrator disabled an account.
    AccountDisabled,
    /// An administrator enabled it again.
    AccountEnabled,
}

impl Event {
    /// The stored name (migration 0095's CHECK).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SignedUp => "signed_up",
            Self::SignUpRefused => "sign_up_refused",
            Self::SignedIn => "signed_in",
            Self::SignInFailed => "sign_in_failed",
            Self::SignedOut => "signed_out",
            Self::SessionRotated => "session_rotated",
            Self::AccountDisabled => "account_disabled",
            Self::AccountEnabled => "account_enabled",
        }
    }
}

/// One line of the log.
#[derive(Debug, Clone, Copy)]
pub struct Line<'a> {
    /// What happened.
    pub event: Event,
    /// The account it is about, when there is one.
    pub identity: Option<Uuid>,
    /// Who acted, when not the account itself.
    pub actor: Option<Uuid>,
    /// The email's keyed digest, for an event with no account to name.
    pub email_digest: Option<&'a [u8; 32]>,
    /// The client address, as the throttles key it (`-` when unknown).
    pub address: &'a str,
    /// A short reason (lowercase letters and underscores).
    pub detail: Option<&'static str>,
}

impl<'a> Line<'a> {
    /// A line about `event` from `address`, naming nothing else yet.
    pub fn new(event: Event, address: &'a str) -> Self {
        Self {
            event,
            identity: None,
            actor: None,
            email_digest: None,
            address,
            detail: None,
        }
    }
}

/// The address as stored: the throttle key, bounded like its column.
pub(crate) fn stored_address(address: &str) -> Option<&str> {
    (!address.is_empty() && address.len() <= 64).then_some(address)
}

async fn insert<'e>(executor: impl sqlx::PgExecutor<'e>, line: Line<'_>) -> Result<(), DbError> {
    sqlx::query!(
        "INSERT INTO staff_auth_events (id, event, identity_id, actor_id, email_digest, address, detail)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
        Uuid::now_v7(),
        line.event.as_str(),
        line.identity,
        line.actor,
        line.email_digest.map(|digest| &digest[..]),
        stored_address(line.address),
        line.detail
    )
    .execute(executor)
    .await
    .map_err(DbError::from_query)?;
    Ok(())
}

/// Records `line` in its own statement.
///
/// # Errors
///
/// The insert failing.
pub async fn record(pool: &PgPool, line: Line<'_>) -> Result<(), DbError> {
    insert(pool, line).await
}

/// Records `line` inside `transaction`, so it commits (or not) with what it describes.
///
/// # Errors
///
/// The insert failing.
pub async fn record_in(
    transaction: &mut Transaction<'static, Postgres>,
    line: Line<'_>,
) -> Result<(), DbError> {
    insert(&mut **transaction, line).await
}

/// Counts one sign-up attempt from `address` against `limit` an hour. `None` when it is within
/// the limit; otherwise the seconds until the window ends (at least one). Every attempt counts,
/// refused ones included, in its own committed statement.
///
/// # Errors
///
/// The statement failing.
pub async fn signup_attempt(
    pool: &PgPool,
    address: &str,
    limit: u32,
) -> Result<Option<u32>, DbError> {
    let limit = i32::try_from(limit).unwrap_or(i32::MAX);
    let row = sqlx::query!(
        r#"SELECT admitted AS "admitted!", retry_after AS "retry_after!"
           FROM colophon_signup_attempt($1, $2)"#,
        stored_address(address).unwrap_or("-"),
        limit
    )
    .fetch_one(pool)
    .await
    .map_err(DbError::from_query)?;
    Ok((!row.admitted).then(|| u32::try_from(row.retry_after).unwrap_or(1).max(1)))
}

/// Failed sign-ins from one address the log records in an hour (D-235): past it, one line marked
/// `log_capped`, then none until the hour ends, so a spray can't grow the append-only log at
/// its own rate.
pub const SIGN_IN_FAILURES_LOGGED_PER_HOUR: i32 = 20;

/// Whether a failed sign-in from `address` is recorded, and how: `Some(None)` as itself,
/// `Some(Some("log_capped"))` as the line that says the budget ran out, `None` not at all. Counts
/// the miss either way, in its own statement.
///
/// # Errors
///
/// The statement failing.
pub async fn sign_in_failure_logged(
    pool: &PgPool,
    address: &str,
) -> Result<Option<Option<&'static str>>, DbError> {
    let misses = sqlx::query_scalar!(
        r#"SELECT colophon_sign_in_log_admit($1) AS "misses!""#,
        stored_address(address).unwrap_or("-")
    )
    .fetch_one(pool)
    .await
    .map_err(DbError::from_query)?;
    Ok(match misses {
        n if n <= SIGN_IN_FAILURES_LOGGED_PER_HOUR => Some(None),
        n if n == SIGN_IN_FAILURES_LOGGED_PER_HOUR + 1 => Some(Some("log_capped")),
        _ => None,
    })
}
