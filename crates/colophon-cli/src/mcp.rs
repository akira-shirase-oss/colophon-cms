//! `colophon mcp`: the same MCP stdio server the standalone `colophon-mcp` binary runs
//! (docs/modules/09-automation.md, "MCP adapter"; D-106, D-119) — the same `Config`,
//! `Api` and `serve` calls, linked into this binary rather than spawned, so the agent
//! host's stdin and stdout are this process's own. Configuration comes from the
//! environment (`COLOPHON_SERVER`, `COLOPHON_TOKEN`, `COLOPHON_PROJECT`); logs go to
//! stderr only, at the level in `COLOPHON_MCP_LOG` (`info` by default), and never hold
//! the token or content.
//!
//! Exit codes are the MCP binary's, not the CLI contract's: 0 when the client closed
//! the input, 1 when the session failed, 2 for invalid configuration.

use std::process::ExitCode;

use colophon_mcp::{Config, api::Api};
use tracing_subscriber::EnvFilter;

/// Runs `colophon mcp`.
pub(crate) async fn run() -> ExitCode {
    // The CLI installs no other subscriber; `try_init` keeps serving even if one ever
    // did — protocol on stdout matters more than logs on stderr.
    let _unused = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(
            EnvFilter::try_from_env("COLOPHON_MCP_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .try_init();
    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("colophon mcp: {error}");
            return ExitCode::from(2);
        }
    };
    let api = match Api::new(&config) {
        Ok(api) => api,
        Err(error) => {
            eprintln!("colophon mcp: {error}");
            return ExitCode::from(1);
        }
    };
    tracing::info!(
        server = %config.server,
        project = %config.project,
        protocol = colophon_mcp::PROTOCOL_VERSION,
        "serving MCP on stdio"
    );
    match colophon_mcp::serve(api, tokio::io::stdin(), tokio::io::stdout()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "the session ended with an error");
            ExitCode::from(1)
        }
    }
}
