//! Email-and-password accounts end to end against the real binary (D-084): sign-up, sign-in,
//! the field and lockout refusals, and that a password session is an ordinary session for
//! `/me` and for writes. Needs the stack (scripts/stack.sh up) and migrations
//! (scripts/migrate.sh).
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{ORIGIN, Server, Session, app, cookie, get, post, unique_slug};
use reqwest::{StatusCode, header};
use serde_json::{Value, json};

/// `POST` a JSON body to an `/auth` route with no session, as a browser arriving fresh does:
/// status, headers (the `Set-Cookie`s) and the body.
async fn post_auth(path: &str, body: &Value) -> (StatusCode, header::HeaderMap, Value) {
    let response = app()
        .post(format!("{ORIGIN}{path}"))
        .json(body)
        .send()
        .await
        .expect("request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json().await.unwrap_or(Value::Null);
    (status, headers, body)
}

/// The `Set-Cookie` name/value pairs of a response's headers.
fn cookies_from_headers(headers: &header::HeaderMap) -> Vec<(String, String)> {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| {
            value
                .split(';')
                .next()?
                .split_once('=')
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
        })
        .collect()
}

/// The session a sign-in or sign-up answer sets, read from its `Set-Cookie` headers.
fn session_from(cookies: &[(String, String)]) -> Session {
    let (session, csrf) = (
        cookie(cookies, "colophon_session"),
        cookie(cookies, "colophon_csrf"),
    );
    assert_eq!((session.len(), csrf.len()), (43, 43), "{cookies:?}");
    Session {
        cookie: format!("colophon_session={session}; colophon_csrf={csrf}"),
        csrf,
    }
}

fn an_email() -> String {
    format!("{}@example.test", unique_slug())
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn an_email_account_signs_up_and_uses_its_session() {
    let _server = Server::start().await;
    let email = an_email();
    let password = "a long enough password";

    // Signing up validates the fields: the email must look like one, the password must be
    // eight characters and bounded.
    let (status, _, body) = post_auth(
        "/auth/signup",
        &json!({"email": "nope", "password": password}),
    )
    .await;
    assert_eq!(
        (status, &body["error"]["details"]["reason"]),
        (StatusCode::UNPROCESSABLE_ENTITY, &json!("invalid_email")),
        "{body}"
    );
    let (status, _, body) = post_auth(
        "/auth/signup",
        &json!({"email": email, "password": "short"}),
    )
    .await;
    assert_eq!(
        (status, &body["error"]["details"]["reason"]),
        (StatusCode::UNPROCESSABLE_ENTITY, &json!("weak_password")),
        "{body}"
    );

    // Signing up makes the account and starts its session at once, for an invitation's join.
    let (status, response, _) = post_auth(
        "/auth/signup",
        &json!({"email": email, "password": password, "display_name": "Erin"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let cookies = cookies_from_headers(&response);
    let session = session_from(&cookies);
    let raw: Vec<_> = response
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap_or_default().to_owned())
        .collect();
    assert!(
        raw.iter().any(|c| c.starts_with("colophon_session=")
            && c.contains("HttpOnly")
            && c.contains("Secure")
            && c.contains("SameSite=Lax")),
        "{raw:?}"
    );
    let (status, me) = get(Some(&session), "/me").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["data"]["issuer"], "password");
    assert_eq!(me["data"]["subject"], email);
    assert_eq!(me["data"]["display_name"], "Erin");
    assert!(
        me["data"].get("password_hash").is_none() && me["data"].get("email").is_none(),
        "credentials are never returned: {me}"
    );

    // The session is an ordinary one: a write needs its CSRF token and a trusted Origin, and
    // past those it reaches authorization — Erin is no administrator, so project creation is
    // refused on the permission, not on the session.
    let slug = unique_slug();
    let body = json!({ "slug": slug, "name": "Erin's site" });
    let (status, _) = post(&session, "/api/v1/projects", &body, None, Some(ORIGIN)).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a password session needs CSRF too"
    );
    let (status, body) = post(
        &session,
        "/api/v1/projects",
        &body,
        Some(&session.csrf),
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("administrator")),
        "the write reached authorization: {body}"
    );

    // The email can't sign up a second account, in any casing.
    let (status, _, body) = post_auth(
        "/auth/signup",
        &json!({"email": email.to_uppercase(), "password": "a different password"}),
    )
    .await;
    assert_eq!(
        (status, &body["error"]["details"]["reason"]),
        (StatusCode::CONFLICT, &json!("email_taken")),
        "{body}"
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn signing_in_wrong_and_missing_refuse_alike() {
    let _server = Server::start().await;
    let email = an_email();
    let password = "a long enough password";
    let (status, _, _) = post_auth(
        "/auth/signup",
        &json!({"email": email, "password": password}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Signing in with the right password starts a fresh session; a wrong password and a
    // missing account refuse alike.
    let (status, response, _) = post_auth(
        "/auth/signin",
        &json!({"email": email, "password": password}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let signed_in = session_from(&cookies_from_headers(&response));
    let (status, me) = get(Some(&signed_in), "/me").await;
    assert_eq!(
        (status, &me["data"]["subject"]),
        (StatusCode::OK, &json!(email))
    );
    for body in [
        json!({"email": email, "password": "the wrong password"}),
        json!({"email": an_email(), "password": password}),
    ] {
        let (status, _, body) = post_auth("/auth/signin", &body).await;
        assert_eq!(
            (status, &body["error"]["code"]),
            (StatusCode::BAD_REQUEST, &json!("sign_in_failed")),
            "{body}"
        );
    }
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn enough_misses_lock_the_account_even_for_the_right_password() {
    let _server = Server::start().await;
    let email = an_email();
    let (status, _, _) = post_auth(
        "/auth/signup",
        &json!({"email": email, "password": "the real password"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Seven misses refuse as a failed sign-in; the eighth pauses the account.
    let guess = json!({"email": email, "password": "a guess"});
    for miss in 1..8 {
        let (status, _, body) = post_auth("/auth/signin", &guess).await;
        assert_eq!(
            (status, &body["error"]["code"]),
            (StatusCode::BAD_REQUEST, &json!("sign_in_failed")),
            "miss {miss}"
        );
    }
    for body in [
        guess.clone(),
        json!({"email": email, "password": "the real password"}),
    ] {
        let (status, _, body) = post_auth("/auth/signin", &body).await;
        assert_eq!(
            (status, &body["error"]["details"]["reason"]),
            (StatusCode::CONFLICT, &json!("too_many_attempts")),
            "{body}"
        );
    }
}
