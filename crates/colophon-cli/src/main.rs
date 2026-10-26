//! `colophon`: the command-line interface to Colophon CMS.
//!
//! Exit codes follow the CLI contract in `docs/modules/08-cli.md`: 0 success,
//! 2 usage or validation error, 3 unauthorized, 4 conflict, 5 transient or
//! unavailable, 6 internal failure. Errors and diagnostics go to stderr; stdout
//! is reserved for results.
//!
//! Commands arrive with the milestones that make them true: `doctor` in P0,
//! `login`, `whoami`, `content create` and `content list` in P1, `release …` in P2.

mod admin;
mod client;
mod content;
mod credentials;
mod doctor;
mod login;

use std::{path::PathBuf, process::ExitCode, time::Duration};

use clap::{Parser, Subcommand};

/// The exit codes of the CLI contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exit {
    Success = 0,
    Usage = 2,
    Denied = 3,
    Conflict = 4,
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
    /// Sign in to a server: approve this terminal in a browser where you're signed in.
    Login {
        /// The server's base URL.
        #[arg(
            long,
            env = "COLOPHON_SERVER",
            default_value = "http://localhost:58000"
        )]
        server: String,
        /// The name to keep these credentials under.
        #[arg(long, default_value = "default")]
        profile: String,
        /// What the approval page calls this terminal.
        #[arg(long, default_value = "colophon CLI")]
        name: String,
        /// Print one versioned JSON result instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Sign out: revoke this profile's token and forget it.
    Logout {
        #[arg(long, default_value = "default")]
        profile: String,
    },
    /// Show who you're signed in as, and your projects.
    Whoami {
        #[arg(long, default_value = "default")]
        profile: String,
        #[arg(long)]
        json: bool,
    },
    /// Create and list entries.
    #[command(subcommand)]
    Content(ContentCommand),
    /// Operator mode: direct infrastructure access, separate from normal commands.
    #[command(subcommand)]
    Admin(AdminCommand),
}

/// Where an entry lives.
#[derive(Debug, clap::Args)]
struct Where {
    /// The collection (content type), for example `articles`.
    collection: String,
    /// The project's id or slug.
    #[arg(long)]
    project: String,
    /// The environment.
    #[arg(long, default_value = "production")]
    env: String,
    #[arg(long, default_value = "default")]
    profile: String,
}

#[derive(Debug, Subcommand)]
enum ContentCommand {
    /// Create an entry. Exits 2 if the content doesn't fit its content type, 3 if denied.
    Create {
        #[command(flatten)]
        place: Where,
        /// The content as JSON.
        #[arg(long, conflicts_with = "file")]
        data: Option<String>,
        /// A file holding the content as JSON.
        #[arg(long)]
        file: Option<PathBuf>,
        /// Reuse the key of an earlier attempt that got no answer: it can't create twice.
        #[arg(long)]
        idempotency_key: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List a collection's entries, newest first.
    List {
        #[command(flatten)]
        place: Where,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u8).range(1..=100))]
        limit: u8,
        #[arg(long)]
        json: bool,
    },
}

impl Where {
    fn place(&self) -> content::Place<'_> {
        content::Place {
            profile: &self.profile,
            project: &self.project,
            environment: &self.env,
            collection: &self.collection,
        }
    }
}

/// Prints a failure to stderr and returns its exit code.
fn finish(result: Result<(), client::Failure>) -> Exit {
    match result {
        Ok(()) => Exit::Success,
        Err(failure) => {
            eprintln!("error: {}", failure.message);
            failure.exit
        }
    }
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
        Command::Login {
            server,
            profile,
            name,
            json,
        } => finish(runtime.block_on(login::login(&server, &profile, &name, json))).into(),
        Command::Logout { profile } => finish(runtime.block_on(login::logout(&profile))).into(),
        Command::Whoami { profile, json } => {
            finish(runtime.block_on(login::whoami(&profile, json))).into()
        }
        Command::Content(ContentCommand::Create {
            place,
            data,
            file,
            idempotency_key,
            json,
        }) => finish(runtime.block_on(async {
            let content = content::content(data.as_deref(), file.as_deref()).await?;
            content::create(&place.place(), &content, idempotency_key.as_deref(), json).await
        }))
        .into(),
        Command::Content(ContentCommand::List { place, limit, json }) => {
            finish(runtime.block_on(content::list(&place.place(), limit, json))).into()
        }
    }
}
