//! The staff session guards (D-235) against the real binary: the `Origin` rule for sign-up and
//! sign-in, which carry no session yet (login CSRF); sign-out with a session going through the
//! session's own rule; session rotation on sign-in and sign-up (no fixation); the cookies' flags
//! and `__Host-` names; and the old names read for one release only, for sessions made before
//! the rename. Needs the stack and `scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{ORIGIN, Reply, Server, Session, app, owner_database, unique_slug};
use reqwest::{StatusCode, header};
use serde_json::{Value, json};

const PASSWORD: &str = "colophon-dev";

fn fresh_email() -> String {
    format!("sess-{}@colophon.test", unique_slug())
}

async fn post(path: &str, body: &Value, headers: &[(&str, &str)]) -> Reply {
    let mut request = app().post(format!("{ORIGIN}{path}")).json(body);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = request.send().await.expect("request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json::<Value>().await.unwrap_or(Value::Null);
    Reply {
        status,
        headers,
        body,
    }
}

fn reason(reply: &Reply) -> Option<&str> {
    reply.body["error"]["details"]["reason"].as_str()
}

fn set_cookies(reply: &Reply) -> Vec<String> {
    reply
        .headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok().map(str::to_owned))
        .collect()
}

/// The value a reply sets for cookie `name` (not a clearing one).
fn set_value(reply: &Reply, name: &str) -> Option<String> {
    set_cookies(reply).into_iter().find_map(|line| {
        line.strip_prefix(&format!("{name}="))
            .and_then(|rest| rest.split(';').next())
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn session_of(reply: &Reply) -> Session {
    let session = set_value(reply, "__Host-colophon_session").expect("a session cookie");
    let csrf = set_value(reply, "__Host-colophon_csrf").expect("a CSRF cookie");
    Session {
        cookie: format!("__Host-colophon_session={session}; __Host-colophon_csrf={csrf}"),
        csrf,
    }
}

async fn me(cookie: &str) -> StatusCode {
    app()
        .get(format!("{ORIGIN}/me"))
        .header(header::COOKIE, cookie)
        .send()
        .await
        .expect("request")
        .status()
}

async fn signed_up(email: &str) -> Reply {
    let reply = post(
        "/auth/signup",
        &json!({"email": email, "password": PASSWORD}),
        &[("origin", ORIGIN)],
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    reply
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn sign_up_and_sign_in_refuse_another_origin_and_a_cross_site_browser() {
    let _server = Server::start().await;
    let host = ORIGIN.trim_start_matches("http://");
    let other_scheme = format!("https://{host}");
    let refused: Vec<Vec<(&str, &str)>> = vec![
        vec![("origin", "null")],
        vec![("origin", "http://evil.example")],
        vec![("origin", other_scheme.as_str())],
        vec![("origin", "http://localhost:1")],
        vec![("origin", ORIGIN), ("sec-fetch-site", "cross-site")],
        vec![("sec-fetch-site", "cross-site")],
        vec![("sec-fetch-site", "same-site")],
    ];
    for headers in &refused {
        let email = fresh_email();
        let reply = post(
            "/auth/signup",
            &json!({"email": email, "password": PASSWORD}),
            headers,
        )
        .await;
        assert_eq!(
            (reply.status, reason(&reply)),
            (StatusCode::FORBIDDEN, Some("origin_not_allowed")),
            "sign-up with {headers:?}: {}",
            reply.body
        );
        assert!(set_cookies(&reply).is_empty(), "no session for {headers:?}");
        let reply = post(
            "/auth/signin",
            &json!({"email": "dev-owner@colophon.test", "password": PASSWORD}),
            headers,
        )
        .await;
        assert_eq!(
            (reply.status, reason(&reply)),
            (StatusCode::FORBIDDEN, Some("origin_not_allowed")),
            "sign-in with {headers:?}: {}",
            reply.body
        );
    }
    // Admitted: this origin; a browser saying same-origin or none; and a client sending neither
    // header (a CLI or a script, which no browser's cross-site write looks like).
    let admitted: Vec<Vec<(&str, &str)>> = vec![
        vec![("origin", ORIGIN), ("sec-fetch-site", "same-origin")],
        vec![("sec-fetch-site", "same-origin")],
        vec![("sec-fetch-site", "none")],
        vec![],
    ];
    for headers in &admitted {
        let email = fresh_email();
        let reply = post(
            "/auth/signup",
            &json!({"email": email, "password": PASSWORD}),
            headers,
        )
        .await;
        assert_eq!(
            reply.status,
            StatusCode::CREATED,
            "{headers:?}: {}",
            reply.body
        );
        let reply = post(
            "/auth/signin",
            &json!({"email": email, "password": PASSWORD}),
            headers,
        )
        .await;
        assert_eq!(
            reply.status,
            StatusCode::NO_CONTENT,
            "{headers:?}: {}",
            reply.body
        );
    }
}

/// Sign-out with a session is a cookie write like any other: it needs this origin and the CSRF
/// token, so another site can't sign a person out either.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn sign_out_with_a_session_needs_this_origin_and_the_csrf_token() {
    let _server = Server::start().await;
    let session = session_of(&signed_up(&fresh_email()).await);
    let cases: [(&str, Vec<(&str, &str)>); 4] = [
        ("no origin", vec![("x-csrf-token", session.csrf.as_str())]),
        (
            "another origin",
            vec![
                ("origin", "http://evil.example"),
                ("x-csrf-token", session.csrf.as_str()),
            ],
        ),
        (
            "null origin",
            vec![("origin", "null"), ("x-csrf-token", session.csrf.as_str())],
        ),
        ("no CSRF token", vec![("origin", ORIGIN)]),
    ];
    for (case, headers) in cases {
        let mut headers = headers;
        headers.push(("cookie", session.cookie.as_str()));
        let reply = post("/auth/logout", &json!({}), &headers).await;
        assert_eq!(
            reply.status,
            StatusCode::FORBIDDEN,
            "{case}: {}",
            reply.body
        );
        assert_eq!(
            me(&session.cookie).await,
            StatusCode::OK,
            "{case}: still signed in"
        );
    }
    let reply = post(
        "/auth/logout",
        &json!({}),
        &[
            ("origin", ORIGIN),
            ("x-csrf-token", session.csrf.as_str()),
            ("cookie", session.cookie.as_str()),
        ],
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(me(&session.cookie).await, StatusCode::UNAUTHORIZED);
}

/// No session fixation: a browser that signs in (or up) gets a new session, and the one it
/// presented, under either name, is over. A session an attacker planted on the victim's browser
/// therefore never becomes the victim's, and stops working for the attacker too.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn signing_in_rotates_the_session_and_ends_the_one_presented() {
    let _server = Server::start().await;
    let attacker_email = fresh_email();
    let planted = session_of(&signed_up(&attacker_email).await);
    let victim_email = fresh_email();
    signed_up(&victim_email).await;

    // The victim's browser carries the attacker's session and signs in as the victim.
    let reply = post(
        "/auth/signin",
        &json!({"email": victim_email, "password": PASSWORD}),
        &[("origin", ORIGIN), ("cookie", planted.cookie.as_str())],
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.body);
    let victim = session_of(&reply);
    assert_ne!(
        victim.cookie, planted.cookie,
        "a new session, not the presented one"
    );
    assert_eq!(
        me(&planted.cookie).await,
        StatusCode::UNAUTHORIZED,
        "the planted one is over"
    );
    assert_eq!(me(&victim.cookie).await, StatusCode::OK);

    // Signing in again over one's own session rotates it as well; so does a sign-up.
    let reply = post(
        "/auth/signin",
        &json!({"email": victim_email, "password": PASSWORD}),
        &[("origin", ORIGIN), ("cookie", victim.cookie.as_str())],
    )
    .await;
    let again = session_of(&reply);
    assert_eq!(me(&victim.cookie).await, StatusCode::UNAUTHORIZED);
    assert_eq!(me(&again.cookie).await, StatusCode::OK);
    let reply = post(
        "/auth/signup",
        &json!({"email": fresh_email(), "password": PASSWORD}),
        &[("origin", ORIGIN), ("cookie", again.cookie.as_str())],
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    assert_eq!(me(&again.cookie).await, StatusCode::UNAUTHORIZED);

    // The log names the rotation.
    let db = owner_database().await;
    let rotated: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM staff_auth_events
         WHERE event = 'session_rotated' AND detail = 'signed_in_again' AND at > now() - interval '1 minute'",
    )
    .fetch_one(&db)
    .await
    .expect("count");
    assert!(rotated >= 3, "{rotated}");
    db.close().await;
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn session_cookies_are_host_only_secure_and_the_session_one_unreadable_to_scripts() {
    let _server = Server::start().await;
    let reply = signed_up(&fresh_email()).await;
    let lines = set_cookies(&reply);
    let session = lines
        .iter()
        .find(|line| line.starts_with("__Host-colophon_session=") && !line.contains("Max-Age=0"))
        .expect("the session cookie");
    let csrf = lines
        .iter()
        .find(|line| line.starts_with("__Host-colophon_csrf=") && !line.contains("Max-Age=0"))
        .expect("the CSRF cookie");
    for (line, http_only, same_site) in [
        (session, true, "SameSite=Lax"),
        (csrf, false, "SameSite=Strict"),
    ] {
        let lower = line.to_ascii_lowercase();
        assert!(line.contains("; Secure"), "{line}");
        assert!(
            line.contains("; Path=/;") || line.ends_with("; Path=/"),
            "{line}"
        );
        assert!(
            !lower.contains("domain="),
            "a __Host- cookie names no Domain: {line}"
        );
        assert!(line.contains(same_site), "{line}");
        assert_eq!(line.contains("HttpOnly"), http_only, "{line}");
    }
    // The old names are cleared, so a browser never carries both.
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("colophon_session=;") && line.contains("Max-Age=0")),
        "{lines:?}"
    );
}

/// The old cookie name, for the one release it is still read: a session made before the rename
/// is accepted once and swapped for a new one under `__Host-`; one made after it, presented
/// under the old name (planted from a sibling subdomain, say), is refused.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn the_old_cookie_name_works_once_and_only_for_a_session_made_before_the_rename() {
    let _server = Server::start().await;
    let db = owner_database().await;

    // Made after the rename: refused under the old name.
    let after = session_of(&signed_up(&fresh_email()).await);
    let token = after
        .cookie
        .split("; ")
        .find_map(|pair| pair.strip_prefix("__Host-colophon_session="))
        .expect("the token")
        .to_owned();
    let old_name = format!("colophon_session={token}; colophon_csrf={}", after.csrf);
    assert_eq!(me(&old_name).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        me(&after.cookie).await,
        StatusCode::OK,
        "it still works under its own name"
    );

    // Made before the rename (aged in the database): accepted once, and swapped.
    let before = session_of(&signed_up(&fresh_email()).await);
    let token = before
        .cookie
        .split("; ")
        .find_map(|pair| pair.strip_prefix("__Host-colophon_session="))
        .expect("the token")
        .to_owned();
    let hash = {
        let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, token.as_bytes());
        digest.as_ref().to_vec()
    };
    sqlx::query(
        "UPDATE sessions SET created_at = (SELECT at FROM session_cookie_rename) - interval '1 day'
         WHERE token_hash = $1",
    )
    .bind(&hash)
    .execute(&db)
    .await
    .expect("age the session");
    let old_name = format!("colophon_session={token}; colophon_csrf={}", before.csrf);
    let response = app()
        .get(format!("{ORIGIN}/me"))
        .header(header::COOKIE, &old_name)
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK);
    let reply = Reply {
        status: response.status(),
        headers: response.headers().clone(),
        body: Value::Null,
    };
    let swapped = set_value(&reply, "__Host-colophon_session").expect("a new-name session");
    assert_ne!(swapped, token, "a new session, not the old token renamed");
    assert_eq!(
        set_value(&reply, "__Host-colophon_csrf").as_deref(),
        Some(before.csrf.as_str()),
        "the swapped session keeps its CSRF token, so an open page keeps working"
    );
    assert!(
        set_cookies(&reply)
            .iter()
            .any(|line| line.starts_with("colophon_session=;") && line.contains("Max-Age=0")),
        "the old cookie is expired"
    );
    // Once: the old token is over, the new one works.
    assert_eq!(me(&old_name).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        me(&format!(
            "__Host-colophon_session={swapped}; __Host-colophon_csrf={}",
            before.csrf
        ))
        .await,
        StatusCode::OK
    );
    db.close().await;
}
