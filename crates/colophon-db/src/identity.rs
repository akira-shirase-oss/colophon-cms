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
/// password bound sheds costs nothing else. Then one short
/// transaction counts the attempt against the (staff, email, address) key — refusing at once,
/// unverified, while that key is paused — and reads the account; Argon2 runs with no
/// transaction open and no connection held; a match forgets the key. A guesser pauses only
/// their own address, and a pause answers exactly as a wrong password does.
pub async fn password_sign_in(
    pool: &PgPool,
    gate: &crate::passwords::PasswordGate,
    email: &str,
    address: &str,
    password: &str,
) -> Result<PasswordSignIn, DbError> {
    use crate::throttle::{self, Counted, Realm};
    // A key already paused is refused before it takes a slot (D-213): only work that will
    // hash holds one.
    let mut transaction = crate::begin_tx(pool).await.map_err(DbError::from_query)?;
    let paused = throttle::paused(&mut transaction, Realm::Staff, email, address).await?;
    transaction.commit().await.map_err(DbError::from_query)?;
    if paused {
        return Ok(PasswordSignIn::Refused);
    }
    let slot = gate.slot().await?;
    let mut transaction = crate::begin_tx(pool).await.map_err(DbError::from_query)?;
    if throttle::count(&mut transaction, Realm::Staff, email, address).await? == Counted::Paused {
        return Ok(PasswordSignIn::Refused);
    }
    let row = sqlx::query!(
        "SELECT id, password_hash FROM identities WHERE issuer = $1 AND subject = $2",
        PASSWORD_ISSUER,
        email
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    // Argon2 with nothing held. No such account verifies against the dummy hash anyway, so
    // the answer can't be told apart.
    let hash = row.as_ref().and_then(|row| row.password_hash.as_deref());
    let right = slot.verify(password, hash).await;
    let Some(row) = row.filter(|_| right) else {
        return Ok(PasswordSignIn::Refused);
    };
    let mut transaction = crate::begin_tx(pool).await.map_err(DbError::from_query)?;
    throttle::clear(&mut transaction, Realm::Staff, email, address).await?;
    sqlx::query!(
        "UPDATE identities SET last_sign_in_at = now() WHERE id = $1",
        row.id
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(PasswordSignIn::SignedIn(row.id))
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
