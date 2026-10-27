//! Setting up an installation from the web: one-time setup codes (docs/modules/03-auth.md,
//! "Owner bootstrap"; D-060, amending D-043). While nobody is an administrator, the server issues
//! a code at start-up and prints it to its own terminal; a signed-in person who presents it
//! becomes the first administrator. The application role reaches the tables only through the
//! functions of migration 0020, which refuse everything once an administrator exists.
//!
//! Only SHA-256 hashes of codes reach the database. Comparing a presented hash with the live ones
//! happens in the caller, in constant time; this module only reads and writes.

use std::time::Duration;

use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::DbError;

/// The most codes that can be live at once (migration 0020's cap).
pub const MAX_LIVE_CODES: usize = 10;

/// Stores a new code's hash, valid for `lifetime` (one minute to a day). `None` when nothing was
/// stored: the installation has an administrator already, or ten codes are still live.
pub async fn issue(
    pool: &PgPool,
    code_hash: &[u8; 32],
    lifetime: Duration,
) -> Result<Option<OffsetDateTime>, DbError> {
    // Whole seconds: the function refuses anything under a minute or over a day.
    let seconds = i64::try_from(lifetime.as_secs()).unwrap_or(i64::MAX);
    sqlx::query_scalar!(
        "SELECT colophon_issue_setup_code($1, $2::int8 * interval '1 second')",
        &code_hash[..],
        seconds
    )
    .fetch_one(pool)
    .await
    .map_err(DbError::from_write)
}

/// Whether someone may try a code now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attempt {
    /// Recorded: go ahead and compare.
    Allowed,
    /// Too many attempts lately (five a person, twenty in all, in fifteen minutes): nothing was
    /// recorded; try again after this many seconds.
    Limited {
        /// Seconds until there is room again.
        retry_after: u32,
    },
    /// An administrator exists: codes no longer do anything, and nothing was recorded.
    AlreadySetUp,
}

/// Records an attempt by `identity` to use a setup code, within the rate limit.
pub async fn attempt(pool: &PgPool, identity: Uuid) -> Result<Attempt, DbError> {
    let row = sqlx::query!(
        r#"SELECT outcome AS "outcome!", retry_after AS "retry_after!" FROM colophon_setup_attempt($1)"#,
        identity
    )
    .fetch_one(pool)
    .await
    .map_err(DbError::from_write)?;
    match row.outcome.as_str() {
        "allowed" => Ok(Attempt::Allowed),
        "already_set_up" => Ok(Attempt::AlreadySetUp),
        "limited" => Ok(Attempt::Limited {
            retry_after: u32::try_from(row.retry_after).unwrap_or(1).max(1),
        }),
        _ => Err(DbError::Corrupt("a setup attempt's outcome is unknown")),
    }
}

/// A code that can still be used: its id and hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveCode {
    /// Its id.
    pub id: Uuid,
    /// SHA-256 of the code.
    pub hash: [u8; 32],
}

/// The codes that can still be used (at most [`MAX_LIVE_CODES`]); none once an administrator
/// exists.
pub async fn live_codes(pool: &PgPool) -> Result<Vec<LiveCode>, DbError> {
    let rows = sqlx::query!(
        r#"SELECT id AS "id!", code_hash AS "code_hash!" FROM colophon_live_setup_codes()"#
    )
    .fetch_all(pool)
    .await
    .map_err(DbError::from_query)?;
    rows.into_iter()
        .map(|row| {
            let hash = <[u8; 32]>::try_from(row.code_hash.as_slice())
                .map_err(|_| DbError::Corrupt("a setup code's hash isn't 32 bytes"))?;
            Ok(LiveCode { id: row.id, hash })
        })
        .collect()
}

/// What a claim did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claimed {
    /// `identity` is now the installation's first administrator, and the code is used.
    Claimed,
    /// The code is unknown, used, expired, or has another hash.
    Invalid,
    /// Someone is an administrator already; the code stays unused.
    AlreadySetUp,
}

/// Uses the code `code`, whose hash must be `code_hash`, to make `identity` the first
/// administrator, in one transaction. One code makes one administrator, once.
pub async fn claim(
    pool: &PgPool,
    identity: Uuid,
    code: Uuid,
    code_hash: &[u8; 32],
) -> Result<Claimed, DbError> {
    let outcome = sqlx::query_scalar!(
        r#"SELECT colophon_claim_setup_code($1, $2, $3) AS "outcome!""#,
        identity,
        code,
        &code_hash[..]
    )
    .fetch_one(pool)
    .await
    .map_err(DbError::from_write)?;
    match outcome.as_str() {
        "claimed" => Ok(Claimed::Claimed),
        "invalid" => Ok(Claimed::Invalid),
        "already_set_up" => Ok(Claimed::AlreadySetUp),
        _ => Err(DbError::Corrupt("a setup claim's outcome is unknown")),
    }
}

/// Whether the installation has an administrator yet (bootstrapped, or made one some other way).
pub async fn has_administrator(pool: &PgPool) -> Result<bool, DbError> {
    sqlx::query_scalar!(r#"SELECT colophon_installation_set_up() AS "set_up!""#)
        .fetch_one(pool)
        .await
        .map_err(DbError::from_query)
}
