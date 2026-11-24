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

/// [`record_sign_in`] for an identity that already exists only: closed registration (D-235)
/// admits no first sign-in through the provider either. `None` if `(issuer, subject)` has never
/// signed in, and nothing is written.
pub async fn record_existing_sign_in(
    pool: &PgPool,
    issuer: &str,
    subject: &str,
    display_name: Option<&str>,
) -> Result<Option<Uuid>, DbError> {
    sqlx::query_scalar!(
        "UPDATE identities SET display_name = $3, last_sign_in_at = now()
         WHERE issuer = $1 AND subject = $2
         RETURNING id",
        issuer,
        subject,
        display_name
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)
}

/// The `issuer` for email-and-password accounts (docs/modules/03-auth.md, D-084).
pub const PASSWORD_ISSUER: &str = "password";

/// Attempts a (account, client address) key gets before it pauses, and for how long
/// (D-154): the counts live in `sign_in_throttle`, see [`crate::throttle`].
pub use crate::throttle::{LOCK_AFTER, LOCK_FOR};

/// The project a sign-up joined by claiming its invitation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Joined {
    /// The project.
    pub project: Uuid,
    /// The role the invitation gave.
    pub role: String,
}

/// What signing up did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignedUp {
    /// A new identity with a password credential.
    Made {
        /// The new identity.
        identity: Uuid,
        /// The project it joined, when it claimed an invitation.
        joined: Option<Joined>,
    },
    /// That email already has an account.
    Taken,
    /// Invite-only registration admitted nothing: no invitation, or one that is unknown,
    /// expired, revoked or already claimed, and the installation's one first-run account is
    /// taken (or it is set up). One answer for all of them, so a token's state isn't told.
    InvitationRequired,
}

/// What a sign-up may admit under the installation's registration (D-235). `closed` admits
/// nothing and is refused before any work, so it has no variant here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission<'a> {
    /// Open registration: any new email. A valid invitation is claimed too (the account joins
    /// its project in the same transaction); one that isn't valid doesn't stop the sign-up.
    Open {
        /// The SHA-256 of the invitation token the sign-up presented, if any.
        invitation: Option<&'a [u8; 32]>,
    },
    /// Invite-only: the invitation with this token hash, claimed for the new account in the same
    /// transaction (one invitation, one account); without one, only the installation's first-run
    /// account (see [`first_run_open`]).
    Invite {
        /// The SHA-256 of the invitation token the sign-up presented, if any.
        invitation: Option<&'a [u8; 32]>,
    },
}

/// Whether an invite-only installation still admits its one first-run account without an
/// invitation: nobody is an administrator yet and no person has an account (system and API-token
/// identities, `urn:colophon:*`, don't count). That's the account the Studio's first run creates
/// before the setup code is entered. [`sign_up`] decides it again under its lock; this is the
/// cheap check a handler makes before hashing a password it would only refuse.
pub async fn first_run_open(pool: &PgPool) -> Result<bool, DbError> {
    sqlx::query_scalar!(
        r#"SELECT NOT EXISTS (SELECT 1 FROM administrators)
              AND NOT EXISTS (SELECT 1 FROM identities WHERE issuer NOT LIKE 'urn:colophon:%')
              AS "open!""#
    )
    .fetch_one(pool)
    .await
    .map_err(DbError::from_query)
}

/// Creates an email-and-password account: `('password', email)` with the Argon2id hash and a
/// display name, if `admission` admits it, in one transaction. `Taken` if the email already has
/// an account and the sign-up was otherwise admitted (open registration, or a valid invitation,
/// which stays unclaimed); invite-only registration without a valid invitation answers
/// `InvitationRequired` whether the email is taken or not.
///
/// Under [`Admission::Invite`] with an invitation, the account and the invitation's claim commit
/// together: concurrent sign-ups with one token make exactly one account. Without one, the
/// first-run rule is decided under an installation-wide advisory lock, so concurrent first
/// sign-ups make exactly one account. The account's `signed_up` line in the staff auth log
/// (marked `pre_setup` or `invitation`) commits with it, from `address`.
pub async fn sign_up(
    pool: &PgPool,
    email: &str,
    password_hash: &str,
    display_name: Option<&str>,
    admission: Admission<'_>,
    address: &str,
) -> Result<SignedUp, DbError> {
    let mut transaction = scope::begin(
        pool,
        Scope {
            identity: None,
            project: None,
        },
    )
    .await?;
    if let Admission::Invite { invitation: None } = admission {
        sqlx::query!(
            r#"SELECT 1 AS "locked!" FROM pg_advisory_xact_lock(hashtext('colophon:first-run-account'), 0)"#
        )
        .fetch_one(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
        let open = sqlx::query_scalar!(
            r#"SELECT NOT EXISTS (SELECT 1 FROM administrators)
                  AND NOT EXISTS (SELECT 1 FROM identities WHERE issuer NOT LIKE 'urn:colophon:%')
                  AS "open!""#
        )
        .fetch_one(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
        if !open {
            // Whether the email is taken or not: invite-only registration tells someone without
            // an invitation nothing about who has an account.
            return Ok(SignedUp::InvitationRequired);
        }
    }
    let made = sqlx::query_scalar!(
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
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    let Some(id) = made else {
        return Ok(SignedUp::Taken);
    };
    let (invitation, required) = match admission {
        Admission::Open { invitation } => (invitation, false),
        Admission::Invite { invitation } => (invitation, true),
    };
    let mut joined = None;
    if let Some(token_hash) = invitation {
        // A brand-new identity is never already a member, so anything but `Joined` means the
        // invitation wasn't there to claim.
        match crate::members::accept_in(&mut transaction, id, token_hash).await? {
            crate::members::Accepted::Joined { project_id, role } => {
                joined = Some(Joined {
                    project: project_id,
                    role,
                });
            }
            crate::members::Accepted::AlreadyMember | crate::members::Accepted::Gone => {
                if required {
                    // Dropping the transaction un-makes the account.
                    return Ok(SignedUp::InvitationRequired);
                }
                // Open registration admits the account without it; the claim's failed
                // statements wrote nothing (it only reads before finding the invitation).
            }
        }
    }
    // The staff auth log, in the same transaction (D-235): which account took the first-run
    // slot is something the operator must be able to see.
    let detail = if joined.is_some() {
        Some("invitation")
    } else if admission == (Admission::Invite { invitation: None }) {
        Some("pre_setup")
    } else {
        None
    };
    crate::staff_auth::record_in(
        &mut transaction,
        crate::staff_auth::Line {
            identity: Some(id),
            detail,
            ..crate::staff_auth::Line::new(crate::staff_auth::Event::SignedUp, address)
        },
    )
    .await?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(SignedUp::Made {
        identity: id,
        joined,
    })
}

/// Creates or re-arms a password account (D-146): insert if the email is new, or replace the
/// hash (and fill in email/display name) on the existing one — the provisioning path
/// `colophon admin account` uses where [`sign_up`] refuses. Runs as the migrator; the id of
/// the account either way.
pub async fn ensure_password_account(
    migrator: &PgPool,
    email: &str,
    password_hash: &str,
    display_name: Option<&str>,
) -> Result<Uuid, DbError> {
    sqlx::query_scalar!(
        "INSERT INTO identities (id, issuer, subject, display_name, email, password_hash)
         VALUES ($1, $2, $3, $4, $3, $5)
         ON CONFLICT (issuer, subject) DO UPDATE
           SET password_hash = EXCLUDED.password_hash,
               email = EXCLUDED.email,
               display_name = COALESCE(EXCLUDED.display_name, identities.display_name)
         RETURNING id",
        Uuid::now_v7(),
        PASSWORD_ISSUER,
        email,
        display_name,
        password_hash
    )
    .fetch_one(migrator)
    .await
    .map_err(DbError::from_write)
}

/// The answer to an email-and-password sign-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordSignIn {
    /// Right password; the attempt count for this address resets and the session may start.
    SignedIn(Uuid),
    /// Wrong password, no such account, or this (account, address) paused after too many
    /// misses (D-154) — one answer for all three, so none can be told apart.
    Refused,
}

/// Signs in with an email and a password, as a client at `address` (D-150, D-154, D-213). A key
/// already paused is refused first, with one read and no slot; then the slot, so a request the
/// password bound sheds costs nothing else. Then one statement counts the attempt against the
/// (staff, email, address) key — refusing at once, unverified, while that key is paused — and
/// reads the account; Argon2 runs with no transaction open and no connection held; a match
/// forgets the key. A guesser pauses only their own address, and a pause answers exactly as a
/// wrong password does. Three statements on success, none of them a BEGIN or COMMIT (D-250).
pub async fn password_sign_in(
    pool: &PgPool,
    gate: &crate::passwords::PasswordGate,
    email: &str,
    address: &str,
    password: &str,
) -> Result<PasswordSignIn, DbError> {
    let Some(identity) = verify_password(pool, gate, email, address, password).await? else {
        return Ok(PasswordSignIn::Refused);
    };
    sqlx::query(
        "WITH cleared AS (INSERT INTO sign_in_throttle (realm, email, address, failures, locked_until, trusted)
                          VALUES ($1, $2, $3, 0, NULL, true)
                          ON CONFLICT (realm, email, address) DO UPDATE
                              SET failures = 0, locked_until = NULL, trusted = true, updated_at = now()
                          RETURNING 1)
         UPDATE identities SET last_sign_in_at = now() WHERE id = $4",
    )
    .bind(crate::throttle::Realm::Staff.key())
    .bind(email)
    .bind(address)
    .bind(identity)
    .execute(pool)
    .await
    .map_err(DbError::from_query)?;
    Ok(PasswordSignIn::SignedIn(identity))
}

/// A session a sign-in starts: only its hashes are stored.
#[derive(Debug, Clone, Copy)]
pub struct NewSession<'a> {
    /// SHA-256 of the session token.
    pub token_hash: &'a [u8; 32],
    /// SHA-256 of its CSRF token.
    pub csrf_hash: &'a [u8; 32],
    /// When it ends regardless of use.
    pub expires_at: OffsetDateTime,
}

/// What [`password_sign_in_session`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSignIn {
    /// The password matched; the session is stored and the staff auth log has its line.
    SignedIn {
        /// Who signed in.
        identity: Uuid,
        /// The new session's id.
        session: Uuid,
    },
    /// As [`PasswordSignIn::Refused`]: one answer for a wrong password, no account and a pause.
    Refused,
}

/// [`password_sign_in`], and on a match the session in the same last statement (D-250): the key
/// forgotten, `last_sign_in_at`, the session — none for an account disabled meanwhile (D-235),
/// [`DbError::AccountDisabled`] — and its `signed_in` line in the staff auth log, one round trip,
/// all or nothing. Three round trips on success: the pause check, the count and read, this.
pub async fn password_sign_in_session(
    pool: &PgPool,
    gate: &crate::passwords::PasswordGate,
    email: &str,
    address: &str,
    password: &str,
    session: NewSession<'_>,
) -> Result<SessionSignIn, DbError> {
    let Some(identity) = verify_password(pool, gate, email, address, password).await? else {
        return Ok(SessionSignIn::Refused);
    };
    let id = Uuid::now_v7();
    let made: i64 = sqlx::query_scalar(
        "WITH cleared AS (INSERT INTO sign_in_throttle (realm, email, address, failures, locked_until, trusted)
                          VALUES ($1, $2, $3, 0, NULL, true)
                          ON CONFLICT (realm, email, address) DO UPDATE
                              SET failures = 0, locked_until = NULL, trusted = true, updated_at = now()
                          RETURNING 1),
              seen AS (UPDATE identities SET last_sign_in_at = now()
                       WHERE id = $4 AND disabled_at IS NULL
                       RETURNING id),
              made AS (INSERT INTO sessions (id, token_hash, csrf_hash, identity_id, expires_at)
                       SELECT $5, $6, $7, seen.id, $8 FROM seen
                       RETURNING identity_id),
              logged AS (INSERT INTO staff_auth_events (id, event, identity_id, address, detail)
                         SELECT $9, $10, made.identity_id, $11, 'password' FROM made
                         RETURNING 1)
         SELECT count(*) FROM made",
    )
    .bind(crate::throttle::Realm::Staff.key())
    .bind(email)
    .bind(address)
    .bind(identity)
    .bind(id)
    .bind(&session.token_hash[..])
    .bind(&session.csrf_hash[..])
    .bind(session.expires_at)
    .bind(Uuid::now_v7())
    .bind(crate::staff_auth::Event::SignedIn.as_str())
    .bind(crate::staff_auth::stored_address(address))
    .fetch_one(pool)
    .await
    .map_err(DbError::from_query)?;
    if made == 0 {
        return Err(DbError::AccountDisabled);
    }
    Ok(SessionSignIn::SignedIn {
        identity,
        session: id,
    })
}

/// The pause check, the slot, the count and read, Argon2: the account a matching password names,
/// or `None` for the one refusal (D-154). Two statements, neither in a transaction: each is its
/// own (D-250).
async fn verify_password(
    pool: &PgPool,
    gate: &crate::passwords::PasswordGate,
    email: &str,
    address: &str,
    password: &str,
) -> Result<Option<Uuid>, DbError> {
    use crate::throttle::{self, Realm};
    // A key already paused is refused before it takes a slot (D-213): only work that will
    // hash holds one.
    if throttle::paused(pool, Realm::Staff, email, address).await? {
        return Ok(None);
    }
    let slot = gate.slot().await?;
    // Counted before any Argon2, and the account read in the same statement: a paused key is
    // refused unverified (migration 0100 keeps D-162's lock order inside the function).
    let (paused, id, hash): (bool, Option<Uuid>, Option<String>) = throttle::bind_count(
        sqlx::query_as(
            "SELECT c.paused, i.id, i.password_hash
             FROM colophon_sign_in_count($1, $2, $3, $4, $5, $6, $7) AS c (paused)
             LEFT JOIN identities i ON i.issuer = $8 AND i.subject = $2 AND i.disabled_at IS NULL",
        )
        .bind(Realm::Staff.key())
        .bind(email)
        .bind(address),
    )
    .bind(PASSWORD_ISSUER)
    .fetch_one(pool)
    .await
    .map_err(DbError::from_query)?;
    if paused {
        return Ok(None);
    }
    // Argon2 with nothing held. No such account verifies against the dummy hash anyway, so
    // the answer can't be told apart.
    let right = slot.verify(password, hash.as_deref()).await;
    Ok(id.filter(|_| right))
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
    // A disabled account (D-235) gets no session, whichever route asks.
    let made = sqlx::query!(
        "INSERT INTO sessions (id, token_hash, csrf_hash, identity_id, expires_at)
         SELECT $1, $2, $3, i.id, $5 FROM identities i WHERE i.id = $4 AND i.disabled_at IS NULL",
        id,
        &token_hash[..],
        &csrf_hash[..],
        identity_id,
        expires_at
    )
    .execute(pool)
    .await
    .map_err(DbError::from_query)?;
    if made.rows_affected() == 0 {
        return Err(DbError::AccountDisabled);
    }
    Ok(id)
}

/// What [`set_disabled`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetDisabled {
    /// The account is now as asked: disabled, with every session and API token it made revoked,
    /// or enabled again; the staff auth log has its row.
    Done,
    /// It already was.
    Unchanged,
    /// No such account (an API token's identity isn't one).
    NotFound,
    /// The caller isn't an enabled administrator.
    NotAdministrator,
    /// Nobody disables their own account.
    OwnAccount,
    /// The last enabled administrator stays enabled.
    LastAdministrator,
}

/// Disables or enables `target` as `actor` (D-235), in one transaction run as `actor`'s
/// identity: `colophon_account_set_disabled` takes the actor from it, never from an argument.
///
/// # Errors
///
/// The database's.
pub async fn set_disabled(
    pool: &PgPool,
    actor: Uuid,
    target: Uuid,
    disable: bool,
) -> Result<SetDisabled, DbError> {
    let mut tx = crate::scope::begin(
        pool,
        crate::scope::Scope {
            identity: Some(actor),
            project: None,
        },
    )
    .await?;
    let outcome: String = sqlx::query_scalar("SELECT colophon_account_set_disabled($1, $2, $3)")
        .bind(target)
        .bind(disable)
        .bind(Uuid::now_v7())
        .fetch_one(&mut *tx)
        .await
        .map_err(DbError::from_query)?;
    tx.commit().await.map_err(DbError::from_query)?;
    Ok(match outcome.as_str() {
        "done" => SetDisabled::Done,
        "unchanged" => SetDisabled::Unchanged,
        "not_found" => SetDisabled::NotFound,
        "not_admin" => SetDisabled::NotAdministrator,
        "self" => SetDisabled::OwnAccount,
        "last_admin" => SetDisabled::LastAdministrator,
        _ => {
            return Err(DbError::Corrupt(
                "an unknown answer from colophon_account_set_disabled",
            ));
        }
    })
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

/// Revokes the live session whose token hashes to `token_hash`, if there is one: what a browser
/// presented when it signs in again (D-235, session rotation). The session's identity, if it was
/// live.
pub async fn revoke_session_by_token(
    pool: &PgPool,
    token_hash: &[u8; 32],
) -> Result<Option<Uuid>, DbError> {
    sqlx::query_scalar!(
        "UPDATE sessions SET revoked_at = now()
         WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now()
         RETURNING identity_id",
        &token_hash[..]
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)
}

/// Swaps a session presented under the cookie name before D-235's rename for a new one, in one
/// statement: only a live session made before the rename was deployed
/// (`session_cookie_rename`), so one planted under the old name later is never accepted. The new
/// session keeps the old one's identity, CSRF token, expiry and last use; the old one ends. The
/// identity, if it was swapped.
pub async fn upgrade_legacy_session(
    pool: &PgPool,
    old_token_hash: &[u8; 32],
    new_token_hash: &[u8; 32],
) -> Result<Option<(Uuid, OffsetDateTime)>, DbError> {
    let row = sqlx::query!(
        "WITH old AS (
             UPDATE sessions SET revoked_at = now()
             WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now()
               AND created_at < (SELECT at FROM session_cookie_rename)
             RETURNING identity_id, csrf_hash, expires_at, last_seen_at)
         INSERT INTO sessions (id, token_hash, csrf_hash, identity_id, expires_at, last_seen_at)
         SELECT $2, $3, csrf_hash, identity_id, expires_at, last_seen_at FROM old
         RETURNING identity_id, expires_at",
        &old_token_hash[..],
        Uuid::now_v7(),
        &new_token_hash[..]
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)?;
    Ok(row.map(|row| (row.identity_id, row.expires_at)))
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

/// `profile`'s one row: the identity, then its projects as parallel arrays (D-250).
type ProfileRow = (
    Uuid,
    String,
    String,
    Option<String>,
    bool,
    bool,
    Vec<Uuid>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
);

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
    // The account and its projects in one statement (D-250): the projects as parallel arrays,
    // in `projects::list_in`'s order and under its limit.
    let row: Option<ProfileRow> = sqlx::query_as(
        "SELECT i.id, i.issuer, i.subject, i.display_name,
                EXISTS (SELECT 1 FROM administrators a WHERE a.identity_id = i.id),
                colophon_installation_set_up(),
                coalesce(array_agg(p.id ORDER BY p.name, p.id) FILTER (WHERE p.id IS NOT NULL), '{}'),
                coalesce(array_agg(p.slug ORDER BY p.name, p.id) FILTER (WHERE p.id IS NOT NULL), '{}'),
                coalesce(array_agg(p.name ORDER BY p.name, p.id) FILTER (WHERE p.id IS NOT NULL), '{}'),
                coalesce(array_agg(p.role ORDER BY p.name, p.id) FILTER (WHERE p.id IS NOT NULL), '{}'),
                coalesce(array_agg(p.role_name ORDER BY p.name, p.id) FILTER (WHERE p.id IS NOT NULL), '{}')
         FROM identities i
         LEFT JOIN LATERAL (
             SELECT p.id, p.slug, p.name, m.role, r.name AS role_name FROM projects p
             JOIN memberships m ON m.project_id = p.id AND m.identity_id = i.id
             JOIN roles r ON r.project_id = m.project_id AND r.key = m.role
             ORDER BY p.name, p.id
             LIMIT 1000
         ) p ON true
         WHERE i.id = $1
         GROUP BY i.id",
    )
    .bind(id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    let profile = row.map(
        |(
            id,
            issuer,
            subject,
            display_name,
            administrator,
            installation_set_up,
            ids,
            slugs,
            names,
            roles,
            role_names,
        )| {
            Profile {
                identity: Identity {
                    id,
                    issuer,
                    subject,
                    display_name,
                    administrator,
                    installation_set_up,
                },
                projects: ids
                    .into_iter()
                    .zip(slugs)
                    .zip(names)
                    .zip(roles)
                    .zip(role_names)
                    .map(
                        |((((id, slug), name), role), role_name)| projects::Project {
                            id,
                            slug,
                            name,
                            role,
                            role_name,
                        },
                    )
                    .collect(),
            }
        },
    );
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(profile)
}
