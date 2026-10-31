//! `/auth/login`, `/auth/callback`, `/auth/signup`, `/auth/signin`, `/auth/logout` and `/me`
//! (docs/modules/03-auth.md).

use std::time::Duration;

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use colophon_auth::tokens;
use colophon_db::{identity, passwords};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{
    ApiError, AppState,
    session::{
        CSRF_COOKIE, SESSION_COOKIE, SIGN_IN_COOKIE, SignedIn, cookie, services, set_cookie,
    },
    shape::Data,
    state::Services,
};

/// How long a started sign-in may take.
const SIGN_IN_WINDOW: Duration = Duration::from_secs(600);

#[derive(Deserialize)]
pub(crate) struct LoginQuery {
    return_to: Option<String>,
}

/// A path on this site, or `/`: never another site (`//host`, `/\host`, or an absolute URL).
fn safe_return_path(requested: Option<&str>) -> String {
    match requested {
        Some(path)
            if path.starts_with('/')
                && !path.starts_with("//")
                && !path.contains('\\')
                && !path.chars().any(char::is_control)
                && path.len() <= 2048 =>
        {
            path.to_owned()
        }
        _ => "/".to_owned(),
    }
}

fn random() -> Result<String, ApiError> {
    tokens::random_token()
        .map_err(|_| ApiError::internal(std::io::Error::other("no secure random source")))
}

fn expires_in(duration: Duration) -> OffsetDateTime {
    OffsetDateTime::now_utc() + time::Duration::try_from(duration).unwrap_or(time::Duration::MINUTE)
}

/// Mints a session and CSRF token pair and stores their hashes — the same session any
/// sign-in path (provider callback, password) ends with.
async fn issue_session(
    services: &Services,
    identity_id: uuid::Uuid,
) -> Result<(String, String), ApiError> {
    let (session_token, csrf_token) = (random()?, random()?);
    identity::create_session(
        &services.pool,
        identity_id,
        &tokens::hash(&session_token),
        &tokens::hash(&csrf_token),
        expires_in(services.sessions.lifetime),
    )
    .await
    .map_err(ApiError::from_db)?;
    Ok((session_token, csrf_token))
}

/// Puts the session and CSRF cookies on `response`, exactly as the provider callback does.
fn session_cookies(response: &mut Response, session: &str, csrf: &str, lifetime: Duration) {
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.append(
        header::SET_COOKIE,
        set_cookie(
            SESSION_COOKIE,
            session,
            lifetime.as_secs(),
            true,
            "Lax",
            "/",
        ),
    );
    headers.append(
        header::SET_COOKIE,
        set_cookie(CSRF_COOKIE, csrf, lifetime.as_secs(), false, "Strict", "/"),
    );
}

/// `GET /auth/login?return_to=/path`: starts a sign-in and sends the browser to the provider.
pub(crate) async fn login(
    State(state): State<AppState>,
    Query(query): Query<LoginQuery>,
) -> Result<Response, ApiError> {
    let services = services(&state)?;
    let oidc = services.oidc.as_ref().ok_or(ApiError::Unavailable(
        "sign-in is not configured on this server",
    ))?;
    let (sign_in_state, nonce, pkce_verifier) = (random()?, random()?, random()?);
    let attempt = identity::SignInAttempt {
        nonce: nonce.clone(),
        pkce_verifier: pkce_verifier.clone(),
        return_to: safe_return_path(query.return_to.as_deref()),
    };
    identity::start_sign_in(
        &services.pool,
        &tokens::hash(&sign_in_state),
        &attempt,
        expires_in(SIGN_IN_WINDOW),
    )
    .await
    .map_err(ApiError::from_db)?;
    let url = oidc
        .authorization_url(&sign_in_state, &nonce, &pkce_verifier)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "identity provider unavailable");
            ApiError::Unavailable("the identity provider is unavailable")
        })?;
    let mut response = Redirect::to(url.as_str()).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    // Binds this sign-in to this browser: the callback must come back with it (login CSRF).
    headers.append(
        header::SET_COOKIE,
        set_cookie(
            SIGN_IN_COOKIE,
            &sign_in_state,
            SIGN_IN_WINDOW.as_secs(),
            true,
            "Lax",
            "/auth",
        ),
    );
    Ok(response)
}

#[derive(Deserialize)]
pub(crate) struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

fn clear(name: &str, path: &str) -> HeaderValue {
    set_cookie(name, "", 0, true, "Lax", path)
}

/// `GET /auth/callback`: finishes a sign-in, creates the session, and goes back where the person
/// started.
pub(crate) async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Result<Response, ApiError> {
    let services = services(&state)?;
    let oidc = services.oidc.as_ref().ok_or(ApiError::Unavailable(
        "sign-in is not configured on this server",
    ))?;
    let failed = |why: &str| {
        tracing::info!(reason = why, "sign-in failed");
        ApiError::SignInFailed
    };
    if let Some(error) = query.error.as_deref() {
        return Err(failed(&format!("the provider returned {error:?}")));
    }
    let (Some(code), Some(sign_in_state)) = (query.code.as_deref(), query.state.as_deref()) else {
        return Err(failed("missing code or state"));
    };
    let bound = cookie(&headers, SIGN_IN_COOKIE).unwrap_or_default();
    if !tokens::same_secret(bound.as_bytes(), sign_in_state.as_bytes()) {
        return Err(failed("state not bound to this browser"));
    }
    let attempt = identity::finish_sign_in(&services.pool, &tokens::hash(sign_in_state))
        .await
        .map_err(ApiError::from_db)?
        .ok_or_else(|| failed("unknown, expired or already used state"))?;
    let person = oidc
        .exchange(code, &attempt.pkce_verifier, &attempt.nonce)
        .await
        .map_err(|error| failed(&error.to_string()))?;
    let identity_id = identity::record_sign_in(
        &services.pool,
        &person.issuer,
        &person.subject,
        person.display_name.as_deref(),
    )
    .await
    .map_err(ApiError::from_db)?;
    let (session_token, csrf_token) = issue_session(services, identity_id).await?;
    tracing::info!(identity = %identity_id, "signed in");

    let mut response = Redirect::to(&attempt.return_to).into_response();
    session_cookies(
        &mut response,
        &session_token,
        &csrf_token,
        services.sessions.lifetime,
    );
    response
        .headers_mut()
        .append(header::SET_COOKIE, clear(SIGN_IN_COOKIE, "/auth"));
    Ok(response)
}

/// A password must be at least this long: short enough for people, long enough that
/// guessing costs.
const MIN_PASSWORD_LENGTH: usize = 8;
/// Email and password bounds, over RFC 5321's own limit and any sane password.
const MAX_EMAIL_LENGTH: usize = 254;
const MAX_PASSWORD_LENGTH: usize = 512;

/// An email, lower-cased and shape-checked: one `@`, non-empty parts, within bounds.
fn clean_email(raw: &str) -> Result<String, ApiError> {
    let email = raw.trim().to_lowercase();
    let mut parts = email.split('@');
    let valid = email.len() <= MAX_EMAIL_LENGTH
        && parts.next().is_some_and(|local| !local.is_empty())
        && parts.next().is_some_and(|domain| !domain.is_empty())
        && parts.next().is_none();
    if valid {
        Ok(email)
    } else {
        Err(ApiError::InvalidField {
            reason: "invalid_email",
            pointer: "/email",
            message: "that does not look like an email address".to_owned(),
        })
    }
}

/// Refuses a password that is too short to be safe or too long to be a password at all.
fn check_password(password: &str) -> Result<(), ApiError> {
    if password.len() > MAX_PASSWORD_LENGTH || password.chars().count() < MIN_PASSWORD_LENGTH {
        Err(ApiError::InvalidField {
            reason: "weak_password",
            pointer: "/password",
            message: format!("a password needs at least {MIN_PASSWORD_LENGTH} characters"),
        })
    } else {
        Ok(())
    }
}

/// `POST /auth/signup`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SignUp {
    email: String,
    password: String,
    display_name: Option<String>,
}

/// `POST /auth/signup`: creates an email-and-password account and starts its session — one
/// step, so an invited person goes straight to the invitation.
pub(crate) async fn sign_up(
    State(state): State<AppState>,
    Json(request): Json<SignUp>,
) -> Result<Response, ApiError> {
    let services = services(&state)?;
    let email = clean_email(&request.email)?;
    check_password(&request.password)?;
    let hash = passwords::hashed(&request.password)
        .await
        .map_err(ApiError::from_db)?;
    let display_name = request
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    let identity_id = match identity::sign_up(&services.pool, &email, &hash, display_name)
        .await
        .map_err(ApiError::from_db)?
    {
        identity::SignedUp::Made(id) => id,
        identity::SignedUp::Taken => {
            return Err(ApiError::Conflict("that email already has an account"));
        }
    };
    let (session_token, csrf_token) = issue_session(services, identity_id).await?;
    tracing::info!(identity = %identity_id, "signed up");
    let mut response = StatusCode::CREATED.into_response();
    session_cookies(
        &mut response,
        &session_token,
        &csrf_token,
        services.sessions.lifetime,
    );
    Ok(response)
}

/// `POST /auth/signin`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PasswordSignIn {
    email: String,
    password: String,
}

/// `POST /auth/signin`: the email-and-password sign-in. A wrong password and a missing
/// account get the same answer; too many misses pauses the account briefly.
pub(crate) async fn sign_in(
    State(state): State<AppState>,
    Json(request): Json<PasswordSignIn>,
) -> Result<Response, ApiError> {
    let services = services(&state)?;
    let email = clean_email(&request.email)?;
    let identity_id = match identity::password_sign_in(&services.pool, &email, &request.password)
        .await
        .map_err(ApiError::from_db)?
    {
        identity::PasswordSignIn::SignedIn(id) => id,
        identity::PasswordSignIn::Refused => return Err(ApiError::SignInFailed),
        identity::PasswordSignIn::Locked(_) => {
            return Err(ApiError::Conflict(
                "too many tries; that account is paused for a few minutes",
            ));
        }
    };
    let (session_token, csrf_token) = issue_session(services, identity_id).await?;
    tracing::info!(identity = %identity_id, "signed in");
    let mut response = StatusCode::NO_CONTENT.into_response();
    session_cookies(
        &mut response,
        &session_token,
        &csrf_token,
        services.sessions.lifetime,
    );
    Ok(response)
}

/// `POST /auth/logout`: ends the session (CSRF-checked, like every write).
pub(crate) async fn logout(
    State(state): State<AppState>,
    caller: SignedIn,
) -> Result<Response, ApiError> {
    let services = services(&state)?;
    if caller.credential == crate::session::Credential::Token {
        // The CLI signing out: revoke its token; there are no cookies to clear.
        colophon_db::access::revoke_access_token(&services.pool, caller.session)
            .await
            .map_err(ApiError::from_db)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    identity::revoke_session(&services.pool, caller.session)
        .await
        .map_err(ApiError::from_db)?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    let headers = response.headers_mut();
    headers.append(header::SET_COOKIE, clear(SESSION_COOKIE, "/"));
    headers.append(
        header::SET_COOKIE,
        set_cookie(CSRF_COOKIE, "", 0, false, "Strict", "/"),
    );
    Ok(response)
}

#[derive(Serialize)]
pub(crate) struct Me {
    id: uuid::Uuid,
    issuer: String,
    subject: String,
    display_name: Option<String>,
    /// Whether this person may create projects (D-043).
    administrator: bool,
    projects: Vec<crate::projects::ProjectView>,
}

/// `GET /me`: who is signed in, and their projects.
pub(crate) async fn me(
    State(state): State<AppState>,
    caller: SignedIn,
) -> Result<Json<Data<Me>>, ApiError> {
    let services = services(&state)?;
    let profile = identity::profile(&services.pool, caller.identity)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::Unauthorized)?;
    Ok(Json(Data {
        data: Me {
            id: profile.identity.id,
            issuer: profile.identity.issuer,
            subject: profile.identity.subject,
            display_name: profile.identity.display_name,
            administrator: profile.identity.administrator,
            projects: profile
                .projects
                .into_iter()
                .map(crate::projects::ProjectView::from)
                .collect(),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::safe_return_path;

    #[test]
    fn return_paths_never_leave_the_site() {
        for (requested, expected) in [
            (Some("/studio"), "/studio"),
            (Some("/me?x=1"), "/me?x=1"),
            (Some("//evil.example"), "/"),
            (Some("/\\evil.example"), "/"),
            (Some("https://evil.example"), "/"),
            (Some("/a\nb"), "/"),
            (None, "/"),
        ] {
            assert_eq!(safe_return_path(requested), expected, "{requested:?}");
        }
    }
}
