//! `colophon`: the command-line interface to Colophon CMS.
//!
//! Exit codes follow the CLI contract in `docs/modules/08-cli.md`: 0 success,
//! 2 usage or validation error, 3 unauthorized, 4 conflict, 5 transient or
//! unavailable, 6 internal failure. Errors and diagnostics go to stderr; stdout
//! is reserved for results.
//!
//! Commands arrive with the milestones that make them true: `doctor` in P0,
//! `content create` and `content list` in P1, `release …` in P2.

mod admin;
mod doctor;

use std::{process::ExitCode, time::Duration};

use clap::{Parser, Subcommand};

/// The exit codes of the CLI contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exit {
    Success = 0,
    Usage = 2,
    Unavailable = 5,
    Internal = 6,
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        Self::from(exit as u8)
    }
}

/// Write, review and publish content from the terminal.
#[derive(Debug, Parser)]
#[command(name = "colophon", version, about, arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check whether a Colophon server is ready, and which dependency is not.
    ///
    /// Exits 0 when the server is ready, 5 when it is degraded, not ready or unreachable.
    Doctor {
        /// The server's base URL.
        #[arg(
            long,
            env = "COLOPHON_SERVER",
            default_value = "http://localhost:58000"
        )]
        server: String,
        /// Give up after this many milliseconds.
        #[arg(long, default_value_t = 5_000, value_parser = clap::value_parser!(u64).range(1..=120_000))]
        timeout_ms: u64,
        /// Print one versioned JSON result instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Operator mode: direct infrastructure access, separate from normal commands.
    #[command(subcommand)]
    Admin(AdminCommand),
}

#[derive(Debug, Subcommand)]
enum AdminCommand {
    /// Apply pending database migrations, as the migrator role (the table owner).
    Migrate {
        /// The migrator's connection URL. Never the application's.
        #[arg(long, env = "COLOPHON_MIGRATION_DATABASE_URL", hide_env_values = true)]
        database_url: String,
    },
}

fn main() -> ExitCode {
    // Usage errors, `--help` and `--version` are handled by clap (exit 2, 0, 0).
    let cli = Cli::parse();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("error: could not start the async runtime: {error}");
            return Exit::Internal.into();
        }
    };
    match cli.command {
        Command::Doctor {
            server,
            timeout_ms,
            json,
        } => runtime
            .block_on(doctor::run(
                &server,
                Duration::from_millis(timeout_ms),
                json,
            ))
            .into(),
        Command::Admin(AdminCommand::Migrate { database_url }) => {
            runtime.block_on(admin::migrate(&database_url)).into()
        }
    }
}
