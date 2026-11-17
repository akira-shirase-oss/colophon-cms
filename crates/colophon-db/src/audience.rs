//! The audience a project publishes for (docs/modules/03-auth.md, "Audience"; D-137): site
//! users — the visitors of a project's sites — their Argon2id password credentials, their
//! opaque sessions and the audience roles that gate published content. Members and site users
//! never meet: a member manages the project; a visitor only reads what it delivers.
//!
//! Account functions here take the project explicitly and run inside
//! [`scope::begin`] with no identity: RLS confines them; sign-in attempts are counted per
//! (account, client address) in `sign_in_throttle` like [`crate::identity::password_sign_in`]'s,
//! before the verify that runs with nothing held (D-150, D-154). Session lookups go
//! through `colophon_audience_session` (migration 0052), the SECURITY DEFINER function that
//! finds a session by its token's hash before the project's context exists and stamps its use
//! — at most once a minute, and only inside the idle window, so an idle session's staleness is
//! what the caller still sees.

use sqlx::{PgPool, Postgres, Transaction};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    DbError,
    scope::{self, MemberTransaction, Scope},
};

/// How stale `last_seen_at` may get before a lookup records the session's use — the same
/// bound `colophon_audience_session`'s UPDATE applies.
pub const TOUCH_INTERVAL: Duration = Duration::MINUTE;

/// A session unused this long is over, however far `expires_at` is. The value the
/// `colophon_audience_session` UPDATE keeps from reviving; keep them equal.
pub const IDLE: Duration = Duration::days(7);

/// How long a session lasts at most, however often it is used.
pub const LIFETIME: Duration = Duration::days(30);

/// The most live sessions one site user may hold: a new one revokes the oldest over the
/// bound, never deletes — revoked rows are the audit trail.
pub const MAX_SESSIONS: i64 = 16;

/// The most site users one project may hold: signup stops there rather than letting a
/// flood grow the table without end.
pub const MAX_SITE_USERS: i64 = 100_000;

/// Attempts a (account, client address) key gets before it pauses, and for how long
/// (D-154): the counts live in `sign_in_throttle`, see [`crate::throttle`].
pub use crate::throttle::{LOCK_AFTER, LOCK_FOR};

/// A project-scoped transaction with no person in it: what the public audience endpoints
/// open. Row-level security still confines every statement to the project.
async fn project_tx(
    pool: &PgPool,
    project: Uuid,
) -> Result<Transaction<'static, Postgres>, DbError> {
    scope::begin(
        pool,
        Scope {
            identity: None,
            project: Some(project),
        },
    )
    .await
}

/// Records a visitor auth event in the project's audit log (D-142): the actor is the shared
/// `('urn:colophon:system','audience-auth')` identity a site user can't be, the subject is the
/// site user's id — or, for `audience.magic_link_sent`, the email the link was sent to, since
/// the account may not exist yet. Runs in its own `project_tx`: the write follows a committed
/// sign-in, and the API treats a failure here as a warning, never as a refused auth.
pub async fn record_auth(
    pool: &PgPool,
    project: Uuid,
    action: &str,
    subject: &str,
    detail: &str,
) -> Result<(), DbError> {
    let mut tx = project_tx(pool, project).await?;
    let actor = sqlx::query_scalar!(
        "SELECT id FROM identities WHERE issuer = 'urn:colophon:system' AND subject = 'audience-auth'"
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(DbError::from_query)?;
    crate::audit::record_in(&mut tx, project, actor, action, subject, detail).await?;
    tx.commit().await.map_err(DbError::from_query)
}

/// One of the project's audience roles, as staff see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    /// Its name — what `role:<name>` visibility and a site user's `role` hold.
    pub name: String,
    /// Whether signup may choose it.
    pub self_assign: bool,
    /// When it was made.
    pub created_at: OffsetDateTime,
}

/// The project's audience roles, by name.
pub async fn roles(tx: &mut MemberTransaction) -> Result<Vec<Role>, DbError> {
    sqlx::query_as!(
        Role,
        "SELECT name, self_assign, created_at FROM site_roles WHERE project_id = $1 ORDER BY name",
        tx.project
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Whether the project has an audience role with this name — what `role:<name>` visibility
/// and a site user's `role` must name to mean anything.
pub async fn role_exists_in(tx: &mut MemberTransaction, name: &str) -> Result<bool, DbError> {
    role_exists_scoped(&mut tx.transaction, tx.project, name).await
}

/// [`role_exists_in`] inside an environment transaction: the entry write path's check that a
/// `role:<name>` gate names a role that exists.
pub async fn role_exists_for(
    tx: &mut scope::ProjectTransaction,
    name: &str,
) -> Result<bool, DbError> {
    role_exists_scoped(&mut tx.transaction, tx.project, name).await
}

/// [`role_exists_in`] inside a bare project scope, for the signup path's check.
async fn role_exists_scoped(
    tx: &mut Transaction<'static, Postgres>,
    project: Uuid,
    name: &str,
) -> Result<bool, DbError> {
    sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM site_roles WHERE project_id = $1 AND name = $2) AS \"exists!\"",
        project,
        name
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(DbError::from_query)
}

/// The role a signup may take: `Some` must be a self-assignable role that exists; `None`
/// resolves to the project's single self-assignable role — the common case of one open tier
/// — and is unavailable when there is none or several to choose between.
async fn signup_role_scoped(
    tx: &mut Transaction<'static, Postgres>,
    project: Uuid,
    asked: Option<&str>,
) -> Result<Option<String>, DbError> {
    if let Some(name) = asked {
        let open = sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM site_roles
                          WHERE project_id = $1 AND name = $2 AND self_assign) AS \"exists!\"",
            project,
            name
        )
        .fetch_one(&mut **tx)
        .await
        .map_err(DbError::from_query)?;
        return Ok(open.then(|| name.to_owned()));
    }
    let open = sqlx::query_scalar!(
        "SELECT name FROM site_roles WHERE project_id = $1 AND self_assign ORDER BY name LIMIT 2",
        project
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(DbError::from_query)?;
    Ok(match open.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    })
}

/// Creates the role. `false` when the project already has it — creating is idempotent.
pub async fn create_role(
    tx: &mut MemberTransaction,
    name: &str,
    self_assign: bool,
) -> Result<bool, DbError> {
    let made = sqlx::query!(
        "INSERT INTO site_roles (project_id, name, self_assign) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        tx.project,
        name,
        self_assign
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(made.rows_affected() == 1)
}

/// Sets whether signup may choose the role; `false` when the project has no such role.
pub async fn set_role_assign(
    tx: &mut MemberTransaction,
    name: &str,
    self_assign: bool,
) -> Result<bool, DbError> {
    let changed = sqlx::query!(
        "UPDATE site_roles SET self_assign = $3 WHERE project_id = $1 AND name = $2",
        tx.project,
        name,
        self_assign
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(changed.rows_affected() == 1)
}

/// What deleting a role did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleGone {
    /// It is gone.
    Deleted,
    /// The project never had it.
    Missing,
    /// Site users still hold it: change their role first.
    InUse(i64),
}

/// Deletes the role; refuses while a site user holds it.
pub async fn delete_role(tx: &mut MemberTransaction, name: &str) -> Result<RoleGone, DbError> {
    let holders = sqlx::query_scalar!(
        "SELECT count(*) AS \"count!\" FROM site_users WHERE project_id = $1 AND role = $2",
        tx.project,
        name
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    if holders > 0 {
        return Ok(RoleGone::InUse(holders));
    }
    let gone = sqlx::query!(
        "DELETE FROM site_roles WHERE project_id = $1 AND name = $2",
        tx.project,
        name
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(if gone.rows_affected() == 1 {
        RoleGone::Deleted
    } else {
        RoleGone::Missing
    })
}

/// A site user as staff see one: never the password hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteUser {
    /// Its id.
    pub id: Uuid,
    /// Its email, normalized.
    pub email: String,
    /// The name it gave, if any.
    pub display_name: Option<String>,
    /// The audience role it holds.
    pub role: String,
    /// When the account was made.
    pub created_at: OffsetDateTime,
    /// Its last successful sign-in, if ever.
    pub last_sign_in_at: Option<OffsetDateTime>,
    /// When staff disabled it, if they have.
    pub disabled_at: Option<OffsetDateTime>,
}

/// What signing up did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignedUp {
    /// A new site user.
    Made(SiteUser),
    /// That email already has an account in this project.
    Taken,
    /// The project has [`MAX_SITE_USERS`] accounts already.
    Full,
    /// The role it would hold doesn't exist — or, for a signup, isn't open to it.
    RoleUnavailable,
}

/// Creates a site user in `project` for a public signup: `role` names a role signup may
/// choose — `None` takes the project's single open one. Bounded: a project holds at most
/// [`MAX_SITE_USERS`] accounts.
pub async fn sign_up(
    pool: &PgPool,
    project: Uuid,
    email: &str,
    password_hash: &str,
    display_name: Option<&str>,
    role: Option<&str>,
) -> Result<SignedUp, DbError> {
    let mut tx = project_tx(pool, project).await?;
    let Some(role) = signup_role_scoped(&mut tx, project, role).await? else {
        return Ok(SignedUp::RoleUnavailable);
    };
    let outcome = insert_user(&mut tx, project, email, password_hash, display_name, &role).await?;
    match outcome {
        Inserted::Made(user) => {
            tx.commit().await.map_err(DbError::from_query)?;
            Ok(SignedUp::Made(user))
        }
        Inserted::Taken => Ok(SignedUp::Taken),
        Inserted::Full => Ok(SignedUp::Full),
    }
}

/// Creates a site user inside a staff transaction: `role` must exist but needn't be open —
/// staff put visitors in closed roles too.
pub async fn create_user(
    tx: &mut MemberTransaction,
    email: &str,
    password_hash: &str,
    display_name: Option<&str>,
    role: &str,
) -> Result<SignedUp, DbError> {
    if !role_exists_in(tx, role).await? {
        return Ok(SignedUp::RoleUnavailable);
    }
    let outcome = insert_user(
        &mut tx.transaction,
        tx.project,
        email,
        password_hash,
        display_name,
        role,
    )
    .await?;
    Ok(match outcome {
        Inserted::Made(user) => SignedUp::Made(user),
        Inserted::Taken => SignedUp::Taken,
        Inserted::Full => SignedUp::Full,
    })
}

/// What [`insert_user`] did, mapped to [`SignedUp`] by its callers.
enum Inserted {
    /// A new site user.
    Made(SiteUser),
    /// The email is taken.
    Taken,
    /// The project is at [`MAX_SITE_USERS`].
    Full,
}

/// The insert [`sign_up`] and [`create_user`] share: the count bound, then the row.
async fn insert_user(
    tx: &mut Transaction<'static, Postgres>,
    project: Uuid,
    email: &str,
    password_hash: &str,
    display_name: Option<&str>,
    role: &str,
) -> Result<Inserted, DbError> {
    let count = sqlx::query_scalar!(
        "SELECT count(*) AS \"count!\" FROM site_users WHERE project_id = $1",
        project
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(DbError::from_query)?;
    if count >= MAX_SITE_USERS {
        return Ok(Inserted::Full);
    }
    let id = Uuid::now_v7();
    let row = sqlx::query!(
        "INSERT INTO site_users (project_id, id, email, password_hash, display_name, role)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT DO NOTHING
         RETURNING created_at",
        project,
        id,
        email,
        password_hash,
        display_name,
        role
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(DbError::from_write)?;
    Ok(match row {
        Some(row) => Inserted::Made(SiteUser {
            id,
            email: email.to_owned(),
            display_name: display_name.map(str::to_owned),
            role: role.to_owned(),
            created_at: row.created_at,
            last_sign_in_at: None,
            disabled_at: None,
        }),
        None => Inserted::Taken,
    })
}

/// The answer to a site user's sign-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignIn {
    /// Right password; the attempt count for this address resets and the session may start.
    SignedIn(SiteUser),
    /// Wrong password, no such account, or this (account, address) paused after too many
    /// misses (D-154) — one answer for all three.
    Refused,
    /// Staff disabled the account; its sessions are already revoked.
    Disabled,
}

/// Signs a site user in, as a client at `address`, the way
/// [`crate::identity::password_sign_in`] signs in members (D-150, D-154): the slot first; one
/// short transaction that counts the attempt against the (project audience, email, address)
/// key — refusing unverified while it is paused — and reads the account; the verify with
/// nothing held; on a match the key forgotten and the sign-in stamped, re-checking
/// `disabled_at` so a disable during the verify still wins.
pub async fn sign_in(
    pool: &PgPool,
    gate: &crate::passwords::PasswordGate,
    project: Uuid,
    email: &str,
    address: &str,
    password: &str,
) -> Result<SignIn, DbError> {
    use crate::throttle::{self, Counted, Realm};
    let slot = gate.slot().await?;
    let mut tx = project_tx(pool, project).await?;
    if throttle::count(&mut tx, Realm::Audience(project), email, address).await? == Counted::Paused
    {
        return Ok(SignIn::Refused);
    }
    let row = sqlx::query!(
        "SELECT id, email, display_name, role, created_at, disabled_at, password_hash
         FROM site_users
         WHERE project_id = $1 AND email = $2",
        project,
        email
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(DbError::from_query)?;
    tx.commit().await.map_err(DbError::from_query)?;
    let Some(row) = row else {
        // No such account: the same verify work runs anyway, so the answer can't be told apart.
        let _ = slot.verify(password, None).await;
        return Ok(SignIn::Refused);
    };
    if row.disabled_at.is_some() {
        return Ok(SignIn::Disabled);
    }
    // Argon2 with no transaction open and no connection held.
    if !slot.verify(password, Some(&row.password_hash)).await {
        return Ok(SignIn::Refused);
    }
    let mut tx = project_tx(pool, project).await?;
    throttle::clear(&mut tx, Realm::Audience(project), email, address).await?;
    let enabled = sqlx::query_scalar!(
        "UPDATE site_users SET last_sign_in_at = now()
         WHERE project_id = $1 AND id = $2 AND disabled_at IS NULL
         RETURNING id",
        project,
        row.id
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(DbError::from_query)?;
    tx.commit().await.map_err(DbError::from_query)?;
    if enabled.is_none() {
        return Ok(SignIn::Disabled);
    }
    Ok(SignIn::SignedIn(SiteUser {
        id: row.id,
        email: row.email,
        display_name: row.display_name,
        role: row.role,
        created_at: row.created_at,
        last_sign_in_at: Some(OffsetDateTime::now_utc()),
        disabled_at: None,
    }))
}

/// A site session as the lookup finds it — the session joined to its user, live or not; the
/// caller applies [`Found::live`] to decide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The session's id (not the token).
    pub session_id: Uuid,
    /// Who it belongs to.
    pub site_user_id: Uuid,
    /// The account's email.
    pub email: String,
    /// The name it gave, if any.
    pub display_name: Option<String>,
    /// The audience role the account holds — read fresh on every lookup, so a role change is
    /// immediate.
    pub role: String,
    /// When staff disabled the account, if they have.
    pub disabled_at: Option<OffsetDateTime>,
    /// When the account was made.
    pub created_at: OffsetDateTime,
    /// When the session was revoked, if it was.
    pub revoked_at: Option<OffsetDateTime>,
    /// When it was last used.
    pub last_seen_at: OffsetDateTime,
    /// Its absolute end, use or not.
    pub expires_at: OffsetDateTime,
    /// The CSRF token's hash — cookie-authenticated writes echo the token in a header, as a
    /// staff session's do.
    pub csrf_hash: Vec<u8>,
}

impl Found {
    /// Whether the session may act as the audience now: not revoked, not past its absolute
    /// end, not idle over [`IDLE`], and its account not disabled.
    pub fn live(&self) -> bool {
        let now = OffsetDateTime::now_utc();
        self.revoked_at.is_none()
            && self.disabled_at.is_none()
            && self.expires_at > now
            && now - self.last_seen_at <= IDLE
    }

    /// The user behind the session, as staff see one.
    pub fn user(&self) -> SiteUser {
        SiteUser {
            id: self.site_user_id,
            email: self.email.clone(),
            display_name: self.display_name.clone(),
            role: self.role.clone(),
            created_at: self.created_at,
            last_sign_in_at: None,
            disabled_at: self.disabled_at,
        }
    }
}

/// The live session `hash` names in `project`, joined to its user: `colophon_audience_session`
/// (migration 0052) does the lookup and the bounded touch in one statement, as the migrator,
/// before any project context exists. Revoked, expired, idle and disabled sessions are found
/// too — [`Found::live`] says which.
pub async fn find_session(
    pool: &PgPool,
    project: Uuid,
    token_hash: &[u8; 32],
) -> Result<Option<Found>, DbError> {
    sqlx::query_as!(
        Found,
        r#"SELECT session_id AS "session_id!", site_user_id AS "site_user_id!", email AS "email!",
                  display_name, role AS "role!", disabled_at, created_at AS "created_at!",
                  revoked_at, last_seen_at AS "last_seen_at!", expires_at AS "expires_at!",
                  csrf_hash AS "csrf_hash!"
           FROM colophon_audience_session($1, $2)"#,
        project,
        &token_hash[..]
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)
}

/// Stores a new session for the site user: revokes the oldest of theirs over
/// [`MAX_SESSIONS`] first and sweeps up to a hundred sessions dead over ninety days — rows
/// are kept for the audit trail, not forever. Only the token's hash is stored.
pub async fn create_session(
    pool: &PgPool,
    project: Uuid,
    site_user_id: Uuid,
    token_hash: &[u8; 32],
    csrf_hash: &[u8; 32],
) -> Result<Uuid, DbError> {
    let mut tx = project_tx(pool, project).await?;
    // The new session is about to be one more: keep at most MAX_SESSIONS - 1 live before it,
    // so the total never exceeds the bound.
    sqlx::query!(
        "UPDATE site_sessions SET revoked_at = now()
         WHERE project_id = $1 AND site_user_id = $2 AND revoked_at IS NULL AND id IN (
             SELECT id FROM site_sessions
             WHERE project_id = $1 AND site_user_id = $2
               AND revoked_at IS NULL AND expires_at > now()
             ORDER BY last_seen_at DESC OFFSET $3)",
        project,
        site_user_id,
        MAX_SESSIONS - 1
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_write)?;
    sqlx::query!(
        "DELETE FROM site_sessions WHERE project_id = $1 AND id IN (
             SELECT id FROM site_sessions
             WHERE project_id = $1 AND expires_at < now() - interval '90 days'
             LIMIT 100)",
        project
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_write)?;
    let id = Uuid::now_v7();
    sqlx::query!(
        "INSERT INTO site_sessions (project_id, id, site_user_id, token_hash, csrf_hash, expires_at)
         VALUES ($1, $2, $3, $4, $5, now() + make_interval(secs => $6))",
        project,
        id,
        site_user_id,
        &token_hash[..],
        &csrf_hash[..],
        f64::from(u32::try_from(LIFETIME.whole_seconds()).unwrap_or(u32::MAX))
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_write)?;
    tx.commit().await.map_err(DbError::from_query)?;
    Ok(id)
}

/// Revokes the session — signout, or a session gone idle. Kept, not deleted.
pub async fn revoke_session(pool: &PgPool, project: Uuid, session_id: Uuid) -> Result<(), DbError> {
    let mut tx = project_tx(pool, project).await?;
    sqlx::query!(
        "UPDATE site_sessions SET revoked_at = now()
         WHERE project_id = $1 AND id = $2 AND revoked_at IS NULL",
        project,
        session_id
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_write)?;
    tx.commit().await.map_err(DbError::from_query)
}

/// A site session as staff see one — no token material, ever.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// Its id.
    pub id: Uuid,
    /// Who it belongs to.
    pub site_user_id: Uuid,
    /// When it was made.
    pub created_at: OffsetDateTime,
    /// When it was last used.
    pub last_seen_at: OffsetDateTime,
    /// Its absolute end, use or not.
    pub expires_at: OffsetDateTime,
    /// When it was revoked, if it was.
    pub revoked_at: Option<OffsetDateTime>,
}

/// The site user's sessions, newest first — bounded at [`crate::entries::MAX_PAGE`] like every
/// other list.
pub async fn sessions_of(
    tx: &mut MemberTransaction,
    site_user_id: Uuid,
) -> Result<Vec<Session>, DbError> {
    sqlx::query_as!(
        Session,
        "SELECT id, site_user_id, created_at, last_seen_at, expires_at, revoked_at
         FROM site_sessions
         WHERE project_id = $1 AND site_user_id = $2
         ORDER BY created_at DESC
         LIMIT $3",
        tx.project,
        site_user_id,
        crate::entries::MAX_PAGE
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Revokes the session inside a staff transaction; `false` when the project has no such
/// session (or it was already revoked).
pub async fn revoke_session_in(
    tx: &mut MemberTransaction,
    session_id: Uuid,
) -> Result<bool, DbError> {
    let gone = sqlx::query!(
        "UPDATE site_sessions SET revoked_at = now()
         WHERE project_id = $1 AND id = $2 AND revoked_at IS NULL",
        tx.project,
        session_id
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(gone.rows_affected() == 1)
}

/// The project's site users, newest first — [`crate::entries::MAX_PAGE`] is the page bound.
pub async fn list_users(
    tx: &mut MemberTransaction,
    before: Option<Uuid>,
    limit: i64,
) -> Result<Vec<SiteUser>, DbError> {
    sqlx::query_as!(
        SiteUser,
        r#"SELECT id, email, display_name, role, created_at, last_sign_in_at, disabled_at
           FROM site_users
           WHERE project_id = $1 AND ($2::uuid IS NULL OR id < $2)
           ORDER BY id DESC
           LIMIT $3"#,
        tx.project,
        before,
        limit.clamp(1, crate::entries::MAX_PAGE + 1)
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// The site user `id` names, if it is this project's.
pub async fn get_user(tx: &mut MemberTransaction, id: Uuid) -> Result<Option<SiteUser>, DbError> {
    sqlx::query_as!(
        SiteUser,
        r#"SELECT id, email, display_name, role, created_at, last_sign_in_at, disabled_at
           FROM site_users
           WHERE project_id = $1 AND id = $2"#,
        tx.project,
        id
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// What changing a site user did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserChanged {
    /// The row carries the change.
    Updated(SiteUser),
    /// No such site user in this project.
    Missing,
    /// The role it would hold doesn't exist.
    UnknownRole,
}

/// Applies a role change and/or a disable-or-enable to the site user, in one statement;
/// disabling revokes every live session of theirs in the same transaction, so a disabled
/// account's next request already reads as anonymous.
pub async fn update_user(
    tx: &mut MemberTransaction,
    id: Uuid,
    role: Option<&str>,
    disabled: Option<bool>,
) -> Result<UserChanged, DbError> {
    if let Some(role) = role
        && !role_exists_in(tx, role).await?
    {
        return Ok(UserChanged::UnknownRole);
    }
    let row = sqlx::query_as!(
        SiteUser,
        r#"UPDATE site_users
           SET role = coalesce($3, role),
               disabled_at = CASE WHEN $4::boolean IS NULL THEN disabled_at
                                  WHEN $4 THEN now() ELSE NULL END
           WHERE project_id = $1 AND id = $2
           RETURNING id, email, display_name, role, created_at, last_sign_in_at, disabled_at"#,
        tx.project,
        id,
        role,
        disabled
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    let Some(row) = row else {
        return Ok(UserChanged::Missing);
    };
    if disabled == Some(true) {
        sqlx::query!(
            "UPDATE site_sessions SET revoked_at = now()
             WHERE project_id = $1 AND site_user_id = $2 AND revoked_at IS NULL",
            tx.project,
            id
        )
        .execute(&mut *tx.transaction)
        .await
        .map_err(DbError::from_write)?;
    }
    Ok(UserChanged::Updated(row))
}

/// How long a magic link lives: long enough for a mail round trip, short enough that a leaked
/// inbox copy is worthless by the time anyone finds it (D-140).
pub const MAGIC_LIFETIME: Duration = Duration::minutes(15);

/// The most origins one project may list for its magic links (D-148): a site and its
/// preview hosts, not a directory of the web.
pub const MAX_ORIGINS: usize = 32;

/// The origins the project's magic links may land on (D-148), sorted.
pub async fn origins(tx: &mut MemberTransaction) -> Result<Vec<String>, DbError> {
    sqlx::query_scalar!(
        "SELECT origin FROM audience_origins WHERE project_id = $1 ORDER BY origin",
        tx.project
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Replaces the project's origin list with `origins` — each already normalized by the
/// caller (`scheme://host[:port]`, lowercase), at most [`MAX_ORIGINS`] of them.
pub async fn set_origins(tx: &mut MemberTransaction, origins: &[String]) -> Result<(), DbError> {
    sqlx::query!(
        "DELETE FROM audience_origins WHERE project_id = $1",
        tx.project
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    sqlx::query!(
        "INSERT INTO audience_origins (project_id, origin)
         SELECT $1, origin FROM unnest($2::text[]) AS listed (origin)
         ON CONFLICT DO NOTHING",
        tx.project,
        origins
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// The bounds on magic-link requests (D-153, D-161), configurable per deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MagicLinkLimits {
    /// Links one client address may ask for in one project per [`Self::address_window`]:
    /// checked first, so one client can never spend the project's shared ceiling.
    pub per_address: i64,
    /// The window [`Self::per_address`] counts over.
    pub address_window: Duration,
    /// Links one project may record per [`Self::project_window`], every address together.
    pub per_project: i64,
    /// The window [`Self::per_project`] counts over.
    pub project_window: Duration,
    /// The wait after a recipient's first unused link; each further unused link in the last
    /// day doubles it, up to [`Self::cooldown_cap`].
    pub cooldown: Duration,
    /// The longest a recipient waits between links.
    pub cooldown_cap: Duration,
}

impl MagicLinkLimits {
    /// Five links an address per ten minutes, three hundred a project per minute, and a
    /// recipient's wait doubling from a minute to an hour.
    pub const DEFAULT: Self = Self {
        per_address: 5,
        address_window: Duration::minutes(10),
        per_project: 300,
        project_window: Duration::MINUTE,
        cooldown: Duration::MINUTE,
        cooldown_cap: Duration::HOUR,
    };
}

impl Default for MagicLinkLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A duration as the float seconds `make_interval` takes.
fn seconds_of(duration: Duration) -> f64 {
    f64::from(u32::try_from(duration.whole_seconds()).unwrap_or(u32::MAX))
}

/// What a magic-link request came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkRequest {
    /// The link is recorded; the caller may send it.
    Recorded,
    /// `return_to`'s origin isn't one the project lists (D-148): nothing was recorded, so no
    /// token exists for anyone to receive.
    OriginNotAllowed,
    /// Too soon (D-153, D-161): the asking client is over its own share, the recipient's
    /// newest unused link is younger than its (escalating) cooldown, or the project is at its
    /// ceiling. Nothing was recorded; ask again after this many seconds.
    Wait(u32),
}

/// Whole seconds until `remaining` has passed, at least one: what `Retry-After` says.
fn seconds_until(remaining: f64) -> u32 {
    if remaining.is_finite() && remaining > 1.0 {
        // Saturating float-to-int: a remaining time is at most a window or the cooldown cap.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "positive, finite and at most the window by construction"
        )]
        let whole = remaining.ceil().min(f64::from(u32::MAX)) as u32;
        whole
    } else {
        1
    }
}

/// Whether a link request must wait, and how many seconds (D-153, D-161), counted inside the
/// request's transaction: the asking client's share, then the recipient's escalating
/// cooldown, then the project's ceiling.
async fn too_soon(
    tx: &mut Transaction<'static, Postgres>,
    limits: &MagicLinkLimits,
    project: Uuid,
    email: &str,
    address: &str,
) -> Result<Option<u32>, DbError> {
    // One request per asking client and per recipient at a time (D-153, D-161): the counts
    // below and the insert run under both advisory locks — the client's first, always, so two
    // requests can never wait on each other's — and concurrent requests can't both find room.
    sqlx::query!(
        "SELECT 1 AS \"locked!\" FROM pg_advisory_xact_lock(22354, hashtext($1::uuid::text || '/' || $2))",
        project,
        address
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(DbError::from_query)?;
    sqlx::query!(
        "SELECT 1 AS \"locked!\" FROM pg_advisory_xact_lock(22353, hashtext($1::uuid::text || '/' || $2))",
        project,
        email
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(DbError::from_query)?;
    // The asking client's own share first (D-161): one client over it is refused before it
    // can touch the ceiling everyone shares.
    let address_window = seconds_of(limits.address_window);
    let asked = sqlx::query!(
        r#"SELECT count(*) AS "links!",
                  EXTRACT(EPOCH FROM min(created_at) + make_interval(secs => $3) - now())::float8
                      AS "remaining"
           FROM audience_magic_links
           WHERE project_id = $1 AND requested_from = $2
             AND created_at > now() - make_interval(secs => $3)"#,
        project,
        address,
        address_window
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(DbError::from_query)?;
    if asked.links >= limits.per_address {
        return Ok(Some(seconds_until(
            asked.remaining.unwrap_or(address_window),
        )));
    }
    // The recipient's cooldown escalates (D-161): with k links unused in the last day, the
    // newest must be `cooldown × 2^(k−1)` old (at most `cooldown_cap`) before another goes —
    // a visitor who never asked is mailed a handful of times, not every minute. Spending a link
    // retires its siblings, so a visitor who signs in starts again at zero.
    let waiting = sqlx::query_scalar!(
        "SELECT EXTRACT(EPOCH FROM max(created_at)
                    + make_interval(secs => least($3 * power(2::float8, (count(*) - 1)::float8), $4))
                    - now())::float8
         FROM audience_magic_links
         WHERE project_id = $1 AND email = $2 AND used_at IS NULL
           AND created_at > now() - interval '1 day'",
        project,
        email,
        seconds_of(limits.cooldown),
        seconds_of(limits.cooldown_cap)
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(DbError::from_query)?;
    if let Some(remaining) = waiting
        && remaining > 0.0
    {
        return Ok(Some(seconds_until(remaining)));
    }
    let window = seconds_of(limits.project_window);
    let recent = sqlx::query!(
        r#"SELECT count(*) AS "links!",
                  EXTRACT(EPOCH FROM min(created_at) + make_interval(secs => $2) - now())::float8
                      AS "remaining"
           FROM audience_magic_links
           WHERE project_id = $1 AND created_at > now() - make_interval(secs => $2)"#,
        project,
        window
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(DbError::from_query)?;
    if recent.links >= limits.per_project {
        return Ok(Some(seconds_until(recent.remaining.unwrap_or(window))));
    }
    Ok(None)
}

/// Records a magic link for `email` whose `return_to` lands on `origin` — only when the
/// project lists that origin (D-148). Earlier links for the address stay live: a request is
/// not proof of anything, so it must not be able to cancel the link a visitor already holds;
/// spending any one link retires the rest ([`consume_magic_link`]). `token_hash` is the
/// SHA-256 of the token the caller handed out; the token itself is never stored.
#[expect(
    clippy::too_many_arguments,
    reason = "one request's facts: who asked, for whom, where it lands, under which bounds"
)]
pub async fn request_magic_link(
    pool: &PgPool,
    limits: &MagicLinkLimits,
    project: Uuid,
    email: &str,
    address: &str,
    token_hash: &[u8; 32],
    return_to: &str,
    origin: &str,
) -> Result<LinkRequest, DbError> {
    let mut tx = project_tx(pool, project).await?;
    let listed = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM audience_origins WHERE project_id = $1 AND origin = $2)
             AS \"listed!\"",
        project,
        origin
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(DbError::from_query)?;
    if !listed {
        return Ok(LinkRequest::OriginNotAllowed);
    }
    if let Some(wait) = too_soon(&mut tx, limits, project, email, address).await? {
        return Ok(LinkRequest::Wait(wait));
    }
    sqlx::query!(
        "INSERT INTO audience_magic_links
             (project_id, id, email, token_hash, return_to, expires_at, requested_from)
         VALUES ($1, $2, $3, $4, $5, now() + make_interval(secs => $6), $7)",
        project,
        Uuid::now_v7(),
        email,
        &token_hash[..],
        return_to,
        seconds_of(MAGIC_LIFETIME),
        address
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_query)?;
    tx.commit().await.map_err(DbError::from_query)?;
    Ok(LinkRequest::Recorded)
}

/// What a magic link's token turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MagicLink {
    /// A live link naming an existing, enabled account: signed in.
    SignedIn(SiteUser),
    /// A live link naming an email with no account here: the caller may create one in the
    /// project's open self-assign role, then sign in.
    NewUser(String),
    /// Unknown, used, expired or retired by a sibling's use — one answer for all of them.
    Invalid,
}

/// Consumes a magic link by its token's hash: single use, and only while live. The account
/// lookup runs in the same transaction, so a link can never sign in an account disabled
/// between the check and the session.
pub async fn consume_magic_link(
    pool: &PgPool,
    project: Uuid,
    token_hash: &[u8; 32],
) -> Result<MagicLink, DbError> {
    let mut tx = project_tx(pool, project).await?;
    let link = sqlx::query!(
        "SELECT id, email FROM audience_magic_links
         WHERE project_id = $1 AND token_hash = $2 AND used_at IS NULL AND expires_at > now()
         FOR UPDATE",
        project,
        &token_hash[..]
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(DbError::from_query)?;
    let Some(link) = link else {
        return Ok(MagicLink::Invalid);
    };
    // Spending one link retires every other one outstanding for the address (D-148): the
    // locks this takes make a concurrent spend of a sibling re-check `used_at` and refuse.
    sqlx::query!(
        "UPDATE audience_magic_links SET used_at = now()
         WHERE project_id = $1 AND email = $2 AND used_at IS NULL",
        project,
        link.email
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_query)?;
    let user = sqlx::query!(
        "SELECT id, email, display_name, role, created_at, last_sign_in_at
         FROM site_users
         WHERE project_id = $1 AND email = $2 AND disabled_at IS NULL
         FOR UPDATE",
        project,
        link.email
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(DbError::from_query)?;
    let Some(user) = user else {
        tx.commit().await.map_err(DbError::from_query)?;
        return Ok(MagicLink::NewUser(link.email));
    };
    sqlx::query!(
        "UPDATE site_users SET last_sign_in_at = now(), failed_sign_ins = 0, locked_until = NULL
         WHERE project_id = $1 AND id = $2",
        project,
        user.id
    )
    .execute(&mut *tx)
    .await
    .map_err(DbError::from_query)?;
    tx.commit().await.map_err(DbError::from_query)?;
    Ok(MagicLink::SignedIn(SiteUser {
        id: user.id,
        email: user.email,
        display_name: user.display_name,
        role: user.role,
        created_at: user.created_at,
        last_sign_in_at: Some(OffsetDateTime::now_utc()),
        disabled_at: None,
    }))
}
