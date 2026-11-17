//! `colophon locales list` and `colophon locales set` against a canned server: the read, the
//! save on top of the `ETag` the read carried (with one idempotency key), the refusal told in
//! plain words beside the API's own error object, and a file refused before anything is sent
//! (docs/modules/08-cli.md).
#![expect(
    clippy::expect_used,
    reason = "process-level tests: failing to bind, run or parse is a test failure"
)]

use std::{
    io::{Read as _, Write as _},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{Arc, Mutex},
    thread,
};

use serde_json::{Value, json};

const PROJECT: &str = "0190a4b0-0000-7000-8000-0000000000aa";
const LOCALES: &str =
    "/api/v1/projects/0190a4b0-0000-7000-8000-0000000000aa/environments/production/locales";

/// One request as the canned server saw it.
#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    if_match: Option<String>,
    idempotency_key: Option<String>,
    body: String,
}

/// An answer: status line, extra headers, body.
type Answer = (&'static str, Vec<(&'static str, String)>, String);

/// A canned server answering by method and path (`/version` and the project list are
/// always there), logging every request it reads.
struct Server {
    url: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

fn read_request(stream: &mut TcpStream) -> Seen {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let head_end = loop {
        let read = stream.read(&mut chunk).unwrap_or(0);
        if read == 0 {
            break bytes.len();
        }
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(at) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break at + 4;
        }
    };
    let head = String::from_utf8_lossy(&bytes[..head_end]).into_owned();
    let header = |name: &str| {
        head.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
    };
    let length: usize = header("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    while bytes.len() < head_end + length {
        let read = stream.read(&mut chunk).unwrap_or(0);
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    let mut words = head.split(' ');
    Seen {
        method: words.next().unwrap_or("").to_owned(),
        path: words.next().unwrap_or("").to_owned(),
        if_match: header("if-match"),
        idempotency_key: header("idempotency-key"),
        body: String::from_utf8_lossy(&bytes[head_end..]).into_owned(),
    }
}

impl Server {
    fn start(routes: Vec<(&'static str, &'static str, Answer)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let version = json!({
            "version": env!("CARGO_PKG_VERSION"), "commit": "unknown",
            "build_date": "2026-11-17", "api": ["v1"]
        })
        .to_string();
        let projects =
            json!({ "data": [{ "id": PROJECT, "slug": "demo" }], "next_cursor": null }).to_string();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let request = read_request(&mut stream);
                let fixed = match request.path.as_str() {
                    "/version" => Some(("200 OK", Vec::new(), version.clone())),
                    "/api/v1/projects" => Some(("200 OK", Vec::new(), projects.clone())),
                    _ => None,
                };
                let (status, headers, body) = fixed
                    .or_else(|| {
                        routes
                            .iter()
                            .find(|(method, path, _)| {
                                *method == request.method && *path == request.path
                            })
                            .map(|(_, _, answer)| answer.clone())
                    })
                    .unwrap_or_else(|| {
                        (
                            "404 Not Found",
                            Vec::new(),
                            json!({ "error": { "code": "route_not_found", "message": "no route" } })
                                .to_string(),
                        )
                    });
                log.lock().expect("log").push(request);
                let mut extra = String::new();
                for (name, value) in &headers {
                    extra.push_str(name);
                    extra.push_str(": ");
                    extra.push_str(value);
                    extra.push_str("\r\n");
                }
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _written = stream.write_all(response.as_bytes());
            }
        });
        Self { url, seen }
    }

    /// What was asked beyond the version check and the project lookup.
    fn asked(&self) -> Vec<Seen> {
        self.seen
            .lock()
            .expect("log")
            .iter()
            .filter(|seen| seen.path != "/version" && seen.path != "/api/v1/projects")
            .cloned()
            .collect()
    }
}

/// A configuration directory of the test's own, signed in to `server`.
fn signed_in(server: &Server, name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("colophon-locales-{}-{name}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("config dir");
    let credentials = json!({ "profiles": { "default": {
        "server": server.url, "token": "test-token", "expires_at": "2099-01-01T00:00:00Z"
    } } });
    std::fs::write(dir.join("credentials.json"), credentials.to_string()).expect("write");
    dir
}

fn colophon(config: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_colophon"))
        .args(args)
        .env("COLOPHON_CONFIG_DIR", config)
        .env_remove("COLOPHON_SERVER")
        .output()
        .expect("the colophon binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn languages() -> Value {
    json!([
        { "tag": "en", "label": "English", "default": true, "fallback": [] },
        { "tag": "pt", "label": "Português", "default": false, "fallback": ["en"] },
    ])
}

fn listed(version: u32, locales: &Value) -> Answer {
    (
        "200 OK",
        vec![("ETag", format!("\"locales-{version}\""))],
        json!({ "data": { "version": version, "locales": locales } }).to_string(),
    )
}

/// A file of the config directory's, holding `document`.
fn file(config: &Path, document: &Value) -> String {
    let path = config.join("locales.json");
    std::fs::write(&path, document.to_string()).expect("write the file");
    path.to_string_lossy().into_owned()
}

#[test]
fn list_prints_label_tag_and_role_and_its_json_carries_the_etag() {
    let server = Server::start(vec![("GET", LOCALES, listed(3, &languages()))]);
    let config = signed_in(&server, "list");

    let out = colophon(&config, &["locales", "list", "--project", "demo"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "Languages (version 3):\n  English    en  default, no fallback\n  Português  pt  falls back to en\n"
    );

    let out = colophon(&config, &["locales", "list", "--project", "demo", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let result: Value = serde_json::from_slice(&out.stdout).expect("one JSON value");
    assert_eq!(
        result,
        json!({ "version": 2, "command": "locales list", "data": {
            "version": 3, "etag": "\"locales-3\"", "locales": languages()
        } })
    );
}

#[test]
fn set_saves_on_top_of_the_etag_it_read_with_one_key() {
    let mut wider = languages();
    wider
        .as_array_mut()
        .expect("a list")
        .push(json!({ "tag": "pt-BR", "label": "Português (Brasil)", "default": false, "fallback": ["pt", "en"] }));
    let server = Server::start(vec![
        ("GET", LOCALES, listed(3, &languages())),
        ("PUT", LOCALES, listed(4, &wider)),
    ]);
    let config = signed_in(&server, "set");
    let path = file(&config, &json!({ "locales": wider }));

    let out = colophon(
        &config,
        &[
            "locales",
            "set",
            "--project",
            "demo",
            "--file",
            &path,
            "--json",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let asked = server.asked();
    assert_eq!(
        asked
            .iter()
            .map(|seen| seen.method.as_str())
            .collect::<Vec<_>>(),
        ["GET", "PUT"]
    );
    let put = &asked[1];
    assert_eq!(
        put.if_match.as_deref(),
        Some("\"locales-3\""),
        "the read's ETag"
    );
    let key = put.idempotency_key.clone().expect("a key");
    assert!(key.starts_with("cli-"), "{key}");
    assert_eq!(
        serde_json::from_str::<Value>(&put.body).expect("a JSON body"),
        json!({ "locales": wider })
    );
    let result: Value = serde_json::from_slice(&out.stdout).expect("one JSON value");
    assert_eq!(result["command"], "locales set");
    assert_eq!(result["data"]["status"], "updated");
    assert_eq!(result["data"]["version"], 4);
    assert_eq!(result["data"]["etag"], "\"locales-4\"");
    assert_eq!(result["data"]["idempotency_key"], key.as_str());
}

#[test]
fn set_with_the_list_as_it_is_saves_nothing() {
    let server = Server::start(vec![("GET", LOCALES, listed(3, &languages()))]);
    let config = signed_in(&server, "same");
    let path = file(&config, &json!({ "locales": languages() }));

    let out = colophon(
        &config,
        &["locales", "set", "--project", "demo", "--file", &path],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("already match the file (version 3)"),
        "{}",
        stdout(&out)
    );
    assert_eq!(server.asked().len(), 1, "only the read: no PUT");
}

#[test]
fn a_stale_list_is_a_conflict_in_plain_words_and_the_api_s_object_whole() {
    let refusal = json!({ "error": {
        "code": "conflict", "message": "the languages changed",
        "details": { "reason": "stale_locales" }, "request_id": "0190a4b0-0000-7000-8000-000000000001"
    } });
    let server = Server::start(vec![
        ("GET", LOCALES, listed(3, &languages())),
        (
            "PUT",
            LOCALES,
            (
                "409 Conflict",
                vec![("ETag", "\"locales-4\"".to_owned())],
                refusal.to_string(),
            ),
        ),
    ]);
    let config = signed_in(&server, "stale");
    let path = file(&config, &json!({ "locales": [] }));

    let out = colophon(
        &config,
        &[
            "locales",
            "set",
            "--project",
            "demo",
            "--file",
            &path,
            "--json",
        ],
    );
    assert_eq!(out.status.code(), Some(4), "a conflict: {}", stderr(&out));
    assert!(
        stderr(&out).contains("someone changed the languages after this command read them"),
        "{}",
        stderr(&out)
    );
    let result: Value = serde_json::from_slice(&out.stdout).expect("one JSON value");
    assert_eq!(
        result["error"], refusal["error"],
        "the API's error object, whole"
    );
    assert_eq!(
        server.asked().len(),
        2,
        "one read, one refused save, no retry"
    );
}

#[test]
fn a_file_that_is_not_a_list_is_refused_before_anything_is_sent() {
    let server = Server::start(Vec::new());
    let config = signed_in(&server, "bad-file");
    let path = file(&config, &json!([{ "tag": "en" }]));

    let out = colophon(
        &config,
        &["locales", "set", "--project", "demo", "--file", &path],
    );
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("{\"locales\": [...]}"),
        "{}",
        stderr(&out)
    );
    assert!(
        server.seen.lock().expect("log").is_empty(),
        "no request at all"
    );
}
