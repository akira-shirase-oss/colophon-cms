//! Sign-in guessing, paused per (account, client address) (D-154; migration 0057): a key is
//! the realm — the staff accounts, or one project's audience — the normalized email and the
//! address the attempt came from. Every attempt is counted before any Argon2 runs; the
//! [`LOCK_AFTER`]th sets a [`LOCK_FOR`] pause on that key only, and an attempt while it is
//! paused is refused unverified — with the very answer a wrong password gets, so a pause says
//! nothing about whether the account exists (the key doesn't depend on it either). A match
//! clears the key and marks the address trusted for the account. A guesser pauses their own
//! address, never the owner's, and a pause that ran out starts the count over: at most
//! [`LOCK_AFTER`] guesses a key per [`LOCK_FOR`], never an indefinite lock someone else can
//! hold over the account. Across addresses (D-162), [`ACCOUNT_LOCK_AFTER`] misses from
//! untrusted ones within [`ACCOUNT_WINDOW`] pause every untrusted address for the account,
//! while the addresses the owner signed in from keep working.

use sqlx::{Postgres, Transaction};
use time::Duration;
use uuid::Uuid;

use crate::DbError;

/// Attempts a key gets before it pauses.
pub const LOCK_AFTER: i32 = 8;
/// How long a paused key stays paused.
pub const LOCK_FOR: Duration = Duration::minutes(15);
/// Misses from addresses that never signed in to an account, summed across them, past which
/// every such address is paused for that account (D-162): the bound on guessing from many
/// addresses. Addresses the owner signed in from are trusted and never paused by it.
pub const ACCOUNT_LOCK_AFTER: i64 = 50;
/// The window [`ACCOUNT_LOCK_AFTER`] counts over.
pub const ACCOUNT_WINDOW: Duration = Duration::HOUR;

/// Whose sign-ins a key counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Realm {
    /// The installation's own password accounts.
    Staff,
    /// One project's site users.
    Audience(Uuid),
}

impl Realm {
    pub(crate) fn key(self) -> String {
        match self {
            Self::Staff => "staff".to_owned(),
            Self::Audience(project) => format!("audience:{project}"),
        }
    }
}

/// What counting an attempt found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Counted {
    /// The key is paused: refuse without verifying.
    Paused,
    /// Counted; verify the password.
    Verify,
}

/// Whether the key is paused right now, read without a lock and without counting (D-213):
/// asked before an attempt takes a password slot, so a paused key never holds one. [`count`]
/// still decides under its lock; this only spares the slot when the answer is already known.
/// One statement (D-250): the key and, for an address the account never let in, the account's
/// untrusted misses.
pub(crate) async fn paused<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    realm: Realm,
    email: &str,
    address: &str,
) -> Result<bool, DbError> {
    let paused: bool = sqlx::query_scalar(
        "SELECT coalesce(k.locked_until > now(), false)
             OR (NOT coalesce(k.trusted, false)
                 AND (SELECT coalesce(sum(t.failures), 0)::bigint FROM sign_in_throttle t
                      WHERE t.realm = $1 AND t.email = $2 AND NOT t.trusted
                        AND t.updated_at > now() - make_interval(secs => $4)) >= $5)
         FROM (SELECT 1) one
         LEFT JOIN sign_in_throttle k ON k.realm = $1 AND k.email = $2 AND k.address = $3",
    )
    .bind(realm.key())
    .bind(email)
    .bind(address)
    .bind(window_secs())
    .bind(ACCOUNT_LOCK_AFTER)
    .fetch_one(executor)
    .await
    .map_err(DbError::from_query)?;
    Ok(paused)
}

fn window_secs() -> f64 {
    f64::from(u32::try_from(ACCOUNT_WINDOW.whole_seconds()).unwrap_or(u32::MAX))
}

fn lock_for_secs() -> f64 {
    f64::from(u32::try_from(LOCK_FOR.whole_seconds()).unwrap_or(u32::MAX))
}

/// Binds `colophon_sign_in_count($1, … $7)`'s parameters after the realm, email and address
/// (`$1` to `$3`): the lock and the account ceiling, so a caller reading in the same statement
/// ([`count`], the staff sign-in) counts exactly as this module does.
pub(crate) fn bind_count<O>(
    query: sqlx::query::QueryAs<'_, Postgres, O, sqlx::postgres::PgArguments>,
) -> sqlx::query::QueryAs<'_, Postgres, O, sqlx::postgres::PgArguments> {
    query
        .bind(LOCK_AFTER)
        .bind(lock_for_secs())
        .bind(ACCOUNT_LOCK_AFTER)
        .bind(window_secs())
}

/// Counts one attempt for the key inside `tx`, before any verify. A paused key's attempt
/// isn't counted (the pause doesn't grow); a pause that ran out starts the count over; the
/// [`LOCK_AFTER`]th attempt pauses the key for the attempts after it, whatever it turns out
/// to be — a match then clears it ([`clear`]). An address the account never let in is also
/// paused while the account's untrusted misses are at [`ACCOUNT_LOCK_AFTER`].
pub(crate) async fn count(
    tx: &mut Transaction<'_, Postgres>,
    realm: Realm,
    email: &str,
    address: &str,
) -> Result<Counted, DbError> {
    // One statement (D-250, migration 0100): the account's advisory lock, then the key's row,
    // the account's sum and the count, in that order, under the transaction (D-162).
    let (paused,): (bool,) = bind_count(
        sqlx::query_as("SELECT colophon_sign_in_count($1, $2, $3, $4, $5, $6, $7)")
            .bind(realm.key())
            .bind(email)
            .bind(address),
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(DbError::from_query)?;
    Ok(if paused {
        Counted::Paused
    } else {
        Counted::Verify
    })
}

/// Clears the key after a match and marks the address trusted for the account (D-162): the
/// next miss from it starts at one, and the account's ceiling never pauses it.
pub(crate) async fn clear(
    tx: &mut Transaction<'_, Postgres>,
    realm: Realm,
    email: &str,
    address: &str,
) -> Result<(), DbError> {
    sqlx::query!(
        "INSERT INTO sign_in_throttle (realm, email, address, failures, locked_until, trusted)
         VALUES ($1, $2, $3, 0, NULL, true)
         ON CONFLICT (realm, email, address) DO UPDATE
             SET failures = 0, locked_until = NULL, trusted = true, updated_at = now()",
        realm.key(),
        email,
        address
    )
    .execute(&mut **tx)
    .await
    .map_err(DbError::from_query)?;
    Ok(())
}
