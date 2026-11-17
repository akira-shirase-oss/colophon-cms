//! Thin binary: parse the role, load and validate configuration, set up logging, run.

use anyhow::Context as _;
use clap::Parser;
use colophon_server::{Config, Role, ShutdownOutcome, shutdown, telemetry};

/// The static Linux (musl) build allocates through mimalloc (D-176). musl's own allocator
/// serializes threads on one lock: measured on one runner against a glibc build of the same
/// commit, rich-text validation ran at a fifth of the throughput and three times the p99.
#[cfg(all(target_os = "linux", target_env = "musl"))]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// The Colophon server. Configuration comes from `COLOPHON_*` environment variables; see
/// `crates/colophon-server/src/config.rs` for the list.
#[derive(Debug, Parser)]
#[command(name = "colophon-server", version = colophon_version::long_version!(), about)]
struct Args {
    /// Which part of the server this process runs.
    #[arg(long, value_enum, env = "COLOPHON_ROLE", default_value_t = Role::All)]
    role: Role,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    // Fail fast: an invalid environment exits non-zero with every problem listed.
    let config = Config::from_env(args.role).context("loading configuration")?;
    telemetry::init(config.log_format, &config.log_filter).context("setting up logging")?;
    let signal = shutdown::os_signal().context("installing signal handlers")?;

    match colophon_server::run(config, signal).await? {
        ShutdownOutcome::Graceful => tracing::info!("shutdown complete"),
        ShutdownOutcome::DeadlineExceeded => {
            tracing::warn!("shutdown complete, abandoning requests still in flight");
        }
    }
    Ok(())
}
