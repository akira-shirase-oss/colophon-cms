//! `/api/v1/projects`: the projects the caller belongs to (docs/modules/04-api.md). A project the
//! caller isn't a member of answers exactly like one that doesn't exist.

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
};
use colophon_db::{
    DbError,
    administrators::{self, Standing},
    projects, scope,
};
use colophon_domain::project::{ProjectName, Slug};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    roles::{PermissionView, permission_views},
    session::{SignedIn, services},
    shape::{Data, Page},
};

/// A project as the API shows it. Reading one project also says what the caller's role allows
/// in it, so a client can show only what will work (D-058); the server still checks every request.
#[derive(Serialize)]
pub(crate) struct ProjectView {
    id: Uuid,
    slug: String,
    name: String,
    role: String,
    role_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    permissions: Option<Vec<PermissionView>>,
    /// Whether someone who submits a publish request may approve it themselves (D-044): then one
    /// click publishes it (D-059). Only when one project is read.
    #[serde(skip_serializing_if = "Option::is_none")]
    self_review: Option<bool>,
}

impl From<projects::Project> for ProjectView {
    fn from(project: projects::Project) -> Self {
        Self {
            id: project.id,
            slug: project.slug,
            name: project.name,
            role: project.role,
            role_name: project.role_name,
            permissions: None,
            self_review: None,
        }
    }
}

/// `GET /api/v1/projects`: every project the caller belongs to (at most 1,000, on one page).
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: SignedIn,
) -> Result<Json<Page<ProjectView>>, ApiError> {
    let services = services(&state)?;
    let data = projects::list(&services.pool, caller.identity)
        .await
        .map_err(ApiError::from_db)?
        .into_iter()
        .map(ProjectView::from)
        .collect();
    Ok(Json(Page {
        data,
        next_cursor: None,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewProject {
    slug: String,
    name: String,
}

/// `POST /api/v1/projects`: creates a project; the caller, who must be an administrator (D-043),
/// becomes its owner. The database refuses anyone else too.
pub(crate) async fn create(
    State(state): State<AppState>,
    caller: SignedIn,
    body: Result<Json<NewProject>, JsonRejection>,
) -> Result<(StatusCode, Json<Data<ProjectView>>), ApiError> {
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
    match administrators::standing(&services.pool, caller.identity)
        .await
        .map_err(ApiError::from_db)?
    {
        Standing::Administrator => {}
        Standing::Member => {
            return Err(ApiError::forbidden(
                "administrators_only",
                "only an administrator can create projects; ask one to invite you to a project",
            ));
        }
        Standing::NotBootstrapped => {
            return Err(ApiError::forbidden(
                "not_set_up",
                "this installation has no administrator yet: enter the setup code its server printed when it started, or its operator runs `colophon admin bootstrap`",
            ));
        }
    }
    let project = projects::create(
        &services.pool,
        caller.identity,
        slug.as_str(),
        name.as_str(),
    )
    .await
    .map_err(|error| match error {
        DbError::Conflict => {
            ApiError::conflict("project_exists", "a project with this slug already exists")
        }
        other => ApiError::from_db(other),
    })?;
    Ok((
        StatusCode::CREATED,
        Json(Data {
            data: ProjectView::from(project),
        }),
    ))
}

/// `GET /api/v1/projects/{project}`.
pub(crate) async fn get(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(id): Path<String>,
) -> Result<Json<Data<ProjectView>>, ApiError> {
    let services = services(&state)?;
    // An id that isn't a UUID can't name a project: not found, same as any other.
    let id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;
    let mut tx = scope::begin_as_member(&services.pool, caller.identity, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let project = projects::describe(&mut tx)
        .await
        .map_err(ApiError::from_db)?;
    let permissions = permission_views(&tx.policy);
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(Json(Data {
        data: ProjectView {
            permissions: Some(permissions),
            self_review: Some(!colophon_db::publish::REQUIRE_DIFFERENT_REVIEWER),
            ..ProjectView::from(project)
        },
    }))
}
