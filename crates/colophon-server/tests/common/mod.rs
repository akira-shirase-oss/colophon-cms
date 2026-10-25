//! The real binary over real HTTP, with sign-in through the local stack's Keycloak.
#![allow(
    dead_code,
    reason = "each test file uses a different subset of these helpers"
)]

use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use reqwest::{Method, StatusCode, header};
use serde_json::Value;

/// The port these tests run the server on; its callback is registered in the development realm.
pub(crate) const PORT: u16 = 58092;
/// The server's public origin.
pub(crate) const ORIGIN: &str = "http://localhost:58092";

/// The real binary, killed when dropped.
pub(crate) struct Server(Child);

impl Server {
    pub(crate) async fn start() -> Self {
        Self::start_with(&[]).await
    }

    /// Starts the server with extra environment variables, such as `COLOPHON_FAULT`.
    pub(crate) async fn start_with(extra: &[(&str, &str)]) -> Self {
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
            .env("COLOPHON_LOG", "warn")
            .envs(extra.iter().copied());
        let server = Self(command.spawn().expect("the server starts"));
        let deadline = Instant::now() + Duration::from_secs(10);
        while reqwest::get(format!("{ORIGIN}/livez")).await.is_err() {
            assert!(Instant::now() < deadline, "the server never answered");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        server
    }

    /// Waits up to five seconds for the process to exit on its own; its exit code.
    pub(crate) async fn exit_code(&mut self) -> Option<i32> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Some(status) = self.0.try_wait().expect("wait") {
                return status.code();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        None
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _already_gone = self.0.kill();
        let _reaped = self.0.wait();
    }
}

/// Colophon's client: no redirects followed, no cookie store (cookies are checked by hand).
pub(crate) fn app() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client")
}

pub(crate) fn cookies_from(response: &reqwest::Response) -> Vec<(String, String)> {
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

pub(crate) fn cookie(cookies: &[(String, String)], name: &str) -> String {
    cookies
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
        .unwrap_or_default()
}

/// A signed-in browser: its cookies and the CSRF token writes must echo.
pub(crate) struct Session {
    pub(crate) cookie: String,
    pub(crate) csrf: String,
}

/// Starts a sign-in and logs in on Keycloak's form; returns the callback URL and the binding
/// cookie, without completing the callback.
pub(crate) async fn reach_callback(user: &str) -> (String, String) {
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

pub(crate) async fn complete(callback: &str, binding: &str) -> reqwest::Response {
    app()
        .get(callback)
        .header(header::COOKIE, format!("colophon_signin={binding}"))
        .send()
        .await
        .expect("callback")
}

pub(crate) async fn sign_in(user: &str) -> Session {
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

pub(crate) async fn get(session: Option<&Session>, path: &str) -> (StatusCode, Value) {
    let mut request = app().get(format!("{ORIGIN}{path}"));
    if let Some(session) = session {
        request = request.header(header::COOKIE, &session.cookie);
    }
    let response = request.send().await.expect("request");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

pub(crate) async fn post(
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

pub(crate) fn unique_slug() -> String {
    format!("t-{}", &uuid::Uuid::now_v7().simple().to_string()[20..])
}

/// A response: status, headers and JSON body (`Null` when there is none).
pub(crate) struct Reply {
    pub(crate) status: StatusCode,
    pub(crate) headers: header::HeaderMap,
    pub(crate) body: Value,
}

impl Reply {
    /// Whether the server answered with a recorded response (`Idempotent-Replayed: true`).
    pub(crate) fn replayed(&self) -> bool {
        self.headers
            .get("idempotent-replayed")
            .is_some_and(|value| value == "true")
    }

    /// The `ETag` header, if any.
    pub(crate) fn etag(&self) -> Option<String> {
        self.headers
            .get(header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    }
}

/// A cookie-authenticated request from the server's own origin, with the CSRF token for writes,
/// plus any extra headers (`If-Match`, `Idempotency-Key`).
pub(crate) async fn call(
    session: &Session,
    method: Method,
    path: &str,
    body: Option<&Value>,
    extra: &[(&str, &str)],
) -> Result<Reply, reqwest::Error> {
    let mut request = app()
        .request(method, format!("{ORIGIN}{path}"))
        .header(header::COOKIE, &session.cookie)
        .header("x-csrf-token", &session.csrf)
        .header(header::ORIGIN, ORIGIN);
    for (name, value) in extra {
        request = request.header(*name, *value);
    }
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request.send().await?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json().await.unwrap_or(Value::Null);
    Ok(Reply {
        status,
        headers,
        body,
    })
}

/// A fresh idempotency key.
pub(crate) fn key() -> String {
    format!("test-{}", uuid::Uuid::now_v7().simple())
}

/// An article with a one-paragraph body.
pub(crate) fn article(title: &str, text: &str) -> Value {
    serde_json::json!({
        "title": title,
        "body": {"schema": "colophon/rich-text", "version": 1, "doc": {"type": "doc", "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": text}]}
        ]}}
    })
}

/// Creates a project as `session`; its id.
pub(crate) async fn project(session: &Session) -> String {
    let slug = unique_slug();
    let reply = call(
        session,
        Method::POST,
        "/api/v1/projects",
        Some(&serde_json::json!({"slug": slug, "name": "Test project"})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    reply.body["data"]["id"].as_str().expect("an id").to_owned()
}

/// The entries path of `project`'s production environment.
pub(crate) fn entries(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production/collections/articles/entries")
}
