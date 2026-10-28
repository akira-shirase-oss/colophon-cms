//! Members and invitations (docs/modules/03-auth.md; D-042):
//!
//! - `GET    /api/v1/projects/{project}/members`: any member;
//! - `PUT    /api/v1/projects/{project}/members/{identity}`: `{"role"}`, owners only;
//! - `DELETE /api/v1/projects/{project}/members/{identity}`: owners, or a member leaving;
//! - `GET    /api/v1/projects/{project}/invitations`, `POST` (`{"role", "label"}`) and
//!   `DELETE …/invitations/{id}`: owners only;
//! - `POST   /api/v1/invitations/lookup` and `/accept`: `{"token"}`, anyone signed in.
//!
//! A project never loses its last owner (409). The invitation token is returned once, in a link
//! that carries it after `#`, which browsers never send to a server; lookups and acceptance take
//! it in the body, so it never appears in a request line or a log.

use std::time::Duration;

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
};
use colophon_auth::tokens;
use colophon_db::{
    members::{self, Accepted, Change, Invitation, Member},
    scope::{self, MemberTransaction},
};
use colophon_domain::policy::{Action, Policy, Role};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    entries::rfc3339,
    session::{SignedIn, services},
    shape::{Data, Page},
};

/// How long an invitation works.
const INVITATION_LIFETIME: Duration = Duration::from_hours(72);

/// Refuses unless `policy` allows `action`, in `collection` when the action is about one (D-058;
/// deny by default). `None` asks for the action everywhere.
pub(crate) fn require(
    policy: &Policy,
    action: Action,
    collection: Option<&str>,
    why: &'static str,
) -> Result<(), ApiError> {
    if policy.allows(action, collection) {
        Ok(())
    } else {
        Err(ApiError::Forbidden(why))
    }
}

async fn open(
    state: &AppState,
    caller: SignedIn,
    project: &str,
) -> Result<MemberTransaction, ApiError> {
    let services = services(state)?;
    let project = Uuid::try_parse(project).map_err(|_| ApiError::NotFound)?;
    scope::begin_as_member(&services.pool, caller.identity, project)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)
}

/// The caller's role as of now, for a change that must not act on a role read a moment ago (a
/// demotion committing at the same time). Role changes and removals get the same guarantee from
/// the owner lock inside `colophon_db::members`.
async fn fresh_policy(tx: &mut MemberTransaction) -> Result<Policy, ApiError> {
    Ok(members::current_policy(tx)
        .await
        .map_err(ApiError::from_db)?
        .unwrap_or_default())
}

#[derive(Serialize)]
pub(crate) struct MemberView {
    identity_id: Uuid,
    display_name: Option<String>,
    role: String,
    joined_at: String,
}

impl MemberView {
    fn new(member: Member) -> Result<Self, ApiError> {
        Ok(Self {
            identity_id: member.identity_id,
            display_name: member.display_name,
            role: member.role,
            joined_at: rfc3339(member.joined_at)?,
        })
    }
}

#[derive(Serialize)]
pub(crate) struct InvitationView {
    id: Uuid,
    role: String,
    label: String,
    created_at: String,
    expires_at: String,
    /// Only in the answer to the request that made it.
    #[serde(skip_serializing_if = "Option::is_none")]
    link: Option<String>,
}

impl InvitationView {
    fn new(invitation: Invitation, link: Option<String>) -> Result<Self, ApiError> {
        Ok(Self {
            id: invitation.id,
            role: invitation.role,
            label: invitation.label,
            created_at: rfc3339(invitation.created_at)?,
            expires_at: rfc3339(invitation.expires_at)?,
            link,
        })
    }
}

#[derive(Deserialize)]
pub(crate) struct ProjectPath {
    project: String,
}

#[derive(Deserialize)]
pub(crate) struct MemberPath {
    project: String,
    identity: String,
}

#[derive(Deserialize)]
pub(crate) struct InvitationPath {
    project: String,
    invitation: String,
}

/// `{"role": "editor"}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewRole {
    role: String,
}

/// `{"role": "editor", "label": "Sam, copy editor"}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewInvitation {
    role: String,
    label: String,
}

/// `{"token": "..."}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TokenBody {
    token: String,
}

fn parse_role(text: &str) -> Result<Role, ApiError> {
    Role::parse(text).ok_or_else(|| ApiError::InvalidField {
        reason: "invalid_role",
        pointer: "/role",
        message: "a role is owner, developer, editor or publisher".to_owned(),
    })
}

fn change_result(change: Change) -> Result<StatusCode, ApiError> {
    match change {
        Change::Done => Ok(StatusCode::NO_CONTENT),
        Change::NotFound => Err(ApiError::NotFound),
        Change::LastOwner => Err(ApiError::Conflict("a project must keep at least one owner")),
        Change::NotOwner => Err(ApiError::Forbidden(
            "you are no longer an owner of this project",
        )),
    }
}

/// `GET /api/v1/projects/{project}/members`.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ProjectPath>,
) -> Result<Json<Page<MemberView>>, ApiError> {
    let mut tx = open(&state, caller, &path.project).await?;
    // Any member may see who else is in the project (D-042): being in the transaction is the check.
    let rows = members::list(&mut tx).await.map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let data = rows
        .into_iter()
        .map(MemberView::new)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(Page {
        data,
        next_cursor: None,
    }))
}

/// `PUT /api/v1/projects/{project}/members/{identity}`.
pub(crate) async fn change(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<MemberPath>,
    body: Result<Json<NewRole>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(body) = body?;
    let role = parse_role(&body.role)?;
    let identity = Uuid::try_parse(&path.identity).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project).await?;
    require(
        &tx.policy,
        Action::MembersManage,
        None,
        "only a project's owners can change roles",
    )?;
    let outcome = members::change_role(&mut tx, identity, role.as_str())
        .await
        .map_err(ApiError::from_db)?;
    if outcome == Change::Done {
        tx.commit().await.map_err(ApiError::from_db)?;
    }
    change_result(outcome)
}

/// `DELETE /api/v1/projects/{project}/members/{identity}`: an owner removing someone, or a
/// member leaving.
pub(crate) async fn remove(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<MemberPath>,
) -> Result<StatusCode, ApiError> {
    let identity = Uuid::try_parse(&path.identity).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project).await?;
    if identity != caller.identity {
        require(
            &tx.policy,
            Action::MembersManage,
            None,
            "only a project's owners can remove members",
        )?;
    }
    let outcome = members::remove(&mut tx, identity)
        .await
        .map_err(ApiError::from_db)?;
    if outcome == Change::Done {
        tx.commit().await.map_err(ApiError::from_db)?;
    }
    change_result(outcome)
}

/// `GET /api/v1/projects/{project}/invitations`: the pending ones.
pub(crate) async fn invitations(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ProjectPath>,
) -> Result<Json<Page<InvitationView>>, ApiError> {
    let mut tx = open(&state, caller, &path.project).await?;
    require(
        &tx.policy,
        Action::MembersManage,
        None,
        "only a project's owners can see its invitations",
    )?;
    let rows = members::invitations(&mut tx)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let data = rows
        .into_iter()
        .map(|invitation| InvitationView::new(invitation, None))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(Page {
        data,
        next_cursor: None,
    }))
}

/// `POST /api/v1/projects/{project}/invitations`.
pub(crate) async fn invite(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ProjectPath>,
    body: Result<Json<NewInvitation>, JsonRejection>,
) -> Result<(StatusCode, Json<Data<InvitationView>>), ApiError> {
    let services = services(&state)?;
    let Json(body) = body?;
    let role = parse_role(&body.role)?;
    let label = body.label.trim();
    if !(1..=100).contains(&label.chars().count()) || label.chars().any(char::is_control) {
        return Err(ApiError::InvalidField {
            reason: "invalid_label",
            pointer: "/label",
            message: "a label is 1 to 100 characters, without control characters".to_owned(),
        });
    }
    let mut tx = open(&state, caller, &path.project).await?;
    require(
        &fresh_policy(&mut tx).await?,
        Action::MembersManage,
        None,
        "only a project's owners can invite people",
    )?;
    let token = tokens::random_token()
        .map_err(|_| ApiError::internal(std::io::Error::other("no secure random source")))?;
    let expires_at = OffsetDateTime::now_utc()
        + time::Duration::try_from(INVITATION_LIFETIME).unwrap_or(time::Duration::DAY);
    let invitation = members::invite(
        &mut tx,
        &tokens::hash(&token),
        role.as_str(),
        label,
        expires_at,
    )
    .await
    .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let origin = services
        .trusted_origins
        .first()
        .cloned()
        .unwrap_or_default();
    let link = format!("{origin}/studio/invite#{token}");
    Ok((
        StatusCode::CREATED,
        Json(Data {
            data: InvitationView::new(invitation, Some(link))?,
        }),
    ))
}

/// `DELETE /api/v1/projects/{project}/invitations/{invitation}`.
pub(crate) async fn revoke(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<InvitationPath>,
) -> Result<StatusCode, ApiError> {
    let id = Uuid::try_parse(&path.invitation).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project).await?;
    require(
        &fresh_policy(&mut tx).await?,
        Action::MembersManage,
        None,
        "only a project's owners can revoke invitations",
    )?;
    if members::revoke(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
    {
        tx.commit().await.map_err(ApiError::from_db)?;
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

#[derive(Serialize)]
pub(crate) struct OfferView {
    project_id: Uuid,
    project_name: String,
    role: String,
    invited_by: Option<String>,
    expires_at: String,
}

fn token_hash(token: &str) -> Result<[u8; 32], ApiError> {
    if token.len() == 43 {
        Ok(tokens::hash(token))
    } else {
        Err(ApiError::NotFound)
    }
}

/// `POST /api/v1/invitations/lookup`: what an invitation offers, for the person holding it.
pub(crate) async fn lookup(
    State(state): State<AppState>,
    caller: SignedIn,
    body: Result<Json<TokenBody>, JsonRejection>,
) -> Result<Json<Data<OfferView>>, ApiError> {
    let services = services(&state)?;
    let Json(body) = body?;
    let offer = members::offer(&services.pool, caller.identity, &token_hash(&body.token)?)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(Data {
        data: OfferView {
            project_id: offer.project_id,
            project_name: offer.project_name,
            role: offer.role,
            invited_by: offer.invited_by,
            expires_at: rfc3339(offer.expires_at)?,
        },
    }))
}

#[derive(Serialize)]
pub(crate) struct JoinedView {
    project_id: Uuid,
    role: String,
}

/// `POST /api/v1/invitations/accept`.
pub(crate) async fn accept(
    State(state): State<AppState>,
    caller: SignedIn,
    body: Result<Json<TokenBody>, JsonRejection>,
) -> Result<Json<Data<JoinedView>>, ApiError> {
    let services = services(&state)?;
    let Json(body) = body?;
    match members::accept(&services.pool, caller.identity, &token_hash(&body.token)?)
        .await
        .map_err(ApiError::from_db)?
    {
        Accepted::Joined { project_id, role } => {
            tracing::info!(identity = %caller.identity, %project_id, "invitation accepted");
            Ok(Json(Data {
                data: JoinedView { project_id, role },
            }))
        }
        Accepted::AlreadyMember => Err(ApiError::Conflict(
            "you are already a member of this project",
        )),
        Accepted::Gone => Err(ApiError::NotFound),
    }
}
