//! The shared fixtures (`fixtures/rich-text/v1`): the contract between the Rust validator, the
//! Studio's editor and the renderer. Every valid fixture must be accepted and come out in
//! exactly the canonical form the editor produces; every invalid one must be refused with
//! exactly the code and JSON Pointer it names.
#![expect(
    clippy::expect_used,
    reason = "fixture tests: an unreadable fixture is a test failure"
)]

use std::{fs, path::PathBuf};

use colophon_domain::rich_text::{Rejection, validate};
use serde_json::Value;

struct Fixture {
    name: String,
    value: Value,
    reject: Option<(String, String)>,
}

fn fixtures(kind: &str) -> Vec<Fixture> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/rich-text/v1")
        .join(kind);
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .expect("the fixture directory exists")
        .map(|entry| entry.expect("readable entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    entries.sort();
    assert!(!entries.is_empty(), "no fixtures in {}", dir.display());
    entries
        .into_iter()
        .map(|path| {
            let file: Value =
                serde_json::from_str(&fs::read_to_string(&path).expect("readable fixture"))
                    .expect("fixture files are JSON");
            let reject = file.get("reject").map(|reject| {
                (
                    reject["code"].as_str().expect("reject.code").to_owned(),
                    reject["path"].as_str().expect("reject.path").to_owned(),
                )
            });
            Fixture {
                name: path
                    .file_stem()
                    .expect("a name")
                    .to_string_lossy()
                    .into_owned(),
                value: file["value"].clone(),
                reject,
            }
        })
        .collect()
}

#[test]
fn every_valid_fixture_is_accepted_in_exactly_the_editors_canonical_form() {
    let mut failures = Vec::new();
    for fixture in fixtures("valid") {
        // With serde_json's preserve_order (a dev-dependency feature), this is the fixture's
        // own key order: the editor's canonical form, which JSON.stringify writes the same way.
        let expected = serde_json::to_string(&fixture.value).expect("serializable");
        match validate(&fixture.value) {
            Ok(document) if document.canonical_json() == expected => {}
            Ok(document) => failures.push(format!(
                "{}: canonical form differs\n  expected {expected}\n  got      {}",
                fixture.name,
                document.canonical_json()
            )),
            Err(rejection) => failures.push(format!("{}: refused: {rejection}", fixture.name)),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_invalid_fixture_is_refused_with_exactly_its_code_and_pointer() {
    let mut failures = Vec::new();
    let all = fixtures("invalid");
    for fixture in &all {
        let (code, pointer) = fixture
            .reject
            .clone()
            .expect("invalid fixtures name their rejection");
        match validate(&fixture.value) {
            Err(Rejection {
                code: got_code,
                pointer: got_pointer,
            }) if got_code.as_str() == code && got_pointer == pointer => {}
            Err(rejection) => failures.push(format!(
                "{}: expected {code} at {pointer:?}, got {} at {:?}",
                fixture.name, rejection.code, rejection.pointer
            )),
            Ok(_) => failures.push(format!(
                "{}: accepted, expected {code} at {pointer:?}",
                fixture.name
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} wrong:\n{}",
        failures.len(),
        all.len(),
        failures.join("\n")
    );
}

#[test]
fn valid_fixtures_report_their_size() {
    let deepest = fixtures("valid")
        .into_iter()
        .find(|fixture| fixture.name == "11-depth-at-the-limit")
        .expect("the depth fixture exists");
    let document = validate(&deepest.value).expect("valid");
    assert_eq!(document.depth(), colophon_domain::rich_text::MAX_DEPTH);
}
