//! `colophon doctor` against a canned server: each readiness state, each failure, and the
//! exit code and output contract for each (docs/modules/08-cli.md).
#![expect(
    clippy::expect_used,
    reason = "process-level tests: failing to bind or run is a test failure"
)]

use std::{
    io::{Read as _, Write as _},
    net::TcpListener,
    process::{Command, Output},
    thread,
};

use serde_json::{Value, json};

/// Serves one canned HTTP response to every connection, on a free port.
fn canned_server(status_line: &'static str, body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("address");
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = [0_u8; 4096];
            let _read = stream.read(&mut request);
            let response = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _written = stream.write_all(response.as_bytes());
        }
    });
    format!("http://{addr}")
}

fn doctor(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_colophon"))
        .arg("doctor")
        .args(args)
        .env_remove("COLOPHON_SERVER")
        .output()
        .expect("the colophon binary runs")
}

fn stdout_json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout).expect("--json prints exactly one JSON value")
}

const READY: &str = r#"{"status":"ready","role":"all","checks":{"database":"up","cache":"up"}}"#;
const DEGRADED: &str =
    r#"{"status":"degraded","role":"all","checks":{"database":"up","cache":"down"}}"#;
const NOT_READY: &str =
    r#"{"status":"not_ready","role":"api","checks":{"database":"down","cache":"not_configured"}}"#;

#[test]
fn a_ready_server_exits_zero_and_prints_each_check() {
    let server = canned_server("200 OK", READY);
    let out = doctor(&["--server", &server]);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout);
    for expected in [
        "status    ready",
        "database  up",
        "cache     up",
        "role      all",
    ] {
        assert!(
            text.contains(expected),
            "{expected:?} missing from:\n{text}"
        );
    }
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");
}

#[test]
fn json_output_is_one_versioned_result() {
    let server = canned_server("200 OK", READY);
    let out = doctor(&["--server", &server, "--json"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        stdout_json(&out),
        json!({
            "version": 1,
            "command": "doctor",
            "server": server,
            "status": "ready",
            "role": "all",
            "checks": { "database": "up", "cache": "up" }
        })
    );
}

#[test]
fn degraded_and_not_ready_exit_with_the_unavailable_code() {
    for (status_line, body, status) in [
        ("200 OK", DEGRADED, "degraded"),
        ("503 Service Unavailable", NOT_READY, "not_ready"),
    ] {
        let server = canned_server(status_line, body);
        let out = doctor(&["--server", &server, "--json"]);
        assert_eq!(out.status.code(), Some(5), "{status}");
        assert_eq!(stdout_json(&out)["status"], status);
    }
}

#[test]
fn an_unreachable_server_exits_5_and_says_so() {
    // Bind and drop: nothing listens on the port any more.
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("address")
        .port();
    let server = format!("http://127.0.0.1:{port}");
    let out = doctor(&["--server", &server, "--json", "--timeout-ms", "2000"]);
    assert_eq!(out.status.code(), Some(5));
    assert_eq!(stdout_json(&out)["status"], "unreachable");
    assert!(String::from_utf8_lossy(&out.stderr).contains("connection failed"));
}

#[test]
fn a_reply_that_is_not_a_readiness_report_is_an_internal_failure() {
    let server = canned_server("200 OK", r#"{"hello":"world"}"#);
    let out = doctor(&["--server", &server]);
    assert_eq!(out.status.code(), Some(6));
    assert!(out.stdout.is_empty(), "errors go to stderr");

    let server = canned_server("404 Not Found", r#"{"error":{"code":"route_not_found"}}"#);
    assert_eq!(doctor(&["--server", &server]).status.code(), Some(6));
}

#[test]
fn a_bad_server_url_is_a_usage_error() {
    for bad in ["not a url", "ftp://localhost:58000"] {
        let out = doctor(&["--server", bad]);
        assert_eq!(out.status.code(), Some(2), "{bad}");
    }
}

#[test]
fn new_checks_from_the_server_are_shown_without_a_cli_release() {
    let server = canned_server(
        "200 OK",
        r#"{"status":"ready","role":"all","checks":{"database":"up","cache":"up","search":"up"}}"#,
    );
    let out = doctor(&["--server", &server]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("search    up"));
}
