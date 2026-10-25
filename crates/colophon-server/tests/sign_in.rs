//! Sign-in end to end: the real binary, the local stack's Keycloak, its real login form, and
//! every check that protects a session, over HTTP. Needs the stack (scripts/stack.sh up) and
//! migrations (scripts/migrate.sh).
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use reqwest::{StatusCode, header};
use serde_json::{Value, json};

const PORT: u16 = 58092;
const ORIGIN: &str = "http://localhost:58092";

struct Server(Child);

impl Server {
    async fn start() -> Self {
        let env_file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/local.env");
        let mut command = Command::new(env!("CARGO_BIN_EXE_colophon-server"));
        command
            .env_clear()
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        for line in std::fs::read_to_string(env_file)
            .expect("deploy/local.env")
            .lines()
        {
            if let Some((key, value)) = line.split_once('=')
                && !key.starts_with('#')
            {
                command.env(key.trim(), value.trim());
            }
        }
        command
            .env("COLOPHON_LISTEN_ADDR", format!("127.0.0.1:{PORT}"))
            .env("COLOPHON_PUBLIC_URL", ORIGIN)
            .env("COLOPHON_LOG", "warn");
        let server = Self(command.spawn().expect("the server starts"));
        let deadline = Instant::now() + Duration::from_secs(10);
        while reqwest::get(format!("{ORIGIN}/livez")).await.is_err() {
            assert!(Instant::now() < deadline, "the server never answered");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        server
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _already_gone = self.0.kill();
        let _reaped = self.0.wait();
    }
}

/// Colophon's client: no redirects followed, no cookie store (cookies are checked by hand).
fn app() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client")
}

fn cookies_from(response: &reqwest::Response) -> Vec<(String, String)> {
    response
        .headers()
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

fn cookie(cookies: &[(String, String)], name: &str) -> String {
    cookies
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
        .unwrap_or_default()
}

struct Session {
    cookie: String,
    csrf: String,
}

/// Starts a sign-in and logs in on Keycloak's form; returns the callback URL and the binding
/// cookie, without completing the callback.
async fn reach_callback(user: &str) -> (String, String) {
    let response = app()
        .get(format!("{ORIGIN}/auth/login?return_to=/me"))
        .send()
        .await
        .expect("login");
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let binding = cookie(&cookies_from(&response), "colophon_signin");
    assert_eq!(binding.len(), 43, "a sign-in binding cookie");
    let provider = response.headers()[header::LOCATION]
        .to_str()
        .expect("ascii")
        .to_owned();

    let keycloak = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client");
    let page = keycloak
        .get(&provider)
        .send()
        .await
        .expect("login page")
        .text()
        .await
        .expect("html");
    let action = page
        .split("action=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("a login form")
        .replace("&amp;", "&");
    let response = keycloak
        .post(&action)
        .form(&[("username", user), ("password", "colophon-dev")])
        .send()
        .await
        .expect("credentials");
    assert_eq!(
        response.status(),
        StatusCode::FOUND,
        "Keycloak accepts the credentials"
    );
    let callback = response.headers()[header::LOCATION]
        .to_str()
        .expect("ascii")
        .to_owned();
    assert!(
        callback.starts_with(&format!("{ORIGIN}/auth/callback?")),
        "{callback}"
    );
    (callback, binding)
}

async fn complete(callback: &str, binding: &str) -> reqwest::Response {
    app()
        .get(callback)
        .header(header::COOKIE, format!("colophon_signin={binding}"))
        .send()
        .await
        .expect("callback")
}

async fn sign_in(user: &str) -> Session {
    let (callback, binding) = reach_callback(user).await;
    let response = complete(&callback, &binding).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/me");
    let set = cookies_from(&response);
    let (session, csrf) = (
        cookie(&set, "colophon_session"),
        cookie(&set, "colophon_csrf"),
    );
    assert_eq!((session.len(), csrf.len()), (43, 43));
    let raw: Vec<_> = response
        .headers()
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
    Session {
        cookie: format!("colophon_session={session}; colophon_csrf={csrf}"),
        csrf,
    }
}

async fn get(session: Option<&Session>, path: &str) -> (StatusCode, Value) {
    let mut request = app().get(format!("{ORIGIN}{path}"));
    if let Some(session) = session {
        request = request.header(header::COOKIE, &session.cookie);
    }
    let response = request.send().await.expect("request");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

async fn post(
    session: &Session,
    path: &str,
    body: &Value,
    csrf: Option<&str>,
    origin: Option<&str>,
) -> (StatusCode, Value) {
    let mut request = app()
        .post(format!("{ORIGIN}{path}"))
        .header(header::COOKIE, &session.cookie)
        .json(body);
    if let Some(csrf) = csrf {
        request = request.header("x-csrf-token", csrf);
    }
    if let Some(origin) = origin {
        request = request.header(header::ORIGIN, origin);
    }
    let response = request.send().await.expect("request");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

fn unique_slug() -> String {
    format!("t-{}", &uuid::Uuid::now_v7().simple().to_string()[20..])
}

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

    let editor = sign_in("dev-editor").await;
    let (status, me) = get(Some(&editor), "/me").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["display_name"], "Dev Editor");
    assert_eq!(me["issuer"], "http://localhost:58080/realms/colophon");

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
    assert_eq!(created["role"], "owner");

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
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = get(Some(&editor), "/me").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_project_is_invisible_to_people_who_are_not_members() {
    let _server = Server::start().await;
    let editor = sign_in("dev-editor").await;
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
    let project_id = created["id"].as_str().expect("an id").to_owned();

    // Another person can't see it, by listing or by id.
    let owner = sign_in("dev-owner").await;
    let (status, error) = get(Some(&owner), &format!("/api/v1/projects/{project_id}")).await;
    assert_eq!(
        (status, error["error"]["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("not_found"))
    );
    let (_, list) = get(Some(&owner), "/api/v1/projects").await;
    assert!(
        !list.to_string().contains(&project_id),
        "not listed for a non-member"
    );
    let (status, _) = get(Some(&owner), "/api/v1/projects/not-a-uuid").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, mine) = get(Some(&editor), &format!("/api/v1/projects/{project_id}")).await;
    assert_eq!(
        (status, mine["slug"].as_str()),
        (StatusCode::OK, Some(slug.as_str()))
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_callback_works_once_and_only_in_the_browser_that_started_it() {
    let _server = Server::start().await;
    let (callback, binding) = reach_callback("dev-editor").await;

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
