//! Sessions: who is asking, from the session cookie, and CSRF protection for writes
//! (docs/modules/03-auth.md, "Sessions").
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
use colophon_db::identity;
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

/// A signed-in caller. Extracting it checks the session, and for writes, CSRF and `Origin`.
#[derive(Debug, Clone, Copy)]
pub struct SignedIn {
    /// The identity.
    pub identity: Uuid,
    /// The session (not its token).
    pub session: Uuid,
}

impl FromRequestParts<AppState> for SignedIn {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let services = services(state)?;
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
                return Err(ApiError::Forbidden("this origin may not make changes"));
            }
            let csrf = parts
                .headers
                .get(CSRF_HEADER)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            if !tokens::same_secret(&tokens::hash(csrf), &session.csrf_hash) {
                return Err(ApiError::Forbidden("missing or invalid CSRF token"));
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
