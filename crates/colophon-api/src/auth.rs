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
    /// `create` starts on the provider's form for making an account, if it has one.
    start: Option<String>,
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

/// What the sign-in page may offer: the built-in accounts are always there; `sso` says whether
/// an external identity provider is configured, so the page never shows a button that can only
/// fail (D-141).
#[derive(Serialize)]
pub(crate) struct Providers {
    sso: bool,
}

/// `GET /auth/providers`: public — a sign-in page asks before offering the provider button.
pub(crate) async fn providers(
    State(state): State<AppState>,
) -> Result<Json<Data<Providers>>, ApiError> {
    let services = services(&state)?;
    Ok(Json(Data {
        data: Providers {
            sso: services.oidc.is_some(),
        },
    }))
}

/// `GET /auth/login?return_to=/path[&start=create]`: starts a sign-in and sends the browser to the
/// provider: to its sign-in form, or with `start=create` to its form for making an account.
/// Either way the person comes back signed in; an account grants nothing by itself (D-043).
pub(crate) async fn login(
    State(state): State<AppState>,
    Query(query): Query<LoginQuery>,
) -> Result<Response, ApiError> {
    let services = services(&state)?;
    let oidc = services.oidc.as_ref().ok_or(ApiError::Unavailable(
        "sign-in is not configured on this server",
    ))?;
    let start = match query.start.as_deref() {
        Some("create") => colophon_auth::Start::CreateAccount,
        _ => colophon_auth::Start::SignIn,
    };
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
        .authorization_url(&sign_in_state, &nonce, &pkce_verifier, start)
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

/// Expires cookie `name` — signing out. Shared with the audience routes.
pub(crate) fn clear(name: &str, path: &str) -> HeaderValue {
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

    let mut response = Redirect::to(&attempt.return_to).into_response();
    session_cookies(
        &mut response,
        &session_token,
        &csrf_token,
        services.sessions.lifetime.as_secs(),
    );
    response
        .headers_mut()
        .append(header::SET_COOKIE, clear(SIGN_IN_COOKIE, "/auth"));
    Ok(response)
}

/// Issues a session for `identity` and answers its fresh token and CSRF token.
async fn issue_session(
    services: &crate::state::Services,
    identity: uuid::Uuid,
) -> Result<(String, String), ApiError> {
    let (session_token, csrf_token) = (random()?, random()?);
    identity::create_session(
        &services.pool,
        identity,
        &tokens::hash(&session_token),
        &tokens::hash(&csrf_token),
        expires_in(services.sessions.lifetime),
    )
    .await
    .map_err(ApiError::from_db)?;
    tracing::info!(identity = %identity, "signed in");
    Ok((session_token, csrf_token))
}

/// The `Set-Cookie` pair a new session puts on a browser.
fn session_cookies(response: &mut Response, session_token: &str, csrf_token: &str, lifetime: u64) {
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

/// What making an account asks for.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SignUp {
    email: String,
    password: String,
    /// A name to show; optional, the email fills in.
    display_name: Option<String>,
}

/// What signing in asks for.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PasswordSignIn {
    email: String,
    password: String,
}

/// Normalizes an email for storing or finding: trimmed and lowered, then checked it can be
/// one address (`a@b`, nothing else, 254 bytes at most). Shared with the audience routes —
/// members and visitors follow the same rule.
pub(crate) fn clean_email(email: &str) -> Result<String, ApiError> {
    let email = email.trim().to_lowercase();
    let valid = email.len() <= 254
        && match email.split_once('@') {
            Some((local, domain)) => {
                !local.is_empty() && !domain.is_empty() && !domain.contains('@')
            }
            None => false,
        };
    if !valid {
        return Err(ApiError::InvalidField {
            reason: "invalid_email",
            pointer: "/email",
            message: "this doesn't look like an email address".to_owned(),
        });
    }
    Ok(email)
}

/// Checks a password can be tried: at least eight characters, and bounded so a giant body
/// can't keep Argon2 busy far past the request limit. Shared with the audience routes.
pub(crate) fn check_password(password: &str) -> Result<(), ApiError> {
    if password.chars().count() < 8 {
        return Err(ApiError::InvalidField {
            reason: "weak_password",
            pointer: "/password",
            message: "pick at least eight characters".to_owned(),
        });
    }
    if password.len() > 512 {
        return Err(ApiError::InvalidField {
            reason: "weak_password",
            pointer: "/password",
            message: "keep it under 512 bytes".to_owned(),
        });
    }
    Ok(())
}

/// `POST /auth/signup`: makes an email-and-password account and signs it in at once, so a
/// person who followed an invitation can join right after.
pub(crate) async fn sign_up(
    State(state): State<AppState>,
    Json(body): Json<SignUp>,
) -> Result<Response, ApiError> {
    let services = services(&state)?;
    let email = clean_email(&body.email)?;
    check_password(&body.password)?;
    let phc = colophon_db::passwords::hashed(&body.password)
        .await
        .map_err(ApiError::from_db)?;
    let name = body
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty());
    let id = match identity::sign_up(&services.pool, &email, &phc, name).await {
        Ok(identity::SignedUp::Made(id)) => id,
        Ok(identity::SignedUp::Taken) => {
            return Err(ApiError::Conflict {
                reason: "email_taken",
                message: "that email already has an account",
            });
        }
        Err(error) => return Err(ApiError::from_db(error)),
    };
    let (session_token, csrf_token) = issue_session(services, id).await?;
    let mut response = StatusCode::CREATED.into_response();
    session_cookies(
        &mut response,
        &session_token,
        &csrf_token,
        services.sessions.lifetime.as_secs(),
    );
    Ok(response)
}

/// `POST /auth/signin`: trades an email and password for a session. A wrong password, a
/// missing account and a locked account are refused the same way, and a locked account says
/// how long it pauses for.
pub(crate) async fn sign_in(
    State(state): State<AppState>,
    Json(body): Json<PasswordSignIn>,
) -> Result<Response, ApiError> {
    let services = services(&state)?;
    let email = clean_email(&body.email)?;
    match identity::password_sign_in(&services.pool, &email, &body.password).await {
        Ok(identity::PasswordSignIn::SignedIn(id)) => {
            let (session_token, csrf_token) = issue_session(services, id).await?;
            let mut response = StatusCode::NO_CONTENT.into_response();
            session_cookies(
                &mut response,
                &session_token,
                &csrf_token,
                services.sessions.lifetime.as_secs(),
            );
            Ok(response)
        }
        Ok(identity::PasswordSignIn::Refused) => Err(ApiError::SignInFailed),
        Ok(identity::PasswordSignIn::Locked(_)) => Err(ApiError::Conflict {
            reason: "too_many_attempts",
            message: "too many tries; wait a while and try again",
        }),
        Err(error) => Err(ApiError::from_db(error)),
    }
}

/// What signing out answers in a browser.
#[derive(Serialize)]
struct SignedOut {
    /// Where the browser goes to end its session at the identity provider too, so the next
    /// person at this computer must sign in again; `null` when the provider can't say (it has
    /// no such page, or can't be reached: Colophon's own session has ended either way).
    provider_sign_out: Option<String>,
}

/// `POST /auth/logout`: ends this session (or revokes the CLI's token) at once, whatever the
/// identity provider does. A browser is told where to end its session at the provider as well
/// (RP-Initiated Logout), coming back to the Studio's front door.
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
    let provider_sign_out = match (services.oidc.as_ref(), services.trusted_origins.first()) {
        (Some(oidc), Some(origin)) => match oidc.logout_url(&format!("{origin}/studio/")).await {
            Ok(url) => url.map(String::from),
            Err(error) => {
                tracing::warn!(%error, "identity provider unavailable; signed out here only");
                None
            }
        },
        _ => None,
    };
    let mut response = Json(Data {
        data: SignedOut { provider_sign_out },
    })
    .into_response();
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
    /// The installation as a whole.
    installation: Installation,
    projects: Vec<crate::projects::ProjectView>,
}

#[derive(Serialize)]
pub(crate) struct Installation {
    /// Whether anyone is an administrator yet. Until someone is, the setup code the server prints
    /// when it starts makes the first one (`POST /api/v1/setup`, D-061).
    has_administrator: bool,
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
            installation: Installation {
                has_administrator: profile.identity.installation_set_up,
            },
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
