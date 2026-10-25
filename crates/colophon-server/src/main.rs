//! `colophon-server`: the composition root. It will load and validate
//! configuration once, build the adapters, and run the API role, the worker
//! role, or both (`docs/ARCHITECTURE.md`, "Process topology").
//!
//! Today it only accepts the documented `--role` contract. The HTTP server,
//! health checks and error envelope arrive in milestone P0, step 0.3.

use clap::{Parser, ValueEnum};

/// Which part of Colophon this process runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Role {
    /// The management and public delivery APIs.
    Api,
    /// Background work: the outbox dispatcher, retries and reconciliation.
    Worker,
    /// Both roles in one process (the v0.1 reference profile).
    All,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::Api => "api",
            Role::Worker => "worker",
            Role::All => "all",
        }
    }
}

/// The Colophon server.
#[derive(Debug, Parser)]
#[command(name = "colophon-server", version, about)]
struct Args {
    /// Which part of the server this process runs.
    #[arg(long, value_enum, default_value_t = Role::All)]
    role: Role,
}

fn main() {
    let args = Args::parse();
    eprintln!(
        "colophon-server {}: role `{}` has no services yet (they arrive in milestone P0, step 0.3)",
        env!("CARGO_PKG_VERSION"),
        args.role.as_str()
    );
}
