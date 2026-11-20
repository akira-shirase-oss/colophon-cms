//! Oversized and slow bodies (docs/security/attack-surface.md, D-230): a JSON route reads at most 1 MiB
//! unless the route table documents more, and a connection that trickles its headers is closed after the
//! header timeout. Before, every signed-in JSON route read up to the upload limit (21 MiB), and nothing
//! timed a request's headers: axum's server set no timer, so hyper's header timeout never fired.

use std::time::{Duration, Instant};

use reqwest::{Method, StatusCode};
use serde_json::json;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use super::send;
use crate::common::{ORIGIN, Server, entries, project, sign_in};

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_json_route_reads_a_mebibyte_and_an_import_its_documented_limit() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    // Two MiB of JSON at an entry write: refused, with the 413 envelope, and nothing written.
    let big = json!({"title": "Big", "notes": "x".repeat(2 * 1024 * 1024)});
    let refused = send(&owner, Method::POST, &entries(&project), Some(&big)).await;
    assert_eq!(
        refused.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "{}",
        refused.body
    );
    let listed = send(&owner, Method::GET, &entries(&project), None).await;
    assert_eq!(
        listed.body["data"].as_array().map(Vec::len),
        Some(0),
        "{}",
        listed.body
    );
    // An import documents a larger need: two MiB of JSON Lines is read (and answered on its merits).
    let line = json!({"title": "Imported", "body": "y".repeat(1000)}).to_string();
    let file = vec![line; 2 * 1024].join("\n");
    assert!(file.len() > 2 * 1024 * 1024);
    let import = format!(
        "/api/v1/projects/{project}/environments/production/collections/articles/import?dry_run=true"
    );
    let response = crate::common::app()
        .post(format!("{ORIGIN}{import}"))
        .header("cookie", &owner.cookie)
        .header("x-csrf-token", &owner.csrf)
        .header("origin", ORIGIN)
        .body(file)
        .send()
        .await
        .expect("request");
    assert_ne!(
        response.status(),
        StatusCode::PAYLOAD_TOO_LARGE,
        "an import's limit is its own"
    );
    assert!(
        response.status().is_client_error(),
        "2,048 items is over an import's 512: {}",
        response.status()
    );
}

/// A slowloris connection: headers that never finish. The server closes it once the header timeout
/// passes, whatever else is going on.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_connection_that_trickles_its_headers_is_closed() {
    let _server = Server::start_with(&[("COLOPHON_HEADER_TIMEOUT_MS", "500")]).await;
    let address = ORIGIN.strip_prefix("http://").expect("an http origin");
    let mut socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect");
    let started = Instant::now();
    socket
        .write_all(b"GET /livez HTTP/1.1\r\nHost: localhost\r\nX-Slow: ")
        .await
        .expect("the first bytes");
    // One more byte now and then, as a slowloris client sends them, never the blank line.
    let mut closed = false;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if socket.write_all(b"a").await.is_err() {
            closed = true;
            break;
        }
        let mut buffer = [0_u8; 512];
        // A 408 (or any answer) before the close is fine; the next read sees the close.
        if let Ok(Ok(0) | Err(_)) =
            tokio::time::timeout(Duration::from_millis(1), socket.read(&mut buffer)).await
        {
            closed = true;
            break;
        }
    }
    assert!(
        closed,
        "the connection was still open after {:?}",
        started.elapsed()
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "closed only after {:?}",
        started.elapsed()
    );
}
