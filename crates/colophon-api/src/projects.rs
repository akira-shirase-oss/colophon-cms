//! `/api/v1/projects`: the projects the caller belongs to (docs/modules/04-api.md). A project the
//! caller isn't a member of answers exactly like one that doesn't exist.

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
};
use colophon_db::{DbError, projects};
use colophon_domain::project::{ProjectName, Slug};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    session::{SignedIn, services},
};

/// A project as the API shows it.
#[derive(Debug, Serialize)]
pub(crate) struct ProjectView {
    id: Uuid,
    slug: String,
    name: String,
    role: String,
}

impl From<projects::Project> for ProjectView {
    fn from(project: projects::Project) -> Self {
        Self {
            id: project.id,
            slug: project.slug,
            name: project.name,
            role: project.role,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct ProjectList {
    items: Vec<ProjectView>,
}

/// `GET /api/v1/projects`.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: SignedIn,
) -> Result<Json<ProjectList>, ApiError> {
    let services = services(&state)?;
    let items = projects::list(&services.pool, caller.identity)
        .await
        .map_err(ApiError::internal)?
        .into_iter()
        .map(ProjectView::from)
        .collect();
    Ok(Json(ProjectList { items }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewProject {
    slug: String,
    name: String,
}

/// `POST /api/v1/projects`: creates a project; the caller becomes its owner.
pub(crate) async fn create(
    State(state): State<AppState>,
    caller: SignedIn,
    body: Result<Json<NewProject>, JsonRejection>,
) -> Result<(StatusCode, Json<ProjectView>), ApiError> {
    let services = services(&state)?;
    let Json(body) = body?;
    let invalid = |error: colophon_domain::project::ProjectFieldError, pointer: &'static str| {
        ApiError::InvalidField {
            reason: error.code(),
            pointer,
            message: error.to_string(),
        }
    };
    let slug = Slug::parse(&body.slug).map_err(|error| invalid(error, "/slug"))?;
    let name = ProjectName::parse(&body.name).map_err(|error| invalid(error, "/name"))?;
    let project = projects::create(
        &services.pool,
        caller.identity,
        slug.as_str(),
        name.as_str(),
    )
    .await
    .map_err(|error| match error {
        DbError::Conflict => ApiError::Conflict("a project with this slug already exists"),
        other => ApiError::internal(other),
    })?;
    Ok((StatusCode::CREATED, Json(ProjectView::from(project))))
}

/// `GET /api/v1/projects/{project}`.
pub(crate) async fn get(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(id): Path<String>,
) -> Result<Json<ProjectView>, ApiError> {
    let services = services(&state)?;
    // An id that isn't a UUID can't name a project: not found, same as any other.
    let id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;
    let project = projects::get(&services.pool, caller.identity, id)
        .await
        .map_err(ApiError::internal)?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(ProjectView::from(project)))
}
