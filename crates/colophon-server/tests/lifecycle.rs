//! The real binary over a real socket: it starts and reports honestly while its database is
//! down, and on SIGTERM it drains and exits cleanly. The last test needs the local stack and
//! stops real services to prove that readiness follows real outages.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

use std::{
    io::{BufRead as _, BufReader},
    net::SocketAddr,
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;

/// Nothing listens on port 1, so this database is always down.
const DEAD_DATABASE: &str = "postgres://colophon:x@127.0.0.1:1/colophon";

struct Server {
    child: Child,
    addr: SocketAddr,
    lines: mpsc::Receiver<String>,
}

impl Server {
    /// Starts the binary with a clean environment plus `env`, and waits for its "listening" log
    /// line to learn the port it bound.
    fn start(args: &[&str], env: &[(&str, &str)]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_colophon-server"))
            .args(args)
            .env_clear()
            .env("COLOPHON_LISTEN_ADDR", "127.0.0.1:0")
            .env("COLOPHON_LOG_FORMAT", "json")
            .env("COLOPHON_PROBE_INTERVAL_MS", "200")
            .env("COLOPHON_PROBE_TIMEOUT_MS", "150")
            .envs(env.iter().copied())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the colophon-server binary starts");
        let stdout = child.stdout.take().expect("stdout is piped");
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let mut server = Self {
            child,
            addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            lines,
        };
        let listening = server.wait_for_log("listening", Duration::from_secs(10));
        server.addr = listening["local_addr"]
            .as_str()
            .and_then(|addr| addr.parse().ok())
            .expect("the listening line names the bound address");
        server
    }

    /// Waits for a JSON log line whose message is `message`.
    fn wait_for_log(&self, message: &str, within: Duration) -> Value {
        let deadline = Instant::now() + within;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self
                .lines
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("no {message:?} log line within {within:?}"));
            if let Ok(event) = serde_json::from_str::<Value>(&line)
                && event["message"] == message
            {
                return event;
            }
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    async fn readyz(&self) -> (u16, Value) {
        let response = reqwest::get(self.url("/readyz"))
            .await
            .expect("the server answers");
        let status = response.status().as_u16();
        (status, response.json().await.expect("a JSON report"))
    }

    /// Polls `/readyz` until its status is `status`.
    async fn wait_for_readiness(&self, status: &str, within: Duration) -> (u16, Value) {
        let deadline = Instant::now() + within;
        loop {
            let reply = self.readyz().await;
            if reply.1["status"] == status {
                return reply;
            }
            assert!(
                Instant::now() < deadline,
                "readiness never became {status:?}; last: {}",
                reply.1
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn terminate(&self) {
        let pid = self.child.id().to_string();
        let status = Command::new("kill")
            .args(["-TERM", &pid])
            .status()
            .expect("kill runs");
        assert!(status.success());
    }

    fn wait_for_exit(&mut self, within: Duration) -> ExitStatus {
        let deadline = Instant::now() + within;
        loop {
            if let Some(status) = self.child.try_wait().expect("try_wait works") {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "the server did not exit within {within:?}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _already_exited_is_fine = self.child.kill();
        let _reaped = self.child.wait();
    }
}

#[tokio::test]
async fn starts_and_reports_honestly_while_the_database_is_down() {
    let server = Server::start(
        &["--role", "api"],
        &[("COLOPHON_DATABASE_URL", DEAD_DATABASE)],
    );

    let live = reqwest::get(server.url("/livez")).await.expect("answers");
    assert_eq!(
        live.status().as_u16(),
        200,
        "liveness doesn't depend on the database"
    );

    let (status, report) = server
        .wait_for_readiness("not_ready", Duration::from_secs(5))
        .await;
    assert_eq!(status, 503);
    assert_eq!(report["role"], "api");
    assert_eq!(report["checks"]["database"], "down");
    assert_eq!(report["checks"]["cache"], "not_configured");

    server.wait_for_log("dependency is down", Duration::from_secs(1));
}

#[tokio::test]
async fn sigterm_drains_then_exits_cleanly() {
    let mut server = Server::start(
        &[],
        &[
            ("COLOPHON_DATABASE_URL", DEAD_DATABASE),
            ("COLOPHON_DRAIN_DELAY_MS", "1500"),
            ("COLOPHON_SHUTDOWN_GRACE_MS", "5000"),
        ],
    );
    server
        .wait_for_readiness("not_ready", Duration::from_secs(5))
        .await;

    let signalled = Instant::now();
    server.terminate();
    server.wait_for_log("shutdown signal received", Duration::from_secs(2));

    // During the drain delay the server still answers, and says it is draining, so load
    // balancers can take it out of rotation before connections are refused.
    let (status, report) = server.readyz().await;
    assert_eq!((status, report["status"].as_str()), (503, Some("draining")));

    let exit = server.wait_for_exit(Duration::from_secs(6));
    assert!(exit.success(), "a graceful shutdown exits 0, got {exit}");
    assert!(
        signalled.elapsed() >= Duration::from_millis(1400),
        "the drain delay was honoured"
    );
    server.wait_for_log("shutdown complete", Duration::from_secs(1));
}

/// Stops and restarts real services of the local stack, so it restores them on drop even when
/// an assertion fails halfway.
struct StackServices;

impl StackServices {
    fn compose(args: &[&str]) {
        let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/compose.yaml");
        let status = Command::new("docker")
            .args(["compose", "-f"])
            .arg(file)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("docker compose runs");
        assert!(status.success(), "docker compose {args:?} failed");
    }

    fn stop(service: &str) {
        Self::compose(&["stop", service]);
    }

    fn start(service: &str) {
        Self::compose(&["start", service]);
    }
}

impl Drop for StackServices {
    fn drop(&mut self) {
        Self::compose(&["start", "postgres", "valkey"]);
    }
}

#[tokio::test]
#[ignore = "needs the local stack, and stops its services: scripts/stack.sh up"]
async fn readiness_follows_real_outages_and_recoveries() {
    let _restore = StackServices;
    let server = Server::start(
        &[],
        &[
            (
                "COLOPHON_DATABASE_URL",
                "postgres://colophon:colophon-dev@127.0.0.1:55432/colophon",
            ),
            (
                "COLOPHON_CACHE_URL",
                "redis://:colophon-dev@127.0.0.1:56379",
            ),
        ],
    );
    let (status, report) = server
        .wait_for_readiness("ready", Duration::from_secs(10))
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        report["checks"],
        serde_json::json!({ "database": "up", "cache": "up" })
    );

    // Losing the cache slows Colophon down; it must not take it out of rotation.
    StackServices::stop("valkey");
    let (status, report) = server
        .wait_for_readiness("degraded", Duration::from_secs(10))
        .await;
    assert_eq!(status, 200);
    assert_eq!(report["checks"]["cache"], "down");
    StackServices::start("valkey");
    server
        .wait_for_readiness("ready", Duration::from_secs(15))
        .await;

    // Losing the database makes it unsafe to serve.
    StackServices::stop("postgres");
    let (status, report) = server
        .wait_for_readiness("not_ready", Duration::from_secs(10))
        .await;
    assert_eq!(status, 503);
    assert_eq!(report["checks"]["database"], "down");
    StackServices::start("postgres");
    server
        .wait_for_readiness("ready", Duration::from_secs(30))
        .await;
}

/// Regression (found by the testbed, 2026-10-25): the server answered 413 to a declared
/// oversized body without reading it, then closed the connection while the client was still
/// sending. The kernel reset the connection, and clients that finish sending before they read
/// (Node's fetch, Vite's proxy) got ECONNRESET, EPIPE or a 502 instead of the envelope.
/// RFC 9112 §9.6: close in stages, so the client can read the last response.
///
/// reqwest doesn't show the bug (hyper's client reads the response even when its upload
/// fails), so this test is a plain socket client that sends everything it declared, then reads.
#[test]
fn oversized_uploads_get_the_413_envelope_not_a_connection_reset() {
    use std::{io::Write as _, net::TcpStream};

    let server = Server::start(&[], &[("COLOPHON_DATABASE_URL", DEAD_DATABASE)]);
    let mut failures = Vec::new();
    for megabytes in [2_usize, 16] {
        let body = vec![0_u8; megabytes << 20];
        for attempt in 0..10 {
            let mut stream = TcpStream::connect(server.addr).expect("connect");
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .expect("timeout");
            let head = format!(
                "POST /api/v1/upload HTTP/1.1\r\nHost: {}\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\n\r\n",
                server.addr,
                body.len()
            );
            let sent = stream
                .write_all(head.as_bytes())
                .and_then(|()| stream.write_all(&body));
            match read_response(&mut stream) {
                Some((head, body))
                    if head.starts_with("http/1.1 413")
                        && String::from_utf8_lossy(&body).contains("payload_too_large") => {}
                other => failures.push(format!(
                    "{megabytes} MiB #{attempt}: send {:?}, got {:?}",
                    sent.err().map(|e| e.kind()),
                    other.map(|(head, _)| head.lines().next().unwrap_or_default().to_owned())
                )),
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of 20 failed: {failures:#?}",
        failures.len()
    );
}

/// Reads one HTTP/1.1 response (headers plus a Content-Length body) from a socket.
fn read_response(stream: &mut std::net::TcpStream) -> Option<(String, Vec<u8>)> {
    use std::io::Read as _;
    let mut buffer = Vec::new();
    let mut byte = [0_u8; 1];
    while !buffer.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => buffer.push(byte[0]),
            _ => return None,
        }
    }
    let head = String::from_utf8_lossy(&buffer).to_ascii_lowercase();
    let length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0_u8; length];
    stream.read_exact(&mut body).ok()?;
    Some((head, body))
}

/// Regression (found by the testbed, 2026-10-25): when a handler answered without reading the
/// request body, the server closed the connection afterwards without saying so. A client that
/// pools connections then sent its next request into a dead socket (ECONNRESET/EPIPE); curl
/// hid it by reconnecting. A response must either leave the connection usable or announce
/// `connection: close`.
#[test]
fn a_connection_is_never_closed_silently_after_an_unread_body() {
    use std::{io::Write as _, net::TcpStream};

    let server = Server::start(&[], &[("COLOPHON_DATABASE_URL", DEAD_DATABASE)]);
    // Bodies the handler never reads: under the limit (a 404) and over it (a 413).
    for (path, kilobytes) in [
        ("/api/v1/nope", 100_usize),
        ("/api/v1/nope", 900),
        ("/api/v1/upload", 2048),
    ] {
        let mut stream = TcpStream::connect(server.addr).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("timeout");
        let body = vec![b'x'; kilobytes * 1024];
        let head = format!(
            "POST {path} HTTP/1.1\r\nHost: {}\r\nContent-Length: {}\r\n\r\n",
            server.addr,
            body.len()
        );
        let _ = stream
            .write_all(head.as_bytes())
            .and_then(|()| stream.write_all(&body));
        let (first, _) = read_response(&mut stream).expect("the first response arrives");
        if first.contains("connection: close") {
            continue; // Announced: the client knows not to reuse this connection.
        }
        let next = format!("GET /livez HTTP/1.1\r\nHost: {}\r\n\r\n", server.addr);
        let reused = stream
            .write_all(next.as_bytes())
            .ok()
            .and_then(|()| read_response(&mut stream));
        assert!(
            reused
                .as_ref()
                .is_some_and(|(head, _)| head.starts_with("http/1.1 200")),
            "{kilobytes} KiB to {path}: the connection was closed without `connection: close` after:\n{first}"
        );
    }
}
