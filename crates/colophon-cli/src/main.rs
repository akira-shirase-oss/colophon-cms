//! `colophon`: the command-line interface to Colophon CMS.
//!
//! Exit codes follow the CLI contract in `docs/modules/08-cli.md`: 0 success,
//! 2 usage or validation error, 3 unauthorized, 4 conflict, 5 transient or
//! unavailable, 6 internal failure. Errors and diagnostics go to stderr; stdout
//! is reserved for results.
//!
//! There are no commands yet. They arrive with the milestones that make them
//! true: `content create` and `content list` in P1, `release …` in P2.

use clap::Parser;

/// Write, review and publish content from the terminal.
#[derive(Debug, Parser)]
#[command(name = "colophon", version, about, arg_required_else_help = true)]
struct Cli {}

fn main() {
    // With no commands defined, clap handles every invocation itself: `--help`
    // and `--version` exit 0, anything else is a usage error and exits 2.
    let _cli = Cli::parse();
}
