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
use colophon_domain::budget::{self, LimitError, Limits};
use colophon_domain::policy::{
    Action, MAX_TOKEN_DAYS, Policy, RoleError, TokenKind, role_description, role_name,
};
use serde::{Deserialize, Deserializer, Serialize};
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

/// A token's budget ceilings (D-113): every field optional, an absent one unconstrained at the
/// quota layer. Bounded by the same maxima the columns' CHECKs enforce.
#[derive(Deserialize, Serialize, Clone, Copy)]
#[serde(deny_unknown_fields)]
pub(crate) struct LimitsBody {
    /// Entry writes per rolling day: creates, saves, localizations, import items and the
    /// mirror writes their references cause.
    #[serde(default)]
    documents_per_day: Option<i64>,
    /// Delivery jobs held at once.
    #[serde(default)]
    max_concurrent_jobs: Option<i64>,
    /// Management requests per minute (stored; nothing consumes it yet).
    #[serde(default)]
    requests_per_minute: Option<i64>,
}

impl LimitsBody {
    /// The checked [`Limits`], or the field's refusal.
    fn limits(&self) -> Result<Limits, ApiError> {
        Limits {
            documents_per_day: self.documents_per_day,
            max_concurrent_jobs: self.max_concurrent_jobs,
            requests_per_minute: self.requests_per_minute,
        }
        .check()
        .map_err(|error| match error {
            LimitError::Range(field) => ApiError::InvalidField {
                reason: "invalid_limit",
                pointer: match field {
                    "documents_per_day" => "/limits/documents_per_day",
                    "max_concurrent_jobs" => "/limits/max_concurrent_jobs",
                    _ => "/limits/requests_per_minute",
                },
                message: format!(
                    "a token's {field} is between 1 and {}",
                    match field {
                        "documents_per_day" => budget::MAX_DOCUMENTS_PER_DAY,
                        "max_concurrent_jobs" => budget::MAX_CONCURRENT_JOBS,
                        _ => budget::MAX_REQUESTS_PER_MINUTE,
                    }
                ),
            },
            LimitError::Empty => ApiError::InvalidField {
                reason: "limits_empty",
                pointer: "/limits",
                message:
                    "limits must bound something: name at least one limit, or leave `limits` out"
                        .to_owned(),
            },
        })
    }
}

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
    /// Budget ceilings (D-113): absent means unconstrained at the quota layer.
    #[serde(default)]
    limits: Option<LimitsBody>,
}

/// `PATCH` body: only `limits` changes. `null` removes the token's ceilings; an object
/// replaces them.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PatchTokenBody {
    /// `Missing` when the key is left out (nothing asked); a present key says which way.
    #[serde(default)]
    limits: LimitsUpdate,
}

/// The three answers `limits` can give in a patch: absent, `null` (clear the ceilings)
/// or an object (replace them). `Option<…>` alone couldn't tell `null` from absent —
/// both deserialize to `None`.
#[derive(Default)]
pub(crate) enum LimitsUpdate {
    #[default]
    Missing,
    Clear,
    Set(LimitsBody),
}

impl<'de> Deserialize<'de> for LimitsUpdate {
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match Option::<LimitsBody>::deserialize(de)? {
            Some(body) => Self::Set(body),
            None => Self::Clear,
        })
    }
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
    /// Its budget ceilings, `null` when it has none (D-113).
    limits: Option<LimitsBody>,
    /// What it holds or spent, from `budget_reservations`; only on the single-token answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<UsageView>,
    /// Only in the answer that creates it: `colophon_api_…`.
    #[serde(skip_serializing_if = "Option::is_none")]
    secret: Option<String>,
}

/// What the token holds or spent now (migration 0045), as `budget_reservations` counts it.
#[derive(Serialize)]
pub(crate) struct UsageView {
    /// Documents written in the last day (settled, held and uncertain reservations).
    documents_last_day: i64,
    /// Jobs reservations still open (held or uncertain).
    jobs_open: i64,
    /// Management requests in the last minute (recorded; always 0 until consumed).
    requests_last_minute: i64,
}

impl TokenView {
    fn new(
        token: StoredToken,
        secret: Option<String>,
        usage: Option<colophon_db::budgets::Usage>,
    ) -> Result<Self, ApiError> {
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
            limits: token.limits.map(|limits| LimitsBody {
                documents_per_day: limits.documents_per_day,
                max_concurrent_jobs: limits.max_concurrent_jobs,
                requests_per_minute: limits.requests_per_minute,
            }),
            usage: usage.map(|usage| UsageView {
                documents_last_day: usage.documents_last_day,
                jobs_open: usage.jobs_open,
                requests_last_minute: usage.requests_last_minute,
            }),
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
        "limits": token.limits.map(|limits| json!({
            "documents_per_day": limits.documents_per_day,
            "max_concurrent_jobs": limits.max_concurrent_jobs,
            "requests_per_minute": limits.requests_per_minute,
        })),
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
        .map(|token| TokenView::new(token, None, None))
        .collect::<Result<_, _>>()?;
    Ok(Json(Page {
        data,
        next_cursor: None,
    }))
}

/// `GET /api/v1/projects/{project}/tokens/{token}`: the token's view, its limits and its
/// usage — what `budget_reservations` says it holds or spent (D-113).
pub(crate) async fn get(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<TokenPath>,
) -> Result<Json<Data<TokenView>>, ApiError> {
    let id = Uuid::try_parse(&path.token).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project).await?;
    may_manage(&mut tx).await?;
    let Some(token) = tokens::get(&mut tx, id).await.map_err(ApiError::from_db)? else {
        return Err(ApiError::NotFound);
    };
    let usage = colophon_db::budgets::usage(&mut tx, token.identity_id)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(Json(Data {
        data: TokenView::new(token, None, Some(usage))?,
    }))
}

/// `PATCH /api/v1/projects/{project}/tokens/{token}`: replaces its budget ceilings
/// (`{"limits": {…}}`), or removes them (`{"limits": null}`). Recorded as
/// `token.limits_changed` (D-113).
pub(crate) async fn update(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<TokenPath>,
    body: Result<Json<PatchTokenBody>, JsonRejection>,
) -> Result<Json<Data<TokenView>>, ApiError> {
    let Json(body) = body?;
    let id = Uuid::try_parse(&path.token).map_err(|_| ApiError::NotFound)?;
    let limits = match body.limits {
        LimitsUpdate::Missing => {
            return Err(ApiError::InvalidField {
                reason: "nothing_to_change",
                pointer: "/limits",
                message: "patch names `limits`: an object replaces it, `null` removes it"
                    .to_owned(),
            });
        }
        LimitsUpdate::Set(body) => Some(body.limits()?),
        LimitsUpdate::Clear => None,
    };
    let mut tx = open(&state, caller, &path.project).await?;
    may_manage(&mut tx).await?;
    let before = tokens::get(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    match &limits {
        Some(limits) => colophon_db::budgets::set_limits(&mut tx, id, limits).await,
        None => colophon_db::budgets::remove_limits(&mut tx, id).await,
    }
    .map_err(ApiError::from_db)?;
    let token = tokens::get(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    if token.limits != before.limits {
        audit::record(
            &mut tx,
            "token.limits_changed",
            &token.id.to_string(),
            &json!({
                "limits": {
                    "before": before.limits.map(|limits| json!({
                        "documents_per_day": limits.documents_per_day,
                        "max_concurrent_jobs": limits.max_concurrent_jobs,
                        "requests_per_minute": limits.requests_per_minute,
                    })),
                    "after": token.limits.map(|limits| json!({
                        "documents_per_day": limits.documents_per_day,
                        "max_concurrent_jobs": limits.max_concurrent_jobs,
                        "requests_per_minute": limits.requests_per_minute,
                    })),
                }
            }),
        )
        .await?;
    }
    let usage = colophon_db::budgets::usage(&mut tx, token.identity_id)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(Json(Data {
        data: TokenView::new(token, None, Some(usage))?,
    }))
}

/// A creation request, checked: what the token will be.
struct Requested<'a> {
    name: &'a str,
    description: &'a str,
    kind: TokenKind,
    granted: Policy,
    days: u16,
    limits: Option<Limits>,
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
    let limits = body.limits.map(|limits| limits.limits()).transpose()?;
    Ok(Requested {
        name,
        description,
        kind,
        granted,
        days: body.expires_in_days,
        limits,
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
        limits,
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
        limits: limits.as_ref(),
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
            data: TokenView::new(stored, Some(secret), None)?,
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
        data: TokenView::new(token, None, None)?,
    }))
}
