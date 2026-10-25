//! The server's `--role` contract (docs/ARCHITECTURE.md, "Process topology"): exactly `api`,
//! `worker` and `all` are accepted; anything else is a usage error with exit code 2. A valid
//! role gets as far as configuration, which is validated before anything starts.

use std::process::{Command, Output};

#[expect(
    clippy::expect_used,
    reason = "test helper: if the binary can't start, the test should fail loudly"
)]
fn server(args: &[&str], env: &[(&str, &str)]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_colophon-server"))
        .args(args)
        .env_clear()
        .envs(env.iter().copied())
        .output()
        .expect("the colophon-server binary runs")
}

#[test]
fn each_documented_role_is_accepted_and_reaches_configuration() {
    for role in ["api", "worker", "all"] {
        let out = server(&["--role", role], &[]);
        // Not 2: the role parsed. 1: configuration is incomplete, and says exactly what.
        assert_eq!(out.status.code(), Some(1), "role {role}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("COLOPHON_DATABASE_URL is required"),
            "{stderr}"
        );
    }
}

#[test]
fn the_role_can_come_from_the_environment() {
    let out = server(&[], &[("COLOPHON_ROLE", "worker")]);
    assert_eq!(out.status.code(), Some(1));
    let out = server(&[], &[("COLOPHON_ROLE", "everything")]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "an invalid COLOPHON_ROLE is a usage error too"
    );
}

#[test]
fn every_configuration_problem_is_reported_at_once() {
    let out = server(
        &[],
        &[
            ("COLOPHON_LISTEN_ADDR", "not-an-address"),
            ("COLOPHON_PROBE_INTERVAL_MS", "0"),
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    for expected in [
        "3 problem(s)",
        "COLOPHON_DATABASE_URL",
        "COLOPHON_LISTEN_ADDR",
        "COLOPHON_PROBE_INTERVAL_MS",
    ] {
        assert!(
            stderr.contains(expected),
            "{expected} missing from: {stderr}"
        );
    }
}

#[test]
fn an_undocumented_role_is_a_usage_error_that_lists_the_valid_ones() {
    let out = server(&["--role", "everything"], &[]);

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    for role in ["api", "worker", "all"] {
        assert!(stderr.contains(role), "the error lists `{role}`: {stderr}");
    }
}

#[test]
fn version_prints_name_and_version_and_exits_zero() {
    let out = server(&["--version"], &[]);

    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("colophon-server {}", env!("CARGO_PKG_VERSION"))
    );
}
