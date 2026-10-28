//! Sessions: who is asking, from the session cookie or a personal access token, and CSRF
//! protection for cookie writes (docs/modules/03-auth.md, "Sessions"; D-041).
//!
//! - A request with `Authorization: Bearer <token>` is authenticated by that token alone: a
//!   bearer token is never sent by a browser on its own, so it needs no CSRF token, and an invalid
//!   one is 401 rather than a fall back to the cookie.
//! - A bearer token that starts with [`API_TOKEN_PREFIX`] is a project's API token (step R.4,
//!   D-063): it acts as its own identity with its own permissions, only on its project's content
//!   routes (`/api/v1/projects/{project}/environments/...`, and the stateless rich-text
//!   validator). Another project's routes are 404 to it, as projects a person isn't in are; every
//!   other route is 403 `api_token_not_allowed`, and so is scheduling, which runs later as a
//!   person. A revoked or expired token is 401.
//!
//! - The session cookie holds a random token; only its hash is stored. `HttpOnly; Secure;
//!   SameSite=Lax`, so scripts can't read it and other sites can't send it with their writes.
//! - Every cookie-authenticated write (anything but GET, HEAD, OPTIONS) needs both an `Origin`
//!   that is trusted exactly and an `X-CSRF-Token` header matching the session's CSRF token.
//!   The CSRF token lives in a readable, `SameSite=Strict` cookie that only this site's pages
//!   can read and echo back.

use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, HeaderValue, Method, header, request::Parts},
};
use colophon_auth::tokens;
use colophon_db::{access, identity, tokens as api_tokens};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{ApiError, AppState, state::Services};

/// The session cookie (`HttpOnly`).
pub const SESSION_COOKIE: &str = "colophon_session";
/// The CSRF token cookie (readable by this site's scripts).
pub const CSRF_COOKIE: &str = "colophon_csrf";
/// The sign-in binding cookie: ties a sign-in's state to the browser that started it.
pub const SIGN_IN_COOKIE: &str = "colophon_signin";
/// The header writes must carry.
pub const CSRF_HEADER: &str = "x-csrf-token";

/// The value of cookie `name`, from any `Cookie` header.
pub fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find_map(|(key, value)| (key == name).then_some(value))
}

/// A `Set-Cookie` header value. `Secure` always: browsers accept it on `localhost`, and anything
/// else must be HTTPS anyway.
pub fn set_cookie(
    name: &str,
    value: &str,
    max_age_seconds: u64,
    http_only: bool,
    same_site: &str,
    path: &str,
) -> HeaderValue {
    let http_only = if http_only { "; HttpOnly" } else { "" };
    let text = format!(
        "{name}={value}; Path={path}; Max-Age={max_age_seconds}; Secure; SameSite={same_site}{http_only}"
    );
    // Every part is ASCII: names are constants, values are base64url tokens.
    HeaderValue::from_str(&text).unwrap_or_else(|_| HeaderValue::from_static(""))
}

pub(crate) fn services(state: &AppState) -> Result<&Services, ApiError> {
    state.services.as_deref().ok_or(ApiError::Unavailable(
        "this server has no database configured",
    ))
}

/// How a caller proved who they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Credential {
    /// A browser session cookie.
    Session,
    /// A personal access token (`Authorization: Bearer`), as the CLI uses.
    Token,
    /// A project's API token (`Authorization: Bearer colophon_api_…`, step R.4).
    ApiToken,
}

/// What every API token's secret starts with, so it can't be mistaken for a personal token and
/// secret scanners can recognise one that leaked.
pub const API_TOKEN_PREFIX: &str = "colophon_api_";

/// A signed-in caller. Extracting it checks the token, or the session and, for writes, CSRF and
/// `Origin`.
#[derive(Debug, Clone, Copy)]
pub struct SignedIn {
    /// The identity.
    pub identity: Uuid,
    /// The session's or the token's id (not the secret).
    pub session: Uuid,
    /// Which of them it is.
    pub credential: Credential,
    /// For an API token, the one project it belongs to; `None` for people.
    pub project: Option<Uuid>,
}

/// The caller behind `Authorization: Bearer <token>`.
async fn bearer(
    services: &Services,
    value: &HeaderValue,
    path: &str,
) -> Result<SignedIn, ApiError> {
    let token = value
        .to_str()
        .ok()
        .and_then(|text| text.strip_prefix("Bearer "))
        .ok_or(ApiError::Unauthorized)?;
    if let Some(secret) = token.strip_prefix(API_TOKEN_PREFIX) {
        return api_token(services, secret, token, path).await;
    }
    if token.len() != 43 {
        return Err(ApiError::Unauthorized);
    }
    let found = access::find_access_token(&services.pool, &tokens::hash(token))
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::Unauthorized)?;
    if OffsetDateTime::now_utc() - found.last_used_at >= access::TOUCH_INTERVAL
        && let Err(error) = access::touch_access_token(&services.pool, found.id).await
    {
        tracing::warn!(%error, "could not record token use");
    }
    Ok(SignedIn {
        identity: found.identity_id,
        session: found.id,
        credential: Credential::Token,
        project: None,
    })
}

/// The project API token `token` (whose random part is `secret`), on `path`.
async fn api_token(
    services: &Services,
    secret: &str,
    token: &str,
    path: &str,
) -> Result<SignedIn, ApiError> {
    if secret.len() != 43 {
        return Err(ApiError::Unauthorized);
    }
    let found = api_tokens::find(&services.pool, &tokens::hash(token))
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::Unauthorized)?;
    let now = OffsetDateTime::now_utc();
    if found.revoked_at.is_some() || found.expires_at <= now {
        return Err(ApiError::Unauthorized);
    }
    let own = format!("/api/v1/projects/{}/", found.project_id);
    if let Some(rest) = path.strip_prefix(&own) {
        if !rest.starts_with("environments/") {
            return Err(not_for_tokens(
                "an API token works only with its project's content; members, roles, tokens and the audit log stay with people",
            ));
        }
        if rest.ends_with("/schedule") {
            return Err(not_for_tokens(
                "a schedule publishes later as the person who made it, so API tokens can't schedule",
            ));
        }
    } else if path.starts_with("/api/v1/projects/") {
        // Another project: as unknown to the token as one a person isn't in.
        return Err(ApiError::NotFound);
    } else if path != "/api/v1/rich-text/validate" {
        return Err(not_for_tokens(
            "an API token works only with its project's content",
        ));
    }
    if found
        .last_used_at
        .is_none_or(|last| now - last >= api_tokens_touch_interval())
        && let Err(error) = api_tokens::touch(&services.pool, &found).await
    {
        tracing::warn!(%error, "could not record API token use");
    }
    Ok(SignedIn {
        identity: found.identity_id,
        session: found.id,
        credential: Credential::ApiToken,
        project: Some(found.project_id),
    })
}

fn not_for_tokens(message: &'static str) -> ApiError {
    ApiError::forbidden("api_token_not_allowed", message)
}

/// How often an API token's last use is written: at most once a minute, like a session's.
const fn api_tokens_touch_interval() -> time::Duration {
    time::Duration::MINUTE
}

impl FromRequestParts<AppState> for SignedIn {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let services = services(state)?;
        if let Some(value) = parts.headers.get(header::AUTHORIZATION) {
            return bearer(services, value, parts.uri.path()).await;
        }
        let token = cookie(&parts.headers, SESSION_COOKIE).ok_or(ApiError::Unauthorized)?;
        if token.len() != 43 {
            return Err(ApiError::Unauthorized);
        }
        let session = identity::find_session(&services.pool, &tokens::hash(token))
            .await
            .map_err(ApiError::from_db)?
            .ok_or(ApiError::Unauthorized)?;
        let idle = time::Duration::try_from(services.sessions.idle).unwrap_or(time::Duration::HOUR);
        let unused_for = OffsetDateTime::now_utc() - session.last_seen_at;
        if unused_for > idle {
            identity::revoke_session(&services.pool, session.id)
                .await
                .map_err(ApiError::from_db)?;
            return Err(ApiError::Unauthorized);
        }
        if !matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS) {
            let origin = parts
                .headers
                .get(header::ORIGIN)
                .and_then(|value| value.to_str().ok());
            if !origin.is_some_and(|origin| {
                services
                    .trusted_origins
                    .iter()
                    .any(|trusted| trusted == origin)
            }) {
                return Err(ApiError::forbidden(
                    "origin_not_allowed",
                    "this origin may not make changes",
                ));
            }
            let csrf = parts
                .headers
                .get(CSRF_HEADER)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            if !tokens::same_secret(&tokens::hash(csrf), &session.csrf_hash) {
                return Err(ApiError::forbidden(
                    "csrf_token_invalid",
                    "missing or invalid CSRF token",
                ));
            }
        }
        // At most one write a minute per session: most requests only read.
        if unused_for >= identity::TOUCH_INTERVAL
            && let Err(error) = identity::touch_session(&services.pool, session.id).await
        {
            tracing::warn!(%error, "could not record session use");
        }
        Ok(Self {
            identity: session.identity_id,
            session: session.id,
            credential: Credential::Session,
            project: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookies_are_found_across_headers_and_pairs() {
        let mut headers = HeaderMap::new();
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("a=1; colophon_session=abc"),
        );
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("colophon_csrf=def"),
        );
        assert_eq!(cookie(&headers, SESSION_COOKIE), Some("abc"));
        assert_eq!(cookie(&headers, CSRF_COOKIE), Some("def"));
        assert_eq!(cookie(&headers, "missing"), None);
        assert_eq!(
            cookie(&headers, "colophon"),
            None,
            "names match exactly, not by prefix"
        );
    }

    #[test]
    fn session_cookies_are_locked_down() {
        let value = set_cookie(SESSION_COOKIE, "token", 60, true, "Lax", "/");
        assert_eq!(
            value,
            "colophon_session=token; Path=/; Max-Age=60; Secure; SameSite=Lax; HttpOnly"
        );
    }
}
