//! The audience a project publishes for (docs/modules/03-auth.md, "Audience"; D-137): site
//! users — the visitors of a project's sites — their Argon2id password credentials, their
//! opaque sessions and the audience roles that gate published content. Members and site users
//! never meet: a member manages the project; a visitor only reads what it delivers.
//!
//! Account functions here take the project explicitly and run inside
//! [`scope::begin`] with no identity: RLS confines them, and `site_users`' lockout state is
//! written under a row lock like [`crate::identity::password_sign_in`]'s. Session lookups go
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

/// Consecutive misses before the account pauses — bounded, so guessing costs time and shows.
pub const LOCK_AFTER: i16 = 8;
/// How long a locked account stays locked.
pub const LOCK_FOR: Duration = Duration::minutes(15);

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
    /// Right password; the counter resets and the session may start.
    SignedIn(SiteUser),
    /// Wrong password, or no such account — the same answer, after the same work.
    Refused,
    /// Too many misses; the account pauses until this time.
    Locked(OffsetDateTime),
    /// Staff disabled the account; its sessions are already revoked.
    Disabled,
}

/// Signs a site user in with an email and a password: the lock, disabled and counter checks
/// all run under one row lock, as [`crate::identity::password_sign_in`] does for members.
pub async fn sign_in(
    pool: &PgPool,
    project: Uuid,
    email: &str,
    password: &str,
) -> Result<SignIn, DbError> {
    let mut tx = project_tx(pool, project).await?;
    let row = sqlx::query!(
        "SELECT id, email, display_name, role, created_at, last_sign_in_at, disabled_at,
                password_hash, locked_until
         FROM site_users
         WHERE project_id = $1 AND email = $2
         FOR UPDATE",
        project,
        email
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(DbError::from_query)?;
    let Some(row) = row else {
        // No such account: the same verify work runs anyway, so the answer can't be told apart.
        let _ = crate::passwords::verify_in(password, None).await;
        return Ok(SignIn::Refused);
    };
    if row.disabled_at.is_some() {
        return Ok(SignIn::Disabled);
    }
    if row
        .locked_until
        .is_some_and(|until| until > OffsetDateTime::now_utc())
    {
        return Ok(SignIn::Locked(
            row.locked_until.unwrap_or_else(OffsetDateTime::now_utc),
        ));
    }
    let right = crate::passwords::verify_in(password, Some(&row.password_hash)).await;
    if right {
        sqlx::query!(
            "UPDATE site_users SET failed_sign_ins = 0, locked_until = NULL, last_sign_in_at = now()
             WHERE project_id = $1 AND id = $2",
            project,
            row.id
        )
        .execute(&mut *tx)
        .await
        .map_err(DbError::from_query)?;
        tx.commit().await.map_err(DbError::from_query)?;
        return Ok(SignIn::SignedIn(SiteUser {
            id: row.id,
            email: row.email,
            display_name: row.display_name,
            role: row.role,
            created_at: row.created_at,
            last_sign_in_at: Some(OffsetDateTime::now_utc()),
            disabled_at: None,
        }));
    }
    let outcome = sqlx::query_scalar!(
        "UPDATE site_users
         SET failed_sign_ins = least(failed_sign_ins + 1, 32767),
             locked_until = CASE WHEN failed_sign_ins + 1 >= $3 THEN now() + make_interval(secs => $4) END
         WHERE project_id = $1 AND id = $2
         RETURNING locked_until",
        project,
        row.id,
        i32::from(LOCK_AFTER),
        f64::from(u32::try_from(LOCK_FOR.whole_seconds()).unwrap_or(u32::MAX))
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(DbError::from_query)?;
    tx.commit().await.map_err(DbError::from_query)?;
    Ok(match outcome {
        Some(until) => SignIn::Locked(until),
        None => SignIn::Refused,
    })
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
