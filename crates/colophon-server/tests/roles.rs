//! The server's `--role` contract (docs/ARCHITECTURE.md, "Process topology"):
//! exactly `api`, `worker` and `all` are accepted; anything else is a usage
//! error with exit code 2.

use std::process::{Command, Output};

#[expect(
    clippy::expect_used,
    reason = "test helper: if the binary can't start, the test should fail loudly"
)]
fn server(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_colophon-server"))
        .args(args)
        .output()
        .expect("the colophon-server binary runs")
}

#[test]
fn each_documented_role_is_accepted() {
    for role in ["api", "worker", "all"] {
        let out = server(&["--role", role]);

        assert_eq!(out.status.code(), Some(0), "role {role}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(&format!("role `{role}`")),
            "role {role} is echoed back"
        );
    }
}

#[test]
fn an_undocumented_role_is_a_usage_error_that_lists_the_valid_ones() {
    let out = server(&["--role", "everything"]);

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    for role in ["api", "worker", "all"] {
        assert!(stderr.contains(role), "the error lists `{role}`: {stderr}");
    }
}

#[test]
fn version_prints_name_and_version_and_exits_zero() {
    let out = server(&["--version"]);

    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("colophon-server {}", env!("CARGO_PKG_VERSION"))
    );
}
