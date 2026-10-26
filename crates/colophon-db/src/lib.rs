//! Colophon's PostgreSQL adapter: constraints and transactions that make invalid states
//! impossible to store, the transactional outbox, idempotency records and row-level
//! security context.
//!
//! Design: `docs/modules/02-db.md`. Migrations live in `migrations/` and are embedded in the
//! binary ([`MIGRATOR`]); queries are checked against the schema at compile time (D-026).

pub mod identity;
pub mod projects;
pub mod scope;

use std::{str::FromStr, time::Duration};

use sqlx::{
    Executor as _,
    postgres::{PgConnectOptions, PgPoolOptions},
};

pub use sqlx::postgres::PgPool;

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
    /// The query failed.
    #[error("database query failed")]
    Query(#[source] sqlx::Error),
    /// No pooled connection became free within the acquire timeout: the database is at
    /// capacity. Nothing was sent to it, so the caller can safely answer "try again".
    #[error("no database connection became free in time")]
    Busy,
    /// No answer within the deadline.
    #[error("database did not answer within {0:?}")]
    Timeout(Duration),
    /// A unique value (such as a slug) is already taken.
    #[error("already exists")]
    Conflict,
    /// Applying migrations failed.
    #[error("applying migrations failed")]
    Migrate(#[source] sqlx::migrate::MigrateError),
}

impl DbError {
    /// A query's error: an exhausted pool becomes `Busy`, everything else `Query`.
    pub(crate) fn from_query(error: sqlx::Error) -> Self {
        match error {
            sqlx::Error::PoolTimedOut => Self::Busy,
            error => Self::Query(error),
        }
    }

    /// A write's error: unique violations become `Conflict`, the rest as [`Self::from_query`].
    pub(crate) fn from_write(error: sqlx::Error) -> Self {
        match &error {
            sqlx::Error::Database(database) if database.is_unique_violation() => Self::Conflict,
            _ => Self::from_query(error),
        }
    }
}

/// Builds the pool without connecting. Connections open on first use, so the server starts
/// (and reports "not ready") while the database is down, instead of failing to start.
pub fn connect_lazy(url: &str, settings: PoolSettings) -> Result<PgPool, DbError> {
    let options = PgConnectOptions::from_str(url)
        .map_err(|error| DbError::InvalidUrl {
            reason: redact_url_error(&error),
        })?
        .application_name("colophon-server")
        .options([(
            "statement_timeout",
            settings.statement_timeout.as_millis().to_string(),
        )]);
    // No ping on checkout: it is a round trip on every query. sqlx still pings each connection
    // as it returns to the pool and drops broken ones, so only a connection that breaks while
    // idle (a database restart) reaches a caller, and fails that one request (D-033).
    Ok(PgPoolOptions::new()
        .max_connections(settings.max_connections)
        .min_connections(0)
        .acquire_timeout(settings.acquire_timeout)
        .test_before_acquire(false)
        .connect_lazy_with(options))
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
