//! `/auth/login`, `/auth/callback`, `/auth/logout` and `/me` (docs/modules/03-auth.md).

use std::time::Duration;

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use colophon_auth::tokens;
use colophon_db::identity;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{
    ApiError, AppState,
    session::{
        CSRF_COOKIE, SESSION_COOKIE, SIGN_IN_COOKIE, SignedIn, cookie, services, set_cookie,
    },
    shape::Data,
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
    tracing::info!(identity = %identity_id, "signed in");

    let lifetime = services.sessions.lifetime.as_secs();
    let mut response = Redirect::to(&attempt.return_to).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.append(
        header::SET_COOKIE,
        set_cookie(SESSION_COOKIE, &session_token, lifetime, true, "Lax", "/"),
    );
    headers.append(
        header::SET_COOKIE,
        set_cookie(CSRF_COOKIE, &csrf_token, lifetime, false, "Strict", "/"),
    );
    headers.append(header::SET_COOKIE, clear(SIGN_IN_COOKIE, "/auth"));
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
