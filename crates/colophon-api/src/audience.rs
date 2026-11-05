//! The audience a project publishes for (docs/modules/03-auth.md, "Audience"; D-137): the
//! visitors of its sites — separate from members, who manage the project, and from API
//! tokens, which act for agents. A site user signs up and in with an email and an Argon2id
//! password (`colophon_db::audience`), holds opaque sessions whose hashes alone are stored,
//! and carries one of the project's audience roles — a label that `role:<name>` visibility
//! grants published content to.
//!
//! Public, site-facing routes (`/audience/v1/{project}/…`): a site's own pages call them,
//! usually through a same-origin proxy the site runs, so the cookies are first-party:
//!
//! - `POST /audience/v1/{project}/signup`: `{"email", "password", "display_name"?, "role"?}`
//!   makes the account and starts its session. `role` names a role open to signup
//!   (`self_assign`); left out, the project's single open role is taken.
//! - `POST /audience/v1/{project}/signin`: `{"email", "password"}` starts a session. A wrong
//!   password, a missing account and an unknown project answer alike
//!   (`sign_in_failed`); a paused account is `too_many_attempts`, a disabled one
//!   `account_disabled`.
//! - `POST /audience/v1/{project}/signout`: ends the session — a cookie-carried one asks the
//!   `X-CSRF-Token` header to match its CSRF cookie, as a staff session's writes do; a bearer
//!   credential needs nothing, never being ambient.
//! - `GET /audience/v1/{project}/me`: who the session belongs to, or 401.
//!
//! The session token is the same shape as a staff session's (43 URL-safe characters, SHA-256
//! at rest): `colophon_audience` `HttpOnly; Secure; SameSite=Lax; Path=/`, and the CSRF token
//! a readable `SameSite=Strict` sibling. The answer's `session` field carries the token too,
//! for clients that don't keep cookies (`Authorization: Bearer` works everywhere the cookie
//! does). The cookie is one project's at a time: a host serving two sites' audiences should
//! sign them in on distinct (sub)domains.
//!
//! Staff routes, under `{P}` like members' (`audience.manage`, never an API token):
//!
//! - `GET    {P}/audience/roles`, `POST` (`{"name", "self_assign"?}`),
//!   `PATCH {P}/audience/roles/{name}` (`{"self_assign"}`), `DELETE` it;
//! - `GET    {P}/audience/users`, `POST` (`{"email", "password", "display_name"?, "role"}`),
//!   `GET | PATCH {P}/audience/users/{user}` (`{"role"?, "disabled"?}`),
//!   `GET    {P}/audience/users/{user}/sessions`, `DELETE {P}/audience/sessions/{session}`.
//!
//! A password hash never leaves the database: views carry a user's state, never `password_hash`.

use axum::{
    Json,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use colophon_auth::tokens;
use colophon_db::{
    audience::{self, Found, RoleGone, SignIn, SignedUp, SiteUser, UserChanged},
    delivery::Audience,
    scope::MemberTransaction,
};
use colophon_domain::policy::{Action, is_role_key};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ApiError, AppState, Services,
    auth::{check_password, clean_email},
    entries::rfc3339,
    members,
    session::{self, Credential, SignedIn, cookie, set_cookie},
    shape::{Data, Page},
};

/// The site-session cookie (`HttpOnly`).
pub(crate) const SESSION_COOKIE: &str = "colophon_audience";
/// The CSRF token cookie a site session's scripts read and echo back.
pub(crate) const CSRF_COOKIE: &str = "colophon_audience_csrf";

/// `?limit=&cursor=` for the staff user list: `cursor` is the previous page's last user id.
#[derive(Deserialize)]
pub(crate) struct UsersQuery {
    limit: Option<i64>,
    cursor: Option<String>,
}

const DEFAULT_PAGE: i64 = 50;

/// A site user as an answer shows one: never the password hash, never session material.
#[derive(Serialize)]
pub(crate) struct SiteUserView {
    id: Uuid,
    email: String,
    display_name: Option<String>,
    /// The audience role it holds — the label `role:<name>` visibility grants to.
    role: String,
    created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_sign_in_at: Option<String>,
    /// `true` once staff disabled it: its sessions are dead and sign-in says so.
    disabled: bool,
}

impl SiteUserView {
    fn new(user: SiteUser) -> Result<Self, ApiError> {
        Ok(Self {
            id: user.id,
            email: user.email,
            display_name: user.display_name,
            role: user.role,
            created_at: rfc3339(user.created_at)?,
            last_sign_in_at: user.last_sign_in_at.map(rfc3339).transpose()?,
            disabled: user.disabled_at.is_some(),
        })
    }
}

/// An audience role as an answer shows one.
#[derive(Serialize)]
pub(crate) struct RoleView {
    name: String,
    /// Whether signup may choose it.
    self_assign: bool,
    created_at: String,
}

impl RoleView {
    fn new(role: audience::Role) -> Result<Self, ApiError> {
        Ok(Self {
            name: role.name,
            self_assign: role.self_assign,
            created_at: rfc3339(role.created_at)?,
        })
    }
}

/// A site session as an answer shows one — never token material.
#[derive(Serialize)]
pub(crate) struct SessionView {
    id: Uuid,
    created_at: String,
    last_seen_at: String,
    expires_at: String,
    revoked: bool,
}

impl SessionView {
    fn new(session: &audience::Session) -> Result<Self, ApiError> {
        Ok(Self {
            id: session.id,
            created_at: rfc3339(session.created_at)?,
            last_seen_at: rfc3339(session.last_seen_at)?,
            expires_at: rfc3339(session.expires_at)?,
            revoked: session.revoked_at.is_some(),
        })
    }
}

/// The answer a signup or sign-in gives: the user, and the session it just made — the token
/// and its CSRF sibling, for clients that hold them rather than cookies.
#[derive(Serialize)]
struct AudienceSession {
    user: SiteUserView,
    /// The session token: `Authorization: Bearer` it, or let the `Set-Cookie` pair carry it.
    session: String,
    /// The CSRF token: cookie writes echo it in `X-CSRF-Token`.
    csrf: String,
}

/// What a public signup asks for.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AudienceSignUp {
    email: String,
    password: String,
    /// A name to show; optional.
    display_name: Option<String>,
    /// The role to hold; optional — the project's single open role is the default.
    role: Option<String>,
}

/// What a public sign-in asks for.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AudienceSignIn {
    email: String,
    password: String,
}

/// What staff naming a role ask for (`POST {P}/audience/roles`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewAudienceRole {
    name: String,
    /// Whether signup may choose it; `false` unless said.
    self_assign: Option<bool>,
}

/// What staff changing a role ask for (`PATCH {P}/audience/roles/{name}`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RoleChange {
    self_assign: bool,
}

/// What staff making a site user ask for (`POST {P}/audience/users`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewSiteUser {
    email: String,
    password: String,
    display_name: Option<String>,
    /// Any role the project has, open or not.
    role: String,
}

/// What staff changing a site user ask for (`PATCH {P}/audience/users/{user}`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SiteUserChange {
    /// A new role for it, if changing.
    role: Option<String>,
    /// `true` disables (and revokes its sessions), `false` re-enables.
    disabled: Option<bool>,
}

/// The paths' project, for the public routes and the staff list alike.
#[derive(Deserialize)]
pub(crate) struct AudiencePath {
    project: String,
}

/// `{P}/audience/roles/{name}`.
#[derive(Deserialize)]
pub(crate) struct RolePath {
    project: String,
    name: String,
}

/// `{P}/audience/users/{user}` and below.
#[derive(Deserialize)]
pub(crate) struct UserPath {
    project: String,
    user: String,
}

/// `{P}/audience/sessions/{session}`.
#[derive(Deserialize)]
pub(crate) struct SessionPath {
    project: String,
    session: String,
}

/// The site-session credential a request carries: `Authorization: Bearer` first — never sent
/// by a browser on its own, so it needs no CSRF — then the `colophon_audience` cookie.
fn credential_of(headers: &HeaderMap) -> Option<(String, Credential)> {
    if let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    {
        return Some((token.to_owned(), Credential::Token));
    }
    cookie(headers, SESSION_COOKIE).map(|token| (token.to_owned(), Credential::Session))
}

/// The live site session the request carries in `project`, if any: the lookup reads the row
/// fresh (a role change or a disable bites at once), [`Found::live`] says whether it still
/// acts. A dead credential isn't an error here — `me` says 401, delivery reads as anonymous.
async fn found(
    services: &Services,
    project: Uuid,
    headers: &HeaderMap,
) -> Result<Option<(Found, Credential)>, ApiError> {
    let Some((token, credential)) = credential_of(headers) else {
        return Ok(None);
    };
    if token.len() != 43 {
        return Ok(None);
    }
    let found = audience::find_session(&services.pool, project, &tokens::hash(&token))
        .await
        .map_err(ApiError::from_db)?;
    Ok(found
        .filter(audience::Found::live)
        .map(|found| (found, credential)))
}

/// Who a delivery request reads as (D-137): a live site session's role, or anonymous —
/// the value the delivery transaction's `app.audience_role` becomes. Costs one session
/// lookup, and only when a credential is present at all.
pub(crate) async fn reader(
    services: &Services,
    project: Uuid,
    headers: &HeaderMap,
) -> Result<Audience, ApiError> {
    Ok(match found(services, project, headers).await? {
        Some((live, _)) => Audience::Member { role: live.role },
        None => Audience::Anonymous,
    })
}

/// The `Set-Cookie` pair a new site session puts on a browser: the session `HttpOnly`, the
/// CSRF token readable.
fn session_cookies(response: &mut Response, session_token: &str, csrf_token: &str) {
    let lifetime = u64::try_from(audience::LIFETIME.whole_seconds()).unwrap_or(u64::MAX);
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.append(
        header::SET_COOKIE,
        set_cookie(SESSION_COOKIE, session_token, lifetime, true, "Lax", "/"),
    );
    headers.append(
        header::SET_COOKIE,
        set_cookie(CSRF_COOKIE, csrf_token, lifetime, false, "Strict", "/"),
    );
}

/// Makes the session a signup or sign-in answers with: a random token and CSRF sibling,
/// only their hashes stored — [`audience::create_session`] bounds how many live sessions
/// one user holds.
async fn issue(
    services: &Services,
    project: Uuid,
    user: SiteUser,
) -> Result<AudienceSession, ApiError> {
    let session = tokens::random_token()
        .map_err(|_| ApiError::internal(std::io::Error::other("no secure random source")))?;
    let csrf = tokens::random_token()
        .map_err(|_| ApiError::internal(std::io::Error::other("no secure random source")))?;
    audience::create_session(
        &services.pool,
        project,
        user.id,
        &tokens::hash(&session),
        &tokens::hash(&csrf),
    )
    .await
    .map_err(ApiError::from_db)?;
    Ok(AudienceSession {
        user: SiteUserView::new(user)?,
        session,
        csrf,
    })
}

/// `POST /audience/v1/{project}/signup`.
pub(crate) async fn sign_up(
    State(state): State<AppState>,
    Path(path): Path<AudiencePath>,
    body: Result<Json<AudienceSignUp>, JsonRejection>,
) -> Result<Response, ApiError> {
    let services = session::services(&state)?;
    let Json(body) = body?;
    let project = Uuid::try_parse(&path.project).map_err(|_| ApiError::NotFound)?;
    let email = clean_email(&body.email)?;
    check_password(&body.password)?;
    let phc = colophon_db::passwords::hashed(&body.password)
        .await
        .map_err(ApiError::from_db)?;
    let name = body
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    let role = body
        .role
        .as_deref()
        .map(str::trim)
        .filter(|role| !role.is_empty());
    if let Some(role) = role
        && !is_role_key(role)
    {
        return Err(ApiError::InvalidField {
            reason: "invalid_role",
            pointer: "/role",
            message:
                "an audience role is 1–63 characters of a–z, 0–9 and '-', starting with a letter"
                    .to_owned(),
        });
    }
    let user = match audience::sign_up(&services.pool, project, &email, &phc, name, role).await {
        Ok(SignedUp::Made(user)) => user,
        Ok(SignedUp::Taken) => {
            return Err(ApiError::conflict(
                "email_taken",
                "that email already has an account",
            ));
        }
        Ok(SignedUp::Full) => {
            return Err(ApiError::conflict(
                "audience_full",
                "this site isn't taking new accounts",
            ));
        }
        Ok(SignedUp::RoleUnavailable) => {
            return Err(ApiError::InvalidField {
                reason: "unavailable_role",
                pointer: "/role",
                message: "signing up doesn't offer that role".to_owned(),
            });
        }
        Err(error) => return Err(ApiError::from_db(error)),
    };
    let made = issue(services, project, user).await?;
    let body = serde_json::to_string(&Data { data: &made }).map_err(ApiError::internal)?;
    let mut response = crate::entries::json(StatusCode::CREATED, body, None, false);
    session_cookies(&mut response, &made.session, &made.csrf);
    Ok(response)
}

/// `POST /audience/v1/{project}/signin`.
pub(crate) async fn sign_in(
    State(state): State<AppState>,
    Path(path): Path<AudiencePath>,
    body: Result<Json<AudienceSignIn>, JsonRejection>,
) -> Result<Response, ApiError> {
    let services = session::services(&state)?;
    let Json(body) = body?;
    let project = Uuid::try_parse(&path.project).map_err(|_| ApiError::NotFound)?;
    let email = clean_email(&body.email)?;
    let user = match audience::sign_in(&services.pool, project, &email, &body.password).await {
        Ok(SignIn::SignedIn(user)) => user,
        // A wrong password and a missing account answer alike, after the same Argon2 work.
        Ok(SignIn::Refused) => return Err(ApiError::SignInFailed),
        Ok(SignIn::Locked(_)) => {
            return Err(ApiError::conflict(
                "too_many_attempts",
                "too many tries; wait a while and try again",
            ));
        }
        Ok(SignIn::Disabled) => {
            return Err(ApiError::forbidden(
                "account_disabled",
                "this account has been disabled",
            ));
        }
        Err(error) => return Err(ApiError::from_db(error)),
    };
    let made = issue(services, project, user).await?;
    let body = serde_json::to_string(&Data { data: &made }).map_err(ApiError::internal)?;
    let mut response = crate::entries::json(StatusCode::OK, body, None, false);
    session_cookies(&mut response, &made.session, &made.csrf);
    Ok(response)
}

/// `POST /audience/v1/{project}/signout`: ends the session the request carries, if it is
/// still live, and clears the cookies either way — signing out twice answers the same.
pub(crate) async fn sign_out(
    State(state): State<AppState>,
    Path(path): Path<AudiencePath>,
    request: HeaderMap,
) -> Result<Response, ApiError> {
    let services = session::services(&state)?;
    let project = Uuid::try_parse(&path.project).map_err(|_| ApiError::NotFound)?;
    if let Some((live, credential)) = found(services, project, &request).await? {
        if credential == Credential::Session {
            // The ambient credential asks for the double-submit, like a staff session.
            let csrf = request
                .get(session::CSRF_HEADER)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            if !tokens::same_secret(&tokens::hash(csrf), &live.csrf_hash) {
                return Err(ApiError::forbidden(
                    "csrf_token_invalid",
                    "missing or invalid CSRF token",
                ));
            }
        }
        audience::revoke_session(&services.pool, project, live.session_id)
            .await
            .map_err(ApiError::from_db)?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    let headers = response.headers_mut();
    headers.append(header::SET_COOKIE, crate::auth::clear(SESSION_COOKIE, "/"));
    headers.append(
        header::SET_COOKIE,
        set_cookie(CSRF_COOKIE, "", 0, false, "Strict", "/"),
    );
    Ok(response)
}

/// `GET /audience/v1/{project}/me`: the site user the session belongs to.
pub(crate) async fn me(
    State(state): State<AppState>,
    Path(path): Path<AudiencePath>,
    request: HeaderMap,
) -> Result<Json<Data<SiteUserView>>, ApiError> {
    let services = session::services(&state)?;
    let project = Uuid::try_parse(&path.project).map_err(|_| ApiError::NotFound)?;
    let (live, _) = found(services, project, &request)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    Ok(Json(Data {
        data: SiteUserView::new(live.user())?,
    }))
}

// ---------------------------------------------------------------------------
// Staff routes: the project's audience, managed under `audience.manage` like members are
// under `members.manage`. A member transaction is what proves membership; the action is
// checked against the caller's live policy.

/// The member transaction every staff route opens; rejects API tokens inside `members::open`.
async fn open_staff(
    state: &AppState,
    caller: SignedIn,
    project: &str,
) -> Result<MemberTransaction, ApiError> {
    members::open(state, caller, project).await
}

/// `audience.manage`, against the policy read fresh inside the transaction.
async fn require_manage(tx: &mut MemberTransaction) -> Result<(), ApiError> {
    members::require(
        &members::fresh_policy(tx).await?,
        Action::AudienceManage,
        None,
        "only people whose role manages the audience can do this",
    )
}

/// `GET {P}/audience/roles`.
pub(crate) async fn list_roles(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<AudiencePath>,
) -> Result<Json<Data<Vec<RoleView>>>, ApiError> {
    let mut tx = open_staff(&state, caller, &path.project).await?;
    require_manage(&mut tx).await?;
    let roles = audience::roles(&mut tx).await.map_err(ApiError::from_db)?;
    let data = roles
        .into_iter()
        .map(RoleView::new)
        .collect::<Result<Vec<_>, _>>()?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(Json(Data { data }))
}

/// `POST {P}/audience/roles`.
pub(crate) async fn create_role(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<AudiencePath>,
    body: Result<Json<NewAudienceRole>, JsonRejection>,
) -> Result<(StatusCode, Json<Data<RoleView>>), ApiError> {
    let Json(body) = body?;
    let name = body.name.trim();
    if !is_role_key(name) {
        return Err(ApiError::InvalidField {
            reason: "invalid_role",
            pointer: "/name",
            message: "a role name is 1–63 characters of a–z, 0–9 and '-', starting with a letter"
                .to_owned(),
        });
    }
    let mut tx = open_staff(&state, caller, &path.project).await?;
    require_manage(&mut tx).await?;
    let self_assign = body.self_assign.unwrap_or(false);
    if !audience::create_role(&mut tx, name, self_assign)
        .await
        .map_err(ApiError::from_db)?
    {
        return Err(ApiError::conflict(
            "role_exists",
            "the project has this role already",
        ));
    }
    let detail = serde_json::json!({"name": name, "self_assign": self_assign});
    crate::audit::record(&mut tx, "audience.role_created", name, &detail).await?;
    let roles = audience::roles(&mut tx).await.map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let role = roles
        .into_iter()
        .find(|role| role.name == name)
        .ok_or(ApiError::NotFound)?;
    Ok((
        StatusCode::CREATED,
        Json(Data {
            data: RoleView::new(role)?,
        }),
    ))
}

/// `PATCH {P}/audience/roles/{name}`: only `self_assign` changes — a rename is a delete and
/// a create, so nothing holds a name that changed under it.
pub(crate) async fn update_role(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<RolePath>,
    body: Result<Json<RoleChange>, JsonRejection>,
) -> Result<Json<Data<RoleView>>, ApiError> {
    let Json(body) = body?;
    let mut tx = open_staff(&state, caller, &path.project).await?;
    require_manage(&mut tx).await?;
    if !audience::set_role_assign(&mut tx, &path.name, body.self_assign)
        .await
        .map_err(ApiError::from_db)?
    {
        return Err(ApiError::NotFound);
    }
    let detail = serde_json::json!({"self_assign": body.self_assign});
    crate::audit::record(&mut tx, "audience.role_updated", &path.name, &detail).await?;
    let roles = audience::roles(&mut tx).await.map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let role = roles
        .into_iter()
        .find(|role| role.name == path.name)
        .ok_or(ApiError::NotFound)?;
    Ok(Json(Data {
        data: RoleView::new(role)?,
    }))
}

/// `DELETE {P}/audience/roles/{name}`.
pub(crate) async fn delete_role(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<RolePath>,
) -> Result<StatusCode, ApiError> {
    let mut tx = open_staff(&state, caller, &path.project).await?;
    require_manage(&mut tx).await?;
    match audience::delete_role(&mut tx, &path.name)
        .await
        .map_err(ApiError::from_db)?
    {
        RoleGone::Deleted => {
            let none = serde_json::json!({});
            crate::audit::record(&mut tx, "audience.role_deleted", &path.name, &none).await?;
            tx.commit().await.map_err(ApiError::from_db)?;
            Ok(StatusCode::NO_CONTENT)
        }
        RoleGone::Missing => Err(ApiError::NotFound),
        RoleGone::InUse(count) => Err(ApiError::conflict(
            "role_in_use",
            match count {
                1 => "one site user holds this role; change their role first",
                _ => "site users hold this role; change their roles first",
            },
        )),
    }
}

/// `GET {P}/audience/users`.
pub(crate) async fn list_users(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<AudiencePath>,
    Query(query): Query<UsersQuery>,
) -> Result<Json<Page<SiteUserView>>, ApiError> {
    let before = query
        .cursor
        .as_deref()
        .map(|text| Uuid::try_parse(text).map_err(|_| ApiError::InvalidHeader("bad cursor")))
        .transpose()?;
    let limit = query.limit.unwrap_or(DEFAULT_PAGE);
    let mut tx = open_staff(&state, caller, &path.project).await?;
    require_manage(&mut tx).await?;
    let mut rows = audience::list_users(&mut tx, before, limit + 1)
        .await
        .map_err(ApiError::from_db)?;
    let page = usize::try_from(limit.max(0)).unwrap_or(usize::MAX);
    let more = rows.len() > page;
    rows.truncate(page);
    let next_cursor = if more {
        rows.last().map(|user| user.id.to_string())
    } else {
        None
    };
    let data = rows
        .into_iter()
        .map(SiteUserView::new)
        .collect::<Result<Vec<_>, _>>()?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(Json(Page { data, next_cursor }))
}

/// `POST {P}/audience/users`: staff make a site user, in any role the project has.
pub(crate) async fn create_user(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<AudiencePath>,
    body: Result<Json<NewSiteUser>, JsonRejection>,
) -> Result<(StatusCode, Json<Data<SiteUserView>>), ApiError> {
    let Json(body) = body?;
    let email = clean_email(&body.email)?;
    check_password(&body.password)?;
    let role = body.role.trim();
    if !is_role_key(role) {
        return Err(ApiError::InvalidField {
            reason: "invalid_role",
            pointer: "/role",
            message: "an audience role is one of the project's audience roles, by name".to_owned(),
        });
    }
    let phc = colophon_db::passwords::hashed(&body.password)
        .await
        .map_err(ApiError::from_db)?;
    let name = body
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    let mut tx = open_staff(&state, caller, &path.project).await?;
    require_manage(&mut tx).await?;
    let user = match audience::create_user(&mut tx, &email, &phc, name, role).await {
        Ok(SignedUp::Made(user)) => user,
        Ok(SignedUp::Taken) => {
            return Err(ApiError::conflict(
                "email_taken",
                "that email already has an account",
            ));
        }
        Ok(SignedUp::Full) => {
            return Err(ApiError::conflict(
                "audience_full",
                "the project is at its site-user bound",
            ));
        }
        Ok(SignedUp::RoleUnavailable) => {
            return Err(ApiError::InvalidField {
                reason: "unknown_audience_role",
                pointer: "/role",
                message: "this project has no audience role with that name".to_owned(),
            });
        }
        Err(error) => return Err(ApiError::from_db(error)),
    };
    let detail = serde_json::json!({"email": user.email, "role": user.role});
    crate::audit::record(
        &mut tx,
        "audience.user_created",
        &user.id.to_string(),
        &detail,
    )
    .await?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok((
        StatusCode::CREATED,
        Json(Data {
            data: SiteUserView::new(user)?,
        }),
    ))
}

/// `GET {P}/audience/users/{user}`.
pub(crate) async fn get_user(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<UserPath>,
) -> Result<Json<Data<SiteUserView>>, ApiError> {
    let id = Uuid::try_parse(&path.user).map_err(|_| ApiError::NotFound)?;
    let mut tx = open_staff(&state, caller, &path.project).await?;
    require_manage(&mut tx).await?;
    let user = audience::get_user(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(Json(Data {
        data: SiteUserView::new(user)?,
    }))
}

/// `PATCH {P}/audience/users/{user}`: a role change and/or a disable. Disabling revokes the
/// user's live sessions in the same transaction, so its next request reads as anonymous.
pub(crate) async fn update_user(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<UserPath>,
    body: Result<Json<SiteUserChange>, JsonRejection>,
) -> Result<Json<Data<SiteUserView>>, ApiError> {
    let Json(body) = body?;
    let id = Uuid::try_parse(&path.user).map_err(|_| ApiError::NotFound)?;
    let role = body
        .role
        .as_deref()
        .map(str::trim)
        .filter(|role| !role.is_empty());
    if let Some(role) = role
        && !is_role_key(role)
    {
        return Err(ApiError::InvalidField {
            reason: "invalid_role",
            pointer: "/role",
            message: "an audience role is one of the project's audience roles, by name".to_owned(),
        });
    }
    if role.is_none() && body.disabled.is_none() {
        return Err(ApiError::InvalidField {
            reason: "empty_change",
            pointer: "/",
            message: "nothing to change: give a role, or disabled".to_owned(),
        });
    }
    let mut tx = open_staff(&state, caller, &path.project).await?;
    require_manage(&mut tx).await?;
    let changed = audience::update_user(&mut tx, id, role, body.disabled)
        .await
        .map_err(ApiError::from_db)?;
    match changed {
        UserChanged::Updated(user) => {
            let detail =
                serde_json::json!({"role": user.role, "disabled": user.disabled_at.is_some()});
            crate::audit::record(
                &mut tx,
                "audience.user_updated",
                &user.id.to_string(),
                &detail,
            )
            .await?;
            tx.commit().await.map_err(ApiError::from_db)?;
            Ok(Json(Data {
                data: SiteUserView::new(user)?,
            }))
        }
        UserChanged::Missing => Err(ApiError::NotFound),
        UserChanged::UnknownRole => Err(ApiError::InvalidField {
            reason: "unknown_audience_role",
            pointer: "/role",
            message: "this project has no audience role with that name".to_owned(),
        }),
    }
}

/// `GET {P}/audience/users/{user}/sessions`: the sessions, live and dead — the audit trail.
pub(crate) async fn list_sessions(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<UserPath>,
) -> Result<Json<Data<Vec<SessionView>>>, ApiError> {
    let id = Uuid::try_parse(&path.user).map_err(|_| ApiError::NotFound)?;
    let mut tx = open_staff(&state, caller, &path.project).await?;
    require_manage(&mut tx).await?;
    // The user must exist for the list to mean anything.
    let _user = audience::get_user(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let sessions = audience::sessions_of(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?;
    let data = sessions
        .iter()
        .map(SessionView::new)
        .collect::<Result<Vec<_>, _>>()?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(Json(Data { data }))
}

/// `DELETE {P}/audience/sessions/{session}`: revoke one session, whatever state it is in.
pub(crate) async fn revoke_session(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<SessionPath>,
) -> Result<StatusCode, ApiError> {
    let id = Uuid::try_parse(&path.session).map_err(|_| ApiError::NotFound)?;
    let mut tx = open_staff(&state, caller, &path.project).await?;
    require_manage(&mut tx).await?;
    if !audience::revoke_session_in(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
    {
        return Err(ApiError::NotFound);
    }
    let none = serde_json::json!({});
    crate::audit::record(&mut tx, "audience.session_revoked", &id.to_string(), &none).await?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(StatusCode::NO_CONTENT)
}

/// What a magic-link request asks for (D-140).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MagicLinkRequest {
    email: String,
    /// Where the reader lands: the site's own absolute http(s) URL.
    return_to: String,
}

/// `{P}/audience/magic-link/{token}`.
#[derive(Deserialize)]
pub(crate) struct MagicLinkPath {
    project: String,
    token: String,
}

/// The `return_to` a magic link may carry: an absolute http(s) URL, nothing else.
fn check_return_to(value: &str) -> Result<String, ApiError> {
    let trimmed = value.trim();
    if trimmed.len() > 2048 {
        return Err(ApiError::InvalidField {
            reason: "invalid_return_to",
            pointer: "/return_to",
            message: "a return address is at most 2048 characters".to_owned(),
        });
    }
    match url::Url::parse(trimmed) {
        Ok(url) if matches!(url.scheme(), "http" | "https") && url.host().is_some() => {
            Ok(trimmed.to_owned())
        }
        _ => Err(ApiError::InvalidField {
            reason: "invalid_return_to",
            pointer: "/return_to",
            message: "a return address is an absolute http or https URL".to_owned(),
        }),
    }
}

/// `POST /audience/v1/{project}/magic-link`: records a single-use link and answers `202`
/// whether or not the email has an account — the same answer either way, so nothing here can
/// be used to enumerate visitors. The link itself is emailed by a sender this server does not
/// have yet; with `COLOPHON_DEV_MAIL=1` it is also handed back in `dev_link`, which is how the
/// development stack and the demo sites test the flow without a mail server.
pub(crate) async fn request_magic_link(
    State(state): State<AppState>,
    Path(path): Path<AudiencePath>,
    body: Result<Json<MagicLinkRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let services = session::services(&state)?;
    let Json(body) = body?;
    let project = Uuid::try_parse(&path.project).map_err(|_| ApiError::NotFound)?;
    let email = clean_email(&body.email)?;
    let return_to = check_return_to(&body.return_to)?;
    let token = tokens::random_token()
        .map_err(|_| ApiError::internal(std::io::Error::other("no secure random source")))?;
    audience::request_magic_link(
        &services.pool,
        project,
        &email,
        &tokens::hash(&token),
        &return_to,
    )
    .await
    .map_err(ApiError::from_db)?;
    let separator = if return_to.contains('?') { '&' } else { '?' };
    let mut data = serde_json::json!({ "sent": true });
    if services.dev_mail {
        data["dev_link"] = serde_json::json!(format!("{return_to}{separator}token={token}"));
    }
    let body =
        serde_json::to_string(&serde_json::json!({ "data": data })).map_err(ApiError::internal)?;
    Ok(crate::entries::json(
        StatusCode::ACCEPTED,
        body,
        None,
        false,
    ))
}

/// `POST /audience/v1/{project}/magic-link/{token}`: consumes the link and answers like a
/// sign-in — the session, its CSRF sibling and the cookies. An existing account signs in; an
/// unknown email becomes a site user in the project's open self-assign role, so a project that
/// opens no role simply has no passwordless signup.
pub(crate) async fn magic_link_sign_in(
    State(state): State<AppState>,
    Path(path): Path<MagicLinkPath>,
) -> Result<Response, ApiError> {
    let services = session::services(&state)?;
    let project = Uuid::try_parse(&path.project).map_err(|_| ApiError::NotFound)?;
    let user =
        match audience::consume_magic_link(&services.pool, project, &tokens::hash(&path.token))
            .await
            .map_err(ApiError::from_db)?
        {
            audience::MagicLink::SignedIn(user) => user,
            audience::MagicLink::Invalid => {
                return Err(ApiError::forbidden(
                    "magic_link_invalid",
                    "this sign-in link is used, expired or unknown",
                ));
            }
            audience::MagicLink::NewUser(email) => {
                // An account made by a link holds a password nobody knows: the link was the
                // credential, and a later password sign-in must set one deliberately.
                let unusable = tokens::random_token().map_err(|_| {
                    ApiError::internal(std::io::Error::other("no secure random source"))
                })?;
                let phc = colophon_db::passwords::hashed(&unusable)
                    .await
                    .map_err(ApiError::from_db)?;
                match audience::sign_up(&services.pool, project, &email, &phc, None, None).await {
                    Ok(audience::SignedUp::Made(user)) => user,
                    Ok(audience::SignedUp::RoleUnavailable) => {
                        return Err(ApiError::forbidden(
                            "no_open_role",
                            "this project has no role a new account may take",
                        ));
                    }
                    Ok(_) => {
                        return Err(ApiError::conflict(
                            "email_taken",
                            "that email already has an account",
                        ));
                    }
                    Err(error) => return Err(ApiError::from_db(error)),
                }
            }
        };
    let made = issue(services, project, user).await?;
    let body = serde_json::to_string(&Data { data: &made }).map_err(ApiError::internal)?;
    let mut response = crate::entries::json(StatusCode::OK, body, None, false);
    session_cookies(&mut response, &made.session, &made.csrf);
    Ok(response)
}
