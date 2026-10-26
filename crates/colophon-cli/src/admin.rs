//! `colophon admin …`: operator mode (docs/modules/08-cli.md). The only commands with direct
//! infrastructure access; normal commands go through the API and never need these credentials.

use std::time::Duration;

use colophon_db::{
    PgPool, PoolSettings,
    administrators::{self, Bootstrapped},
};
use serde_json::json;

use crate::Exit;

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

/// One connection as the migrator, checked; the exit code if there is none.
async fn connect(database_url: &str) -> Result<PgPool, Exit> {
    let settings = PoolSettings {
        max_connections: 1,
        acquire_timeout: Duration::from_secs(10),
        // Migrations may take longer than the application's statement limit.
        statement_timeout: Duration::from_secs(300),
    };
    let pool = colophon_db::connect_lazy(database_url, settings).map_err(|error| {
        eprintln!("error: {error}");
        Exit::Usage
    })?;
    if let Err(error) = colophon_db::ping(&pool, Duration::from_secs(10)).await {
        eprintln!("error: can't reach the database: {}", chain(&error));
        return Err(Exit::Unavailable);
    }
    Ok(pool)
}

/// `colophon admin migrate`: applies pending schema migrations as the migrator role.
pub(crate) async fn migrate(database_url: &str) -> Exit {
    let pool = match connect(database_url).await {
        Ok(pool) => pool,
        Err(exit) => return exit,
    };
    let known = colophon_db::MIGRATOR.iter().count();
    match colophon_db::migrate(&pool).await {
        Ok(()) => {
            println!("migrations up to date ({known} known)");
            Exit::Success
        }
        Err(error) => {
            eprintln!("error: {}", chain(&error));
            Exit::Internal
        }
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
pub(crate) async fn bootstrap(database_url: &str, issuer: &str, subject: &str, json: bool) -> Exit {
    if let Err(message) = check(issuer, subject) {
        eprintln!("error: {message}");
        return Exit::Usage;
    }
    let pool = match connect(database_url).await {
        Ok(pool) => pool,
        Err(exit) => return exit,
    };
    let (status, identity) = match administrators::bootstrap(&pool, issuer, subject).await {
        Ok(Bootstrapped::Done { identity }) => ("bootstrapped", identity),
        Ok(Bootstrapped::AlreadyDone { identity }) => ("already_bootstrapped", identity),
        Ok(Bootstrapped::Refused) => {
            eprintln!(
                "error: this installation was already bootstrapped for someone else; bootstrap runs once"
            );
            return Exit::Conflict;
        }
        Err(error) => {
            eprintln!("error: {}", chain(&error));
            return Exit::Internal;
        }
    };
    if json {
        let result = json!({
            "version": 1, "command": "admin bootstrap", "status": status,
            "identity": identity, "issuer": issuer, "subject": subject,
        });
        println!("{result}");
    } else if status == "bootstrapped" {
        println!(
            "{subject} at {issuer} is now this installation's first administrator; bootstrap is now disabled"
        );
    } else {
        println!("already bootstrapped for {subject} at {issuer}; nothing changed");
    }
    Exit::Success
}

#[cfg(test)]
mod tests {
    use super::check;

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
}
