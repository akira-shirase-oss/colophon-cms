//! `colophon admin migrate-status` (the upgrade runbook's pre-check and rollback gate,
//! D-130) against the real binary and a scratch database on the local stack: a database
//! nobody migrated reports `behind` with every migration pending, a migrated one reports
//! `current`, one missing the newest applied row reports `behind`, and one carrying a
//! migration the binary doesn't know is refused with exit 4. Needs the stack.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use std::{path::PathBuf, process::Command};

use common::{local_env, unique_slug};
use serde_json::{Value, json};

/// The migration database URL from `deploy/local.env`, with its last path segment swapped
/// for `database` (the scratch database this test owns).
fn database_url(database: &str) -> String {
    let url = local_env()
        .into_iter()
        .find_map(|(key, value)| (key == "COLOPHON_MIGRATION_DATABASE_URL").then_some(value))
        .expect("deploy/local.env names the migration database");
    let (base, _) = url.rsplit_once('/').expect("the URL names a database");
    format!("{base}/{database}")
}

/// `colophon admin <args> --json` against `url`; returns (exit code, stdout as JSON).
fn admin(url: &str, args: &[&str]) -> (Option<i32>, Value) {
    let cli = PathBuf::from(env!("CARGO_BIN_EXE_colophon-server")).with_file_name("colophon");
    let output = Command::new(cli)
        .args(["admin"])
        .args(args)
        .args(["--json", "--database-url", url])
        .env_clear()
        .envs(local_env())
        .output()
        .expect("the CLI runs");
    (
        output.status.code(),
        serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!(
                "migrate-status printed no JSON: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        }),
    )
}

#[tokio::test]
#[ignore = "needs the local stack's postgres"]
async fn migrate_status_reports_current_behind_and_refuses_an_unknown_schema() {
    // A scratch database of the test's own, created and dropped on the stack's postgres.
    let name = format!(
        "colophon_ms_{}",
        unique_slug()
            .chars()
            .filter(|c| *c != '-')
            .collect::<String>()
    );
    let pool = common::owner_database().await;
    // Audited: `name` is `colophon_ms_` plus a uuid's hex digits, so no quoting can be in it.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
        .execute(&pool)
        .await
        .expect("create the scratch database");
    let scratch = database_url(&name);
    let scratch_pool = sqlx::PgPool::connect(&scratch)
        .await
        .expect("the scratch database");

    // Never migrated: every migration is pending; there is no error and nothing applied.
    let (code, out) = admin(&scratch, &["migrate-status"]);
    assert_eq!(code, Some(0), "{out}");
    assert_eq!(
        (
            out["data"]["status"].as_str(),
            out["data"]["applied"].as_array().map(Vec::len)
        ),
        (Some("behind"), Some(0))
    );
    let pending = out["data"]["pending"].as_array().expect("a pending list");
    assert!(pending.len() > 1, "{pending:?}");

    // `admin migrate` applies them all; the next status is current.
    let (code, out) = admin(&scratch, &["migrate"]);
    assert_eq!(code, Some(0), "{out}");
    let (code, out) = admin(&scratch, &["migrate-status"]);
    assert_eq!(code, Some(0), "{out}");
    assert_eq!(out["data"]["status"], "current");
    assert_eq!(out["data"]["pending"], json!([]));
    let applied = out["data"]["applied"]
        .as_array()
        .expect("an applied list")
        .clone();

    // Drop the newest applied row on the scratch: the status slips to behind and names it.
    let newest = applied.last().and_then(Value::as_i64).expect("a version");
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = $1")
        .bind(newest)
        .execute(&scratch_pool)
        .await
        .expect("drop the newest applied row");
    let (code, out) = admin(&scratch, &["migrate-status"]);
    assert_eq!(code, Some(0), "{out}");
    assert_eq!(out["data"]["status"], "behind");
    assert_eq!(
        out["data"]["pending"]
            .as_array()
            .and_then(|pending| pending
                .first()
                .and_then(|row| row.as_str())
                .map(str::to_owned))
            .map(|name| name.starts_with(&newest.to_string())),
        Some(true),
        "{out}"
    );

    // A migration this binary doesn't know: exit 4, the runbook's rollback gate.
    sqlx::query(
        "INSERT INTO _sqlx_migrations
            (version, description, installed_on, success, checksum, execution_time)
         VALUES (9999, 'unknown-to-this-binary', now(), true, '\\x00', 0)",
    )
    .execute(&scratch_pool)
    .await
    .expect("insert a foreign migration row");
    let (code, out) = admin(&scratch, &["migrate-status"]);
    assert_eq!(code, Some(4), "{out}");
    assert_eq!(out["data"]["status"], "unknown");

    drop(scratch_pool);
    // Audited as above: the scratch database's own generated name.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {name} WITH (FORCE)"
    )))
    .execute(&pool)
    .await
    .expect("drop the scratch database");
}
