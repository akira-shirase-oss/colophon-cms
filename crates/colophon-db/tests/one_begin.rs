//! D-217: every transaction on a pool begins through `colophon_db::begin_tx` (or its snapshot
//! form), which a cancelled caller can't cut short. A `pool.begin()`, `begin_with(…)`,
//! `Connection::begin` or a hand-written `BEGIN` anywhere else in the workspace's sources fails
//! this test; the one held-connection begin that has to stay says why on the line above it,
//! starting `// D-217:`.
#![expect(
    clippy::expect_used,
    reason = "a source tree that can't be read is a test failure"
)]

use std::path::{Path, PathBuf};

/// The only file allowed to begin on a pool directly.
const THE_ONE: &str = "colophon-db/src/transactions.rs";

/// Each forbidden use in `source`, as `line: text`; a line whose previous line starts with
/// `// D-217:` is a reasoned exception.
fn offences(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut previous = "";
    for (number, line) in source.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("");
        let forbidden = code.contains(".begin()")
            || code.contains("begin_with(")
            || code.contains("\"BEGIN")
            || code.contains("r\"BEGIN")
            || code.contains("raw_sql(\"BEGIN");
        if forbidden && !previous.trim_start().starts_with("// D-217:") {
            found.push(format!("{}: {}", number + 1, line.trim()));
        }
        // A comment's continuation line keeps the exception open for the code below it.
        if !line.trim_start().starts_with("//") || line.trim_start().starts_with("// D-217:") {
            previous = line;
        }
    }
    found
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read a source directory") {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn every_pool_transaction_begins_through_begin_tx() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crates directory");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(crates).expect("read crates/") {
        let src = entry.expect("a crate").path().join("src");
        if src.is_dir() {
            rust_files(&src, &mut files);
        }
    }
    assert!(
        files.len() > 50,
        "found the workspace's sources ({})",
        files.len()
    );
    let mut all = Vec::new();
    for file in files {
        let shown = file
            .strip_prefix(crates)
            .expect("under crates/")
            .to_string_lossy()
            .replace('\\', "/");
        if shown == THE_ONE {
            continue;
        }
        let source = std::fs::read_to_string(&file).expect("read a source file");
        all.extend(
            offences(&source)
                .into_iter()
                .map(|offence| format!("crates/{shown}:{offence}")),
        );
    }
    assert!(
        all.is_empty(),
        "begin transactions with colophon_db::begin_tx (D-217):\n{}",
        all.join("\n")
    );
}

#[test]
fn the_check_finds_each_way_of_beginning() {
    let source = r#"
let a = pool.begin().await;
let b = pool.begin_with("BEGIN ISOLATION LEVEL SERIALIZABLE").await;
let c = conn.begin().await;
sqlx::raw_sql("BEGIN").execute(&mut *conn).await;
// pool.begin() in a comment is fine
// D-217: held connection, the reason continues
// on a second line
let d = conn.begin().await;
let e = colophon_db::begin_tx(&pool).await;
"#;
    let found = offences(source);
    assert_eq!(found.len(), 4, "{found:#?}");
    assert!(
        found
            .iter()
            .all(|line| !line.contains("let d") && !line.contains("let e"))
    );
}
