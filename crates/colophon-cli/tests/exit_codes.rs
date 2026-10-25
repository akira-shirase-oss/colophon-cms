//! The CLI's exit-code contract (docs/modules/08-cli.md): usage errors exit
//! with 2, and errors never write to stdout, which is reserved for results.

use std::process::{Command, Output};

#[expect(
    clippy::expect_used,
    reason = "test helper: if the binary can't start, the test should fail loudly"
)]
fn colophon(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_colophon"))
        .args(args)
        .output()
        .expect("the colophon binary runs")
}

#[test]
fn version_prints_name_and_version_and_exits_zero() {
    let out = colophon(&["--version"]);

    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("colophon {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn unknown_flag_is_a_usage_error_with_exit_code_2_and_nothing_on_stdout() {
    let out = colophon(&["--publish-everything"]);

    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "errors go to stderr, never stdout");
    assert!(String::from_utf8_lossy(&out.stderr).contains("--publish-everything"));
}

#[test]
fn no_arguments_prints_help_to_stderr_and_exits_with_the_usage_code() {
    let out = colophon(&[]);

    assert_eq!(out.status.code(), Some(2));
    assert!(
        out.stdout.is_empty(),
        "help for a usage error goes to stderr"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("Usage:"));
}
