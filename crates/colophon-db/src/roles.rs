//! A project's roles (docs/modules/03-auth.md, "Roles and memberships"; D-058, step R.2).
//!
//! Any member reads them. Writes run after the API has checked `roles.manage` and that the
//! caller holds every power they grant ([`Policy::covers`]), and they queue on the project's owner
//! role row, so two changes to one project's roles never interleave. The owner role is fixed:
//! these functions say so before trying, and migration 0018 refuses it anyway.

use colophon_domain::policy::{MAX_ROLES, Policy};
use sqlx::types::time::OffsetDateTime;

use crate::{DbError, scope::MemberTransaction};

/// The role nobody may change.
pub const OWNER: &str = "owner";

/// A role, with who uses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRole {
    /// Its key, as memberships and invitations name it.
    pub key: String,
    /// Its name, for people.
    pub name: String,
    /// What it's for.
    pub description: String,
    /// One of the four every project starts with.
    pub built_in: bool,
    /// Its version, starting at 1; every change adds one.
    pub version: i32,
    /// When it last changed.
    pub updated_at: OffsetDateTime,
    /// What it allows.
    pub policy: Policy,
    /// How many members hold it.
    pub members: i64,
    /// How many pending invitations offer it.
    pub invitations: i64,
}

/// What creating or changing a role did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Written {
    /// Done; the role as stored now.
    Done(Box<StoredRole>),
    /// The project already has a role with this key.
    Exists,
    /// The project has [`MAX_ROLES`] roles already.
    TooMany,
    /// No such role.
    NotFound,
    /// The owner role, which nobody changes.
    Fixed,
    /// The role is at another version now; nothing was written.
    Stale {
        /// Its version now.
        current: i32,
    },
}

/// What deleting a role did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removed {
    /// Done.
    Done,
    /// No such role.
    NotFound,
    /// The owner role, which nobody deletes.
    Fixed,
    /// The role is at another version now; nothing was deleted.
    Stale {
        /// Its version now.
        current: i32,
    },
    /// People hold it, or pending invitations offer it.
    InUse {
        /// Members who hold it.
        members: i64,
        /// Pending invitations that offer it.
        invitations: i64,
    },
}

/// The project's roles, the built-in ones first in D-042's order (they are seeded in one
/// statement, so they share a creation time), then in the order they were made; or the one with
/// `key`.
async fn read(tx: &mut MemberTransaction, key: Option<&str>) -> Result<Vec<StoredRole>, DbError> {
    let rows = sqlx::query!(
        r#"SELECT r.key, r.name, r.description, r.built_in, r.version, r.updated_at,
                  (SELECT count(*) FROM memberships m
                   WHERE m.project_id = r.project_id AND m.role = r.key) AS "members!",
                  (SELECT count(*) FROM invitations n
                   WHERE n.project_id = r.project_id AND n.role = r.key
                     AND n.accepted_at IS NULL AND n.revoked_at IS NULL
                     AND n.expires_at > now()) AS "invitations!",
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
           WHERE r.project_id = $1 AND ($2::text IS NULL OR r.key = $2)
           ORDER BY array_position(ARRAY['owner', 'developer', 'editor', 'publisher'], r.key),
                    r.created_at, r.key
           LIMIT 100"#,
        tx.project,
        key
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    rows.into_iter()
        .map(|row| {
            Ok(StoredRole {
                policy: crate::scope::policy(
                    row.actions,
                    row.subjects,
                    row.fields,
                    row.conditions,
                )?,
                key: row.key,
                name: row.name,
                description: row.description,
                built_in: row.built_in,
                version: row.version,
                updated_at: row.updated_at,
                members: row.members,
                invitations: row.invitations,
            })
        })
        .collect()
}

/// The project's roles, the built-in ones first.
pub async fn list(tx: &mut MemberTransaction) -> Result<Vec<StoredRole>, DbError> {
    read(tx, None).await
}

/// The role with this key.
pub async fn get(tx: &mut MemberTransaction, key: &str) -> Result<Option<StoredRole>, DbError> {
    Ok(read(tx, Some(key)).await?.pop())
}

/// What the role with this key allows, holding a key-share lock on it until the transaction
/// ends, so it can't be deleted while a membership or an invitation is made to name it. `None` if
/// the project has no such role.
pub async fn policy_of(tx: &mut MemberTransaction, key: &str) -> Result<Option<Policy>, DbError> {
    let found = sqlx::query_scalar!(
        "SELECT 1 AS found FROM roles WHERE project_id = $1 AND key = $2 FOR KEY SHARE",
        tx.project,
        key
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    if found.is_none() {
        return Ok(None);
    }
    let rows = sqlx::query!(
        r#"SELECT action, subject, array_to_string(fields, ',') AS "fields?", condition
           FROM role_permissions WHERE project_id = $1 AND role_key = $2"#,
        tx.project,
        key
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    let mut actions = Vec::with_capacity(rows.len());
    let mut subjects = Vec::with_capacity(rows.len());
    let mut fields = Vec::with_capacity(rows.len());
    let mut conditions = Vec::with_capacity(rows.len());
    for row in rows {
        actions.push(row.action);
        subjects.push(row.subject);
        fields.push(row.fields);
        conditions.push(row.condition);
    }
    crate::scope::policy(actions, subjects, fields, conditions).map(Some)
}

/// Queues this transaction behind any other change to the project's roles. `NO KEY UPDATE`, so
/// it doesn't wait for (or block) the key-share locks that member changes and invitations take on
/// the owner role, which would let a role deletion and a member change wait for each other.
async fn lock(tx: &mut MemberTransaction) -> Result<(), DbError> {
    sqlx::query_scalar!(
        "SELECT 1 AS locked FROM roles WHERE project_id = $1 AND key = 'owner' FOR NO KEY UPDATE",
        tx.project
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?
    .ok_or(DbError::Corrupt("a project has no owner role"))?;
    Ok(())
}

async fn insert_permissions(
    tx: &mut MemberTransaction,
    key: &str,
    policy: &Policy,
) -> Result<(), DbError> {
    let (actions, subjects): (Vec<&str>, Vec<&str>) = policy
        .permissions()
        .iter()
        .map(|permission| (permission.action.as_str(), permission.subject.as_str()))
        .unzip();
    sqlx::query!(
        "INSERT INTO role_permissions (project_id, role_key, action, subject)
         SELECT $1, $2, action, subject FROM unnest($3::text[], $4::text[]) AS t (action, subject)",
        tx.project,
        key,
        &actions as &[&str],
        &subjects as &[&str]
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

async fn written(tx: &mut MemberTransaction, key: &str) -> Result<Written, DbError> {
    let role = get(tx, key)
        .await?
        .ok_or(DbError::Corrupt("a role vanished while it was written"))?;
    Ok(Written::Done(Box::new(role)))
}

/// Creates a role. The key is checked by the caller (and by the database).
pub async fn create(
    tx: &mut MemberTransaction,
    key: &str,
    name: &str,
    description: &str,
    policy: &Policy,
) -> Result<Written, DbError> {
    lock(tx).await?;
    let count = sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM roles WHERE project_id = $1"#,
        tx.project
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    if usize::try_from(count).unwrap_or(usize::MAX) >= MAX_ROLES {
        return Ok(Written::TooMany);
    }
    let made = sqlx::query!(
        "INSERT INTO roles (project_id, key, name, description) VALUES ($1, $2, $3, $4)
         ON CONFLICT DO NOTHING",
        tx.project,
        key,
        name,
        description
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    if made.rows_affected() == 0 {
        return Ok(Written::Exists);
    }
    insert_permissions(tx, key, policy).await?;
    written(tx, key).await
}

/// Replaces a role's name, description and permissions, if it is still at version `expected`.
pub async fn update(
    tx: &mut MemberTransaction,
    key: &str,
    expected: i32,
    name: &str,
    description: &str,
    policy: &Policy,
) -> Result<Written, DbError> {
    if key == OWNER {
        return Ok(Written::Fixed);
    }
    lock(tx).await?;
    let moved = sqlx::query_scalar!(
        "UPDATE roles SET name = $3, description = $4, version = version + 1, updated_at = now()
         WHERE project_id = $1 AND key = $2 AND version = $5
         RETURNING version",
        tx.project,
        key,
        name,
        description,
        expected
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    if moved.is_none() {
        return Ok(match version(tx, key).await? {
            None => Written::NotFound,
            Some(current) => Written::Stale { current },
        });
    }
    sqlx::query!(
        "DELETE FROM role_permissions WHERE project_id = $1 AND role_key = $2",
        tx.project,
        key
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    insert_permissions(tx, key, policy).await?;
    written(tx, key).await
}

async fn version(tx: &mut MemberTransaction, key: &str) -> Result<Option<i32>, DbError> {
    sqlx::query_scalar!(
        "SELECT version FROM roles WHERE project_id = $1 AND key = $2",
        tx.project,
        key
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Deletes a role at version `expected` that nobody holds and no pending invitation offers.
pub async fn delete(
    tx: &mut MemberTransaction,
    key: &str,
    expected: i32,
) -> Result<Removed, DbError> {
    if key == OWNER {
        return Ok(Removed::Fixed);
    }
    lock(tx).await?;
    // Locking the row waits for memberships and invitations being made to name it (they hold a
    // key-share lock on it); the counts below, a separate statement, then see them.
    let current = sqlx::query_scalar!(
        "SELECT version FROM roles WHERE project_id = $1 AND key = $2 FOR UPDATE",
        tx.project,
        key
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    let Some(current) = current else {
        return Ok(Removed::NotFound);
    };
    if current != expected {
        return Ok(Removed::Stale { current });
    }
    let used = sqlx::query!(
        r#"SELECT (SELECT count(*) FROM memberships
                   WHERE project_id = $1 AND role = $2) AS "members!",
                  (SELECT count(*) FROM invitations
                   WHERE project_id = $1 AND role = $2
                     AND accepted_at IS NULL AND revoked_at IS NULL
                     AND expires_at > now()) AS "invitations!""#,
        tx.project,
        key
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    if used.members > 0 || used.invitations > 0 {
        return Ok(Removed::InUse {
            members: used.members,
            invitations: used.invitations,
        });
    }
    sqlx::query!(
        "DELETE FROM roles WHERE project_id = $1 AND key = $2",
        tx.project,
        key
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(Removed::Done)
}
