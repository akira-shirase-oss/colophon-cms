//! Projects and memberships, through row-level security: a project exists, for the application,
//! only inside its own context or for its members.

use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::{
    DbError,
    scope::{self, Scope},
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
    /// The asking identity's role in it.
    pub role: String,
}

/// Creates a project with a `production` environment, and makes `owner` its owner, in one
/// transaction. A taken slug is `DbError::Conflict`.
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
    sqlx::query!(
        "INSERT INTO environments (project_id, id, slug) VALUES ($1, $2, 'production')",
        id,
        Uuid::now_v7()
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::Query)?;
    sqlx::query!(
        "INSERT INTO memberships (project_id, identity_id, role) VALUES ($1, $2, 'owner')",
        id,
        owner
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::Query)?;
    transaction.commit().await.map_err(DbError::Query)?;
    Ok(Project {
        id,
        slug: slug.to_owned(),
        name: name.to_owned(),
        role: "owner".to_owned(),
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
    transaction.commit().await.map_err(DbError::Query)?;
    Ok(projects)
}

/// [`list`], inside a transaction already scoped to `identity`.
pub(crate) async fn list_in(
    connection: &mut PgConnection,
    identity: Uuid,
) -> Result<Vec<Project>, DbError> {
    sqlx::query_as!(
        Project,
        "SELECT p.id, p.slug, p.name, m.role FROM projects p
         JOIN memberships m ON m.project_id = p.id AND m.identity_id = $1
         ORDER BY p.name, p.id
         LIMIT 1000",
        identity
    )
    .fetch_all(connection)
    .await
    .map_err(DbError::Query)
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
        "SELECT p.id, p.slug, p.name, m.role FROM projects p
         JOIN memberships m ON m.project_id = p.id AND m.identity_id = $1
         WHERE p.id = $2",
        identity,
        id
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::Query)?;
    transaction.commit().await.map_err(DbError::Query)?;
    Ok(project)
}
