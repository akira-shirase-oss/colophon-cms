//! The session cookies' old names, for one release after D-235's rename to
//! `__Host-colophon_session` / `__Host-colophon_csrf`, so a deploy doesn't sign everyone out.
//!
//! An old-name cookie (`colophon_session`) can be planted on this host by a sibling subdomain, so
//! it is accepted only for a session made before the rename was deployed (the migration records
//! when), and only once: its first use swaps it for a new session under the new name, with the
//! same identity, CSRF token, expiry and last use, and expires the old cookies. Anything else
//! under the old name is dropped from the request, as if it weren't there, and cleared. A
//! request that carries the new name already ignores the old one.
//!
//! REMOVE in the release after the one that ships the rename (CHANGELOG says which).

use axum::{
    extract::{Request, State},
    http::{HeaderValue, header},
    middleware::Next,
    response::Response,
};
use colophon_auth::tokens;

use crate::{
    AppState,
    session::{
        CSRF_COOKIE, LEGACY_CSRF_COOKIE, LEGACY_SESSION_COOKIE, SESSION_COOKIE, cookie, set_cookie,
    },
};

/// The middleware (inside admission, so a flood of old cookies is rate-limited first).
pub(crate) async fn upgrade(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let headers = request.headers();
    let Some(legacy) = cookie(headers, LEGACY_SESSION_COOKIE).map(str::to_owned) else {
        return next.run(request).await;
    };
    let legacy_csrf = cookie(headers, LEGACY_CSRF_COOKIE).map(str::to_owned);
    let has_new = cookie(headers, SESSION_COOKIE).is_some();
    let mut swapped = None;
    if !has_new
        && legacy.len() == 43
        && let Some(services) = state.services.as_deref()
        && let Ok(new_token) = tokens::random_token()
    {
        match colophon_db::identity::upgrade_legacy_session(
            &services.pool,
            &tokens::hash(&legacy),
            &tokens::hash(&new_token),
        )
        .await
        {
            Ok(Some((identity, expires_at))) => {
                let address = crate::admission::ClientAddress::of(&request, &state);
                crate::auth::staff_auth_line(
                    services,
                    colophon_db::staff_auth::Line {
                        identity: Some(identity),
                        detail: Some("legacy_cookie"),
                        ..colophon_db::staff_auth::Line::new(
                            colophon_db::staff_auth::Event::SessionRotated,
                            &address,
                        )
                    },
                )
                .await;
                let lifetime =
                    u64::try_from((expires_at - time::OffsetDateTime::now_utc()).whole_seconds())
                        .unwrap_or(0);
                swapped = Some((new_token, lifetime));
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(error = %error, "could not upgrade an old session cookie"),
        }
    }
    rewrite_cookies(
        &mut request,
        swapped.as_ref().map(|(token, _)| token.as_str()),
        legacy_csrf.as_deref(),
    );
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    if let Some((token, lifetime)) = swapped {
        headers.append(
            header::SET_COOKIE,
            set_cookie(SESSION_COOKIE, &token, lifetime, true, "Lax", "/"),
        );
        if let Some(csrf) = legacy_csrf.as_deref().filter(|csrf| csrf.len() == 43) {
            headers.append(
                header::SET_COOKIE,
                set_cookie(CSRF_COOKIE, csrf, lifetime, false, "Strict", "/"),
            );
        }
    }
    headers.append(
        header::SET_COOKIE,
        crate::auth::clear(LEGACY_SESSION_COOKIE, "/"),
    );
    headers.append(
        header::SET_COOKIE,
        set_cookie(LEGACY_CSRF_COOKIE, "", 0, false, "Strict", "/"),
    );
    response
}

/// The request's cookies without the old names; with the swapped session (and the old CSRF
/// token, which the swapped session keeps) under the new names when there is one.
fn rewrite_cookies(request: &mut Request, swapped: Option<&str>, legacy_csrf: Option<&str>) {
    let mut pairs: Vec<String> = request
        .headers()
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .map(str::trim)
        .filter(|pair| {
            let name = pair.split_once('=').map_or(*pair, |(name, _)| name);
            !pair.is_empty() && name != LEGACY_SESSION_COOKIE && name != LEGACY_CSRF_COOKIE
        })
        .map(str::to_owned)
        .collect();
    if let Some(token) = swapped {
        pairs.push(format!("{SESSION_COOKIE}={token}"));
        if let Some(csrf) = legacy_csrf {
            pairs.push(format!("{CSRF_COOKIE}={csrf}"));
        }
    }
    let headers = request.headers_mut();
    headers.remove(header::COOKIE);
    if !pairs.is_empty()
        && let Ok(value) = HeaderValue::from_str(&pairs.join("; "))
    {
        headers.insert(header::COOKIE, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_names_leave_the_request_and_a_swapped_session_arrives_under_the_new() {
        let mut request = Request::builder()
            .header(
                header::COOKIE,
                "a=1; colophon_session=old; colophon_csrf=tok",
            )
            .header(header::COOKIE, "b=2")
            .body(axum::body::Body::empty())
            .expect("a request");
        rewrite_cookies(&mut request, Some("new"), Some("tok"));
        let headers = request.headers();
        assert_eq!(cookie(headers, LEGACY_SESSION_COOKIE), None);
        assert_eq!(cookie(headers, LEGACY_CSRF_COOKIE), None);
        assert_eq!(cookie(headers, SESSION_COOKIE), Some("new"));
        assert_eq!(cookie(headers, CSRF_COOKIE), Some("tok"));
        assert_eq!(
            (cookie(headers, "a"), cookie(headers, "b")),
            (Some("1"), Some("2"))
        );

        let mut request = Request::builder()
            .header(header::COOKIE, "colophon_session=planted")
            .body(axum::body::Body::empty())
            .expect("a request");
        rewrite_cookies(&mut request, None, None);
        assert!(
            request.headers().get(header::COOKIE).is_none(),
            "nothing left to send"
        );
    }
}
