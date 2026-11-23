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

use common::{
    ORIGIN, Server, Session, app, call, cookie, get, post, project, sign_in, unique_slug,
};
use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};

/// A fresh invitation's token to one of dev-owner's projects: what an invite-only installation
/// takes a sign-up with (D-235).
async fn an_invitation() -> String {
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let invited = call(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/invitations"),
        Some(&json!({"role": "editor", "label": "Erin"})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(invited.status, StatusCode::CREATED, "{}", invited.body);
    let link = invited.body["data"]["link"].as_str().expect("a link");
    link.split_once('#')
        .expect("the token after #")
        .1
        .to_owned()
}

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
        cookie(cookies, "__Host-colophon_session"),
        cookie(cookies, "__Host-colophon_csrf"),
    );
    assert_eq!((session.len(), csrf.len()), (43, 43), "{cookies:?}");
    Session {
        cookie: format!("__Host-colophon_session={session}; __Host-colophon_csrf={csrf}"),
        csrf,
    }
}

fn an_email() -> String {
    format!("{}@example.test", unique_slug())
}

/// `GET /auth/providers` tells the sign-in page which methods this server offers — public, so
/// a signed-out browser can ask (D-141). The default stack configures no provider since
/// D-146, so `sso` is false; the OIDC suite (`sign_in.rs`) covers `true`.
#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn the_sign_in_page_asks_which_methods_this_server_offers() {
    for mode in ["open", "invite", "closed"] {
        let _server = Server::start_with(&[("COLOPHON_REGISTRATION", mode)]).await;
        let (status, body) = get(None, "/auth/providers").await;
        assert_eq!(
            (status, body["data"]["sso"].as_bool()),
            (StatusCode::OK, Some(false)),
            "{body}"
        );
        // The page shows "closed" or asks for the invitation before anyone fills the form.
        assert_eq!(body["data"]["registration"], mode, "{body}");
    }
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn an_email_account_signs_up_and_uses_its_session() {
    // Invite-only: there, an account that isn't an administrator can't create projects (D-235;
    // the development stack runs open).
    let _server = Server::start_with(&[("COLOPHON_REGISTRATION", "invite")]).await;
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

    // Signing up makes the account and starts its session at once; on this invite-only
    // installation it claims an invitation as it does (D-235).
    let (status, response, _) = post_auth(
        "/auth/signup",
        &json!({
            "email": email,
            "password": password,
            "display_name": "Erin",
            "invitation": an_invitation().await,
        }),
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
        raw.iter().any(|c| c.starts_with("__Host-colophon_session=")
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

    // The email can't sign up a second account, in any casing, even with a valid invitation.
    let (status, _, body) = post_auth(
        "/auth/signup",
        &json!({
            "email": email.to_uppercase(),
            "password": "a different password",
            "invitation": an_invitation().await,
        }),
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

/// `POST /auth/signin` as a client at `address`: the test server trusts loopback as a proxy
/// (`COLOPHON_TRUSTED_PROXIES`), so `X-Forwarded-For` says who the client is.
async fn sign_in_from(address: &str, body: &Value) -> (StatusCode, Value) {
    let response = app()
        .post(format!("{ORIGIN}/auth/signin"))
        .header("x-forwarded-for", address)
        .json(body)
        .send()
        .await
        .expect("request");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

/// Guessing pauses the guesser, not the account (D-154): eight misses from one address pause
/// that (account, address) key for fifteen minutes — the right password from there is then
/// refused with the very answer a wrong one gets, the same answer an address with no account
/// gets after the same guesses — while the owner at another address signs straight in. The
/// pause is bounded: once it runs out, the count starts over.
#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn misses_pause_the_guessing_address_never_the_account() {
    let _server = Server::start_with(&[("COLOPHON_TRUSTED_PROXIES", "127.0.0.1")]).await;
    let email = an_email();
    let (status, _, _) = post_auth(
        "/auth/signup",
        &json!({"email": email, "password": "the real password"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let nobody = an_email();
    let failed = (StatusCode::BAD_REQUEST, json!("sign_in_failed"));

    // Eight misses at the account and eight at an address with no account, from one place.
    let guesser = "198.51.100.7";
    for miss in 1..=8 {
        for target in [&email, &nobody] {
            let (status, body) =
                sign_in_from(guesser, &json!({"email": target, "password": "a guess"})).await;
            assert_eq!(
                (status, body["error"]["code"].clone()),
                failed,
                "miss {miss}: {body}"
            );
        }
    }
    // Paused there: even the right password is refused — exactly as a wrong one, and exactly
    // as the address with no account, so the pause tells the guesser nothing.
    for (target, password) in [
        (&email, "the real password"),
        (&email, "a guess"),
        (&nobody, "a guess"),
    ] {
        let (status, body) =
            sign_in_from(guesser, &json!({"email": target, "password": password})).await;
        assert_eq!((status, body["error"]["code"].clone()), failed, "{body}");
        assert!(
            body["error"]["details"].is_null(),
            "no distinct reason: {body}"
        );
    }

    // The owner, somewhere else, is not locked out.
    let (status, body) = sign_in_from(
        "203.0.113.20",
        &json!({"email": email, "password": "the real password"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    // A pause runs out (aged here rather than waited for), and the right password works from
    // the guesser's address again.
    let owner = common::owner_database().await;
    sqlx::query(
        "UPDATE sign_in_throttle SET locked_until = now() - interval '1 second' WHERE email = $1",
    )
    .bind(&email)
    .execute(&owner)
    .await
    .expect("aged");
    owner.close().await;
    let (status, body) = sign_in_from(
        guesser,
        &json!({"email": email, "password": "the real password"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}

/// Password work is bounded and never starves the database (D-150): Argon2id is 19 MiB and
/// tens of milliseconds of a core that anyone can ask for, so the server runs a fixed number
/// of password jobs at once, lets a bounded number wait briefly, sheds the rest with `503`
/// and `Retry-After` — on sign-in for a missing account too, whose dummy verify costs the
/// same — and verifies outside any transaction, so a flood of sign-ins holds no pooled
/// connection that delivery needs. A password longer than any account can hold is refused
/// before any work at all.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_sign_in_flood_is_shed_with_busy_and_leaves_delivery_its_connections() {
    // One password slot and one pooled connection: the flood and the reads compete for
    // exactly the resources the bound is about.
    let _server = Server::start_with(&[
        ("COLOPHON_PASSWORD_CONCURRENCY", "1"),
        ("COLOPHON_DB_MAX_CONNECTIONS", "1"),
    ])
    .await;

    // Longer than any account's password can be (512 bytes): refused up front.
    let long = "x".repeat(513);
    let (status, _, body) = post_auth(
        "/auth/signin",
        &json!({"email": an_email(), "password": long}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["details"]["reason"].as_str(),
        Some("password_too_long"),
        "{body}"
    );

    // A flood of sign-ins for nobody — each a full Argon2 verify — while delivery reads
    // (each a Postgres lookup, a 404 for a project that doesn't exist) keep arriving.
    let attempts = (0..160)
        .map(|_| {
            let body = json!({"email": an_email(), "password": "a guess at nothing"});
            tokio::spawn(async move {
                let response = app()
                    .post(format!("{ORIGIN}/auth/signin"))
                    .json(&body)
                    .send()
                    .await
                    .expect("request");
                let retry = response.headers().get(header::RETRY_AFTER).cloned();
                let status = response.status();
                let body: Value = response.json().await.unwrap_or(Value::Null);
                (status, retry, body)
            })
        })
        .collect::<Vec<_>>();
    let reads = tokio::spawn(async {
        let mut statuses = Vec::new();
        for _ in 0..40 {
            let project = uuid_like();
            let response = app()
                .get(format!("{ORIGIN}/content/v1/{project}/production/articles"))
                .send()
                .await
                .expect("request");
            statuses.push(response.status());
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        statuses
    });
    let mut refused = 0;
    let mut busy = 0;
    for attempt in attempts {
        let (status, retry, body) = attempt.await.expect("joined");
        match status {
            StatusCode::BAD_REQUEST => {
                assert_eq!(
                    body["error"]["code"].as_str(),
                    Some("sign_in_failed"),
                    "{body}"
                );
                refused += 1;
            }
            StatusCode::SERVICE_UNAVAILABLE => {
                assert!(retry.is_some(), "a shed request says when to retry: {body}");
                busy += 1;
            }
            other => panic!("unexpected {other}: {body}"),
        }
    }
    let reads = reads.await.expect("joined");
    assert!(
        reads.iter().all(|status| *status == StatusCode::NOT_FOUND),
        "delivery kept its connection through the flood: {reads:?}"
    );
    assert!(
        busy > 0,
        "with one slot, a flood of 160 is shed in part ({refused} refused)"
    );
    assert!(refused > 0, "and the slot still serves ({busy} busy)");

    // The server is still itself afterwards: a real account signs up and in.
    let email = an_email();
    let (status, _, body) = post_auth(
        "/auth/signup",
        &json!({"email": email, "password": "the right password"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, _, body) = post_auth(
        "/auth/signin",
        &json!({"email": email, "password": "the right password"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}

/// A project id the server has never seen.
fn uuid_like() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Guessing from many addresses is bounded per account too (D-162): fifty misses within an
/// hour from addresses that never signed in to the account pause every such address for it —
/// one answer, a wrong password's — while an address the owner already signed in from keeps
/// working, so the ceiling can't be turned into a lockout of the owner.
#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn misses_from_many_addresses_hit_a_per_account_ceiling_the_owner_bypasses() {
    let _server = Server::start_with(&[("COLOPHON_TRUSTED_PROXIES", "127.0.0.1")]).await;
    let email = an_email();
    let (status, _, _) = post_auth(
        "/auth/signup",
        &json!({"email": email, "password": "the real password"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let owner_address = "2001:db8:aa::1";
    let right = json!({"email": email, "password": "the real password"});
    let (status, body) = sign_in_from(owner_address, &right).await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "the owner's address, now known: {body}"
    );

    // One guess each from fifty fresh /64s: no single address is near its own pause.
    for n in 0..50 {
        let (status, body) = sign_in_from(
            &format!("2001:db8:{n:x}:{n:x}::1"),
            &json!({"email": email, "password": "a guess"}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "guess {n}: {body}");
    }
    // A fresh address is paused for this account now — even with the right password, and
    // with exactly a wrong password's answer.
    let (status, body) = sign_in_from("2001:db8:ff:ff::1", &right).await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::BAD_REQUEST, json!("sign_in_failed")),
        "{body}"
    );
    // The owner's known address is not.
    let (status, body) = sign_in_from(owner_address, &right).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}

/// The signed-in hint's `Set-Cookie`, if the response carries one.
fn hint_of(headers: &header::HeaderMap) -> Option<String> {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|value| value.starts_with("colophon_signed_in="))
        .map(str::to_owned)
}

/// A request with exactly these cookies (and, for a write, the hint server's origin and `csrf`).
async fn with_cookies(
    method: Method,
    path: &str,
    cookies: &str,
    csrf: Option<&str>,
) -> reqwest::Response {
    let mut request = app()
        .request(method, format!("{ORIGIN}{path}"))
        .header(header::COOKIE, cookies);
    if let Some(csrf) = csrf {
        request = request
            .header(header::ORIGIN, format!("http://127.0.0.1:{}", common::PORT))
            .header("x-csrf-token", csrf)
            .json(&json!({}));
    }
    request.send().await.expect("request")
}

/// The signed-in hint (D-235): set with every new session, refreshed when a session is touched,
/// cleared on sign-out and on any refusal of a presented session or hint, readable by scripts,
/// and worth nothing on its own. The public host is 127.0.0.1 here so a hint domain can cover it.
#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn the_signed_in_hint_follows_the_session_and_authorises_nothing() {
    let public = format!("http://127.0.0.1:{}", common::PORT);
    let _server = Server::start_with(&[
        ("COLOPHON_REGISTRATION", "open"),
        ("COLOPHON_PUBLIC_URL", public.as_str()),
        ("COLOPHON_SIGNED_IN_HINT_DOMAIN", "127.0.0.1"),
    ])
    .await;
    let set = "colophon_signed_in=1; Domain=127.0.0.1; Path=/; Max-Age=7200; Secure; SameSite=Lax";
    let cleared = "colophon_signed_in=1; Domain=127.0.0.1; Path=/; Max-Age=0; Secure; SameSite=Lax";

    let email = an_email();
    let (status, headers, body) = post_auth(
        "/auth/signup",
        &json!({"email": email, "password": "colophon-dev"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        hint_of(&headers).as_deref(),
        Some(set),
        "sign-up sets the hint"
    );
    let (status, headers, body) = post_auth(
        "/auth/signin",
        &json!({"email": email, "password": "colophon-dev"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(hint_of(&headers).as_deref(), Some(set), "sign-in sets it");
    let session = session_from(&cookies_from_headers(&headers));

    // Refreshed when the session is touched, at most once a minute.
    let quiet = with_cookies(Method::GET, "/me", &session.cookie, None).await;
    assert_eq!(
        (quiet.status(), hint_of(quiet.headers())),
        (StatusCode::OK, None)
    );
    sqlx::query(
        "UPDATE sessions SET last_seen_at = now() - interval '2 minutes'
         WHERE identity_id = (SELECT id FROM identities WHERE subject = $1) AND revoked_at IS NULL",
    )
    .bind(&email)
    .execute(&common::owner_database().await)
    .await
    .expect("age the session");
    let touched = with_cookies(Method::GET, "/me", &session.cookie, None).await;
    assert_eq!(touched.status(), StatusCode::OK);
    assert_eq!(
        hint_of(touched.headers()).as_deref(),
        Some(set),
        "a touch refreshes it"
    );

    // The hint alone is nobody: 401 everywhere, and each refusal clears it.
    for (method, path) in [
        (Method::GET, "/me"),
        (Method::GET, "/api/v1/me/usage"),
        (Method::GET, "/api/v1/projects"),
        (Method::POST, "/api/v1/projects"),
    ] {
        let csrf = (method == Method::POST).then_some("x");
        let reply = with_cookies(method.clone(), path, "colophon_signed_in=1", csrf).await;
        assert_eq!(reply.status(), StatusCode::UNAUTHORIZED, "{method} {path}");
        assert_eq!(
            hint_of(reply.headers()).as_deref(),
            Some(cleared),
            "{method} {path}"
        );
    }

    // Sign-out clears it.
    let out = with_cookies(
        Method::POST,
        "/auth/logout",
        &session.cookie,
        Some(&session.csrf),
    )
    .await;
    assert!(out.status().is_success(), "{}", out.status());
    assert_eq!(
        hint_of(out.headers()).as_deref(),
        Some(cleared),
        "sign-out clears it"
    );

    // A session refused later (here, revoked) clears it on its next request.
    let (_, headers, _) = post_auth(
        "/auth/signin",
        &json!({"email": email, "password": "colophon-dev"}),
    )
    .await;
    let again = session_from(&cookies_from_headers(&headers));
    sqlx::query("UPDATE sessions SET revoked_at = now() WHERE identity_id = (SELECT id FROM identities WHERE subject = $1)")
        .bind(&email)
        .execute(&common::owner_database().await)
        .await
        .expect("revoke");
    let refused = with_cookies(Method::GET, "/me", &again.cookie, None).await;
    assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        hint_of(refused.headers()).as_deref(),
        Some(cleared),
        "a refused session clears it"
    );
}
