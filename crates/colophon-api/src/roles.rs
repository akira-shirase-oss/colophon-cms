//! A project's roles (docs/modules/03-auth.md, "Roles and memberships"; D-058, step R.2):
//!
//! - `GET    /api/v1/projects/{project}/roles`: any member;
//! - `GET    /api/v1/projects/{project}/roles/{role}`: any member, with `ETag: "v<version>"`;
//! - `POST   /api/v1/projects/{project}/roles`: `{"key", "name", "description", "permissions"}`;
//! - `PUT    /api/v1/projects/{project}/roles/{role}`: `{"name", "description", "permissions"}`,
//!   with `If-Match`;
//! - `DELETE /api/v1/projects/{project}/roles/{role}`: with `If-Match`.
//!
//! Writes need `roles.manage`, checked against the caller's role as it is now, and a caller can
//! only create, change or delete a role whose every power they hold themselves. The owner role
//! is fixed (409). A permission is `{"action": "content.update", "subject": "pages"}`, narrowed
//! by `"fields": ["title"]` and `"condition": "own"` where they apply (step R.3).

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use colophon_db::roles::{self, Removed, StoredRole, Written};
use colophon_domain::policy::{
    Action, Condition, MAX_PERMISSIONS, Permission, Policy, RoleError, is_role_key,
    role_description, role_name,
};
use serde::{Deserialize, Serialize};

use crate::{
    ApiError, AppState,
    entries::rfc3339,
    members::{ProjectPath, fresh_policy, open, require},
    shape::{Data, Page},
};

#[derive(Serialize)]
pub(crate) struct PermissionView {
    action: &'static str,
    subject: String,
    /// Only these fields (step R.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    fields: Option<Vec<String>>,
    /// `own`: only entries the member created (step R.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    condition: Option<&'static str>,
}

/// A policy's permissions as the API shows them: `{"action", "subject"}`, with `fields` and
/// `condition` where they narrow it, sorted.
pub(crate) fn permission_views(policy: &Policy) -> Vec<PermissionView> {
    policy
        .permissions()
        .iter()
        .map(|permission| PermissionView {
            action: permission.action.as_str(),
            subject: permission.subject.as_str().to_owned(),
            fields: permission.fields.clone(),
            condition: permission.condition.map(Condition::as_str),
        })
        .collect()
}

#[derive(Serialize)]
pub(crate) struct RoleView {
    key: String,
    name: String,
    description: String,
    built_in: bool,
    /// The owner role: always every permission, never changed or deleted.
    fixed: bool,
    version: i32,
    permissions: Vec<PermissionView>,
    members: i64,
    invitations: i64,
    updated_at: String,
}

impl RoleView {
    fn new(role: StoredRole) -> Result<Self, ApiError> {
        Ok(Self {
            fixed: role.key == roles::OWNER,
            permissions: permission_views(&role.policy),
            key: role.key,
            name: role.name,
            description: role.description,
            built_in: role.built_in,
            version: role.version,
            members: role.members,
            invitations: role.invitations,
            updated_at: rfc3339(role.updated_at)?,
        })
    }
}

#[derive(Deserialize)]
pub(crate) struct RolePath {
    project: String,
    role: String,
}

/// `{"action": "content.update", "subject": "pages", "fields": ["title"], "condition": "own"}`;
/// `fields` and `condition` are optional (step R.3).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PermissionBody {
    action: String,
    subject: String,
    #[serde(default)]
    fields: Option<Vec<String>>,
    #[serde(default)]
    condition: Option<String>,
}

/// `POST` body.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewRole {
    key: String,
    name: String,
    #[serde(default)]
    description: String,
    permissions: Vec<PermissionBody>,
}

/// `PUT` body: everything but the key, which never changes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RoleChange {
    name: String,
    #[serde(default)]
    description: String,
    permissions: Vec<PermissionBody>,
}

pub(crate) fn invalid(error: RoleError, pointer: &'static str) -> ApiError {
    ApiError::InvalidField {
        reason: error.code(),
        pointer,
        message: error.to_string(),
    }
}

/// The permissions a body asks for, or the first one refused, by its position.
pub(crate) fn policy(permissions: &[PermissionBody]) -> Result<Policy, ApiError> {
    if permissions.len() > MAX_PERMISSIONS {
        return Err(invalid(RoleError::TooManyPermissions, "/permissions"));
    }
    permissions
        .iter()
        .enumerate()
        .map(|(index, body)| {
            let parsed = Permission::parse_narrowed(
                &body.action,
                &body.subject,
                body.fields.as_deref(),
                body.condition.as_deref(),
            );
            parsed.map_err(|error| ApiError::InvalidField {
                reason: error.code(),
                pointer: "/permissions",
                message: format!("permission {}: {error}", index + 1),
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Policy::new)
}

fn etag(version: i32) -> HeaderValue {
    HeaderValue::from_str(&format!("\"v{version}\"")).unwrap_or(HeaderValue::from_static("\"\""))
}

/// The version an `If-Match` names: exactly `"v<version>"`.
fn if_match(headers: &HeaderMap) -> Result<i32, ApiError> {
    let value = headers
        .get(header::IF_MATCH)
        .ok_or(ApiError::PreconditionRequired(
            "changing or deleting a role needs an If-Match header with the ETag of the version you edited",
        ))?;
    value
        .to_str()
        .ok()
        .and_then(|text| text.strip_prefix("\"v")?.strip_suffix('"')?.parse().ok())
        .filter(|version| *version >= 1)
        .ok_or(ApiError::InvalidHeader(
            "If-Match must be one role ETag, such as \"v3\"",
        ))
}

fn respond(status: StatusCode, role: StoredRole) -> Result<Response, ApiError> {
    let tag = etag(role.version);
    Ok((
        status,
        [(header::ETAG, tag)],
        Json(Data {
            data: RoleView::new(role)?,
        }),
    )
        .into_response())
}

/// Refuses unless the caller may manage roles now, and holds every power of `granted`.
async fn may_grant(
    tx: &mut colophon_db::scope::MemberTransaction,
    granted: &[&Policy],
) -> Result<(), ApiError> {
    let fresh = fresh_policy(tx).await?;
    require(
        &fresh,
        Action::RolesManage,
        None,
        "only people whose role manages roles can change them",
    )?;
    if granted.iter().all(|policy| fresh.covers(policy)) {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "cannot_grant",
            "a role can only be given powers your own role has",
        ))
    }
}

/// `GET {P}/roles`.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: crate::session::SignedIn,
    Path(path): Path<ProjectPath>,
) -> Result<Json<Page<RoleView>>, ApiError> {
    let mut tx = open(&state, caller, &path.project).await?;
    // Any member may see what each role allows: being in the transaction is the check.
    let rows = roles::list(&mut tx).await.map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let data = rows
        .into_iter()
        .map(RoleView::new)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(Page {
        data,
        next_cursor: None,
    }))
}

/// `GET {P}/roles/{role}`.
pub(crate) async fn get(
    State(state): State<AppState>,
    caller: crate::session::SignedIn,
    Path(path): Path<RolePath>,
) -> Result<Response, ApiError> {
    let mut tx = open(&state, caller, &path.project).await?;
    let role = roles::get(&mut tx, &path.role)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    respond(StatusCode::OK, role)
}

/// `POST {P}/roles`.
pub(crate) async fn create(
    State(state): State<AppState>,
    caller: crate::session::SignedIn,
    Path(path): Path<ProjectPath>,
    body: Result<Json<NewRole>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(body) = body?;
    if !is_role_key(&body.key) {
        return Err(invalid(RoleError::InvalidKey, "/key"));
    }
    let name = role_name(&body.name).map_err(|error| invalid(error, "/name"))?;
    let description =
        role_description(&body.description).map_err(|error| invalid(error, "/description"))?;
    let granted = policy(&body.permissions)?;
    let mut tx = open(&state, caller, &path.project).await?;
    may_grant(&mut tx, &[&granted]).await?;
    match roles::create(&mut tx, &body.key, name, description, &granted)
        .await
        .map_err(ApiError::from_db)?
    {
        Written::Done(role) => {
            crate::audit::record(&mut tx, "role.created", &role.key, &described(&role)).await?;
            tx.commit().await.map_err(ApiError::from_db)?;
            respond(StatusCode::CREATED, *role)
        }
        Written::Exists => Err(ApiError::conflict(
            "role_exists",
            "this project already has a role with that key",
        )),
        Written::TooMany => Err(ApiError::InvalidField {
            reason: "too_many_roles",
            pointer: "/key",
            message: format!(
                "a project has at most {} roles",
                colophon_domain::policy::MAX_ROLES
            ),
        }),
        Written::Fixed | Written::NotFound | Written::Stale { .. } => Err(ApiError::internal(
            std::io::Error::other("creating a role answered as a change"),
        )),
    }
}

const FIXED: &str = "the owner role is fixed: it always holds every permission";

/// What the audit log keeps about a role (D-063).
fn described(role: &StoredRole) -> serde_json::Value {
    serde_json::json!({
        "name": role.name,
        "description": role.description,
        "permissions": permission_views(&role.policy),
    })
}

/// `PUT {P}/roles/{role}`.
pub(crate) async fn update(
    State(state): State<AppState>,
    caller: crate::session::SignedIn,
    Path(path): Path<RolePath>,
    headers: HeaderMap,
    body: Result<Json<RoleChange>, JsonRejection>,
) -> Result<Response, ApiError> {
    let expected = if_match(&headers)?;
    let Json(body) = body?;
    let name = role_name(&body.name).map_err(|error| invalid(error, "/name"))?;
    let description =
        role_description(&body.description).map_err(|error| invalid(error, "/description"))?;
    let granted = policy(&body.permissions)?;
    let mut tx = open(&state, caller, &path.project).await?;
    if path.role == roles::OWNER {
        return Err(ApiError::conflict("owner_role_fixed", FIXED));
    }
    let current = roles::get(&mut tx, &path.role)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    may_grant(&mut tx, &[&current.policy, &granted]).await?;
    match roles::update(&mut tx, &path.role, expected, name, description, &granted)
        .await
        .map_err(ApiError::from_db)?
    {
        Written::Done(role) => {
            let change =
                serde_json::json!({"before": described(&current), "after": described(&role)});
            crate::audit::record(&mut tx, "role.updated", &role.key, &change).await?;
            tx.commit().await.map_err(ApiError::from_db)?;
            respond(StatusCode::OK, *role)
        }
        Written::Stale { current } => Err(ApiError::StaleRole { current }),
        Written::NotFound => Err(ApiError::NotFound),
        Written::Fixed => Err(ApiError::conflict("owner_role_fixed", FIXED)),
        Written::Exists | Written::TooMany => Err(ApiError::internal(std::io::Error::other(
            "changing a role answered as a creation",
        ))),
    }
}

/// `DELETE {P}/roles/{role}`.
pub(crate) async fn delete(
    State(state): State<AppState>,
    caller: crate::session::SignedIn,
    Path(path): Path<RolePath>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let expected = if_match(&headers)?;
    let mut tx = open(&state, caller, &path.project).await?;
    if path.role == roles::OWNER {
        return Err(ApiError::conflict("owner_role_fixed", FIXED));
    }
    let current = roles::get(&mut tx, &path.role)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    may_grant(&mut tx, &[&current.policy]).await?;
    match roles::delete(&mut tx, &path.role, expected)
        .await
        .map_err(ApiError::from_db)?
    {
        Removed::Done => {
            crate::audit::record(&mut tx, "role.deleted", &current.key, &described(&current))
                .await?;
            tx.commit().await.map_err(ApiError::from_db)?;
            Ok(StatusCode::NO_CONTENT)
        }
        Removed::InUse {
            members,
            invitations,
        } => Err(ApiError::RoleInUse {
            members,
            invitations,
        }),
        Removed::Stale { current } => Err(ApiError::StaleRole { current }),
        Removed::NotFound => Err(ApiError::NotFound),
        Removed::Fixed => Err(ApiError::conflict("owner_role_fixed", FIXED)),
    }
}

/// What `GET {P}/permissions` answers: the caller's own permissions here.
#[derive(Serialize)]
pub(crate) struct OwnView {
    /// `person` or `token`.
    kind: &'static str,
    /// Its role's key, for a person; `null` for an API token.
    role: Option<String>,
    permissions: Vec<PermissionView>,
}

/// `GET /api/v1/projects/{project}/environments/{environment}/permissions`: what the caller, a
/// person or an API token, may do here, as roles show permissions (D-058, D-068). An agent's MCP
/// server lists only the tools these allow; every call is still decided by the API.
pub(crate) async fn own(
    State(state): State<AppState>,
    caller: crate::session::SignedIn,
    Path(path): Path<crate::content_types::TypesPath>,
) -> Result<Json<Data<OwnView>>, ApiError> {
    let token = caller.project.is_some();
    let tx = crate::entries::open(&state, caller, &path.project, &path.environment).await?;
    let view = OwnView {
        kind: if token { "token" } else { "person" },
        role: (!token).then(|| tx.role.clone()),
        permissions: permission_views(&tx.policy),
    };
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(Json(Data { data: view }))
}
