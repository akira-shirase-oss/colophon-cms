//! Members and invitations (docs/modules/03-auth.md, "Roles and memberships", "Invitations";
//! D-042). Changes run inside an owner's [`MemberTransaction`]; the policy check (who may)
//! happens before, in the API. An invitation's token is a capability: holding it is what lets a
//! signed-in person see the project's name and join it.

use sqlx::{PgPool, types::time::OffsetDateTime};
use uuid::Uuid;

use crate::{
    DbError,
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
    /// The role it grants.
    pub role: String,
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

/// Locks the project's owners, so concurrent changes to them queue; how many there are.
async fn lock_owners(tx: &mut MemberTransaction) -> Result<Vec<Uuid>, DbError> {
    sqlx::query_scalar!(
        "SELECT identity_id FROM memberships WHERE project_id = $1 AND role = 'owner' FOR UPDATE",
        tx.project
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Changes `identity`'s role, refusing to take the last owner away.
pub async fn change_role(
    tx: &mut MemberTransaction,
    identity: Uuid,
    role: &str,
) -> Result<Change, DbError> {
    let owners = lock_owners(tx).await?;
    if role != "owner" && owners == [identity] {
        return Ok(Change::LastOwner);
    }
    let changed = sqlx::query!(
        "UPDATE memberships SET role = $3 WHERE project_id = $1 AND identity_id = $2",
        tx.project,
        identity,
        role
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(if changed.rows_affected() == 0 {
        Change::NotFound
    } else {
        Change::Done
    })
}

/// Removes `identity` from the project, refusing to remove the last owner.
pub async fn remove(tx: &mut MemberTransaction, identity: Uuid) -> Result<Change, DbError> {
    let owners = lock_owners(tx).await?;
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

/// Stores an invitation to the transaction's project, made by its identity; only the token's
/// hash.
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
    let project_name =
        sqlx::query_scalar!("SELECT name FROM projects WHERE id = $1", found.project_id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(Some(Offer {
        project_id: found.project_id,
        project_name,
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
