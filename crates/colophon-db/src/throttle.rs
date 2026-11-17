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
    fn key(self) -> String {
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
    let realm = realm.key();
    // The key's row, locked: attempts on one key serialize here, so the check and the count
    // below can't interleave. (No row yet means no pause: it takes LOCK_AFTER attempts.)
    let key = sqlx::query!(
        r#"SELECT coalesce(locked_until > now(), false) AS "paused!", trusted
           FROM sign_in_throttle
           WHERE realm = $1 AND email = $2 AND address = $3
           FOR UPDATE"#,
        realm,
        email,
        address
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(DbError::from_query)?;
    if key.as_ref().is_some_and(|key| key.paused) {
        return Ok(Counted::Paused);
    }
    // The account's ceiling (D-162), for addresses it has never let in: their misses summed.
    if !key.as_ref().is_some_and(|key| key.trusted) {
        let recent = sqlx::query_scalar!(
            r#"SELECT coalesce(sum(failures), 0)::bigint AS "misses!" FROM sign_in_throttle
               WHERE realm = $1 AND email = $2 AND NOT trusted
                 AND updated_at > now() - make_interval(secs => $3)"#,
            realm,
            email,
            f64::from(u32::try_from(ACCOUNT_WINDOW.whole_seconds()).unwrap_or(u32::MAX))
        )
        .fetch_one(&mut **tx)
        .await
        .map_err(DbError::from_query)?;
        if recent >= ACCOUNT_LOCK_AFTER {
            return Ok(Counted::Paused);
        }
    }
    sqlx::query!(
        "INSERT INTO sign_in_throttle AS t (realm, email, address, failures, locked_until)
         VALUES ($1, $2, $3, 1, CASE WHEN 1 >= $4 THEN now() + make_interval(secs => $5) END)
         ON CONFLICT (realm, email, address) DO UPDATE SET
             failures = CASE WHEN t.locked_until IS NOT NULL THEN 1
                             ELSE least(t.failures + 1, 1000000) END,
             locked_until = CASE
                 WHEN (CASE WHEN t.locked_until IS NOT NULL THEN 1 ELSE t.failures + 1 END) >= $4
                     THEN now() + make_interval(secs => $5)
             END,
             updated_at = now()",
        realm,
        email,
        address,
        LOCK_AFTER,
        f64::from(u32::try_from(LOCK_FOR.whole_seconds()).unwrap_or(u32::MAX))
    )
    .execute(&mut **tx)
    .await
    .map_err(DbError::from_query)?;
    Ok(Counted::Verify)
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
