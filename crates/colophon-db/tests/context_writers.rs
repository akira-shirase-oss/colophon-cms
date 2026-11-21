//! Only sanctioned code writes the row-security context (D-235; SECURITY.md, "The project
//! context"). `app.project_id` is a capability: row security enforces "within the entered
//! project", not membership, so a new code path that set it from a request without proving
//! membership would open another project. This scans every crate's sources and every migration
//! (the D-234 substitutes included) for writers of `app.project_id`, `app.identity_id` and
//! `app.token_id` and for calls to the scope primitives that open a context without a
//! membership check, and fails on any not listed in `context-writers.allow`, and on any listed
//! one that no longer exists. No database needed.
#![expect(
    clippy::expect_used,
    reason = "a source scan: an unreadable file is a test failure"
)]

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// What counts as a write: a set_config of a context setting, a `SET [LOCAL]` of one, a
/// set_config whose setting name is computed (a parameter or a variable), and a call to the
/// scope primitives that set a context without proving membership.
fn writes_context(code: &str) -> bool {
    let lower = code.to_ascii_lowercase();
    let settings = ["app.project_id", "app.identity_id", "app.token_id"];
    let named = settings.iter().any(|setting| {
        lower.contains(&format!("set_config('{setting}'"))
            || lower.contains(&format!("set_config( '{setting}'"))
            || lower.contains(&format!("set {setting}"))
            || lower.contains(&format!("set local {setting}"))
    });
    // A computed name: set_config($1, …) or set_config(name, …) — never a literal elsewhere.
    let computed = lower.match_indices("set_config(").any(|(at, _)| {
        let rest = lower[at + "set_config(".len()..].trim_start();
        rest.starts_with('$')
            || rest
                .split(|c: char| c == ',' || c == ')')
                .next()
                .is_some_and(|arg| {
                    let arg = arg.trim();
                    !arg.is_empty()
                        && !arg.starts_with('\'')
                        && arg
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
                })
    });
    let primitive = [
        "scope::begin(",
        "scope::begin_in_environment(",
        "scope::begin_in_environment_as_token(",
    ]
    .iter()
    .any(|call| lower.contains(call))
        || (lower.contains("begin_in_environment(") && !lower.contains("fn begin_in_environment"));
    named || computed || primitive
}

/// The function a line belongs to: the nearest `fn name(` above it in Rust, `FUNCTION name(`
/// in SQL; `<top>` before any.
fn enclosing(line: &str, rust: bool, current: &mut String) {
    let lower = line.to_ascii_lowercase();
    let marker = if rust { "fn " } else { "function " };
    for (at, _) in lower.match_indices(marker) {
        if at > 0 && lower.as_bytes()[at - 1].is_ascii_alphanumeric() {
            continue;
        }
        let rest = &line[at + marker.len()..];
        let rest = rest.strip_prefix("public.").unwrap_or(rest);
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let after = rest[name.len()..].trim_start();
        if !name.is_empty() && (after.starts_with('(') || after.starts_with('<')) {
            *current = name;
        }
    }
}

fn files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.join("crates")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("a directory") {
            let path = entry.expect("an entry").path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                // Test code may set any context it needs; sources and migrations may not.
                if !matches!(name, "tests" | "target" | "benches" | "examples" | "fuzz") {
                    stack.push(path);
                }
            } else if (name.ends_with(".rs") && path.components().any(|c| c.as_os_str() == "src"))
                || (name.ends_with(".sql")
                    && path.parent().is_some_and(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.starts_with("migrations"))
                    }))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

#[test]
fn only_the_allowlisted_code_writes_the_row_security_context() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root.canonicalize().expect("the workspace root");
    let mut writers = BTreeSet::new();
    for path in files(&root) {
        let text = std::fs::read_to_string(&path).expect("readable");
        let rust = path.extension().is_some_and(|e| e == "rs");
        let relative = path
            .strip_prefix(&root)
            .expect("under the root")
            .to_string_lossy()
            .replace('\\', "/");
        let mut current = "<top>".to_owned();
        for line in text.lines() {
            enclosing(line, rust, &mut current);
            let code = if rust {
                line.split("//").next().unwrap_or("")
            } else {
                line.split("--").next().unwrap_or("")
            };
            if writes_context(code) {
                writers.insert(format!("{relative} {current}"));
            }
        }
    }
    let allow_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("context-writers.allow");
    let allowed: BTreeSet<String> = std::fs::read_to_string(&allow_path)
        .expect("context-writers.allow")
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect();
    let unlisted: Vec<&String> = writers.difference(&allowed).collect();
    assert!(
        unlisted.is_empty(),
        "these write the row-security context but aren't in crates/colophon-db/context-writers.allow \
         (app.project_id is a capability: prove membership first, or add the writer with its reason \
         for review): {unlisted:#?}"
    );
    let stale: Vec<&String> = allowed.difference(&writers).collect();
    assert!(
        stale.is_empty(),
        "context-writers.allow lists writers that no longer exist: {stale:#?}"
    );
}

#[test]
fn the_scan_recognises_every_form_of_a_write() {
    for written in [
        "PERFORM set_config('app.project_id', p_project::text, true);",
        "SELECT set_config( 'app.identity_id', $1, true)",
        "SET app.project_id = ''",
        "SET LOCAL app.token_id = 'x'",
        "    SELECT set_config($1, $2, true)",
        "EXECUTE format('SELECT set_config(%L, %L, true)', name, value); PERFORM set_config(setting_name, v, true);",
        "let tx = scope::begin(pool, Scope { identity: None, project: Some(id) }).await?;",
        "crate::scope::begin(&pool, s)",
        "scope::begin_in_environment(&self.pool, who, project, environment)",
    ] {
        assert!(writes_context(written), "{written:?}");
    }
    for not_written in [
        "current_setting('app.project_id', true)",
        "PERFORM set_config('app.audience_role', '', true);",
        "SELECT set_config('statement_timeout', '5s', true)",
        "scope::begin_as_member(&pool, caller, project)",
        "scope::begin_in_project_isolated(&pool, caller, project, env)",
        "pub async fn begin_in_environment(",
    ] {
        assert!(!writes_context(not_written), "{not_written:?}");
    }
}
