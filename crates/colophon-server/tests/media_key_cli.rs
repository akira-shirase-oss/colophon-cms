//! White-hat checks of the media signing key's operator paths (D-233) against the real `colophon` binary and a
//! scratch database on the local stack: `admin media-key rotate` run with the API's login instead of the
//! migrator's is refused and changes nothing; neither the rotate nor the restore's epoch stage prints a key, in
//! text or JSON, on success or failure; and the epoch stage leaves exactly one key, new, every time it runs (a
//! rerun after a failed later step included). Needs the stack.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use std::{fmt::Write as _, path::PathBuf, process::Command};

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE_NO_PAD},
};
use common::{local_env, unique_slug};
use serde_json::Value;
use sqlx::PgPool;

/// The migration database URL from `deploy/local.env` for `database`, as `login` (`colophon` is the migrator,
/// `colophon_app` the API's login).
fn database_url(login: &str, database: &str) -> String {
    let url = local_env()
        .into_iter()
        .find_map(|(key, value)| (key == "COLOPHON_MIGRATION_DATABASE_URL").then_some(value))
        .expect("deploy/local.env names the migration database");
    let (base, _) = url.rsplit_once('/').expect("the URL names a database");
    let (_, host) = base.rsplit_once('@').expect("the URL names a login");
    format!("postgres://{login}:colophon-dev@{host}/{database}")
}

/// `colophon admin <args> --database-url <url>`: (exit code, stdout, stdout and stderr together).
fn admin(url: &str, args: &[&str]) -> (Option<i32>, String, String) {
    let cli = PathBuf::from(env!("CARGO_BIN_EXE_colophon-server")).with_file_name("colophon");
    let output = Command::new(cli)
        .arg("admin")
        .args(args)
        .args(["--database-url", url])
        .env_clear()
        .envs(local_env())
        .output()
        .expect("the CLI runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let both = format!("{stdout}\n{}", String::from_utf8_lossy(&output.stderr));
    (output.status.code(), stdout, both)
}

fn json(stdout: &str) -> Value {
    serde_json::from_str(stdout).unwrap_or_else(|_| panic!("no JSON on stdout: {stdout}"))
}

async fn keys(pool: &PgPool) -> Vec<(i16, Vec<u8>)> {
    sqlx::query_as("SELECT id, key FROM media_signing_keys ORDER BY id DESC")
        .fetch_all(pool)
        .await
        .expect("the keys")
}

async fn rotations(pool: &PgPool) -> Vec<(i16, bool, String)> {
    sqlx::query_as("SELECT kid, retired_previous, reason FROM media_key_rotations ORDER BY kid")
        .fetch_all(pool)
        .await
        .expect("the rotations")
}

/// The ways a key's bytes could be printed, whole or its first part (hex either case, base64, `Debug`).
fn printed_as(key: &[u8]) -> Vec<String> {
    let hex = |bytes: &[u8]| {
        bytes.iter().fold(String::new(), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
    };
    let mut forms = Vec::new();
    for part in [key, &key[..12]] {
        forms.push(hex(part));
        forms.push(hex(part).to_uppercase());
        forms.push(STANDARD.encode(part));
        forms.push(STANDARD_NO_PAD.encode(part));
        forms.push(URL_SAFE_NO_PAD.encode(part));
    }
    forms.push(
        key[..8]
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(", "),
    );
    forms
}

/// Fails, naming `what` and the form's place but never the form, if any key in `seen` is in `output`.
fn holds_no_key(seen: &[Vec<u8>], output: &str, what: &str) {
    for key in seen {
        for (form, printed) in printed_as(key).iter().enumerate() {
            assert!(
                !output.contains(printed),
                "{what} printed a media key (form {form})"
            );
        }
    }
}

#[tokio::test]
#[ignore = "needs the local stack's postgres"]
#[expect(
    clippy::too_many_lines,
    reason = "one scratch installation's operator paths, in order"
)]
async fn only_the_migrator_rotates_no_output_holds_a_key_and_a_restore_leaves_one_new_key() {
    let name = format!(
        "colophon_mk_{}",
        unique_slug()
            .chars()
            .filter(|c| *c != '-')
            .collect::<String>()
    );
    let pool = common::owner_database().await;
    // Audited: `name` is `colophon_mk_` plus a uuid's hex digits, so no quoting can be in it.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
        .execute(&pool)
        .await
        .expect("create the scratch database");
    let migrator = database_url("colophon", &name);
    let api = database_url("colophon_app", &name);
    let (code, _, out) = admin(&migrator, &["migrate"]);
    assert_eq!(code, Some(0), "{out}");
    let scratch = PgPool::connect(&migrator)
        .await
        .expect("the scratch database");
    let mut seen: Vec<Vec<u8>> = keys(&scratch)
        .await
        .into_iter()
        .map(|(_, key)| key)
        .collect();
    assert_eq!(seen.len(), 1, "0082's key");

    // The API's login, text and JSON: refused, nothing changed, no key printed.
    for json_flag in [false, true] {
        let mut args = vec!["media-key", "rotate", "--retire-previous"];
        if json_flag {
            args.push("--json");
        }
        let (code, _, out) = admin(&api, &args);
        assert_ne!(code, Some(0), "the API's login rotated: {out}");
        holds_no_key(&seen, &out, "a refused rotate");
    }
    assert_eq!(
        keys(&scratch)
            .await
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>(),
        [1],
        "a refused rotate left the keys as they were"
    );
    assert!(rotations(&scratch).await.is_empty(), "and wrote no line");

    // The migrator: a rotation in text, one in JSON; each prints the id, never the key.
    let (code, stdout, out) = admin(&migrator, &["media-key", "rotate"]);
    assert_eq!(code, Some(0), "{out}");
    assert!(stdout.contains("new media signing key 2"), "{stdout}");
    seen.extend(keys(&scratch).await.into_iter().map(|(_, key)| key));
    holds_no_key(&seen, &out, "rotate");
    let (code, stdout, out) = admin(&migrator, &["media-key", "rotate", "--json"]);
    assert_eq!(code, Some(0), "{out}");
    let answer = json(&stdout);
    assert_eq!(answer["data"]["media_key_id"], 3, "{answer}");
    assert_eq!(answer["data"]["retired_previous"], false, "{answer}");
    seen.extend(keys(&scratch).await.into_iter().map(|(_, key)| key));
    holds_no_key(&seen, &out, "rotate --json");
    assert_eq!(
        keys(&scratch)
            .await
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>(),
        [3, 2]
    );

    // The restore's epoch stage, run twice (the procedure reruns it when a later step of the stage failed):
    // each run leaves exactly one key, newer than and different from every key before it, and says its id.
    for (run, json_flag) in [(4_i16, true), (5, false)] {
        let before: Vec<Vec<u8>> = keys(&scratch)
            .await
            .into_iter()
            .map(|(_, key)| key)
            .collect();
        let mut args = vec!["restore", "epoch"];
        if json_flag {
            args.push("--json");
        }
        let (code, stdout, out) = admin(&migrator, &args);
        assert_eq!(code, Some(0), "{out}");
        let now = keys(&scratch).await;
        assert_eq!(
            now.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [run],
            "exactly one key, the new one"
        );
        assert!(
            !before.contains(&now[0].1),
            "the restored copy holds none of the keys before it"
        );
        if json_flag {
            assert_eq!(json(&stdout)["data"]["media_key_id"], run, "{stdout}");
        } else {
            assert!(
                stdout.contains(&format!("new media signing key {run}")),
                "{stdout}"
            );
        }
        seen.extend(now.into_iter().map(|(_, key)| key));
        holds_no_key(&seen, &out, "restore epoch");
    }
    assert_eq!(
        rotations(&scratch).await,
        [
            (2, false, "admin".to_owned()),
            (3, false, "admin".to_owned()),
            (4, true, "restore".to_owned()),
            (5, true, "restore".to_owned()),
        ]
    );

    scratch.close().await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {name} WITH (FORCE)"
    )))
    .execute(&pool)
    .await
    .expect("drop the scratch database");
}
