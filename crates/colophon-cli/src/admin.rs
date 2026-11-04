//! `colophon admin …`: operator mode (docs/modules/08-cli.md). The only commands with direct
//! infrastructure access; normal commands go through the API and never need these credentials.

use std::io::{Read as _, Write as _};
use std::time::Duration;

use object_store::ObjectStore as _;

use colophon_db::{
    PgPool, PoolSettings,
    administrators::{self, Bootstrapped},
    ledger,
};
use serde_json::json;

use crate::{
    Exit,
    out::{self, Out},
};

/// `outer: inner: innermost`: an operator needs the cause, not just "query failed".
fn chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

/// Reports an operator-side failure the way a command does: `error: …` on stderr, and the
/// `error` envelope on a machine format.
fn fail(out: &Out, exit: Exit, message: &str) -> Exit {
    eprintln!("error: {message}");
    out.print_error(&out::cli_error(exit.class(), message));
    exit
}

/// One connection as the migrator, checked; the exit code if there is none.
async fn connect(database_url: &str, out: &Out) -> Result<PgPool, Exit> {
    let settings = PoolSettings {
        max_connections: 1,
        acquire_timeout: Duration::from_secs(10),
        // Migrations may take longer than the application's statement limit.
        statement_timeout: Duration::from_secs(300),
    };
    let pool = colophon_db::connect_lazy(database_url, settings)
        .map_err(|error| fail(out, Exit::Usage, &error.to_string()))?;
    if let Err(error) = colophon_db::ping(&pool, Duration::from_secs(10)).await {
        return Err(fail(
            out,
            Exit::Unavailable,
            &format!("can't reach the database: {}", chain(&error)),
        ));
    }
    Ok(pool)
}

/// `colophon admin migrate`: applies pending schema migrations as the migrator role.
pub(crate) async fn migrate(database_url: &str, out: Out) -> Exit {
    let pool = match connect(database_url, &out).await {
        Ok(pool) => pool,
        Err(exit) => return exit,
    };
    let known = colophon_db::MIGRATOR.iter().count();
    match colophon_db::migrate(&pool).await {
        Ok(()) => {
            if out.format.machine() {
                out.print_data(&json!({ "status": "up_to_date", "known_migrations": known }));
            } else {
                println!("migrations up to date ({known} known)");
            }
            Exit::Success
        }
        Err(error) => fail(&out, Exit::Internal, &chain(&error)),
    }
}

/// Why an issuer or subject was refused, before touching the database.
fn check(issuer: &str, subject: &str) -> Result<(), &'static str> {
    let local = [
        "http://localhost:",
        "http://localhost/",
        "http://127.0.0.1:",
    ];
    if issuer.len() > 512
        || !(issuer.starts_with("https://") || local.iter().any(|start| issuer.starts_with(start)))
    {
        return Err(
            "the issuer is the identity provider's issuer URL: https, or http on localhost",
        );
    }
    if subject.is_empty() || subject.len() > 255 || subject.chars().any(char::is_control) {
        return Err("the subject is 1 to 255 characters, without control characters");
    }
    Ok(())
}

/// `colophon admin bootstrap`: makes `(issuer, subject)` the installation's first administrator,
/// once (D-043). Exits 4 if it was already bootstrapped for someone else.
pub(crate) async fn bootstrap(database_url: &str, issuer: &str, subject: &str, out: Out) -> Exit {
    if let Err(message) = check(issuer, subject) {
        return fail(&out, Exit::Usage, message);
    }
    let pool = match connect(database_url, &out).await {
        Ok(pool) => pool,
        Err(exit) => return exit,
    };
    let (status, identity) = match administrators::bootstrap(&pool, issuer, subject).await {
        Ok(Bootstrapped::Done { identity }) => ("bootstrapped", identity),
        Ok(Bootstrapped::AlreadyDone { identity }) => ("already_bootstrapped", identity),
        Ok(Bootstrapped::Refused) => {
            return fail(
                &out,
                Exit::Conflict,
                "this installation was already bootstrapped for someone else; bootstrap runs once",
            );
        }
        Err(error) => return fail(&out, Exit::Internal, &chain(&error)),
    };
    if out.format.machine() {
        out.print_data(&json!({
            "status": status,
            "identity": identity, "issuer": issuer, "subject": subject,
        }));
    } else if status == "bootstrapped" {
        println!(
            "{subject} at {issuer} is now this installation's first administrator; bootstrap is now disabled"
        );
    } else {
        println!("already bootstrapped for {subject} at {issuer}; nothing changed");
    }
    Exit::Success
}

/// The most bytes an `--s3-key` export buffers for its single `put`, and the most a replayed
/// file may hold (bounded everything): at ~200 bytes a row it is hundreds of thousands of
/// deletions, far past plausible; past it, export to a file and copy the file.
const OBJECT_CAP: usize = 64 * 1024 * 1024;

/// A `Write` that refuses to grow past `cap`: the off-host export's sink when the destination
/// is one `put`.
struct Capped {
    buffer: Vec<u8>,
    cap: usize,
}

impl std::io::Write for Capped {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.buffer.len() + bytes.len() > self.cap {
            return Err(std::io::Error::other(
                "the export outgrew the in-memory bound; export --out a file and copy the file",
            ));
        }
        self.buffer.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The `COLOPHON_S3_*` object store, as the server builds it (D-016); the error text names
/// the missing setting.
fn object_store() -> Result<object_store::aws::AmazonS3, String> {
    fn setting(name: &str) -> Result<String, String> {
        std::env::var(name).map_err(|_| format!("{name} is not set"))
    }
    let endpoint = setting("COLOPHON_S3_ENDPOINT")?;
    object_store::aws::AmazonS3Builder::new()
        .with_endpoint(&endpoint)
        .with_region(std::env::var("COLOPHON_S3_REGION").unwrap_or_else(|_| "garage".into()))
        .with_bucket_name(setting("COLOPHON_S3_BUCKET")?)
        .with_access_key_id(setting("COLOPHON_S3_ACCESS_KEY")?)
        .with_secret_access_key(setting("COLOPHON_S3_SECRET_KEY")?)
        .with_allow_http(endpoint.starts_with("http://"))
        .build()
        .map_err(|error| error.to_string())
}

/// `colophon admin ledger export`: writes the deletion ledger, id-ordered to a high-water
/// mark, as one JSONL snapshot — the off-host copy restore step 4 replays (O2, D-124).
pub(crate) async fn ledger_export(
    database_url: &str,
    file: Option<&str>,
    s3_key: Option<&str>,
    after: i64,
    out: Out,
) -> Exit {
    let pool = match connect(database_url, &out).await {
        Ok(pool) => pool,
        Err(exit) => return exit,
    };
    if let Some(key) = s3_key {
        let mut capped = Capped {
            buffer: Vec::new(),
            cap: OBJECT_CAP,
        };
        let exported = match ledger::export(&pool, after, &mut capped).await {
            Ok(exported) => exported,
            Err(error) => return fail(&out, Exit::Internal, &chain(&error)),
        };
        let store = match object_store() {
            Ok(store) => store,
            Err(message) => return fail(&out, Exit::Usage, &message),
        };
        let put = tokio::time::timeout(
            Duration::from_secs(60),
            store.put_opts(
                &object_store::path::Path::from(key),
                capped.buffer.into(),
                object_store::PutOptions::default(),
            ),
        )
        .await;
        return match put {
            Ok(Ok(_)) => report_export(
                &out,
                &format!("object {key}"),
                exported.high_water,
                exported.rows,
            ),
            Ok(Err(error)) => fail(&out, Exit::Internal, &error.to_string()),
            Err(_) => fail(&out, Exit::Unavailable, "the object store put timed out"),
        };
    }
    let to_stdout = file == Some("-");
    let result = if to_stdout {
        let stdout = std::io::stdout();
        let mut locked = stdout.lock();
        ledger::export(&pool, after, &mut locked).await
    } else {
        let path = file.unwrap_or_default();
        match std::fs::File::create(path) {
            Ok(created) => {
                let mut writer = std::io::BufWriter::new(created);
                match ledger::export(&pool, after, &mut writer).await {
                    Err(error) => Err(error),
                    Ok(exported) => writer
                        .flush()
                        .map(|()| exported)
                        .map_err(ledger::LedgerError::Sink),
                }
            }
            Err(error) => Err(ledger::LedgerError::Sink(error)),
        }
    };
    match result {
        Ok(exported) => {
            let destination = if to_stdout {
                "stdout".to_owned()
            } else {
                format!("file {}", file.unwrap_or_default())
            };
            report_export(&out, &destination, exported.high_water, exported.rows)
        }
        Err(error) => fail(&out, Exit::Internal, &chain(&error)),
    }
}

fn report_export(out: &Out, destination: &str, high_water: i64, rows: u64) -> Exit {
    if out.format.machine() {
        out.print_data(&json!({
            "status": "exported", "destination": destination,
            "high_water": high_water, "rows": rows,
        }));
    } else if destination != "stdout" {
        println!(
            "exported the deletion ledger to {destination} ({rows} rows, through id {high_water})"
        );
    }
    Exit::Success
}

/// `colophon admin ledger replay`: reapplies an export's interval on the restored database
/// (O2's restore step 4). Exits `conflict` while any row can't replay — O2 keeps the
/// installation closed until the gap is resolved.
pub(crate) async fn ledger_replay(database_url: &str, input: &str, out: Out) -> Exit {
    let pool = match connect(database_url, &out).await {
        Ok(pool) => pool,
        Err(exit) => return exit,
    };
    let bytes = if input == "-" {
        let mut bytes = Vec::new();
        match std::io::Read::take(&mut std::io::stdin().lock(), OBJECT_CAP as u64 + 1)
            .read_to_end(&mut bytes)
        {
            Ok(_) => bytes,
            Err(error) => return fail(&out, Exit::Usage, &error.to_string()),
        }
    } else {
        match std::fs::read(input) {
            Ok(bytes) => bytes,
            Err(error) => {
                return fail(&out, Exit::Usage, &format!("can't read {input}: {error}"));
            }
        }
    };
    if bytes.len() > OBJECT_CAP {
        return fail(
            &out,
            Exit::Usage,
            "the export is larger than the replay bound",
        );
    }
    let parsed = match ledger::unmarshal(&bytes) {
        Ok(parsed) => parsed,
        Err(error) => return fail(&out, Exit::Usage, &error.to_string()),
    };
    let rows_in_file = parsed.rows.len();
    match ledger::replay(&pool, &parsed.rows).await {
        Ok(replayed) if replayed.conflicts.is_empty() => {
            if out.format.machine() {
                out.print_data(&json!({
                    "status": "replayed", "high_water": parsed.high_water,
                    "rows": rows_in_file, "applied": replayed.applied,
                    "skipped": replayed.skipped,
                }));
            } else {
                println!(
                    "replayed the deletion ledger through id {} ({} applied, {} already replayed)",
                    parsed.high_water, replayed.applied, replayed.skipped
                );
            }
            Exit::Success
        }
        Ok(replayed) => {
            let conflicts: Vec<_> = replayed
                .conflicts
                .iter()
                .map(|conflict| json!({"id": conflict.id, "reason": conflict.reason}))
                .collect();
            if out.format.machine() {
                out.print_data(&json!({
                    "status": "blocked", "high_water": parsed.high_water,
                    "applied": replayed.applied, "skipped": replayed.skipped,
                    "conflicts": conflicts,
                }));
            } else {
                eprintln!(
                    "{} ledger rows could not be replayed; the restore stays blocked (O2):",
                    conflicts.len()
                );
                for conflict in &conflicts {
                    eprintln!("  {conflict}");
                }
            }
            Exit::Conflict
        }
        Err(error) => fail(&out, Exit::Internal, &chain(&error)),
    }
}

#[cfg(test)]
mod tests {
    use super::{Capped, check};

    #[test]
    fn issuers_and_subjects_are_checked_before_the_database() {
        for (issuer, subject, ok) in [
            ("https://id.example.com/realms/main", "f6c1a2", true),
            ("http://localhost:58080/realms/colophon", "f6c1a2", true),
            ("http://127.0.0.1:58080/realms/colophon", "f6c1a2", true),
            ("http://id.example.com", "f6c1a2", false),
            ("http://localhost.example.com", "f6c1a2", false),
            ("https://id.example.com", "", false),
            ("https://id.example.com", "a\nb", false),
            ("", "f6c1a2", false),
        ] {
            assert_eq!(check(issuer, subject).is_ok(), ok, "{issuer} {subject:?}");
        }
    }

    #[test]
    fn the_capped_sink_refuses_to_grow_past_its_bound() {
        use std::io::Write as _;
        let mut capped = Capped {
            buffer: Vec::new(),
            cap: 8,
        };
        assert_eq!(capped.write(b"12345").expect("fits"), 5);
        assert_eq!(capped.write(b"678").expect("exactly full"), 3);
        assert!(capped.write(b"9").is_err(), "one byte past the bound");
        assert_eq!(capped.buffer.len(), 8);
    }
}
