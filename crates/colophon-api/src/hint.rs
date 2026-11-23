//! The signed-in hint (D-235): `colophon_signed_in=1` on the hint domain, so a static site under
//! it (the landing page) can show "Open the Studio" instead of "Sign in" without asking the API.
//! It is a hint only: readable by scripts (not `HttpOnly`), it authorises nothing; every request
//! is judged by its session alone.
//!
//! One middleware owns it, so no handler does: set when a response starts a session, refreshed
//! when [`SignedIn`](crate::session::SignedIn) touches one (at most once a minute), cleared when a
//! response ends one or a request carrying a session or the hint is refused 401. Nothing happens
//! unless `COLOPHON_SIGNED_IN_HINT_DOMAIN` is set.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use axum::{
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::Response,
};

use crate::{
    AppState,
    session::{SESSION_COOKIE, cookie},
};

/// The hint's name.
pub(crate) const HINT_COOKIE: &str = "colophon_signed_in";

/// Set by `SignedIn` when it touches a session: the hint's lifetime starts again.
#[derive(Clone, Default)]
pub(crate) struct Touched(Arc<AtomicBool>);

impl Touched {
    pub(crate) fn mark(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

fn hint(domain: &str, max_age: u64) -> HeaderValue {
    // The domain was checked at boot to be a plain host name covering the public host.
    HeaderValue::from_str(&format!(
        "{HINT_COOKIE}=1; Domain={domain}; Path=/; Max-Age={max_age}; Secure; SameSite=Lax"
    ))
    .unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// Whether the response sets the session cookie: `Some(true)` starts one, `Some(false)` clears it.
fn session_change(response: &Response) -> Option<bool> {
    let prefix = format!("{SESSION_COOKIE}=");
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.strip_prefix(prefix.as_str()))
        .map(|rest| !rest.starts_with(';') && !rest.contains("Max-Age=0"))
        .next_back()
}

pub(crate) async fn signed_in(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some((domain, idle)) = state.services.as_deref().and_then(|services| {
        Some((
            services.selfserve.hint_domain.clone()?,
            services.sessions.idle.as_secs(),
        ))
    }) else {
        return next.run(request).await;
    };
    let presented = cookie(request.headers(), SESSION_COOKIE).is_some()
        || cookie(request.headers(), HINT_COOKIE).is_some();
    let touched = Touched::default();
    request.extensions_mut().insert(touched.clone());
    let mut response = next.run(request).await;
    let value = match session_change(&response) {
        Some(true) => hint(&domain, idle),
        Some(false) => hint(&domain, 0),
        None if presented && response.status() == StatusCode::UNAUTHORIZED => hint(&domain, 0),
        None if touched.0.load(Ordering::Relaxed) => hint(&domain, idle),
        None => return response,
    };
    response.headers_mut().append(header::SET_COOKIE, value);
    response
}

#[cfg(test)]
mod tests {
    use axum::response::IntoResponse;

    use super::*;

    fn with(cookies: &[&str]) -> Response {
        let mut response = StatusCode::NO_CONTENT.into_response();
        for value in cookies {
            response.headers_mut().append(
                header::SET_COOKIE,
                HeaderValue::from_str(value).expect("ascii"),
            );
        }
        response
    }

    #[test]
    fn a_response_starts_ends_or_leaves_the_session() {
        let starts =
            format!("{SESSION_COOKIE}=abc; Path=/; Max-Age=3600; Secure; SameSite=Lax; HttpOnly");
        let ends = format!("{SESSION_COOKIE}=; Path=/; Max-Age=0; Secure; SameSite=Lax; HttpOnly");
        assert_eq!(session_change(&with(&[&starts])), Some(true));
        assert_eq!(session_change(&with(&[&ends])), Some(false));
        assert_eq!(
            session_change(&with(&["colophon_session=; Max-Age=0", &starts])),
            Some(true)
        );
        assert_eq!(session_change(&with(&["other=1"])), None);
    }

    #[test]
    fn the_hint_is_readable_on_the_domain_and_never_http_only() {
        let value = hint("404human.dev", 1800);
        let text = value.to_str().expect("ascii");
        assert_eq!(
            text,
            "colophon_signed_in=1; Domain=404human.dev; Path=/; Max-Age=1800; Secure; SameSite=Lax"
        );
        assert!(!text.contains("HttpOnly"));
    }
}
