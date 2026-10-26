//! `colophon admin …`: operator mode (docs/modules/08-cli.md). The only commands with direct
//! infrastructure access; normal commands go through the API and never need these credentials.

use std::time::Duration;

use colophon_db::PoolSettings;

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

/// `colophon admin migrate`: applies pending schema migrations as the migrator role.
pub(crate) async fn migrate(database_url: &str) -> Exit {
    let settings = PoolSettings {
        max_connections: 1,
        acquire_timeout: Duration::from_secs(10),
        // Migrations may take longer than the application's statement limit.
        statement_timeout: Duration::from_secs(300),
    };
    let pool = match colophon_db::connect_lazy(database_url, settings) {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("error: {error}");
            return Exit::Usage;
        }
    };
    if let Err(error) = colophon_db::ping(&pool, Duration::from_secs(10)).await {
        eprintln!("error: can't reach the database: {}", chain(&error));
        return Exit::Unavailable;
    }
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
