//! The outage helpers refuse the shared stack (D-187b). A test that stops, pauses or restarts a
//! stack service runs only where the stack is its own: in CI, under a compose project other than
//! `colophon`, or with `COLOPHON_STACK_OWNER=1` — the rule `scripts/stack-guard.sh` applies to the
//! scripts. Run directly (`cargo test --test admission -- --ignored`), an outage test once
//! stopped the maintainer's Valkey twice; the decision is now made before any `docker` call.
//! Needs nothing running: the decision is a function of the environment, and the second test
//! reads the sources.
#![expect(
    clippy::expect_used,
    reason = "a test: an unreadable source file is a test failure"
)]

mod common;

use std::{collections::HashMap, fs, path::Path};

fn decide(vars: &[(&str, &str)]) -> Result<(), String> {
    let vars: HashMap<String, String> = vars
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
    common::stack_guard(|key| vars.get(key).cloned())
}

#[test]
fn the_shared_stack_is_refused_and_an_owned_one_allowed() {
    // Nothing set: the maintainer's stack, project `colophon`.
    let refused = decide(&[]).expect_err("the shared stack is refused");
    assert!(
        refused.contains("COLOPHON_STACK_OWNER=1") && refused.contains("--exact"),
        "the refusal says how to run it safely: {refused}"
    );
    assert!(decide(&[("COMPOSE_PROJECT_NAME", "colophon")]).is_err());
    assert!(decide(&[("CI", "false")]).is_err());
    assert!(decide(&[("COLOPHON_STACK_OWNER", "0")]).is_err());
    assert!(decide(&[("COLOPHON_STACK_OWNER", "")]).is_err());
    assert!(
        decide(&[("COLOPHON_STACK_OWNER", "yes")]).is_err(),
        "only 1 is the owner's word"
    );
    assert!(decide(&[("COMPOSE_PROJECT_NAME", "")]).is_err());

    // The three ways a stack is this run's own.
    assert_eq!(decide(&[("CI", "true")]), Ok(()));
    assert_eq!(decide(&[("COLOPHON_STACK_OWNER", "1")]), Ok(()));
    assert_eq!(decide(&[("COMPOSE_PROJECT_NAME", "colophon-ci-7")]), Ok(()));
}

/// Every `docker` command the integration tests run goes through the guarded helper: a new
/// outage helper can't call docker beside it.
#[test]
fn every_docker_call_under_tests_goes_through_the_guard() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut found = Vec::new();
    for krate in fs::read_dir(&crates).expect("crates/") {
        let tests = krate.expect("an entry").path().join("tests");
        if tests.is_dir() {
            scan(&tests, &mut found);
        }
    }
    let guarded = Path::new("colophon-server/tests/common/mod.rs");
    let outside: Vec<_> = found
        .iter()
        .filter(|(path, _)| !path.ends_with(guarded))
        .collect();
    assert!(
        outside.is_empty(),
        "docker run outside tests/common's guarded compose(): {outside:?}"
    );
    let inside: usize = found.iter().map(|(_, count)| count).sum();
    assert_eq!(inside, 1, "one docker call, inside compose(): {found:?}");
}

fn scan(dir: &Path, found: &mut Vec<(std::path::PathBuf, usize)>) {
    for entry in fs::read_dir(dir).expect("a tests directory") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            scan(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let text = fs::read_to_string(&path).expect("a source file");
            // Split so this file's own text doesn't count itself.
            let count = text
                .matches(concat!("Command::new(", "\"docker\")"))
                .count();
            if count > 0 {
                found.push((path, count));
            }
        }
    }
}
