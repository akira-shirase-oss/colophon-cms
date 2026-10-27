//! Members and invitations (docs/modules/03-auth.md, "Roles and memberships", "Invitations";
//! D-042, D-058). The API checks the role read when the transaction began; role changes and
//! removals check again here, under the locks that keep it true until commit, what the caller's
//! role allows now: managing members, and every power of the roles involved, so nobody gives or
//! takes away more than they hold. An invitation's token is a capability: holding it is what lets
//! a signed-in person see the project's name and join it.

use std::collections::HashMap;

use colophon_domain::policy::{Action, Policy};
use sqlx::{PgPool, types::time::OffsetDateTime};
use uuid::Uuid;

use crate::{
    DbError,
    roles::{self, OWNER},
    scope::{self, MemberTransaction, Scope},
};

/// A member of a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// Their identity.
    pub identity_id: Uuid,
    /// Their name, if the identity provider gave one.
    pub display_name: Option<String>,
    /// Their role.
    pub role: String,
    /// When they joined.
    pub joined_at: OffsetDateTime,
}

/// What a role change or a removal did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Done.
    Done,
    /// Not a member of this project.
    NotFound,
    /// Refused: the project would have no owner left.
    LastOwner,
    /// Refused: the caller's role, as it is now, doesn't manage members, or lacks a power of a
    /// role involved. The role read when their transaction began may have been stale, because a
    /// change to it was committing at the same moment.
    NotPermitted,
    /// Refused: the project has no role with that key.
    UnknownRole,
}

/// A pending invitation, as its project's owners see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invitation {
    /// Its id.
    pub id: Uuid,
    /// The role it grants.
    pub role: String,
    /// Who it's for, as the owner wrote it.
    pub label: String,
    /// When it was made.
    pub created_at: OffsetDateTime,
    /// When it stops working.
    pub expires_at: OffsetDateTime,
}

/// A pending invitation, as the person holding its link sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// The project's id.
    pub project_id: Uuid,
    /// The project's name.
    pub project_name: String,
    /// The role it grants, by key.
    pub role: String,
    /// That role's name, for people.
    pub role_name: String,
    /// What that role is for.
    pub role_description: String,
    /// The name of the person who invited them.
    pub invited_by: Option<String>,
    /// When it stops working.
    pub expires_at: OffsetDateTime,
}

/// What accepting did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Accepted {
    /// The caller is now a member.
    Joined {
        /// The project.
        project_id: Uuid,
        /// Their role.
        role: String,
    },
    /// The caller already belongs to the project; the invitation stays unused for whoever it was
    /// meant for.
    AlreadyMember,
    /// Unknown, expired, revoked or already used.
    Gone,
}

/// The project's members, owners first, then by name.
pub async fn list(tx: &mut MemberTransaction) -> Result<Vec<Member>, DbError> {
    sqlx::query_as!(
        Member,
        "SELECT m.identity_id, i.display_name, m.role, m.created_at AS joined_at
         FROM memberships m JOIN identities i ON i.id = m.identity_id
         WHERE m.project_id = $1
         ORDER BY m.role <> 'owner', i.display_name, m.identity_id
         LIMIT 1000",
        tx.project
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Locks the project's owners, so concurrent changes to them queue; who they are, once any change
/// to them that was in flight has committed.
async fn lock_owners(tx: &mut MemberTransaction) -> Result<Vec<Uuid>, DbError> {
    sqlx::query_scalar!(
        "SELECT identity_id FROM memberships WHERE project_id = $1 AND role = 'owner' FOR UPDATE",
        tx.project
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// What a member change needs, read under the locks that keep it true until commit: the caller's
/// role and the target's (both memberships locked; every member change first queues on the owners,
/// so the order can't deadlock), and the role being given (key-share locked, so it can't be deleted
/// first). Two round trips under the owner lock, because every change to one project's members
/// waits on that lock.
struct Standing {
    caller: Option<Policy>,
    target: Option<Policy>,
    /// `None` when no role is being given, or the project has no such role.
    granted: Option<Policy>,
}

async fn standing(
    tx: &mut MemberTransaction,
    identity: Uuid,
    granted: Option<&str>,
) -> Result<Standing, DbError> {
    let held = sqlx::query!(
        "SELECT identity_id, role FROM memberships
         WHERE project_id = $1 AND identity_id = ANY($2)
         FOR UPDATE",
        tx.project,
        &[tx.identity, identity][..]
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    let role_of = |who: Uuid| {
        held.iter()
            .find(|row| row.identity_id == who)
            .map(|row| row.role.clone())
    };
    let (caller_role, target_role) = (role_of(tx.identity), role_of(identity));
    let mut keys: Vec<String> = [caller_role.clone(), target_role.clone()]
        .into_iter()
        .flatten()
        .chain(granted.map(str::to_owned))
        .collect();
    keys.sort();
    keys.dedup();
    let rows = sqlx::query!(
        r#"SELECT r.key,
                  coalesce(p.actions, '{}') AS "actions!",
                  coalesce(p.subjects, '{}') AS "subjects!",
                  coalesce(p.fields, '{}') AS "fields!: Vec<Option<String>>",
                  coalesce(p.conditions, '{}') AS "conditions!: Vec<Option<String>>"
           FROM roles r
           LEFT JOIN LATERAL (
               SELECT array_agg(action ORDER BY action, subject) AS actions,
                      array_agg(subject ORDER BY action, subject) AS subjects,
                      array_agg(array_to_string(fields, ',') ORDER BY action, subject) AS fields,
                      array_agg(condition ORDER BY action, subject) AS conditions
               FROM role_permissions
               WHERE project_id = r.project_id AND role_key = r.key
           ) p ON true
           WHERE r.project_id = $1 AND r.key = ANY($2)
           FOR KEY SHARE OF r"#,
        tx.project,
        &keys
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    let mut policies = HashMap::with_capacity(rows.len());
    for row in rows {
        let policy = scope::policy(row.actions, row.subjects, row.fields, row.conditions)?;
        policies.insert(row.key, policy);
    }
    let held_policy = |role: Option<String>| {
        role.map(|role| {
            policies.get(&role).cloned().ok_or(DbError::Corrupt(
                "a member holds a role the project doesn't have",
            ))
        })
        .transpose()
    };
    Ok(Standing {
        caller: held_policy(caller_role)?,
        target: held_policy(target_role)?,
        granted: granted.and_then(|role| policies.get(role).cloned()),
    })
}

/// Changes `identity`'s role to `role`, refusing to take the last owner away, or to act for a
/// caller whose role, as it is now, doesn't cover it.
pub async fn change_role(
    tx: &mut MemberTransaction,
    identity: Uuid,
    role: &str,
) -> Result<Change, DbError> {
    let owners = lock_owners(tx).await?;
    let standing = standing(tx, identity, Some(role)).await?;
    let Some(caller) = standing.caller else {
        return Ok(Change::NotPermitted);
    };
    let Some(current) = standing.target else {
        return Ok(Change::NotFound);
    };
    let Some(granted) = standing.granted else {
        return Ok(Change::UnknownRole);
    };
    if !(caller.allows(Action::MembersManage, None)
        && caller.covers(&current)
        && caller.covers(&granted))
    {
        return Ok(Change::NotPermitted);
    }
    if role != OWNER && owners == [identity] {
        return Ok(Change::LastOwner);
    }
    sqlx::query!(
        "UPDATE memberships SET role = $3 WHERE project_id = $1 AND identity_id = $2",
        tx.project,
        identity,
        role
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(Change::Done)
}

/// Removes `identity` from the project, refusing to remove the last owner. Anyone may remove
/// themselves; removing someone else needs the caller's role, as it is now, to manage members and
/// to cover the role they hold.
pub async fn remove(tx: &mut MemberTransaction, identity: Uuid) -> Result<Change, DbError> {
    let owners = lock_owners(tx).await?;
    if identity != tx.identity {
        let standing = standing(tx, identity, None).await?;
        let Some(caller) = standing.caller else {
            return Ok(Change::NotPermitted);
        };
        let Some(current) = standing.target else {
            return Ok(Change::NotFound);
        };
        if !(caller.allows(Action::MembersManage, None) && caller.covers(&current)) {
            return Ok(Change::NotPermitted);
        }
    }
    if owners == [identity] {
        return Ok(Change::LastOwner);
    }
    let removed = sqlx::query!(
        "DELETE FROM memberships WHERE project_id = $1 AND identity_id = $2",
        tx.project,
        identity
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(if removed.rows_affected() == 0 {
        Change::NotFound
    } else {
        Change::Done
    })
}

/// What the caller's role allows now, holding their membership until the transaction ends: the
/// policy read when the transaction began may be stale, and a change to the role that is in flight
/// is waited for. `None` if they were just removed.
pub async fn current_policy(tx: &mut MemberTransaction) -> Result<Option<Policy>, DbError> {
    let role = sqlx::query_scalar!(
        "SELECT role FROM memberships WHERE project_id = $1 AND identity_id = $2 FOR SHARE",
        tx.project,
        tx.identity
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    let Some(role) = role else { return Ok(None) };
    roles::policy_of(tx, &role)
        .await?
        .ok_or(DbError::Corrupt(
            "a member holds a role the project doesn't have",
        ))
        .map(Some)
}

/// Stores an invitation to the transaction's project, made by its identity; only the token's
/// hash. The caller has read the role with [`roles::policy_of`] in this transaction, whose lock
/// keeps it from being deleted before this commits.
pub async fn invite(
    tx: &mut MemberTransaction,
    token_hash: &[u8; 32],
    role: &str,
    label: &str,
    expires_at: OffsetDateTime,
) -> Result<Invitation, DbError> {
    sqlx::query_as!(
        Invitation,
        "INSERT INTO invitations (id, project_id, token_hash, role, label, invited_by, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, role, label, created_at, expires_at",
        Uuid::now_v7(),
        tx.project,
        &token_hash[..],
        role,
        label,
        tx.identity,
        expires_at
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)
}

/// The project's pending invitations, newest first.
pub async fn invitations(tx: &mut MemberTransaction) -> Result<Vec<Invitation>, DbError> {
    sqlx::query_as!(
        Invitation,
        "SELECT id, role, label, created_at, expires_at FROM invitations
         WHERE project_id = $1 AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at > now()
         ORDER BY created_at DESC
         LIMIT 1000",
        tx.project
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Revokes a pending invitation of this project; false if there is none.
pub async fn revoke(tx: &mut MemberTransaction, id: Uuid) -> Result<bool, DbError> {
    let revoked = sqlx::query!(
        "UPDATE invitations SET revoked_at = now()
         WHERE id = $1 AND project_id = $2 AND accepted_at IS NULL AND revoked_at IS NULL",
        id,
        tx.project
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(revoked.rows_affected() == 1)
}

/// The pending invitation with this token hash, for the person holding the link.
pub async fn offer(
    pool: &PgPool,
    identity: Uuid,
    token_hash: &[u8; 32],
) -> Result<Option<Offer>, DbError> {
    let found = sqlx::query!(
        "SELECT n.project_id, n.role, n.expires_at, i.display_name AS invited_by
         FROM invitations n JOIN identities i ON i.id = n.invited_by
         WHERE n.token_hash = $1 AND n.accepted_at IS NULL AND n.revoked_at IS NULL AND n.expires_at > now()",
        &token_hash[..]
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)?;
    let Some(found) = found else { return Ok(None) };
    // The token is what entitles its holder to see this project's name.
    let mut transaction = scope::begin(
        pool,
        Scope {
            identity: Some(identity),
            project: Some(found.project_id),
        },
    )
    .await?;
    let named = sqlx::query!(
        "SELECT p.name AS project_name, r.name AS role_name, r.description AS role_description
         FROM projects p JOIN roles r ON r.project_id = p.id AND r.key = $2
         WHERE p.id = $1",
        found.project_id,
        found.role
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    // A pending invitation's role can't be deleted (colophon_db::roles), so this is only `None`
    // for an invitation that is no longer any use.
    let Some(named) = named else { return Ok(None) };
    Ok(Some(Offer {
        project_id: found.project_id,
        project_name: named.project_name,
        role_name: named.role_name,
        role_description: named.role_description,
        role: found.role,
        invited_by: found.invited_by,
        expires_at: found.expires_at,
    }))
}

/// Accepts the invitation with this token hash as `identity`: the membership and the invitation's
/// use, in one transaction.
pub async fn accept(
    pool: &PgPool,
    identity: Uuid,
    token_hash: &[u8; 32],
) -> Result<Accepted, DbError> {
    let mut transaction = scope::begin(
        pool,
        Scope {
            identity: Some(identity),
            project: None,
        },
    )
    .await?;
    let found = sqlx::query!(
        "SELECT id, project_id, role FROM invitations
         WHERE token_hash = $1 AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at > now()
         FOR UPDATE",
        &token_hash[..]
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    let Some(found) = found else {
        return Ok(Accepted::Gone);
    };
    sqlx::query!(
        "SELECT set_config('app.project_id', $1, true) AS project_scope",
        found.project_id.to_string()
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    let joined = sqlx::query!(
        "INSERT INTO memberships (project_id, identity_id, role) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        found.project_id,
        identity,
        found.role
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_write)?;
    if joined.rows_affected() == 0 {
        return Ok(Accepted::AlreadyMember);
    }
    sqlx::query!(
        "UPDATE invitations SET accepted_by = $2, accepted_at = now() WHERE id = $1",
        found.id,
        identity
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(Accepted::Joined {
        project_id: found.project_id,
        role: found.role,
    })
}
