//! The CLI checks the server's `/version` before it does anything else, and says in plain words
//! when it is too old or too new for that server (D-062; `docs/modules/08-cli.md`, "Versions").
//! Each test runs the real binary against a canned server that records what was asked of it.
#![expect(
    clippy::expect_used,
    reason = "process-level tests: failing to bind, run or parse is a test failure"
)]

use std::{
    io::{Read as _, Write as _},
    net::TcpListener,
    path::PathBuf,
    process::{Command, Output},
    sync::{Arc, Mutex},
    thread,
};

use semver::Version;
use serde_json::{Value, json};

/// A canned server: one answer per path, 404 with the error envelope for any other path, and a
/// log of the paths it was asked for (marked when the request carried a token).
struct Server {
    url: String,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn start(routes: Vec<(&'static str, &'static str, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let asked = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&asked);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut request = [0_u8; 8192];
                let read = stream.read(&mut request).unwrap_or(0);
                let head = String::from_utf8_lossy(&request[..read]);
                let path = head.split(' ').nth(1).unwrap_or("").to_owned();
                let token = head.to_ascii_lowercase().contains("\r\nauthorization:");
                log.lock().expect("log").push(format!(
                    "{path}{}",
                    if token { " (with a token)" } else { "" }
                ));
                let (status, body) = routes
                    .iter()
                    .find(|(route, _, _)| *route == path)
                    .map_or_else(
                        || {
                            (
                                "404 Not Found",
                                json!({ "error": { "code": "route_not_found", "message": "no route matches this path" } })
                                    .to_string(),
                            )
                        },
                        |(_, status, body)| (*status, body.clone()),
                    );
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _written = stream.write_all(response.as_bytes());
            }
        });
        Self { url, asked }
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("log").clone()
    }
}

/// An empty configuration directory of the test's own, so no real credentials are touched.
fn config_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("colophon-compat-{}-{name}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("config dir");
    dir
}

fn colophon(config: &PathBuf, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_colophon"))
        .args(args)
        .env("COLOPHON_CONFIG_DIR", config)
        .env_remove("COLOPHON_SERVER")
        .output()
        .expect("the colophon binary runs")
}

fn ours() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the package version is SemVer")
}

/// The first release of the line after this colophon's: 0.2.0 for 0.1.x, 2.0.0 for 1.x.
fn next_line() -> String {
    let version = ours();
    if version.major == 0 {
        format!("0.{}.0", version.minor + 1)
    } else {
        format!("{}.0.0", version.major + 1)
    }
}

fn report(version: &str, api: &[&str]) -> String {
    json!({
        "version": version,
        "commit": "50558a7601474ef9245d9c4afc916b17487b14f6",
        "build_date": "2026-10-28",
        "api": api
    })
    .to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn a_server_on_a_later_line_is_refused_before_sign_in_starts() {
    let newer = next_line();
    let server = Server::start(vec![("/version", "200 OK", report(&newer, &["v1"]))]);
    let out = colophon(&config_dir("later"), &["login", "--server", &server.url]);

    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(out.stdout.is_empty(), "errors go to stderr");
    let message = stderr(&out);
    assert!(
        message.contains(&format!(
            "colophon {} is too old for the server at {}, which runs {newer}",
            env!("CARGO_PKG_VERSION"),
            server.url
        )),
        "{message}"
    );
    assert!(
        message.contains(&format!("Install colophon {newer}")),
        "{message}"
    );
    assert_eq!(server.asked(), ["/version"], "no sign-in was started");
}

#[test]
fn a_server_on_an_earlier_line_is_told_to_upgrade() {
    let server = Server::start(vec![("/version", "200 OK", report("0.0.1", &["v1"]))]);
    let out = colophon(&config_dir("earlier"), &["login", "--server", &server.url]);

    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    let message = stderr(&out);
    assert!(message.contains("is too new for the server"), "{message}");
    assert!(message.contains("which runs 0.0.1"), "{message}");
    assert!(message.contains("Upgrade the server"), "{message}");
    assert_eq!(server.asked(), ["/version"]);
}

#[test]
fn a_server_from_before_version_reports_is_named_as_too_old() {
    // No /version route at all: the canned server answers with Colophon's 404 envelope, as every
    // server before 0.1.0-alpha.1 does.
    let server = Server::start(vec![]);
    let out = colophon(
        &config_dir("unversioned"),
        &["login", "--server", &server.url],
    );

    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    let message = stderr(&out);
    assert!(message.contains("is too new for the server"), "{message}");
    assert!(message.contains("doesn't report its version"), "{message}");
    assert!(message.contains("Upgrade the server"), "{message}");
    assert_eq!(server.asked(), ["/version"]);
}

#[test]
fn a_server_that_dropped_this_api_version_is_named() {
    let server = Server::start(vec![(
        "/version",
        "200 OK",
        report(env!("CARGO_PKG_VERSION"), &["v2"]),
    )]);
    let out = colophon(&config_dir("api"), &["login", "--server", &server.url]);

    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    let message = stderr(&out);
    assert!(
        message.contains("no longer serves API v1, which this colophon speaks (it serves v2)"),
        "{message}"
    );
}

#[test]
fn an_answer_that_is_not_a_version_report_is_an_internal_failure_not_a_guess() {
    for body in [
        json!({ "hello": "world" }).to_string(),
        json!({ "version": "one", "api": ["v1"] }).to_string(),
        "<html>a proxy page</html>".to_owned(),
    ] {
        let server = Server::start(vec![("/version", "200 OK", body.clone())]);
        let out = colophon(&config_dir("garbage"), &["login", "--server", &server.url]);
        assert_eq!(out.status.code(), Some(6), "{body}: {}", stderr(&out));
        assert!(
            stderr(&out).contains("isn't a Colophon version report"),
            "{}",
            stderr(&out)
        );
        assert_eq!(server.asked(), ["/version"]);
    }
}

#[test]
fn a_fitting_server_is_used() {
    let server = Server::start(vec![
        (
            "/version",
            "200 OK",
            report(env!("CARGO_PKG_VERSION"), &["v1"]),
        ),
        (
            "/auth/device",
            "403 Forbidden",
            json!({ "error": { "code": "forbidden", "message": "sign-in is closed" } }).to_string(),
        ),
    ]);
    let out = colophon(&config_dir("fits"), &["login", "--server", &server.url]);

    // The check passed and login went on to ask for a device code.
    assert_eq!(server.asked(), ["/version", "/auth/device"]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("sign-in is closed"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn signed_in_commands_check_too_and_never_send_the_token_to_version() {
    let newer = next_line();
    let server = Server::start(vec![("/version", "200 OK", report(&newer, &["v1"]))]);
    let config = config_dir("whoami");
    let credentials: Value = json!({
        "profiles": { "default": {
            "server": server.url, "token": "test-token", "expires_at": "2099-01-01T00:00:00Z"
        } }
    });
    std::fs::write(config.join("credentials.json"), credentials.to_string()).expect("write");

    for args in [
        &["whoami"][..],
        &["content", "list", "articles", "--project", "demo"][..],
    ] {
        let out = colophon(&config, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains("is too old for the server"),
            "{}",
            stderr(&out)
        );
    }
    assert_eq!(
        server.asked(),
        ["/version", "/version"],
        "nothing else was asked"
    );
}

#[test]
fn an_unreachable_server_is_a_transient_failure() {
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("address")
        .port();
    let out = colophon(
        &config_dir("unreachable"),
        &["login", "--server", &format!("http://127.0.0.1:{port}")],
    );
    assert_eq!(out.status.code(), Some(5), "{}", stderr(&out));
    assert!(stderr(&out).contains("could not reach"), "{}", stderr(&out));
}
