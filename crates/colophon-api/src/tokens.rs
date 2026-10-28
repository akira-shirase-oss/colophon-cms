//! A project's API tokens (docs/modules/04-api.md; D-058, D-063, step R.4).
//!
//! `POST /api/v1/projects/{project}/tokens` makes one: read-only, full access, or a custom list
//! of permissions, expiring after 1 to 366 days. The answer is the only place its secret
//! appears; only its hash is stored. `GET` lists them and `DELETE …/tokens/{token}` revokes one.
//! All three need `tokens.manage` (owners, by default), checked against the caller's role as it is
//! now, and nobody makes a token with a power they don't hold. Every creation and revocation goes
//! in the audit log, in the same transaction.

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
};
use colophon_auth::tokens as secrets;
use colophon_db::tokens::{self, Created, NewToken, Revoked, StoredToken};
use colophon_domain::policy::{
    Action, MAX_TOKEN_DAYS, Policy, RoleError, TokenKind, role_description, role_name,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    ApiError, AppState, audit,
    entries::rfc3339,
    members::{ProjectPath, fresh_policy, open, require},
    roles::{PermissionBody, PermissionView, invalid, permission_views, policy},
    session::{API_TOKEN_PREFIX, SignedIn},
    shape::{Data, Page},
};

/// `POST` body.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewTokenBody {
    name: String,
    #[serde(default)]
    description: String,
    /// `read-only`, `full-access` or `custom`.
    kind: String,
    /// Only for `custom`.
    #[serde(default)]
    permissions: Option<Vec<PermissionBody>>,
    /// 1 to 366.
    expires_in_days: u16,
}

#[derive(Deserialize)]
pub(crate) struct TokenPath {
    project: String,
    token: String,
}

#[derive(Serialize)]
struct PersonView {
    id: Uuid,
    name: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct TokenView {
    id: Uuid,
    /// The identity it acts as: what it writes names this id.
    identity_id: Uuid,
    name: String,
    description: String,
    kind: &'static str,
    permissions: Vec<PermissionView>,
    /// `active`, `expired` or `revoked`.
    state: &'static str,
    created_by: PersonView,
    created_at: String,
    expires_at: String,
    last_used_at: Option<String>,
    revoked_at: Option<String>,
    /// Only in the answer that creates it: `colophon_api_…`.
    #[serde(skip_serializing_if = "Option::is_none")]
    secret: Option<String>,
}

impl TokenView {
    fn new(token: StoredToken, secret: Option<String>) -> Result<Self, ApiError> {
        let state = if token.revoked_at.is_some() {
            "revoked"
        } else if token.expires_at <= OffsetDateTime::now_utc() {
            "expired"
        } else {
            "active"
        };
        Ok(Self {
            id: token.id,
            identity_id: token.identity_id,
            name: token.name,
            description: token.description,
            kind: token.kind.as_str(),
            permissions: permission_views(&token.policy),
            state,
            created_by: PersonView {
                id: token.created_by,
                name: token.created_by_name,
            },
            created_at: rfc3339(token.created_at)?,
            expires_at: rfc3339(token.expires_at)?,
            last_used_at: token.last_used_at.map(rfc3339).transpose()?,
            revoked_at: token.revoked_at.map(rfc3339).transpose()?,
            secret,
        })
    }
}

/// What the audit log keeps about a token: never its secret.
fn recorded(token: &StoredToken) -> serde_json::Value {
    json!({
        "name": token.name,
        "kind": token.kind.as_str(),
        "permissions": permission_views(&token.policy),
        "expires_at": rfc3339(token.expires_at).unwrap_or_default(),
    })
}

/// Refuses unless the caller's role, as it is now, manages tokens.
async fn may_manage(tx: &mut colophon_db::scope::MemberTransaction) -> Result<Policy, ApiError> {
    let fresh = fresh_policy(tx).await?;
    require(
        &fresh,
        Action::TokensManage,
        None,
        "only people whose role manages API tokens can see or change them",
    )?;
    Ok(fresh)
}

/// `GET /api/v1/projects/{project}/tokens`.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ProjectPath>,
) -> Result<Json<Page<TokenView>>, ApiError> {
    let mut tx = open(&state, caller, &path.project).await?;
    may_manage(&mut tx).await?;
    let stored = tokens::list(&mut tx).await.map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let data = stored
        .into_iter()
        .map(|token| TokenView::new(token, None))
        .collect::<Result<_, _>>()?;
    Ok(Json(Page {
        data,
        next_cursor: None,
    }))
}

/// A creation request, checked: what the token will be.
struct Requested<'a> {
    name: &'a str,
    description: &'a str,
    kind: TokenKind,
    granted: Policy,
    days: u16,
}

/// Checks a `POST` body without touching the database.
fn requested(body: &NewTokenBody) -> Result<Requested<'_>, ApiError> {
    let name = role_name(&body.name).map_err(|_| ApiError::InvalidField {
        reason: "invalid_token_name",
        pointer: "/name",
        message: "a token's name is 1 to 100 characters, without control characters".to_owned(),
    })?;
    let description = role_description(&body.description).map_err(|_| ApiError::InvalidField {
        reason: "invalid_token_description",
        pointer: "/description",
        message: "a token's description is at most 500 characters, without control characters"
            .to_owned(),
    })?;
    let kind = TokenKind::parse(&body.kind).ok_or_else(|| ApiError::InvalidField {
        reason: "unknown_token_kind",
        pointer: "/kind",
        message: "a token's kind is read-only, full-access or custom".to_owned(),
    })?;
    if !(1..=MAX_TOKEN_DAYS).contains(&body.expires_in_days) {
        return Err(ApiError::InvalidField {
            reason: "invalid_expiry",
            pointer: "/expires_in_days",
            message: format!("a token expires after 1 to {MAX_TOKEN_DAYS} days"),
        });
    }
    let custom = match (kind, &body.permissions) {
        (TokenKind::Custom, Some(list)) if !list.is_empty() => policy(list)?.permissions().to_vec(),
        (TokenKind::Custom, _) => {
            return Err(ApiError::InvalidField {
                reason: "permissions_required",
                pointer: "/permissions",
                message: "a custom token lists the permissions it holds".to_owned(),
            });
        }
        (_, None) => Vec::new(),
        (_, Some(_)) => {
            return Err(ApiError::InvalidField {
                reason: "permissions_not_applicable",
                pointer: "/permissions",
                message: "only a custom token lists permissions; read-only and full-access tokens have fixed ones"
                    .to_owned(),
            });
        }
    };
    let granted = kind
        .policy(custom)
        .map_err(|error: RoleError| invalid(error, "/permissions"))?;
    Ok(Requested {
        name,
        description,
        kind,
        granted,
        days: body.expires_in_days,
    })
}

/// `POST /api/v1/projects/{project}/tokens`.
pub(crate) async fn create(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ProjectPath>,
    body: Result<Json<NewTokenBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Data<TokenView>>), ApiError> {
    let Json(body) = body?;
    let Requested {
        name,
        description,
        kind,
        granted,
        days,
    } = requested(&body)?;
    let mut tx = open(&state, caller, &path.project).await?;
    let fresh = may_manage(&mut tx).await?;
    if !fresh.covers(&granted) {
        return Err(ApiError::forbidden(
            "cannot_grant",
            "a token can only be given powers your own role has",
        ));
    }
    let random = secrets::random_token()
        .map_err(|_| ApiError::internal(std::io::Error::other("no secure random source")))?;
    let secret = format!("{API_TOKEN_PREFIX}{random}");
    let expires_at = OffsetDateTime::now_utc() + time::Duration::days(i64::from(days));
    let token = NewToken {
        id: Uuid::now_v7(),
        identity_id: Uuid::now_v7(),
        name,
        description,
        kind,
        policy: &granted,
        hash: secrets::hash(&secret),
        expires_at,
    };
    let stored = match tokens::create(&mut tx, &token)
        .await
        .map_err(ApiError::from_db)?
    {
        Created::Done(stored) => *stored,
        Created::TooMany => {
            return Err(ApiError::InvalidField {
                reason: "too_many_tokens",
                pointer: "",
                message: format!(
                    "a project has at most {} active API tokens; revoke one first",
                    tokens::MAX_TOKENS
                ),
            });
        }
    };
    audit::record(
        &mut tx,
        "token.created",
        &stored.id.to_string(),
        &recorded(&stored),
    )
    .await?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok((
        StatusCode::CREATED,
        Json(Data {
            data: TokenView::new(stored, Some(secret))?,
        }),
    ))
}

/// `DELETE /api/v1/projects/{project}/tokens/{token}`: revokes it. Its requests in flight finish
/// first; none starts after. Revoking a revoked token answers it as it is.
pub(crate) async fn revoke(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<TokenPath>,
) -> Result<Json<Data<TokenView>>, ApiError> {
    let id = Uuid::try_parse(&path.token).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project).await?;
    may_manage(&mut tx).await?;
    let token = match tokens::revoke(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
    {
        Revoked::Done(token) => {
            audit::record(
                &mut tx,
                "token.revoked",
                &token.id.to_string(),
                &recorded(&token),
            )
            .await?;
            *token
        }
        Revoked::Already(token) => *token,
        Revoked::NotFound => return Err(ApiError::NotFound),
    };
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(Json(Data {
        data: TokenView::new(token, None)?,
    }))
}
