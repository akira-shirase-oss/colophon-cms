//! Typed configuration, read once from `COLOPHON_*` environment variables and validated at
//! startup. Every problem is reported together, so one restart fixes them all.
//!
//! The lookup is a parameter: tests pass a map and never touch the process environment
//! (`std::env::set_var` is `unsafe` in edition 2024).

use std::{fmt, net::SocketAddr, ops::RangeInclusive, str::FromStr, time::Duration};

use clap::ValueEnum;

/// Which part of Colophon this process runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Role {
    /// The management and public delivery APIs.
    Api,
    /// Background work: the outbox dispatcher, retries and reconciliation.
    Worker,
    /// Both roles in one process (the v0.1 reference profile).
    All,
}

impl Role {
    /// The name used in logs and in the readiness report.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::Worker => "worker",
            Self::All => "all",
        }
    }
}

/// Log output shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// One JSON object per line, for log pipelines.
    Json,
    /// Human-readable, for a terminal.
    Pretty,
}

impl FromStr for LogFormat {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "json" => Ok(Self::Json),
            "pretty" => Ok(Self::Pretty),
            other => Err(format!("expected `json` or `pretty`, got {other:?}")),
        }
    }
}

/// A value that must never be printed: its `Debug` is redacted and it has no `Display`.
#[derive(Clone)]
pub struct Secret(Box<str>);

impl Secret {
    /// Wraps a secret value.
    pub fn new(value: impl Into<Box<str>>) -> Self {
        Self(value.into())
    }

    /// The secret itself. The name makes every use greppable in review.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}

/// The validated configuration. Nothing is parsed again after startup.
#[derive(Debug, Clone)]
pub struct Config {
    /// The role this process runs (from `--role` or `COLOPHON_ROLE`).
    pub role: Role,
    /// Address to listen on. `COLOPHON_LISTEN_ADDR`, default `127.0.0.1:58000`.
    pub listen_addr: SocketAddr,
    /// PostgreSQL URL (holds a password). `COLOPHON_DATABASE_URL`, required.
    pub database_url: Secret,
    /// Redis-protocol cache URL. `COLOPHON_CACHE_URL`, optional: without it, readiness reports
    /// the cache as `not_configured`.
    pub cache_url: Option<Secret>,
    /// `COLOPHON_LOG_FORMAT`: `json` (default) or `pretty`.
    pub log_format: LogFormat,
    /// `EnvFilter` directives. `COLOPHON_LOG`, default `info`.
    pub log_filter: String,
    /// `COLOPHON_REQUEST_TIMEOUT_MS`, default 10 s.
    pub request_timeout: Duration,
    /// `COLOPHON_MAX_BODY_BYTES`, default 1 MiB.
    pub max_body_bytes: usize,
    /// `COLOPHON_DB_MAX_CONNECTIONS`, default 16.
    pub db_max_connections: u32,
    /// Time between readiness probes. `COLOPHON_PROBE_INTERVAL_MS`, default 2 s.
    pub probe_interval: Duration,
    /// Deadline for one probe; shorter than the interval. `COLOPHON_PROBE_TIMEOUT_MS`, default 1 s.
    pub probe_timeout: Duration,
    /// How long readiness says `draining` before the listener closes. `COLOPHON_DRAIN_DELAY_MS`,
    /// default 5 s.
    pub drain_delay: Duration,
    /// Hard deadline from the shutdown signal to exit. `COLOPHON_SHUTDOWN_GRACE_MS`, default 25 s.
    pub shutdown_grace: Duration,
}

/// Every configuration problem found, in one error. Never contains secret values.
#[derive(Debug, thiserror::Error)]
#[error("invalid configuration ({} problem(s)):\n  {}", .problems.len(), .problems.join("\n  "))]
pub struct ConfigError {
    /// One line per problem.
    pub problems: Vec<String>,
}

const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;

struct Reader<F> {
    lookup: F,
    problems: Vec<String>,
}

impl<F: Fn(&str) -> Option<String>> Reader<F> {
    /// The raw value. A blank value is a problem, not "use the default": it usually means a
    /// failed secret mount or a typo in a deployment file.
    fn raw(&mut self, key: &str) -> Option<String> {
        let value = (self.lookup)(key)?;
        if value.trim().is_empty() {
            self.problems.push(format!("{key} is set but blank"));
            return None;
        }
        Some(value)
    }

    fn parse<T>(&mut self, key: &str, default: T) -> T
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        let Some(raw) = self.raw(key) else {
            return default;
        };
        match raw.parse() {
            Ok(value) => value,
            Err(error) => {
                self.problems
                    .push(format!("{key}: cannot parse {raw:?}: {error}"));
                default
            }
        }
    }

    fn ranged<T>(&mut self, key: &str, default: T, range: RangeInclusive<T>) -> T
    where
        T: FromStr + PartialOrd + Copy + fmt::Display,
        T::Err: fmt::Display,
    {
        let value = self.parse(key, default);
        if range.contains(&value) {
            value
        } else {
            self.problems.push(format!(
                "{key}: {value} is outside {}..={}",
                range.start(),
                range.end()
            ));
            default
        }
    }

    fn millis(&mut self, key: &str, default_ms: u64, range: RangeInclusive<u64>) -> Duration {
        Duration::from_millis(self.ranged(key, default_ms, range))
    }

    /// A secret: the value is never echoed, not even in problems.
    fn secret(&mut self, key: &str, required: bool) -> Option<Secret> {
        let value = self.raw(key);
        if value.is_none() && required && (self.lookup)(key).is_none() {
            self.problems.push(format!("{key} is required"));
        }
        value.map(Secret::new)
    }
}

impl Config {
    /// Reads the process environment.
    pub fn from_env(role: Role) -> Result<Self, ConfigError> {
        // A non-UTF-8 value becomes U+FFFD and fails parsing loudly.
        Self::from_lookup(role, |key| {
            std::env::var_os(key).map(|value| value.to_string_lossy().into_owned())
        })
    }

    /// Builds a configuration from any lookup (the environment, or a map in tests).
    pub fn from_lookup(
        role: Role,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, ConfigError> {
        let mut env = Reader {
            lookup,
            problems: Vec::new(),
        };
        let listen_addr = env.parse(
            "COLOPHON_LISTEN_ADDR",
            SocketAddr::from(([127, 0, 0, 1], 58000)),
        );
        let database_url = env.secret("COLOPHON_DATABASE_URL", true);
        let cache_url = env.secret("COLOPHON_CACHE_URL", false);
        let log_format = env.parse("COLOPHON_LOG_FORMAT", LogFormat::Json);
        let log_filter = env.raw("COLOPHON_LOG").unwrap_or_else(|| "info".to_owned());
        let request_timeout = env.millis("COLOPHON_REQUEST_TIMEOUT_MS", 10_000, 100..=120_000);
        let max_body_bytes = env.ranged("COLOPHON_MAX_BODY_BYTES", MIB, KIB..=16 * MIB);
        let db_max_connections = env.ranged("COLOPHON_DB_MAX_CONNECTIONS", 16, 1..=1_000);
        let probe_interval = env.millis("COLOPHON_PROBE_INTERVAL_MS", 2_000, 100..=60_000);
        let probe_timeout = env.millis("COLOPHON_PROBE_TIMEOUT_MS", 1_000, 50..=30_000);
        let drain_delay = env.millis("COLOPHON_DRAIN_DELAY_MS", 5_000, 0..=60_000);
        let shutdown_grace = env.millis("COLOPHON_SHUTDOWN_GRACE_MS", 25_000, 1_000..=600_000);

        if probe_timeout >= probe_interval {
            env.problems.push(
                "COLOPHON_PROBE_TIMEOUT_MS must be shorter than COLOPHON_PROBE_INTERVAL_MS"
                    .to_owned(),
            );
        }
        if drain_delay >= shutdown_grace {
            env.problems.push(
                "COLOPHON_DRAIN_DELAY_MS must be shorter than COLOPHON_SHUTDOWN_GRACE_MS"
                    .to_owned(),
            );
        }

        if let Some(database_url) = database_url
            && env.problems.is_empty()
        {
            Ok(Self {
                role,
                listen_addr,
                database_url,
                cache_url,
                log_format,
                log_filter,
                request_timeout,
                max_body_bytes,
                db_max_connections,
                probe_interval,
                probe_timeout,
                drain_delay,
                shutdown_grace,
            })
        } else {
            Err(ConfigError {
                problems: env.problems,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const DB: &str = "postgres://colophon:hunter2@127.0.0.1:55432/colophon";

    fn load(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        Config::from_lookup(Role::All, |key| map.get(key).cloned())
    }

    #[test]
    fn defaults_apply_when_only_the_database_is_set() {
        let config = load(&[("COLOPHON_DATABASE_URL", DB)]).expect("valid");
        assert_eq!(config.listen_addr.to_string(), "127.0.0.1:58000");
        assert_eq!(config.request_timeout, Duration::from_secs(10));
        assert_eq!(config.max_body_bytes, MIB);
        assert_eq!(config.probe_interval, Duration::from_secs(2));
        assert!(config.cache_url.is_none());
        assert_eq!(config.log_format, LogFormat::Json);
    }

    #[test]
    fn the_database_url_is_required() {
        let error = load(&[]).expect_err("missing database");
        assert_eq!(error.problems, ["COLOPHON_DATABASE_URL is required"]);
    }

    #[test]
    fn every_problem_is_reported_at_once() {
        let error = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_LISTEN_ADDR", "nope"),
            ("COLOPHON_LOG_FORMAT", "xml"),
            ("COLOPHON_MAX_BODY_BYTES", "1"),
            ("COLOPHON_PROBE_INTERVAL_MS", "500"),
            ("COLOPHON_PROBE_TIMEOUT_MS", "500"),
            ("COLOPHON_CACHE_URL", "  "),
        ])
        .expect_err("five problems");
        assert_eq!(error.problems.len(), 5, "{:?}", error.problems);
        let text = error.to_string();
        for key in [
            "COLOPHON_LISTEN_ADDR",
            "COLOPHON_LOG_FORMAT",
            "COLOPHON_MAX_BODY_BYTES",
            "COLOPHON_PROBE_TIMEOUT_MS must be shorter",
            "COLOPHON_CACHE_URL is set but blank",
        ] {
            assert!(text.contains(key), "{key} missing from: {text}");
        }
    }

    #[test]
    fn secrets_never_appear_in_debug_output_or_errors() {
        let config = load(&[("COLOPHON_DATABASE_URL", DB)]).expect("valid");
        assert!(!format!("{config:?}").contains("hunter2"));
        let error = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_LOG_FORMAT", "hunter2"),
        ])
        .expect_err("bad format");
        // The bad *format* value is echoed (it isn't secret); the database URL never is.
        assert!(!error.to_string().contains("55432"));
    }

    #[test]
    fn drain_must_fit_inside_the_grace_period() {
        let error = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_DRAIN_DELAY_MS", "30000"),
            ("COLOPHON_SHUTDOWN_GRACE_MS", "20000"),
        ])
        .expect_err("drain longer than grace");
        assert!(
            error
                .to_string()
                .contains("COLOPHON_DRAIN_DELAY_MS must be shorter")
        );
    }
}
