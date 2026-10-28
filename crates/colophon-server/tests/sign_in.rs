//! Sign-in end to end: the real binary, the local stack's Keycloak, its real login form, and
//! every check that protects a session, over HTTP. Needs the stack (scripts/stack.sh up) and
//! migrations (scripts/migrate.sh).
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{ORIGIN, Server, app, complete, get, post, reach_callback, sign_in, unique_slug};
use reqwest::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn sign_in_works_and_every_write_needs_csrf_and_a_trusted_origin() {
    let _server = Server::start().await;

    // No session: 401, in the envelope.
    let (status, body) = get(None, "/me").await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("unauthorized"))
    );

    let editor = sign_in("dev-owner").await;
    let (status, me) = get(Some(&editor), "/me").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["data"]["display_name"], "Dev Owner");
    assert_eq!(
        me["data"]["issuer"],
        "http://localhost:58080/realms/colophon"
    );

    // Writes need a CSRF token and a trusted Origin.
    let slug = unique_slug();
    let body = json!({ "slug": slug, "name": "Editor's site" });
    let (status, error) = post(&editor, "/api/v1/projects", &body, None, Some(ORIGIN)).await;
    assert_eq!(
        (status, error["error"]["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("forbidden")),
        "no CSRF token"
    );
    let (status, _) = post(
        &editor,
        "/api/v1/projects",
        &body,
        Some(&editor.csrf),
        Some("https://evil.example"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "untrusted origin");
    let (status, _) = post(&editor, "/api/v1/projects", &body, Some(&editor.csrf), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "no origin");
    let (status, _) = post(
        &editor,
        "/api/v1/projects",
        &body,
        Some("forged-token-forged-token-forged-token-1234"),
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "wrong CSRF token");
    let (status, created) = post(
        &editor,
        "/api/v1/projects",
        &body,
        Some(&editor.csrf),
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["data"]["role"], "owner");

    // The same slug again is a conflict; a bad slug says which field and why.
    let (status, error) = post(
        &editor,
        "/api/v1/projects",
        &body,
        Some(&editor.csrf),
        Some(ORIGIN),
    )
    .await;
    assert_eq!(
        (status, error["error"]["code"].as_str()),
        (StatusCode::CONFLICT, Some("conflict"))
    );
    let (status, error) = post(
        &editor,
        "/api/v1/projects",
        &json!({ "slug": "Not A Slug", "name": "x" }),
        Some(&editor.csrf),
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        error["error"]["details"],
        json!({ "reason": "invalid_slug", "pointer": "/slug" })
    );

    // Logging out ends the session.
    let (status, _) = post(
        &editor,
        "/auth/logout",
        &json!({}),
        Some(&editor.csrf),
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = get(Some(&editor), "/me").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_project_is_invisible_to_people_who_are_not_members() {
    let _server = Server::start().await;
    let editor = sign_in("dev-owner").await;
    let slug = unique_slug();
    let body = json!({ "slug": slug, "name": "Editor's site" });
    let (status, created) = post(
        &editor,
        "/api/v1/projects",
        &body,
        Some(&editor.csrf),
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let project_id = created["data"]["id"].as_str().expect("an id").to_owned();

    // Another person can't see it, by listing or by id.
    let other = sign_in("dev-editor").await;
    let (status, error) = get(Some(&other), &format!("/api/v1/projects/{project_id}")).await;
    assert_eq!(
        (status, error["error"]["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("not_found"))
    );
    let (_, list) = get(Some(&other), "/api/v1/projects").await;
    assert!(
        !list.to_string().contains(&project_id),
        "not listed for a non-member"
    );
    let (status, _) = get(Some(&other), "/api/v1/projects/not-a-uuid").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, mine) = get(Some(&editor), &format!("/api/v1/projects/{project_id}")).await;
    assert_eq!(
        (status, mine["data"]["slug"].as_str()),
        (StatusCode::OK, Some(slug.as_str()))
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_callback_works_once_and_only_in_the_browser_that_started_it() {
    let _server = Server::start().await;
    let (callback, binding) = reach_callback("dev-owner").await;

    // Login CSRF: someone else's browser (no binding cookie, or another one) can't complete it.
    let stranger = app().get(&callback).send().await.expect("callback");
    let body: Value = stranger.json().await.expect("json");
    assert_eq!(body["error"]["code"], "sign_in_failed");
    let other = complete(&callback, "a-different-binding-cookie-value-1234567890").await;
    assert_eq!(other.status(), StatusCode::BAD_REQUEST);

    // The right browser completes it, once.
    assert_eq!(
        complete(&callback, &binding).await.status(),
        StatusCode::SEE_OTHER
    );
    let replay = complete(&callback, &binding).await;
    assert_eq!(
        replay.status(),
        StatusCode::BAD_REQUEST,
        "a sign-in state works once"
    );
}

/// A person who makes an account at the provider through `/auth/login?start=create` comes back
/// signed in, and the account grants them nothing (D-043, D-061): no projects, no administrator,
/// and a project of their own refused.
#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_new_account_signs_in_and_grants_nothing() {
    let _server = Server::start().await;
    let response = app()
        .get(format!("{ORIGIN}/auth/login?start=create&return_to=/me"))
        .send()
        .await
        .expect("login");
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let binding = common::cookie(&common::cookies_from(&response), "colophon_signin");
    let provider = response.headers()[reqwest::header::LOCATION]
        .to_str()
        .expect("ascii")
        .to_owned();
    assert!(provider.contains("prompt=create"), "{provider}");

    let keycloak = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client");
    let page = keycloak
        .get(&provider)
        .send()
        .await
        .expect("sign-up page")
        .text()
        .await
        .expect("html");
    assert!(
        page.contains("kc-register-form"),
        "the provider's sign-up form"
    );
    let action = page
        .split("id=\"kc-register-form\"")
        .nth(1)
        .and_then(|rest| rest.split("action=\"").nth(1))
        .and_then(|rest| rest.split('"').next())
        .expect("a form action")
        .replace("&amp;", "&");
    let name = format!("new-{}", &uuid::Uuid::now_v7().simple().to_string()[20..]);
    let response = keycloak
        .post(&action)
        .form(&[
            ("username", name.as_str()),
            ("email", &format!("{name}@example.test")),
            ("firstName", "New"),
            ("lastName", "Person"),
            ("password", "a-long-password-1"),
            ("password-confirm", "a-long-password-1"),
        ])
        .send()
        .await
        .expect("sign up");
    assert_eq!(
        response.status(),
        StatusCode::FOUND,
        "Keycloak made the account"
    );
    let callback = response.headers()[reqwest::header::LOCATION]
        .to_str()
        .expect("ascii")
        .to_owned();
    let response = complete(&callback, &binding).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let set = common::cookies_from(&response);
    let session = common::Session {
        cookie: format!(
            "colophon_session={}; colophon_csrf={}",
            common::cookie(&set, "colophon_session"),
            common::cookie(&set, "colophon_csrf")
        ),
        csrf: common::cookie(&set, "colophon_csrf"),
    };
    let (status, me) = get(Some(&session), "/me").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["data"]["display_name"], "New Person");
    assert_eq!(me["data"]["administrator"], false);
    assert_eq!(me["data"]["projects"], json!([]));
    let (status, _) = post(
        &session,
        "/api/v1/projects",
        &json!({"slug": unique_slug(), "name": "Mine"}),
        Some(&session.csrf),
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "an account grants nothing");
}

/// A browser at Keycloak: signs `user` in through `/auth/login` with its own Keycloak cookies, so
/// the provider's session lives on between sign-ins as it would in a real browser. The Colophon
/// session, as the Studio holds it.
async fn sign_in_keeping(keycloak: &reqwest::Client, user: &str) -> common::Session {
    let response = app()
        .get(format!("{ORIGIN}/auth/login?return_to=/me"))
        .send()
        .await
        .expect("login");
    let binding = common::cookie(&common::cookies_from(&response), "colophon_signin");
    let provider = response.headers()[reqwest::header::LOCATION]
        .to_str()
        .expect("ascii")
        .to_owned();
    let page = keycloak.get(&provider).send().await.expect("provider");
    let reached = if page.status() == StatusCode::FOUND {
        page
    } else {
        let html = page.text().await.expect("html");
        let action = html
            .split("action=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("a login form")
            .replace("&amp;", "&");
        keycloak
            .post(&action)
            .form(&[("username", user), ("password", "colophon-dev")])
            .send()
            .await
            .expect("credentials")
    };
    let callback = reached.headers()[reqwest::header::LOCATION]
        .to_str()
        .expect("ascii")
        .to_owned();
    let done = common::complete(&callback, &binding).await;
    assert_eq!(done.status(), StatusCode::SEE_OTHER);
    let set = common::cookies_from(&done);
    let (session, csrf) = (
        common::cookie(&set, "colophon_session"),
        common::cookie(&set, "colophon_csrf"),
    );
    common::Session {
        cookie: format!("colophon_session={session}; colophon_csrf={csrf}"),
        csrf,
    }
}

/// Whether Keycloak, asked to sign this browser in again, asks for a password (its form) rather
/// than handing back a code at once.
async fn provider_asks_for_a_password(keycloak: &reqwest::Client) -> bool {
    let response = app()
        .get(format!("{ORIGIN}/auth/login?return_to=/me"))
        .send()
        .await
        .expect("login");
    let provider = response.headers()[reqwest::header::LOCATION]
        .to_str()
        .expect("ascii")
        .to_owned();
    let page = keycloak.get(&provider).send().await.expect("provider");
    page.status() == StatusCode::OK && page.text().await.expect("html").contains("kc-form-login")
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn signing_out_also_ends_the_session_at_the_identity_provider() {
    let _server = Server::start().await;
    let keycloak = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client");
    let editor = sign_in_keeping(&keycloak, "dev-editor").await;
    // The gap this closes: at Keycloak this browser is still signed in, so the next sign-in here
    // needs no password, whoever sits at the computer.
    assert!(!provider_asks_for_a_password(&keycloak).await);

    // Signing out here ends Colophon's session at once, and says where to end the provider's.
    let (status, body) = post(
        &editor,
        "/auth/logout",
        &json!({}),
        Some(&editor.csrf),
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let url = body["data"]["provider_sign_out"]
        .as_str()
        .expect("where to sign out at the provider")
        .to_owned();
    assert!(
        url.starts_with("http://localhost:58080/realms/colophon/protocol/openid-connect/logout?")
            && url.contains("client_id=colophon-server")
            && url.contains(&format!(
                "post_logout_redirect_uri={}",
                urlencode(&format!("{ORIGIN}/studio/"))
            )),
        "{url}"
    );

    // Following it (Keycloak asks first, with no ID token hint) ends the provider's session and
    // comes back to the Studio's front door.
    let confirm = keycloak.get(&url).send().await.expect("provider sign-out");
    let html = confirm.text().await.expect("html");
    let action = html
        .split("action=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("the provider's confirmation form")
        .replace("&amp;", "&");
    // Keycloak's form names a path on its own origin.
    let action = if action.starts_with('/') {
        format!("http://localhost:58080{action}")
    } else {
        action
    };
    let session_code = html
        .split("name=\"session_code\" value=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or_default()
        .to_owned();
    let back = keycloak
        .post(&action)
        .form(&[
            ("session_code", session_code.as_str()),
            ("confirmLogout", "Logout"),
        ])
        .send()
        .await
        .expect("confirm");
    assert_eq!(back.status(), StatusCode::FOUND, "{action}");
    assert_eq!(
        back.headers()[reqwest::header::LOCATION],
        format!("{ORIGIN}/studio/").as_str()
    );
    // Now this browser must give a password again, and the Colophon session is gone.
    assert!(provider_asks_for_a_password(&keycloak).await);
    let (status, _) = get(Some(&editor), "/me").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// `text` percent-encoded the way a query string carries it.
fn urlencode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}
