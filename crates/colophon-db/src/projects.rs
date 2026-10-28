//! Projects and memberships, through row-level security: a project exists, for the application,
//! only inside its own context or for its members.

use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::{
    DbError,
    scope::{self, MemberTransaction, Scope},
};

/// A project as one of its members sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Its id.
    pub id: Uuid,
    /// Its unique, URL-safe name.
    pub slug: String,
    /// Its display name.
    pub name: String,
    /// The asking identity's role in it, by key.
    pub role: String,
    /// That role's name, for people (D-058: it may be one the project's owners defined).
    pub role_name: String,
}

/// Creates a project with a `production` environment holding the built-in articles content type,
/// and makes `owner` its owner, in one transaction. A taken slug is `DbError::Conflict`.
pub async fn create(
    pool: &PgPool,
    owner: Uuid,
    slug: &str,
    name: &str,
) -> Result<Project, DbError> {
    let id = Uuid::now_v7();
    let mut transaction = scope::begin(
        pool,
        Scope {
            identity: Some(owner),
            project: Some(id),
        },
    )
    .await?;
    sqlx::query!(
        "INSERT INTO projects (id, slug, name) VALUES ($1, $2, $3)",
        id,
        slug,
        name
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_write)?;
    let environment = Uuid::now_v7();
    sqlx::query!(
        "INSERT INTO environments (project_id, id, slug) VALUES ($1, $2, 'production')",
        id,
        environment
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    sqlx::query!(
        "INSERT INTO collections (project_id, environment_id, slug) VALUES ($1, $2, 'articles')",
        id,
        environment
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    sqlx::query!(
        "INSERT INTO content_type_versions (project_id, environment_id, collection, version, definition, definition_hash)
         VALUES ($1, $2, 'articles', 1, $3, sha256(convert_to($3, 'UTF8')))",
        id,
        environment,
        crate::content_types::ARTICLES_DEFINITION
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    // The built-in roles first: a membership names one of the project's roles (migration 0017).
    sqlx::query("SELECT 1 FROM colophon_seed_roles($1)")
        .bind(id)
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    sqlx::query!(
        "INSERT INTO memberships (project_id, identity_id, role) VALUES ($1, $2, 'owner')",
        id,
        owner
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(Project {
        id,
        slug: slug.to_owned(),
        name: name.to_owned(),
        role: "owner".to_owned(),
        role_name: "Owner".to_owned(),
    })
}

/// The projects `identity` is a member of, by name.
pub async fn list(pool: &PgPool, identity: Uuid) -> Result<Vec<Project>, DbError> {
    let mut transaction = scope::begin(
        pool,
        Scope {
            identity: Some(identity),
            project: None,
        },
    )
    .await?;
    let projects = list_in(&mut transaction, identity).await?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(projects)
}

/// [`list`], inside a transaction already scoped to `identity`.
pub(crate) async fn list_in(
    connection: &mut PgConnection,
    identity: Uuid,
) -> Result<Vec<Project>, DbError> {
    sqlx::query_as!(
        Project,
        "SELECT p.id, p.slug, p.name, m.role, r.name AS role_name FROM projects p
         JOIN memberships m ON m.project_id = p.id AND m.identity_id = $1
         JOIN roles r ON r.project_id = m.project_id AND r.key = m.role
         ORDER BY p.name, p.id
         LIMIT 1000",
        identity
    )
    .fetch_all(connection)
    .await
    .map_err(DbError::from_query)
}

/// One project, if `identity` is a member. Not a member and doesn't exist look the same.
pub async fn get(pool: &PgPool, identity: Uuid, id: Uuid) -> Result<Option<Project>, DbError> {
    let mut transaction = scope::begin(
        pool,
        Scope {
            identity: Some(identity),
            project: None,
        },
    )
    .await?;
    let project = sqlx::query_as!(
        Project,
        "SELECT p.id, p.slug, p.name, m.role, r.name AS role_name FROM projects p
         JOIN memberships m ON m.project_id = p.id AND m.identity_id = $1
         JOIN roles r ON r.project_id = m.project_id AND r.key = m.role
         WHERE p.id = $2",
        identity,
        id
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(project)
}

/// The project of a member's transaction, as that member sees it.
pub async fn describe(tx: &mut MemberTransaction) -> Result<Project, DbError> {
    sqlx::query_as!(
        Project,
        "SELECT p.id, p.slug, p.name, m.role, r.name AS role_name FROM projects p
         JOIN memberships m ON m.project_id = p.id AND m.identity_id = $1
         JOIN roles r ON r.project_id = m.project_id AND r.key = m.role
         WHERE p.id = $2",
        tx.identity,
        tx.project
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}
