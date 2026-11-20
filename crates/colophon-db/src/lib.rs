//! Colophon's PostgreSQL adapter: constraints and transactions that make invalid states
//! impossible to store, the transactional outbox, idempotency records and row-level
//! security context.
//!
//! Design: `docs/modules/02-db.md`. Migrations live in `migrations/` and are embedded in the
//! binary ([`MIGRATOR`]); queries are checked against the schema at compile time (D-026).

pub mod access;
pub mod administrators;
pub mod assets;
pub mod audience;
pub mod audit;
pub mod budgets;
pub mod bulk;
pub mod changesets;
pub mod collaboration;
pub mod components;
pub mod content_types;
pub mod delivery;
pub mod destinations;
pub mod entries;
pub mod events;
pub mod idempotency;
pub mod identity;
pub mod jobs;
pub mod ledger;
pub mod login_roles;
pub mod members;
pub mod migrations;
pub mod passwords;
pub mod previews;
pub mod projects;
pub mod publish;
pub mod query;
pub mod releases;
pub mod restore;
pub mod retention;
pub mod roles;
pub mod schedules;
pub mod scope;
pub mod setup;
pub mod throttle;
pub mod tokens;
pub mod transactions;
pub mod undo;
pub mod work;

use std::{str::FromStr, time::Duration};

use sqlx::{
    ConnectOptions as _, Executor as _,
    postgres::{PgConnectOptions, PgPoolOptions, PgSslMode},
};

pub use sqlx::postgres::{PgConnection, PgListener, PgPool};
pub use sqlx::{Postgres, Transaction};
pub use transactions::{Phases, begin_tx, begin_tx_snapshot, with_phases};

/// The schema migrations, embedded at compile time from `migrations/`.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Applies any migrations the database doesn't have yet. Run as the migrator role (the table
/// owner), never as the application role.
pub async fn migrate(pool: &PgPool) -> Result<(), DbError> {
    MIGRATOR.run(pool).await.map_err(DbError::Migrate)
}

/// Pool settings. Every value is a bound: connections, waiting, and statement time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolSettings {
    /// Most connections the pool opens.
    pub max_connections: u32,
    /// How long a caller waits for a connection before failing.
    pub acquire_timeout: Duration,
    /// Server-side limit on any single statement (`statement_timeout`).
    pub statement_timeout: Duration,
}

impl Default for PoolSettings {
    fn default() -> Self {
        Self {
            max_connections: 16,
            acquire_timeout: Duration::from_secs(2),
            statement_timeout: Duration::from_secs(5),
        }
    }
}

/// Why the database could not be used.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// The connection URL did not parse. The URL itself is never included: it holds a password.
    #[error("invalid database URL: {reason}")]
    InvalidUrl {
        /// What was wrong, without the URL.
        reason: String,
    },
    /// The URL would reach a database on another machine without proving it is the database
    /// (D-156; [`check_url`]). The URL itself is never included.
    #[error("database URL refused: {reason}")]
    Unverified {
        /// What is unsafe and how to fix the URL, without the URL.
        reason: String,
    },
    /// The query failed.
    #[error("database query failed")]
    Query(#[source] sqlx::Error),
    /// No pooled connection became free within the acquire timeout: the database is at
    /// capacity. Nothing was sent to it, so the caller can safely answer "try again".
    #[error("no database connection became free in time")]
    Busy,
    /// Postgres aborted the transaction to break a lock cycle (`40P01`) or a serialization
    /// conflict (`40001`): it rolled back whole, nothing committed, so the caller can answer "try
    /// again" as for [`Self::Busy`], never a 500.
    #[error("the database aborted the transaction for a conflicting one; nothing was committed")]
    Contended,
    /// No answer within the deadline.
    #[error("database did not answer within {0:?}")]
    Timeout(Duration),
    /// A unique value (such as a slug) is already taken.
    #[error("already exists")]
    Conflict,
    /// An entry's slug is another entry's draft or published slug in the same collection
    /// (migration 0016, D-054).
    #[error("the slug is taken")]
    SlugTaken,
    /// A value of a unique field is another entry's, as a draft or published, in the same
    /// collection (migration 0027, D-070).
    #[error("a unique value is taken")]
    ValueTaken,
    /// The collection is a single type and already has its entry (migration 0026, D-067).
    #[error("this single type already has its entry")]
    SingleExists,
    /// The entry's localization group already holds that locale, in any case (migrations 0034 and
    /// 0077, D-212): two writes that raced past the check meet the unique index.
    #[error("the localization group already has that locale")]
    LocaleTaken,
    /// A reference field names an entry that isn't one of its collection's (migration 0024,
    /// D-065); nothing was written.
    #[error("a reference names no entry of its collection")]
    MissingReference {
        /// The field.
        field: String,
        /// The place in its list.
        position: u16,
        /// Whether the field holds a list.
        many: bool,
    },
    /// An asset field or a rich-text image names an upload the environment doesn't have
    /// (migration 0036, media M1, D-177): the id was never an asset of this project or its
    /// upload failed before the row existed. Nothing was written.
    #[error("the content names no asset of the environment")]
    MissingAsset {
        /// RFC 6901 JSON Pointer, in the content, to the value naming it (`/cover`,
        /// `/photos/1`, `/body/doc/content/3/attrs/assetId`).
        pointer: String,
    },
    /// A changeset pin or a publication names an entry that is deleted (migration 0030, D-080):
    /// a deletion committed while it waited for the entry. Nothing was written; the transaction is
    /// aborted and the caller rolls it back.
    #[error("an entry is deleted")]
    EntryDeleted {
        /// The entry, as the database reported it.
        entry: Option<uuid::Uuid>,
    },
    /// The caller's budget for this kind of work is exhausted (migration 0045, D-113): the
    /// reservation was refused under the limits lock, and the transaction is aborted and the
    /// caller rolls it back.
    #[error("the {kind} budget is exhausted ({used} held or spent of {limit})")]
    BudgetExceeded {
        /// What was refused.
        kind: colophon_domain::budget::Kind,
        /// What the principal already held or spent.
        used: i64,
        /// Its ceiling.
        limit: i64,
    },
    /// The project already retains all the unfinished work its bound allows (spec §33,
    /// D-131): [`work::admit`] refused the job row this transaction would have added, under
    /// the admission lock. The transaction is aborted and the caller rolls it back.
    #[error("the project's pending work is at its bound ({open} of {limit} jobs open)")]
    WorkFull {
        /// Unfinished jobs the project retains across the job tables.
        open: i64,
        /// Its bound.
        limit: i64,
    },
    /// The installation already retains all the pending payload metadata its bound allows
    /// (spec §33, D-135): [`work::check_payload`] refused this transaction's stored bytes,
    /// under the payload counter's lock (D-216). The transaction is aborted and the caller rolls it
    /// back.
    #[error(
        "the installation's pending payload is over its bound ({pending} of {limit} bytes held)"
    )]
    PayloadFull {
        /// Payload bytes pending across every project.
        pending: i64,
        /// The bound.
        limit: i64,
    },
    /// Applying migrations failed.
    #[error("applying migrations failed")]
    Migrate(#[source] sqlx::migrate::MigrateError),
    /// Stored data broke an invariant the schema is meant to guarantee: a bug, never input.
    #[error("stored data is inconsistent: {0}")]
    Corrupt(&'static str),
}

impl DbError {
    /// A query's error: an exhausted pool becomes `Busy`, a deadlock or serialization abort
    /// `Contended`, everything else `Query`.
    pub(crate) fn from_query(error: sqlx::Error) -> Self {
        match error {
            sqlx::Error::PoolTimedOut => Self::Busy,
            sqlx::Error::Database(database)
                if matches!(database.code().as_deref(), Some("40P01" | "40001")) =>
            {
                Self::Contended
            }
            error => Self::Query(error),
        }
    }

    /// A write's error: a violation of the slug indexes becomes `SlugTaken`, of the unique
    /// claims `ValueTaken`, other unique violations `Conflict`, a pin or publication of a deleted
    /// entry `EntryDeleted`, the rest as [`Self::from_query`].
    pub(crate) fn from_write(error: sqlx::Error) -> Self {
        match &error {
            sqlx::Error::Database(database) if database.is_unique_violation() => {
                match database.constraint() {
                    Some("entries_draft_slug" | "publications_live_slug") => Self::SlugTaken,
                    Some("entries_single") => Self::SingleExists,
                    Some("entries_one_locale_per_group") => Self::LocaleTaken,
                    Some("unique_claims_value") => Self::ValueTaken,
                    _ => Self::Conflict,
                }
            }
            _ => match deleted_entry(&error) {
                Some(("changeset_items" | "publications" | "comments" | "tasks", entry)) => {
                    Self::EntryDeleted { entry }
                }
                _ => Self::from_query(error),
            },
        }
    }
}

/// The table and the entry of a violation of migration 0030's `<table>_entry_live` rule: a new
/// reference, changeset pin or publication named an entry that is deleted (or never existed).
pub(crate) fn deleted_entry(error: &sqlx::Error) -> Option<(&str, Option<uuid::Uuid>)> {
    let sqlx::Error::Database(database) = error else {
        return None;
    };
    if !database.is_foreign_key_violation() {
        return None;
    }
    let table = database.constraint()?.strip_suffix("_entry_live")?;
    let entry = database
        .try_downcast_ref::<sqlx::postgres::PgDatabaseError>()
        .and_then(|postgres| postgres.detail())
        .and_then(|detail| uuid::Uuid::try_parse(detail).ok());
    Some((table, entry))
}

/// The asset a violation of migration 0039's `<table>_asset_live` rule names: a new reference
/// to an asset that is `deleting` or `deleted` — or has a delete or expire job on the way —
/// answered like a name for an asset that isn't there.
pub(crate) fn deleted_asset(error: &sqlx::Error) -> Option<uuid::Uuid> {
    let sqlx::Error::Database(database) = error else {
        return None;
    };
    if !database.is_foreign_key_violation() {
        return None;
    }
    database.constraint()?.strip_suffix("_asset_live")?;
    database
        .try_downcast_ref::<sqlx::postgres::PgDatabaseError>()
        .and_then(|postgres| postgres.detail())
        .and_then(|detail| uuid::Uuid::try_parse(detail).ok())
}

/// Parses `url` and holds it to the TLS rule (D-156): a database on another machine is reached
/// only with `sslmode=verify-full`, which checks both that a trusted authority signed the
/// server's certificate and that the certificate names the host. Every other mode lets someone
/// on the path answer as the database: `disable`, `allow` and `prefer` may talk in plain text,
/// `require` encrypts but takes any certificate, and `verify-ca` skips the name check while
/// this driver trusts the public (Mozilla) roots as well as any `sslrootcert` — so any publicly
/// trusted certificate, for any name, passes it. `channel_binding=require` is refused
/// everywhere: the driver doesn't do channel binding and would silently ignore it. This
/// machine's own database (a loopback host or a Unix socket) needs no certificate.
///
/// The refusal names every problem and how to fix the URL, and never contains the URL.
pub fn check_url(url: &str) -> Result<PgConnectOptions, DbError> {
    let options = parse_url(url)?;
    let mut problems = Vec::new();
    if let Some(binding) = query_value(url, "channel_binding")
        && !matches!(binding.as_str(), "disable" | "prefer")
    {
        problems.push(format!(
            "channel_binding={binding} can't be honoured: this driver doesn't do channel \
             binding and would ignore it. Remove channel_binding from the URL; \
             sslmode=verify-full is what proves the server is the database"
        ));
    }
    let host = options
        .get_host()
        .trim_start_matches('[')
        .trim_end_matches(']');
    let local = options.get_socket().is_some()
        || host.starts_with('/')
        || host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    let mode = options.get_ssl_mode();
    if !local && !matches!(mode, PgSslMode::VerifyFull) {
        problems.push(format!(
            "the database at {host} isn't checked to be who it says it is: sslmode={} {}. \
             Use sslmode=verify-full, with sslrootcert=<CA file> if a private authority signs \
             its certificate; only a loopback host or a Unix socket may go without",
            ssl_mode_name(mode),
            ssl_mode_weakness(mode),
        ));
    }
    if problems.is_empty() {
        Ok(options)
    } else {
        Err(DbError::Unverified {
            reason: problems.join("; and "),
        })
    }
}

/// Stands in for an empty host. `postgres://app:x@/colophon?host=/var/run/postgresql` is
/// libpq's way to name a Unix socket with credentials, but the URL parser refuses an empty
/// host after a user name. `.invalid` never resolves (RFC 2606), so it can't reach anything.
const NO_HOST: &str = "no-host.invalid";

/// [`PgConnectOptions::from_str`], plus libpq's empty host after credentials: the host is then
/// the query's `host=`, else the default sqlx picks for a URL with no host at all (`PGHOST`,
/// a Unix socket it finds, or `localhost`).
fn parse_url(url: &str) -> Result<PgConnectOptions, DbError> {
    let invalid = |error: sqlx::Error| DbError::InvalidUrl {
        reason: redact_url_error(&error),
    };
    let Some(filled) = fill_empty_host(url) else {
        return PgConnectOptions::from_str(url).map_err(invalid);
    };
    let options = PgConnectOptions::from_str(&filled).map_err(invalid)?;
    if options.get_socket().is_none() && options.get_host() == NO_HOST {
        let default = PgConnectOptions::new_without_pgpass();
        return Ok(options.host(default.get_host()));
    }
    Ok(options)
}

/// The URL with [`NO_HOST`] where its host is missing after credentials
/// (`scheme://user@/…`, `scheme://user@:5433/…`); `None` for any other URL.
fn fill_empty_host(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (userinfo, host_and_port) = rest[..end].rsplit_once('@')?;
    (host_and_port.is_empty() || host_and_port.starts_with(':')).then(|| {
        format!(
            "{scheme}://{userinfo}@{NO_HOST}{host_and_port}{}",
            &rest[end..]
        )
    })
}

/// The last value of `key` in the URL's query, percent-decoded as the driver decodes it.
fn query_value(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    let query = query.split_once('#').map_or(query, |(query, _)| query);
    query
        .rsplit('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _)| decode_component(name) == key)
        .map(|(_, value)| decode_component(value))
}

/// `application/x-www-form-urlencoded` decoding: `+` is a space and `%XX` a byte.
fn decode_component(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        let escaped = (byte == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (byte, escaped) {
            (_, Some(value)) => {
                decoded.push(value);
                index += 3;
            }
            (b'+', None) => {
                decoded.push(b' ');
                index += 1;
            }
            (other, None) => {
                decoded.push(other);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// libpq's name for a mode, as a URL spells it.
fn ssl_mode_name(mode: PgSslMode) -> &'static str {
    match mode {
        PgSslMode::Disable => "disable",
        PgSslMode::Allow => "allow",
        PgSslMode::Prefer => "prefer",
        PgSslMode::Require => "require",
        PgSslMode::VerifyCa => "verify-ca",
        PgSslMode::VerifyFull => "verify-full",
    }
}

/// What a mode leaves open, for the refusal.
fn ssl_mode_weakness(mode: PgSslMode) -> &'static str {
    match mode {
        PgSslMode::Disable | PgSslMode::Allow => "sends passwords and content in plain text",
        PgSslMode::Prefer => {
            "falls back to plain text and takes any certificate (it is the default when the \
             URL names no sslmode)"
        }
        PgSslMode::Require => "encrypts but takes any certificate",
        PgSslMode::VerifyCa => {
            "doesn't check that the certificate names the host, and this driver trusts the \
             public roots as well as sslrootcert, so any publicly trusted certificate passes"
        }
        PgSslMode::VerifyFull => "checks the certificate and the name",
    }
}

/// When a statement is slow enough to log, in two tiers (D-224): evidence from `info`, an alert from `warn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlowStatements {
    /// From here a statement is logged, at INFO (target `sqlx::query`): evidence. The server's log layer
    /// drops the faster ones (sqlx offers every statement at INFO once the tiers are set).
    pub info: Duration,
    /// From here it is an alert, at WARN.
    pub warn: Duration,
}

impl Default for SlowStatements {
    /// sqlx's own single threshold, 1 s, at WARN: what a process gets that doesn't set the tiers (and so
    /// doesn't install the layer that drops the statements under the INFO tier).
    fn default() -> Self {
        Self {
            info: Duration::from_secs(1),
            warn: Duration::from_secs(1),
        }
    }
}

/// The tiers every pool is built with (D-221, D-224; `COLOPHON_DB_SLOW_STATEMENT_INFO_MS`,
/// `COLOPHON_DB_SLOW_STATEMENT_MS`).
static SLOW_STATEMENTS: std::sync::OnceLock<SlowStatements> = std::sync::OnceLock::new();

/// Sets the slow-statement tiers for every pool built afterwards. The first call wins.
pub fn set_slow_statements(tiers: SlowStatements) {
    let _ = SLOW_STATEMENTS.set(tiers);
}

/// The tiers in force: what [`set_slow_statements`] set, else the default.
pub fn slow_statements() -> SlowStatements {
    SLOW_STATEMENTS.get().copied().unwrap_or_default()
}

/// Builds the pool without connecting. Connections open on first use, so the server starts
/// (and reports "not ready") while the database is down, instead of failing to start. The URL
/// is held to [`check_url`]'s TLS rule first.
pub fn connect_lazy(url: &str, settings: PoolSettings) -> Result<PgPool, DbError> {
    let options = pool_connect_options(url, &settings)?;
    // No ping on checkout: it is a round trip on every query. sqlx still pings each connection
    // as it returns to the pool and drops broken ones, so only a connection that breaks while
    // idle (a database restart) reaches a caller, and fails that one request (D-033).
    Ok(PgPoolOptions::new()
        .max_connections(settings.max_connections)
        .min_connections(0)
        .acquire_timeout(settings.acquire_timeout)
        .test_before_acquire(false)
        // A connection handed back inside a transaction is closed, not pooled (D-217), checked on
        // one release in `transactions::CANARY_EVERY` once the begin tripwire is armed (D-226).
        .after_release(transactions::release_check)
        .connect_lazy_with(options))
}

/// Proves, on one throwaway connection made exactly as [`connect_lazy`] makes them, that the begin
/// tripwire works in this process, and arms it (D-226): the release check then runs on one release
/// in [`transactions::CANARY_EVERY`]. Until this succeeds, and whenever it fails, every release is
/// checked; the error says which dependency is missing. Call once at startup, after the tracing
/// subscriber (with [`transactions::begin_tripwire`]) is installed.
pub async fn arm_release_guard(url: &str) -> Result<(), transactions::TripwireOff> {
    let options = pool_connect_options(url, &PoolSettings::default())
        .map_err(|error| transactions::TripwireOff::CouldNotRun(error.to_string()))?;
    transactions::arm_begin_tripwire(options).await
}

/// The options every pool's connections are made with: [`check_url`]'s TLS rule, the server's
/// application name, its statement timeout and the slow-statement log (D-221, D-224).
fn pool_connect_options(url: &str, settings: &PoolSettings) -> Result<PgConnectOptions, DbError> {
    let options = check_url(url)?
        .application_name("colophon-server")
        .options([(
            "statement_timeout",
            settings.statement_timeout.as_millis().to_string(),
        )]);
    // D-224: with the tiers set (the server, which installs the layer that drops every statement
    // under the INFO tier), sqlx offers each statement at INFO and says those past the WARN tier at
    // WARN, "slow statement". Otherwise sqlx's own: WARN from 1 s, the rest at DEBUG.
    Ok(match SLOW_STATEMENTS.get() {
        Some(tiers) => options
            .log_statements(log::LevelFilter::Info)
            .log_slow_statements(log::LevelFilter::Warn, tiers.warn),
        None => options.log_slow_statements(log::LevelFilter::Warn, SlowStatements::default().warn),
    })
}

/// Opens the change-hint listener's connection (D-108): its own, not one of the pool's, since
/// `LISTEN` holds it for the process's life — built as sqlx's `PgListener::connect` builds it,
/// but from [`check_url`]'s options, so the listener takes exactly the URLs the pool takes and
/// is held to the same TLS rule.
pub async fn listener(url: &str) -> Result<PgListener, DbError> {
    let options = check_url(url)?.application_name("colophon-server");
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .max_lifetime(None)
        .idle_timeout(None)
        .connect_with(options)
        .await
        .map_err(DbError::Query)?;
    PgListener::connect_with(&pool)
        .await
        .map_err(DbError::Query)
}

/// A change-hint listener for a process that holds only a pool (the worker's asset wake-up, D-218): its own
/// connection, as [`listener`]'s is, built from the pool's own options — the same URL and TLS rule — and named
/// `application`, so an operator (or a test) can tell it apart in `pg_stat_activity`.
pub async fn listener_from(pool: &PgPool, application: &str) -> Result<PgListener, DbError> {
    let options = (*pool.connect_options())
        .clone()
        .application_name(application);
    let own = PgPoolOptions::new()
        .max_connections(1)
        .max_lifetime(None)
        .idle_timeout(None)
        .connect_with(options)
        .await
        .map_err(DbError::Query)?;
    PgListener::connect_with(&own).await.map_err(DbError::Query)
}

/// sqlx's URL errors describe the problem, but keep only the first line and drop anything
/// that looks like it could echo the URL back.
fn redact_url_error(error: &sqlx::Error) -> String {
    let text = error.to_string();
    let first_line = text.lines().next().unwrap_or("unparseable");
    if first_line.contains("://") || first_line.contains('@') {
        "unparseable".to_owned()
    } else {
        first_line.to_owned()
    }
}

/// The readiness probe: one round trip to the server, bounded by `deadline` however the
/// database misbehaves (refusing, hanging, or slow to hand out a connection).
///
/// A refused connection usually surfaces as [`DbError::Timeout`]: sqlx retries connecting
/// until the pool's acquire timeout instead of reporting the refusal.
pub async fn ping(pool: &PgPool, deadline: Duration) -> Result<(), DbError> {
    match tokio::time::timeout(deadline, pool.execute("SELECT 1")).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) => Err(DbError::Query(error)),
        Err(_elapsed) => Err(DbError::Timeout(deadline)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A driver error carrying only a SQLSTATE, as Postgres reports one.
    #[derive(Debug)]
    struct Coded(&'static str);

    impl std::fmt::Display for Coded {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "SQLSTATE {}", self.0)
        }
    }

    impl std::error::Error for Coded {}

    impl sqlx::error::DatabaseError for Coded {
        fn message(&self) -> &'static str {
            "coded"
        }
        fn code(&self) -> Option<std::borrow::Cow<'_, str>> {
            Some(self.0.into())
        }
        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }
        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }
        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }
        fn kind(&self) -> sqlx::error::ErrorKind {
            sqlx::error::ErrorKind::Other
        }
    }

    #[test]
    fn a_deadlock_or_serialization_abort_is_contended_never_a_query_failure() {
        // CI 44719897863: a sweep aborted with 40P01 reached its caller as `Query`, which the
        // API answers 500. The transaction rolled back whole, so it is a retryable refusal.
        for code in ["40P01", "40001"] {
            let error = DbError::from_query(sqlx::Error::Database(Box::new(Coded(code))));
            assert!(matches!(error, DbError::Contended), "{code}: {error:?}");
            let error = DbError::from_write(sqlx::Error::Database(Box::new(Coded(code))));
            assert!(
                matches!(error, DbError::Contended),
                "{code} on a write: {error:?}"
            );
        }
        let other = DbError::from_query(sqlx::Error::Database(Box::new(Coded("42P01"))));
        assert!(matches!(other, DbError::Query(_)), "{other:?}");
    }

    #[test]
    fn invalid_urls_are_reported_without_echoing_them() {
        let error = connect_lazy("postgres://user:hunter2@[bad", PoolSettings::default())
            .expect_err("an unparseable URL");
        let text = error.to_string();
        assert!(text.starts_with("invalid database URL"), "{text}");
        assert!(
            !text.contains("hunter2"),
            "the password must never appear: {text}"
        );
    }

    #[tokio::test]
    async fn a_database_elsewhere_must_prove_who_it_is() {
        // Off this machine only `verify-full` may connect (D-156): `prefer` (the default) and
        // `require` take any certificate, and `verify-ca` — even with a root named — skips the
        // name check while the driver trusts the public roots too, so any publicly trusted
        // certificate passes. The refusal says how to fix the URL and never echoes it.
        let remote = "postgres://app:s3cret@db.example.com/colophon";
        for query in [
            "",
            "?sslmode=disable",
            "?sslmode=allow",
            "?sslmode=prefer",
            "?sslmode=require",
            "?sslmode=verify-ca",
            "?sslmode=verify-ca&sslrootcert=/etc/colophon/db-ca.pem",
        ] {
            let error = connect_lazy(&format!("{remote}{query}"), PoolSettings::default())
                .expect_err("an unverified remote database");
            assert!(
                matches!(error, DbError::Unverified { .. }),
                "{query}: {error}"
            );
            let text = error.to_string();
            assert!(text.contains("sslmode=verify-full"), "{query}: {text}");
            assert!(text.contains("db.example.com"), "{query}: {text}");
            assert!(!text.contains("s3cret"), "{query}: {text}");
        }
        for query in [
            "?sslmode=verify-full",
            "?sslmode=verify-full&sslrootcert=/etc/colophon/db-ca.pem",
            "?sslmode=verify-full&channel_binding=prefer",
            "?sslmode=verify-full&channel_binding=disable",
        ] {
            let result = connect_lazy(&format!("{remote}{query}"), PoolSettings::default());
            assert!(result.is_ok(), "{query}: {result:?}");
        }
        // This machine's own database needs no certificate: loopback, by name or address, or
        // a Unix socket — including libpq's empty host after credentials, which the URL parser
        // alone refuses.
        for url in [
            "postgres://app:x@localhost/colophon",
            "postgres://app:x@LocalHost:55432/colophon?sslmode=disable",
            "postgres://app:x@127.0.0.1:55432/colophon?sslmode=require",
            "postgres://app:x@[::1]/colophon?sslmode=disable",
            "postgres:///colophon?host=/var/run/postgresql",
            "postgres://app:x@/colophon?host=/var/run/postgresql",
            "postgres://app:x@%2Fvar%2Frun%2Fpostgresql/colophon",
            "postgres://app:x@db.example.com/colophon?host=/var/run/postgresql",
        ] {
            let result = connect_lazy(url, PoolSettings::default());
            assert!(result.is_ok(), "{url}: {result:?}");
        }
        // A name that only looks local is still another machine.
        for host in [
            "localhost.example.com",
            "127.0.0.1.nip.io",
            "[::ffff:127.0.0.1]",
        ] {
            let result = connect_lazy(
                &format!("postgres://app:x@{host}/colophon?sslmode=require"),
                PoolSettings::default(),
            );
            assert!(
                matches!(result, Err(DbError::Unverified { .. })),
                "{host}: {result:?}"
            );
        }
    }

    #[test]
    fn channel_binding_the_driver_would_ignore_is_refused() {
        // Neon's URLs ask for `channel_binding=require`; sqlx doesn't implement channel binding
        // and drops the parameter with a warning, so the URL would promise what the connection
        // doesn't do. Refused on every host, with both problems named when sslmode is weak too.
        for url in [
            "postgres://app:s3cret@db.example.com/colophon?sslmode=verify-full&channel_binding=require",
            "postgres://app:s3cret@127.0.0.1/colophon?channel_binding=require",
            "postgres://app:s3cret@db.example.com/colophon?sslmode=verify-full&channel%5Fbinding=require",
            "postgres://app:s3cret@db.example.com/colophon?sslmode=verify-full&channel_binding=requir",
        ] {
            let error = check_url(url).expect_err("channel binding refused");
            let text = error.to_string();
            assert!(text.contains("channel_binding="), "{url}: {text}");
            assert!(text.contains("Remove channel_binding"), "{url}: {text}");
            assert!(!text.contains("s3cret"), "{url}: {text}");
        }
        let neon = "postgres://app:s3cret@ep-quiet-1.eu-central-1.aws.neon.tech/colophon\
                    ?sslmode=require&channel_binding=require";
        let text = check_url(neon).expect_err("refused").to_string();
        assert!(text.contains("channel_binding=require"), "{text}");
        assert!(text.contains("sslmode=require"), "{text}");
        assert!(text.contains("sslmode=verify-full"), "{text}");
    }

    #[tokio::test]
    async fn the_listener_takes_the_urls_the_pool_takes() {
        // The change-hint listener used sqlx's own URL parser, which refuses libpq's empty host
        // after credentials: the server started on such a URL and its live lists never did.
        let url = "postgres://app:x@/colophon?host=/nonexistent/colophon-socket";
        assert!(
            PgListener::connect(url).await.is_err(),
            "sqlx alone refuses it"
        );
        let error = listener(url).await.expect_err("there is no socket there");
        assert!(
            matches!(error, DbError::Query(_)),
            "got as far as connecting: {error:?}"
        );
        let refused = listener("postgres://app:x@db.example.com/colophon?sslmode=require").await;
        assert!(
            matches!(refused, Err(DbError::Unverified { .. })),
            "held to the TLS rule: {refused:?}"
        );
    }

    #[test]
    fn an_empty_host_after_credentials_is_the_query_host_or_the_default() {
        // libpq reads `postgres://app:x@/colophon?host=/run/postgresql` as a Unix socket with
        // credentials; the URL parser alone refuses it (an empty host after a user name).
        let socket = check_url("postgres://app:p%2Bw@/colophon?host=/var/run/postgresql")
            .expect("a Unix socket URL");
        assert_eq!(
            socket
                .get_socket()
                .map(|path| path.to_string_lossy().into_owned()),
            Some("/var/run/postgresql".to_owned())
        );
        assert_eq!(socket.get_username(), "app");
        assert_eq!(socket.get_database(), Some("colophon"));
        // A port, and a TCP host in the query, both still apply.
        let tcp =
            check_url("postgres://app:x@:5433/colophon?host=db.example.com&sslmode=verify-full")
                .expect("verify-full");
        assert_eq!(tcp.get_host(), "db.example.com");
        assert_eq!(tcp.get_port(), 5433);
        assert!(
            check_url("postgres://app:x@:5433/colophon?host=db.example.com&sslmode=require")
                .is_err(),
            "the host from the query is held to the rule"
        );
        // No host anywhere: the driver's default, never the stand-in.
        let default = check_url("postgres://app:x@/colophon").expect("this machine");
        assert_ne!(default.get_host(), NO_HOST);
    }

    #[tokio::test]
    async fn a_require_url_asks_the_server_for_tls_first() {
        // `sslmode=require` is what Neon's URLs carry (D-156). With a TLS stack built in, the
        // first bytes sqlx sends are the 8-byte SSLRequest (length 8, code 80877103); without
        // one it gives up before sending anything. This listener answers `N` — no TLS here — so
        // the connection fails either way: what arrived first is the proof.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("address").port();
        let seen = tokio::spawn(async move {
            let Ok((socket, _)) = listener.accept().await else {
                return Vec::new();
            };
            let mut first = [0_u8; 8];
            let mut filled = 0;
            while filled < first.len() && socket.readable().await.is_ok() {
                match socket.try_read(&mut first[filled..]) {
                    Ok(0) => break,
                    Ok(read) => filled += read,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => break,
                }
            }
            if socket.writable().await.is_ok() {
                let _refused = socket.try_write(b"N");
            }
            first[..filled].to_vec()
        });
        let pool = connect_lazy(
            &format!("postgres://colophon:x@127.0.0.1:{port}/colophon?sslmode=require"),
            PoolSettings {
                acquire_timeout: Duration::from_secs(3),
                ..PoolSettings::default()
            },
        )
        .expect("a valid URL");
        let attempt = tokio::time::timeout(Duration::from_secs(5), pool.acquire())
            .await
            .map(|connected| connected.map(|_| ()));
        assert!(
            matches!(attempt, Ok(Err(_))),
            "a server without TLS can't take a require URL: {attempt:?}"
        );
        let first = tokio::time::timeout(Duration::from_secs(5), seen)
            .await
            .expect("the listener finished")
            .expect("joined");
        assert_eq!(
            first,
            [0, 0, 0, 8, 0x04, 0xd2, 0x16, 0x2f],
            "the SSLRequest came first"
        );
    }

    #[tokio::test]
    async fn ping_answers_within_its_deadline_when_nothing_listens() {
        // Port 1 on loopback refuses at once, but sqlx doesn't report that: the pool retries
        // with backoff until its acquire timeout (sqlx-core 0.9, pool/inner.rs). The probe's
        // own deadline must win, so readiness changes on the probe's schedule, not sqlx's.
        let pool = connect_lazy(
            "postgres://colophon:x@127.0.0.1:1/colophon",
            PoolSettings::default(),
        )
        .expect("a valid URL");
        let started = std::time::Instant::now();
        let result = ping(&pool, Duration::from_millis(300)).await;
        assert!(matches!(result, Err(DbError::Timeout(_))), "{result:?}");
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn ping_is_bounded_by_its_deadline_when_the_server_hangs() {
        // A listener that accepts and then never speaks: the probe must give up on time.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("address").port();
        let _hold = tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });
        let settings = PoolSettings {
            acquire_timeout: Duration::from_secs(30),
            ..PoolSettings::default()
        };
        let pool = connect_lazy(
            &format!("postgres://colophon:x@127.0.0.1:{port}/colophon"),
            settings,
        )
        .expect("a valid URL");
        let started = std::time::Instant::now();
        let result = ping(&pool, Duration::from_millis(300)).await;
        assert!(matches!(result, Err(DbError::Timeout(_))), "{result:?}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn an_exhausted_pool_is_busy_and_other_failures_are_not() {
        assert!(matches!(
            DbError::from_query(sqlx::Error::PoolTimedOut),
            DbError::Busy
        ));
        assert!(matches!(
            DbError::from_query(sqlx::Error::RowNotFound),
            DbError::Query(_)
        ));
        assert!(matches!(
            DbError::from_write(sqlx::Error::PoolTimedOut),
            DbError::Busy
        ));
    }

    /// Needs the local stack (`scripts/stack.sh up`).
    #[tokio::test]
    #[ignore = "needs the local stack: scripts/stack.sh up"]
    async fn ping_succeeds_against_the_local_stack() {
        let url = std::env::var("COLOPHON_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://colophon:colophon-dev@127.0.0.1:55432/colophon".to_owned()
        });
        let pool = connect_lazy(&url, PoolSettings::default()).expect("a valid URL");
        ping(&pool, Duration::from_secs(2))
            .await
            .expect("the stack's database answers");
    }
}
