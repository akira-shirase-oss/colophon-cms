//! `colophon`: the command-line interface to Colophon CMS.
//!
//! Exit codes follow the CLI contract in `docs/modules/08-cli.md`: 0 success,
//! 2 usage or validation error, 3 unauthorized, 4 conflict, 5 transient or
//! unavailable, 6 internal failure. Diagnostics go to stderr; stdout carries results,
//! machine-readable as the `data`/`error` envelope under `--json` and `--jsonl` (D-119).
//!
//! Commands arrive with the milestones that make them true: `doctor` in P0,
//! `login`, `whoami`, `content create` and `content list` in P1, `release …` in P2,
//! and — with the API's collaboration and migration routes (M.5, E1) — `content query`,
//! `release …`, `migration …`, `comment …` and `task …`. `mcp` is P5: the same MCP
//! stdio server the `colophon-mcp` binary serves, linked into this binary.

mod admin;
mod client;
mod collaboration;
mod compat;
mod content;
mod credentials;
mod doctor;
mod login;
mod mcp;
mod migration;
mod out;
mod release;
mod scope;

use std::{path::PathBuf, process::ExitCode, time::Duration};

use clap::{Parser, Subcommand};
use tokio::runtime::Runtime;

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

impl Exit {
    /// The error class the code stands for — the `code` a failure the CLI itself raised
    /// gets in the `error` envelope.
    fn class(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Usage => "usage",
            Self::Denied => "denied",
            Self::Conflict => "conflict",
            Self::Unavailable => "unavailable",
            Self::Internal => "internal",
        }
    }
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        Self::from(exit as u8)
    }
}

/// Write, review and publish content from the terminal.
#[derive(Debug, Parser)]
#[command(
    name = "colophon",
    version = colophon_version::long_version!(),
    about,
    arg_required_else_help = true
)]
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
        /// Print one versioned JSON result instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Show who you're signed in as, and your projects.
    Whoami {
        #[arg(long, default_value = "default")]
        profile: String,
        #[arg(long)]
        json: bool,
    },
    /// Create, list and query entries.
    #[command(subcommand)]
    Content(ContentCommand),
    /// List releases and read one release's receipt.
    #[command(subcommand)]
    Release(ReleaseCommand),
    /// Plan, inspect and drive content schema migrations.
    #[command(subcommand)]
    Migration(MigrationCommand),
    /// List and write comments on an entry's revisions.
    #[command(subcommand)]
    Comment(CommentCommand),
    /// List, create and update the environment's tasks.
    #[command(subcommand)]
    Task(TaskCommand),
    /// Operator mode: direct infrastructure access, separate from normal commands.
    #[command(subcommand)]
    Admin(AdminCommand),
    /// Serve MCP on this process's stdin and stdout: the same server `colophon-mcp`
    /// is, for an agent host to start. Configuration comes from the environment
    /// (`COLOPHON_SERVER`, `COLOPHON_TOKEN`, `COLOPHON_PROJECT`); logs go to stderr.
    Mcp,
}

/// The project and environment a command works in.
#[derive(Debug, clap::Args)]
struct InProject {
    /// The project's id or slug.
    #[arg(long)]
    project: String,
    /// The environment.
    #[arg(long, default_value = "production")]
    env: String,
    #[arg(long, default_value = "default")]
    profile: String,
}

impl InProject {
    fn scope(&self) -> scope::Scope<'_> {
        scope::Scope {
            profile: &self.profile,
            project: &self.project,
            environment: &self.env,
        }
    }
}

/// Where an entry lives.
#[derive(Debug, clap::Args)]
struct Where {
    /// The collection (content type), for example `articles`.
    collection: String,
    #[command(flatten)]
    scope: InProject,
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
        /// `next_cursor` from a previous page; `--jsonl` starts its stream there.
        #[arg(long)]
        cursor: Option<String>,
        /// Print one versioned JSON result instead of text.
        #[arg(long)]
        json: bool,
        /// Print one JSON object per entry, following every page.
        #[arg(long, conflicts_with = "json")]
        jsonl: bool,
    },
    /// Run a query document against a collection's entries, one page at a time.
    ///
    /// The document goes to the server as it is; its `applied` answer reports the canonical
    /// query and its hash. Exits 2 if the document isn't a query the collection answers.
    Query {
        #[command(flatten)]
        place: Where,
        /// The query document as JSON.
        #[arg(long, conflicts_with = "file")]
        data: Option<String>,
        /// A file holding the query document as JSON.
        #[arg(long)]
        file: Option<PathBuf>,
        /// `next_cursor` from the previous page; `--jsonl` starts its stream there.
        #[arg(long)]
        cursor: Option<String>,
        /// The page's `limit` (1–100); refused if the document sets one already.
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=100))]
        limit: Option<u8>,
        /// Print one versioned JSON result instead of text.
        #[arg(long)]
        json: bool,
        /// Print one JSON object per entry, following every page.
        #[arg(long, conflicts_with = "json")]
        jsonl: bool,
    },
    /// Import entries from a file, in one transaction or not at all.
    ///
    /// The file is the API's own input, sent as it is: JSONL (one content document a
    /// line), a JSON array, or a Strapi or Sanity export. `--dry-run` reports every
    /// item's plan and writes nothing.
    Import {
        #[command(flatten)]
        place: Where,
        /// The items inline: a JSON array or one JSONL document.
        #[arg(long, conflicts_with = "file")]
        data: Option<String>,
        /// The file to import: JSONL, a JSON array, or a Strapi/Sanity export.
        #[arg(long)]
        file: Option<PathBuf>,
        /// Plan every item and report it — `would_create`, `would_update`, `skipped`,
        /// `invalid` — writing nothing and needing no idempotency key.
        #[arg(long)]
        dry_run: bool,
        /// The Sanity `_type` to take from a file that mixes types; documents of other
        /// types are skipped and reported.
        #[arg(long = "type")]
        source_type: Option<String>,
        /// Reuse the key of an earlier attempt that got no answer: it can't import twice.
        /// A dry run plans without one.
        #[arg(long)]
        idempotency_key: Option<String>,
        /// Print one versioned JSON result instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Export the collection's drafts as JSONL — the file `content import` takes back.
    ///
    /// Text mode writes the file itself to stdout (`colophon content export posts >
    /// posts.jsonl`), one document a line as it arrives; `--jsonl` wraps each in the
    /// versioned envelope; `--json` collects the bounded export into one object.
    Export {
        #[command(flatten)]
        place: Where,
        /// Print one versioned JSON result: the whole export in one object.
        #[arg(long)]
        json: bool,
        /// Print one JSON envelope per entry, as its line arrives.
        #[arg(long, conflicts_with = "json")]
        jsonl: bool,
    },
}

/// Releases are read-only here: publishing stays a person's call in the Studio and API.
#[derive(Debug, Subcommand)]
enum ReleaseCommand {
    /// List the environment's releases, newest first.
    List {
        #[command(flatten)]
        scope: InProject,
        /// Print one versioned JSON result instead of text.
        #[arg(long)]
        json: bool,
        /// Print one JSON object per release.
        #[arg(long, conflicts_with = "json")]
        jsonl: bool,
    },
    /// Show one release: its changeset, counts, deliveries and probe observations.
    Show {
        #[command(flatten)]
        scope: InProject,
        /// The release's generation, as `release list` prints it.
        #[arg(value_parser = clap::value_parser!(u64).range(1..))]
        generation: u64,
        /// Print one versioned JSON result instead of text.
        #[arg(long)]
        json: bool,
    },
}

/// Migration plans, for humans: a credentialed member may plan, approve, apply and resolve
/// — the API checks `schema.change` on every one. These commands are intentionally absent
/// from the MCP server (D-106).
#[derive(Debug, Subcommand)]
enum MigrationCommand {
    /// List the environment's migration plans, newest first.
    List {
        #[command(flatten)]
        scope: InProject,
        /// Only this collection's plans.
        #[arg(long)]
        collection: Option<String>,
        /// Only plans in this state.
        #[arg(long, value_parser = ["proposed", "approved", "running", "done", "blocked", "superseded", "cancelled"])]
        state: Option<String>,
        /// Print one versioned JSON result instead of text.
        #[arg(long)]
        json: bool,
        /// Print one JSON object per plan.
        #[arg(long, conflicts_with = "json")]
        jsonl: bool,
    },
    /// Show one plan: its target, hash, items and where it stands.
    Show {
        #[command(flatten)]
        scope: InProject,
        /// The plan's id.
        migration: String,
        #[arg(long)]
        json: bool,
    },
    /// List a plan's items in entry order.
    Items {
        #[command(flatten)]
        scope: InProject,
        /// The plan's id.
        migration: String,
        /// Only items with this outcome.
        #[arg(long, value_parser = ["ok", "migrates", "needs_input", "blocked", "resolved", "applied", "skipped"])]
        outcome: Option<String>,
        /// `next_cursor` from the previous page; `--jsonl` starts its stream there.
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u8).range(1..=100))]
        limit: u8,
        /// Print one versioned JSON result instead of text.
        #[arg(long)]
        json: bool,
        /// Print one JSON object per item, following every page.
        #[arg(long, conflicts_with = "json")]
        jsonl: bool,
    },
    /// Compute and store a dry-run plan for a definition the collection won't take directly.
    ///
    /// `--data`/`--file` is the target definition (a content type document, like
    /// `content create`'s content but of the type); `--operations` is an optional file with a
    /// JSON array of `fill`/`move` operations.
    Plan {
        #[command(flatten)]
        place: Where,
        /// The target definition as JSON.
        #[arg(long, conflicts_with = "file")]
        data: Option<String>,
        /// A file holding the target definition as JSON.
        #[arg(long)]
        file: Option<PathBuf>,
        /// A file holding the operations array as JSON.
        #[arg(long)]
        operations: Option<PathBuf>,
        /// Reuse the key of an earlier attempt that got no answer.
        #[arg(long)]
        idempotency_key: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Approve a proposed plan, bound to the plan hash you looked at.
    Approve {
        #[command(flatten)]
        scope: InProject,
        /// The plan's id.
        migration: String,
        /// The plan's hash, as `migration show` or `migration list` prints it.
        #[arg(long)]
        if_match: String,
        #[arg(long)]
        idempotency_key: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Start an approved plan's run, bound to the plan hash you looked at.
    Apply {
        #[command(flatten)]
        scope: InProject,
        /// The plan's id.
        migration: String,
        /// The plan's hash, as `migration show` prints it.
        #[arg(long)]
        if_match: String,
        #[arg(long)]
        idempotency_key: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Cancel a plan that hasn't run yet.
    Cancel {
        #[command(flatten)]
        scope: InProject,
        /// The plan's id.
        migration: String,
        #[arg(long)]
        idempotency_key: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Give an item that needs input a replacement document the target accepts.
    Resolve {
        #[command(flatten)]
        scope: InProject,
        /// The plan's id.
        migration: String,
        /// The item's entry id.
        entry: String,
        /// The plan's hash, as `migration show` prints it.
        #[arg(long)]
        if_match: String,
        /// The replacement document as JSON.
        #[arg(long, conflicts_with = "file")]
        data: Option<String>,
        /// A file holding the replacement document as JSON.
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        idempotency_key: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum CommentCommand {
    /// List an entry's comments, newest first.
    List {
        #[command(flatten)]
        place: Where,
        /// The entry's id.
        entry: String,
        /// Only comments on this revision.
        #[arg(long)]
        revision: Option<String>,
        /// `open` or `resolved`.
        #[arg(long, value_parser = ["open", "resolved"])]
        state: Option<String>,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u8).range(1..=100))]
        limit: u8,
        /// `next_cursor` from the previous page; `--jsonl` starts its stream there.
        #[arg(long)]
        cursor: Option<String>,
        /// Print one versioned JSON result instead of text.
        #[arg(long)]
        json: bool,
        /// Print one JSON object per comment, following every page.
        #[arg(long, conflicts_with = "json")]
        jsonl: bool,
    },
    /// Write a comment on a revision of an entry.
    Create {
        #[command(flatten)]
        place: Where,
        /// The entry's id.
        entry: String,
        /// The revision it is about (a comment stays bound as drafts move on).
        #[arg(long)]
        revision: String,
        /// What the comment says.
        #[arg(long)]
        body: String,
        /// Where in the content it points, like `author` or `sections/2/byline`.
        #[arg(long)]
        field: Option<String>,
        /// Reuse the key of an earlier attempt that got no answer.
        #[arg(long)]
        idempotency_key: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum TaskCommand {
    /// List the environment's tasks, newest first.
    List {
        #[command(flatten)]
        scope: InProject,
        /// Only this entry's tasks.
        #[arg(long)]
        entry: Option<String>,
        /// Only this collection's tasks.
        #[arg(long)]
        collection: Option<String>,
        /// `me`, `none` or an identity id.
        #[arg(long)]
        assignee: Option<String>,
        /// `open`, `done` or `cancelled`.
        #[arg(long, value_parser = ["open", "done", "cancelled"])]
        status: Option<String>,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u8).range(1..=100))]
        limit: u8,
        /// `next_cursor` from the previous page; `--jsonl` starts its stream there.
        #[arg(long)]
        cursor: Option<String>,
        /// Print one versioned JSON result instead of text.
        #[arg(long)]
        json: bool,
        /// Print one JSON object per task, following every page.
        #[arg(long, conflicts_with = "json")]
        jsonl: bool,
    },
    /// Show one task.
    Show {
        #[command(flatten)]
        scope: InProject,
        /// The task's id.
        task: String,
        #[arg(long)]
        json: bool,
    },
    /// Create a task on an entry.
    Create {
        #[command(flatten)]
        scope: InProject,
        /// The entry it is about.
        #[arg(long)]
        entry: String,
        /// What has to be done (at most 200 characters).
        #[arg(long)]
        title: String,
        /// The revision it is about, if one.
        #[arg(long)]
        revision: Option<String>,
        /// The changeset it is about, if one.
        #[arg(long)]
        changeset: Option<String>,
        /// The identity id it's waiting on, if anyone.
        #[arg(long)]
        assignee: Option<String>,
        /// Reuse the key of an earlier attempt that got no answer.
        #[arg(long)]
        idempotency_key: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Change a task's title, assignee or status, bound to the task you looked at.
    Update {
        #[command(flatten)]
        scope: InProject,
        /// The task's id.
        task: String,
        /// The task's `ETag`, as `task show` or `task list` prints it.
        #[arg(long)]
        if_match: String,
        /// A new title.
        #[arg(long)]
        title: Option<String>,
        /// The identity id it's waiting on.
        #[arg(long, conflicts_with = "unassign")]
        assignee: Option<String>,
        /// Take the assignee off.
        #[arg(long)]
        unassign: bool,
        /// `open`, `done` or `cancelled`.
        #[arg(long, value_parser = ["open", "done", "cancelled"])]
        status: Option<String>,
        #[arg(long)]
        idempotency_key: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

impl Where {
    fn place(&self) -> content::Place<'_> {
        content::Place {
            scope: self.scope.scope(),
            collection: &self.collection,
        }
    }
}

/// Prints a failure — its `error` envelope on a machine format, `error: …` on stderr
/// either way — and returns its exit code.
fn finish(result: Result<(), client::Failure>, out: &out::Out) -> Exit {
    match result {
        Ok(()) => Exit::Success,
        Err(failure) => {
            out.print_failure(&failure);
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
        /// Print one versioned JSON result instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Report applied and pending migrations without applying anything (the upgrade
    /// runbook's pre-check and rollback gate). Exits 4 when the database holds a migration
    /// this binary doesn't know: the schema is newer than the code.
    MigrateStatus {
        /// The migrator's connection URL. Never the application's.
        #[arg(long, env = "COLOPHON_MIGRATION_DATABASE_URL", hide_env_values = true)]
        database_url: String,
        /// Print one versioned JSON result instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Make the first administrator, once: bound to an identity provider's issuer and subject,
    /// never to an email address. Afterwards bootstrap is disabled for good.
    Bootstrap {
        /// The migrator's connection URL. Never the application's.
        #[arg(long, env = "COLOPHON_MIGRATION_DATABASE_URL", hide_env_values = true)]
        database_url: String,
        /// The identity provider's issuer URL; by default the server's.
        #[arg(long, env = "COLOPHON_OIDC_ISSUER")]
        issuer: String,
        /// The person's subject at that provider (`colophon whoami` shows yours).
        #[arg(long)]
        subject: String,
        #[arg(long)]
        json: bool,
    },
    /// Copy the deletion ledger off-host (docs/modules/02-db.md "The deletion ledger"; O2's
    /// restore step 4 needs it): a versioned JSONL snapshot to a file, or into the
    /// `COLOPHON_S3_*` object store. Run from the backup schedule; each export is a complete
    /// interval up to its high-water mark.
    LedgerExport {
        /// The migrator's connection URL. Never the application's.
        #[arg(long, env = "COLOPHON_MIGRATION_DATABASE_URL", hide_env_values = true)]
        database_url: String,
        /// Write the export to this file ('-' for stdout).
        #[arg(long, conflicts_with = "s3_key", required_unless_present = "s3_key")]
        out: Option<String>,
        /// Put the export under this key in the `COLOPHON_S3_*` object store instead. The key
        /// convention is `deletion-ledger/<high-water>.jsonl`.
        #[arg(long)]
        s3_key: Option<String>,
        /// Only rows above this earlier export's watermark (an incremental copy; a restore
        /// still needs every file back to before its backup).
        #[arg(long, default_value_t = 0)]
        after: i64,
        /// Print one versioned JSON result instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Reapply an exported deletion-ledger interval on a freshly restored database, as the
    /// restore procedure's step 4 (O2): before read APIs or caches serve. Idempotent; exits
    /// non-zero while any row can't be replayed — the restore stays blocked.
    LedgerReplay {
        /// The migrator's connection URL of the restored database.
        #[arg(long, env = "COLOPHON_MIGRATION_DATABASE_URL", hide_env_values = true)]
        database_url: String,
        /// The export file to replay ('-' for standard input).
        #[arg(long)]
        input: String,
        /// Print one versioned JSON result instead of text.
        #[arg(long)]
        json: bool,
    },
    /// One stage of the restore procedure's database work on a freshly restored database
    /// (module 18 "Restore procedure", spec §33; O2, O3, F13; D-126): `scripts/restore.sh`
    /// orders these between decrypting the backup and letting anything serve.
    Restore {
        #[command(subcommand)]
        step: RestoreStep,
    },
}

/// The restore procedure's per-stage database actions (each run as the migrator against the
/// restored database's URL — never the application's).
#[derive(Debug, Subcommand)]
enum RestoreStep {
    /// Check the restored schema against the backup's manifest and this binary's known
    /// migrations (spec step 2): exits 4 when the backup applied anything this binary doesn't
    /// know — the backup is newer than the code and serving it would be guessing.
    Verify {
        /// The migrator's connection URL of the restored database.
        #[arg(long, env = "COLOPHON_MIGRATION_DATABASE_URL", hide_env_values = true)]
        database_url: String,
        /// The backup's manifest.json.
        #[arg(long)]
        manifest: String,
        #[arg(long)]
        json: bool,
    },
    /// Register the recovery epoch (spec step 3, O2): every environment's epoch advances, so a
    /// cached generation or stale lease from the old timeline can never act on the new one.
    /// The procedure runs it exactly once, before anything serves.
    Epoch {
        /// The migrator's connection URL of the restored database.
        #[arg(long, env = "COLOPHON_MIGRATION_DATABASE_URL", hide_env_values = true)]
        database_url: String,
        #[arg(long)]
        json: bool,
    },
    /// Freeze every unfinished delivery job as 'held' (spec step 5, O3): nothing the old
    /// timeline dispatched — sent or not — may fire again before external effects are
    /// reconciled. Reports each held job with the state it was taken from.
    Hold {
        /// The migrator's connection URL of the restored database.
        #[arg(long, env = "COLOPHON_MIGRATION_DATABASE_URL", hide_env_values = true)]
        database_url: String,
        #[arg(long)]
        json: bool,
    },
    /// List the restore's work inventory for the operator's reconciliation: the held outbox,
    /// plus due schedules and in-flight asset, bulk and content-migration work.
    Jobs {
        /// The migrator's connection URL of the restored database.
        #[arg(long, env = "COLOPHON_MIGRATION_DATABASE_URL", hide_env_values = true)]
        database_url: String,
        #[arg(long)]
        json: bool,
    },
    /// Return every held job to 'pending' with a fresh attempt window — the explicit enable of
    /// spec step 6. Only after reconciliation; jobs past their old deadline get a new lifetime.
    Resume {
        /// The migrator's connection URL of the restored database.
        #[arg(long, env = "COLOPHON_MIGRATION_DATABASE_URL", hide_env_values = true)]
        database_url: String,
        /// The fresh lifetime a resumed job gets when its old deadline has passed (seconds).
        #[arg(long, default_value_t = 3600)]
        lifetime_secs: i32,
        #[arg(long)]
        json: bool,
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
                out::Out::new("doctor", out::Format::of(json, false)),
            ))
            .into(),
        Command::Admin(command) => run_admin(&runtime, command),
        Command::Login {
            server,
            profile,
            name,
            json,
        } => {
            let out = out::Out::new("login", out::Format::of(json, false));
            finish(
                runtime.block_on(login::login(&server, &profile, &name, out)),
                &out,
            )
            .into()
        }
        Command::Logout { profile, json } => {
            let out = out::Out::new("logout", out::Format::of(json, false));
            finish(runtime.block_on(login::logout(&profile, out)), &out).into()
        }
        Command::Whoami { profile, json } => {
            let out = out::Out::new("whoami", out::Format::of(json, false));
            finish(runtime.block_on(login::whoami(&profile, out)), &out).into()
        }
        Command::Content(command) => run_content(&runtime, command),
        Command::Release(command) => run_release(&runtime, command),
        Command::Migration(command) => run_migration(&runtime, command),
        Command::Comment(command) => run_comment(&runtime, command),
        Command::Task(command) => run_task(&runtime, command),
        Command::Mcp => runtime.block_on(mcp::run()),
    }
}

fn run_admin(runtime: &Runtime, command: AdminCommand) -> ExitCode {
    match command {
        AdminCommand::Migrate { database_url, json } => runtime
            .block_on(admin::migrate(
                &database_url,
                out::Out::new("admin migrate", out::Format::of(json, false)),
            ))
            .into(),
        AdminCommand::MigrateStatus { database_url, json } => runtime
            .block_on(admin::migrate_status(
                &database_url,
                out::Out::new("admin migrate-status", out::Format::of(json, false)),
            ))
            .into(),
        AdminCommand::Bootstrap {
            database_url,
            issuer,
            subject,
            json,
        } => runtime
            .block_on(admin::bootstrap(
                &database_url,
                &issuer,
                &subject,
                out::Out::new("admin bootstrap", out::Format::of(json, false)),
            ))
            .into(),
        AdminCommand::LedgerExport {
            database_url,
            out: file,
            s3_key,
            after,
            json,
        } => runtime
            .block_on(admin::ledger_export(
                &database_url,
                file.as_deref(),
                s3_key.as_deref(),
                after,
                out::Out::new("admin ledger export", out::Format::of(json, false)),
            ))
            .into(),
        AdminCommand::LedgerReplay {
            database_url,
            input,
            json,
        } => runtime
            .block_on(admin::ledger_replay(
                &database_url,
                &input,
                out::Out::new("admin ledger replay", out::Format::of(json, false)),
            ))
            .into(),
        AdminCommand::Restore { step } => run_restore(runtime, step),
    }
}

fn run_content(runtime: &Runtime, command: ContentCommand) -> ExitCode {
    match command {
        ContentCommand::Create {
            place,
            data,
            file,
            idempotency_key,
            json,
        } => {
            let out = out::Out::new("content create", out::Format::of(json, false));
            finish(
                runtime.block_on(async {
                    let content = content::content(data.as_deref(), file.as_deref()).await?;
                    content::create(&place.place(), &content, idempotency_key.as_deref(), out).await
                }),
                &out,
            )
            .into()
        }
        ContentCommand::List {
            place,
            limit,
            cursor,
            json,
            jsonl,
        } => {
            let out = out::Out::new("content list", out::Format::of(json, jsonl));
            finish(
                runtime.block_on(content::list(&place.place(), limit, cursor.as_deref(), out)),
                &out,
            )
            .into()
        }
        ContentCommand::Query {
            place,
            data,
            file,
            cursor,
            limit,
            json,
            jsonl,
        } => {
            let out = out::Out::new("content query", out::Format::of(json, jsonl));
            finish(
                runtime.block_on(async {
                    let document = content::content(data.as_deref(), file.as_deref()).await?;
                    content::query(&place.place(), &document, cursor.as_deref(), limit, out).await
                }),
                &out,
            )
            .into()
        }
        ContentCommand::Import {
            place,
            data,
            file,
            dry_run,
            source_type,
            idempotency_key,
            json,
        } => {
            let out = out::Out::new("content import", out::Format::of(json, false));
            finish(
                runtime.block_on(async {
                    let body = content::import_body(data.as_deref(), file.as_deref()).await?;
                    let source_type = source_type
                        .as_deref()
                        .map(|kind| scope::query_unreserved(kind, "--type"))
                        .transpose()?;
                    content::import(
                        &place.place(),
                        body,
                        dry_run,
                        source_type,
                        idempotency_key.as_deref(),
                        out,
                    )
                    .await
                }),
                &out,
            )
            .into()
        }
        ContentCommand::Export { place, json, jsonl } => {
            let out = out::Out::new("content export", out::Format::of(json, jsonl));
            finish(runtime.block_on(content::export(&place.place(), out)), &out).into()
        }
    }
}

fn run_release(runtime: &Runtime, command: ReleaseCommand) -> ExitCode {
    match command {
        ReleaseCommand::List { scope, json, jsonl } => {
            let out = out::Out::new("release list", out::Format::of(json, jsonl));
            finish(runtime.block_on(release::list(&scope.scope(), out)), &out).into()
        }
        ReleaseCommand::Show {
            scope,
            generation,
            json,
        } => {
            let out = out::Out::new("release show", out::Format::of(json, false));
            finish(
                runtime.block_on(release::show(&scope.scope(), generation, out)),
                &out,
            )
            .into()
        }
    }
}

fn run_migration(runtime: &Runtime, command: MigrationCommand) -> ExitCode {
    if matches!(
        command,
        MigrationCommand::List { .. }
            | MigrationCommand::Show { .. }
            | MigrationCommand::Items { .. }
    ) {
        run_migration_read(runtime, command)
    } else {
        run_migration_write(runtime, command)
    }
}

/// The read side: `migration list`, `show`, `items`.
fn run_migration_read(runtime: &Runtime, command: MigrationCommand) -> ExitCode {
    match command {
        MigrationCommand::List {
            scope,
            collection,
            state,
            json,
            jsonl,
        } => {
            let out = out::Out::new("migration list", out::Format::of(json, jsonl));
            finish(
                runtime.block_on(migration::list(
                    &scope.scope(),
                    collection.as_deref(),
                    state.as_deref(),
                    out,
                )),
                &out,
            )
            .into()
        }
        MigrationCommand::Show {
            scope,
            migration,
            json,
        } => {
            let out = out::Out::new("migration show", out::Format::of(json, false));
            finish(
                runtime.block_on(migration::show(&scope.scope(), &migration, out)),
                &out,
            )
            .into()
        }
        MigrationCommand::Items {
            scope,
            migration,
            outcome,
            cursor,
            limit,
            json,
            jsonl,
        } => {
            let out = out::Out::new("migration items", out::Format::of(json, jsonl));
            finish(
                runtime.block_on(migration::items(
                    &scope.scope(),
                    &migration,
                    outcome.as_deref(),
                    cursor.as_deref(),
                    limit,
                    out,
                )),
                &out,
            )
            .into()
        }
        _ => {
            eprintln!("error: a migration write command reached the read dispatch");
            Exit::Internal.into()
        }
    }
}

/// The human side (D-106): `migration plan`, `approve`, `apply`, `cancel`, `resolve`.
fn run_migration_write(runtime: &Runtime, command: MigrationCommand) -> ExitCode {
    match command {
        MigrationCommand::Plan {
            place,
            data,
            file,
            operations,
            idempotency_key,
            json,
        } => migration_plan(
            runtime,
            &place,
            data.as_deref(),
            file.as_deref(),
            operations.as_deref(),
            idempotency_key.as_deref(),
            json,
        ),
        MigrationCommand::Approve {
            scope,
            migration,
            if_match,
            idempotency_key,
            json,
        } => migration_act(
            runtime,
            &scope,
            &migration,
            "approve",
            Some(&if_match),
            idempotency_key.as_deref(),
            json,
        ),
        MigrationCommand::Apply {
            scope,
            migration,
            if_match,
            idempotency_key,
            json,
        } => migration_act(
            runtime,
            &scope,
            &migration,
            "apply",
            Some(&if_match),
            idempotency_key.as_deref(),
            json,
        ),
        MigrationCommand::Cancel {
            scope,
            migration,
            idempotency_key,
            json,
        } => migration_act(
            runtime,
            &scope,
            &migration,
            "cancel",
            None,
            idempotency_key.as_deref(),
            json,
        ),
        MigrationCommand::Resolve {
            scope,
            migration,
            entry,
            if_match,
            data,
            file,
            idempotency_key,
            json,
        } => migration_resolve(
            runtime,
            &scope,
            &migration,
            &entry,
            &if_match,
            data.as_deref(),
            file.as_deref(),
            idempotency_key.as_deref(),
            json,
        ),
        _ => {
            eprintln!("error: a migration read command reached the write dispatch");
            Exit::Internal.into()
        }
    }
}

/// `migration plan`: the definition file, read, then the dry-run request.
fn migration_plan(
    runtime: &Runtime,
    place: &Where,
    data: Option<&str>,
    file: Option<&std::path::Path>,
    operations: Option<&std::path::Path>,
    idempotency_key: Option<&str>,
    json: bool,
) -> ExitCode {
    let out = out::Out::new("migration plan", out::Format::of(json, false));
    finish(
        runtime.block_on(async {
            let definition = content::content(data, file).await?;
            migration::plan(
                &place.place(),
                &definition,
                operations,
                idempotency_key,
                out,
            )
            .await
        }),
        &out,
    )
    .into()
}

/// `migration resolve`: the replacement document, read, then the item write.
#[expect(
    clippy::too_many_arguments,
    reason = "the command's flags, grouped where they can be: scope, plan, entry, etag, body, key, format"
)]
fn migration_resolve(
    runtime: &Runtime,
    scope: &InProject,
    migration: &str,
    entry: &str,
    if_match: &str,
    data: Option<&str>,
    file: Option<&std::path::Path>,
    idempotency_key: Option<&str>,
    json: bool,
) -> ExitCode {
    let out = out::Out::new("migration resolve", out::Format::of(json, false));
    finish(
        runtime.block_on(async {
            let resolution = content::content(data, file).await?;
            migration::resolve(
                &scope.scope(),
                migration,
                entry,
                &resolution,
                if_match,
                idempotency_key,
                out,
            )
            .await
        }),
        &out,
    )
    .into()
}

/// `migration approve|apply|cancel`: one POST at `…/{id}/{action}`.
fn migration_act(
    runtime: &Runtime,
    scope: &InProject,
    migration: &str,
    action: &'static str,
    if_match: Option<&str>,
    idempotency_key: Option<&str>,
    json: bool,
) -> ExitCode {
    // `command` names the full command: `migration approve`, `migration apply`, `migration cancel`.
    let command = match action {
        "approve" => "migration approve",
        "apply" => "migration apply",
        "cancel" => "migration cancel",
        _ => "migration act",
    };
    let out = out::Out::new(command, out::Format::of(json, false));
    finish(
        runtime.block_on(migration::act(
            &scope.scope(),
            migration,
            action,
            if_match,
            idempotency_key,
            out,
        )),
        &out,
    )
    .into()
}

fn run_comment(runtime: &Runtime, command: CommentCommand) -> ExitCode {
    match command {
        CommentCommand::List {
            place,
            entry,
            revision,
            state,
            limit,
            cursor,
            json,
            jsonl,
        } => {
            let out = out::Out::new("comment list", out::Format::of(json, jsonl));
            finish(
                runtime.block_on(collaboration::list_comments(
                    &place.place(),
                    &entry,
                    revision.as_deref(),
                    state.as_deref(),
                    limit,
                    cursor.as_deref(),
                    out,
                )),
                &out,
            )
            .into()
        }
        CommentCommand::Create {
            place,
            entry,
            revision,
            body,
            field,
            idempotency_key,
            json,
        } => {
            let out = out::Out::new("comment create", out::Format::of(json, false));
            finish(
                runtime.block_on(collaboration::create_comment(
                    &place.place(),
                    &entry,
                    &revision,
                    field.as_deref(),
                    &body,
                    idempotency_key.as_deref(),
                    out,
                )),
                &out,
            )
            .into()
        }
    }
}

fn run_task(runtime: &Runtime, command: TaskCommand) -> ExitCode {
    match command {
        TaskCommand::List {
            scope,
            entry,
            collection,
            assignee,
            status,
            limit,
            cursor,
            json,
            jsonl,
        } => {
            let out = out::Out::new("task list", out::Format::of(json, jsonl));
            finish(
                runtime.block_on(collaboration::list_tasks(
                    &scope.scope(),
                    entry.as_deref(),
                    collection.as_deref(),
                    assignee.as_deref(),
                    status.as_deref(),
                    limit,
                    cursor.as_deref(),
                    out,
                )),
                &out,
            )
            .into()
        }
        TaskCommand::Show { scope, task, json } => {
            let out = out::Out::new("task show", out::Format::of(json, false));
            finish(
                runtime.block_on(collaboration::show_task(&scope.scope(), &task, out)),
                &out,
            )
            .into()
        }
        TaskCommand::Create {
            scope,
            entry,
            title,
            revision,
            changeset,
            assignee,
            idempotency_key,
            json,
        } => {
            let out = out::Out::new("task create", out::Format::of(json, false));
            finish(
                runtime.block_on(collaboration::create_task(
                    &scope.scope(),
                    &entry,
                    &title,
                    revision.as_deref(),
                    changeset.as_deref(),
                    assignee.as_deref(),
                    idempotency_key.as_deref(),
                    out,
                )),
                &out,
            )
            .into()
        }
        TaskCommand::Update {
            scope,
            task,
            if_match,
            title,
            assignee,
            unassign,
            status,
            idempotency_key,
            json,
        } => {
            let out = out::Out::new("task update", out::Format::of(json, false));
            finish(
                runtime.block_on(collaboration::update_task(
                    &scope.scope(),
                    &task,
                    &if_match,
                    title.as_deref(),
                    assignee.as_deref(),
                    unassign,
                    status.as_deref(),
                    idempotency_key.as_deref(),
                    out,
                )),
                &out,
            )
            .into()
        }
    }
}

fn run_restore(runtime: &Runtime, step: RestoreStep) -> ExitCode {
    match step {
        RestoreStep::Verify {
            database_url,
            manifest,
            json,
        } => runtime
            .block_on(admin::restore_verify(
                &database_url,
                &manifest,
                out::Out::new("admin restore verify", out::Format::of(json, false)),
            ))
            .into(),
        RestoreStep::Epoch { database_url, json } => runtime
            .block_on(admin::restore_epoch(
                &database_url,
                out::Out::new("admin restore epoch", out::Format::of(json, false)),
            ))
            .into(),
        RestoreStep::Hold { database_url, json } => runtime
            .block_on(admin::restore_hold(
                &database_url,
                out::Out::new("admin restore hold", out::Format::of(json, false)),
            ))
            .into(),
        RestoreStep::Jobs { database_url, json } => runtime
            .block_on(admin::restore_jobs(
                &database_url,
                out::Out::new("admin restore jobs", out::Format::of(json, false)),
            ))
            .into(),
        RestoreStep::Resume {
            database_url,
            lifetime_secs,
            json,
        } => runtime
            .block_on(admin::restore_resume(
                &database_url,
                lifetime_secs,
                out::Out::new("admin restore resume", out::Format::of(json, false)),
            ))
            .into(),
    }
}
