//! `colophon doctor`: asks a running server whether it is ready, and says why not.
//!
//! It reads the server's `/readyz` rather than probing dependencies itself, so it reports
//! exactly what a load balancer sees. Dependency names are not hard-coded: a check the
//! server adds later appears here without a CLI release.

use std::{collections::BTreeMap, time::Duration};

use serde::Deserialize;
use serde_json::json;

use crate::{
    Exit,
    out::{self, Out},
};

/// The `/readyz` body, read loosely: unknown statuses and checks are shown, not rejected.
#[derive(Debug, Deserialize)]
struct Report {
    status: String,
    role: String,
    checks: BTreeMap<String, String>,
}

/// What went wrong talking to the server, and the exit code it maps to.
#[derive(Debug)]
enum Failure {
    /// Bad `--server` value: a usage error.
    InvalidServer(String),
    /// No answer: a transient failure.
    Unreachable(String),
    /// An answer that isn't a readiness report: something is badly wrong.
    UnexpectedReply(String),
}

impl Failure {
    fn exit_code(&self) -> Exit {
        match self {
            Self::InvalidServer(_) => Exit::Usage,
            Self::Unreachable(_) => Exit::Unavailable,
            Self::UnexpectedReply(_) => Exit::Internal,
        }
    }

    fn status(&self) -> &'static str {
        match self {
            Self::InvalidServer(_) => "invalid_server",
            Self::Unreachable(_) => "unreachable",
            Self::UnexpectedReply(_) => "unexpected_reply",
        }
    }

    fn message(&self) -> &str {
        match self {
            Self::InvalidServer(message)
            | Self::Unreachable(message)
            | Self::UnexpectedReply(message) => message,
        }
    }
}

async fn fetch(server: &str, timeout: Duration) -> Result<Report, Failure> {
    let base = reqwest::Url::parse(server)
        .map_err(|error| Failure::InvalidServer(format!("--server {server:?}: {error}")))?;
    if !matches!(base.scheme(), "http" | "https") {
        return Err(Failure::InvalidServer(format!(
            "--server {server:?}: expected an http:// or https:// URL"
        )));
    }
    let url = base
        .join("/readyz")
        .map_err(|error| Failure::InvalidServer(format!("--server {server:?}: {error}")))?;
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(timeout)
        .build()
        .map_err(|error| Failure::UnexpectedReply(format!("building the HTTP client: {error}")))?;
    let response = client.get(url).send().await.map_err(|error| {
        let reason = if error.is_timeout() {
            format!("no answer within {timeout:?}")
        } else {
            "connection failed".to_owned()
        };
        Failure::Unreachable(format!("{server}: {reason}"))
    })?;
    let status = response.status();
    // 200 and 503 both carry a readiness report; anything else is not one.
    if !(status.is_success() || status == reqwest::StatusCode::SERVICE_UNAVAILABLE) {
        return Err(Failure::UnexpectedReply(format!(
            "{server}/readyz answered HTTP {status}"
        )));
    }
    response.json::<Report>().await.map_err(|_| {
        Failure::UnexpectedReply(format!("{server}/readyz did not return a readiness report"))
    })
}

/// Runs the command; returns the process exit code.
pub(crate) async fn run(server: &str, timeout: Duration, out: Out) -> Exit {
    match fetch(server, timeout).await {
        Ok(report) => {
            let code = if report.status == "ready" {
                Exit::Success
            } else {
                Exit::Unavailable
            };
            if out.format.machine() {
                out.print_data(&json!({
                    "server": server,
                    "status": report.status,
                    "role": report.role,
                    "checks": report.checks,
                }));
            } else {
                println!("server    {server}");
                println!("role      {}", report.role);
                println!("status    {}", report.status);
                for (name, state) in &report.checks {
                    println!("{name:<9} {state}");
                }
            }
            code
        }
        Err(failure) => {
            out.print_error(&out::cli_error(failure.status(), failure.message()));
            eprintln!("error: {}", failure.message());
            failure.exit_code()
        }
    }
}
