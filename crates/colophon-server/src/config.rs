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

/// The OpenID Connect client registration. All three settings, or none.
#[derive(Debug, Clone)]
pub struct OidcSettings {
    /// `COLOPHON_OIDC_ISSUER`: the provider's issuer, exactly.
    pub issuer: String,
    /// `COLOPHON_OIDC_CLIENT_ID`.
    pub client_id: String,
    /// `COLOPHON_OIDC_CLIENT_SECRET`.
    pub client_secret: Secret,
}

/// The development realm's public client secret (deploy/keycloak). Refused off loopback.
const DEV_CLIENT_SECRET: &str = "colophon-dev-client-secret";

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
    /// The origin browsers reach this server at (`COLOPHON_PUBLIC_URL`, default
    /// `http://localhost:58000`): the sign-in redirect URI and the first trusted origin.
    pub public_origin: String,
    /// Sign-in; `None` when the three `COLOPHON_OIDC_*` settings are absent.
    pub oidc: Option<OidcSettings>,
    /// More origins allowed to send cookie-authenticated writes (`COLOPHON_TRUSTED_ORIGINS`,
    /// comma-separated), after the public origin.
    pub extra_trusted_origins: Vec<String>,
    /// A Studio build to serve at `/studio` (`COLOPHON_STUDIO_DIR`); none by default. Checked
    /// when the server starts.
    pub studio_dir: Option<std::path::PathBuf>,
    /// How often the worker looks for due jobs. `COLOPHON_WORKER_POLL_MS`, default 500 ms.
    pub worker_poll: Duration,
    /// How long a worker's claim on a job lasts. `COLOPHON_WORKER_LEASE_MS`, default 30 s; longer
    /// than an attempt's 10 s deadline.
    pub worker_lease: Duration,
    /// The most delivery attempts in flight at once. `COLOPHON_WORKER_SLOTS`, default 8.
    pub worker_slots: usize,
    /// Each delivery attempt's deadline. `COLOPHON_WORKER_ATTEMPT_MS`, default 10 s; shorter
    /// than the lease, so no request outlives its holder's claim.
    pub worker_attempt: Duration,
    /// The worker's own database pool, separate from the API's so deliveries can't starve
    /// requests. `COLOPHON_WORKER_DB_CONNECTIONS`, default 4.
    pub worker_db_connections: u32,
    /// Where the worker runs, as its probes' evidence names it, and which pinned probes it may
    /// run (D-051). `COLOPHON_WORKER_LOCATION`: 1 to 63 lowercase letters, digits and dashes,
    /// default `local`.
    pub worker_location: String,
    /// Rate limits (D-076): `COLOPHON_RATE_LIMIT_PER_ADDRESS` (default `300/600`, requests a
    /// second and burst, or `off`), `COLOPHON_RATE_LIMIT_PER_CREDENTIAL` (default `150/300`),
    /// `COLOPHON_RATE_LIMIT_DELIVERY` (public delivery per address, default `1000/2000`), and
    /// `COLOPHON_TRUSTED_PROXIES` (comma-separated addresses or networks whose
    /// `X-Forwarded-For` names the client; default none).
    pub admission: colophon_api::admission::AdmissionSettings,
}

/// Whether `text` can name a worker's location: what destinations.location accepts.
fn is_location(text: &str) -> bool {
    let bytes = text.as_bytes();
    (1..=63).contains(&bytes.len())
        && bytes
            .first()
            .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

/// `scheme://host[:port]` with no path, query or fragment, as browsers send in `Origin`.
fn parse_origin(text: &str) -> Option<String> {
    let url = url::Url::parse(text).ok()?;
    let bare = matches!(url.path(), "" | "/") && url.query().is_none() && url.fragment().is_none();
    let web = matches!(url.scheme(), "http" | "https");
    (bare && web && url.host().is_some()).then(|| url.origin().ascii_serialization())
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

    /// A rate limit (D-076): `off`, `<per second>` or `<per second>/<burst>`.
    fn rate(&mut self, key: &str, default: &str) -> Option<colophon_api::admission::Rate> {
        let raw = self.raw(key).unwrap_or_else(|| default.to_owned());
        colophon_api::admission::Rate::parse(&raw).unwrap_or_else(|error| {
            self.problems.push(format!("{key}: {error}"));
            colophon_api::admission::Rate::parse(default).ok().flatten()
        })
    }

    /// Comma-separated addresses or networks; none when unset.
    fn prefixes(&mut self, key: &str) -> Vec<colophon_api::admission::IpPrefix> {
        let Some(raw) = self.raw(key) else {
            return Vec::new();
        };
        let mut prefixes = Vec::new();
        for item in raw.split(',') {
            match colophon_api::admission::IpPrefix::parse(item) {
                Ok(prefix) => prefixes.push(prefix),
                Err(error) => self.problems.push(format!("{key}: {item:?}: {error}")),
            }
        }
        prefixes
    }

    /// Sign-in settings: the public origin, extra trusted origins, and the OIDC client, with the
    /// rule that the development client's public secret only runs on loopback.
    fn sign_in(&mut self, listen_addr: SocketAddr) -> (String, Vec<String>, Option<OidcSettings>) {
        let env = self;
        let public_url = env
            .raw("COLOPHON_PUBLIC_URL")
            .unwrap_or_else(|| "http://localhost:58000".to_owned());
        let public_origin = parse_origin(&public_url).unwrap_or_else(|| {
            env.problems.push(format!("COLOPHON_PUBLIC_URL: {public_url:?} is not an http(s) origin like https://cms.example.com"));
            String::new()
        });
        let mut extra_trusted_origins = Vec::new();
        if let Some(list) = env.raw("COLOPHON_TRUSTED_ORIGINS") {
            for item in list
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
            {
                match parse_origin(item) {
                    Some(origin) => extra_trusted_origins.push(origin),
                    None => env.problems.push(format!(
                        "COLOPHON_TRUSTED_ORIGINS: {item:?} is not an http(s) origin"
                    )),
                }
            }
        }
        let issuer = env.raw("COLOPHON_OIDC_ISSUER");
        let client_id = env.raw("COLOPHON_OIDC_CLIENT_ID");
        let client_secret = env.secret("COLOPHON_OIDC_CLIENT_SECRET", false);
        let oidc = match (issuer, client_id, client_secret) {
            (Some(issuer), Some(client_id), Some(client_secret)) => Some(OidcSettings {
                issuer,
                client_id,
                client_secret,
            }),
            (None, None, None) => None,
            _ => {
                env.problems.push("COLOPHON_OIDC_ISSUER, COLOPHON_OIDC_CLIENT_ID and COLOPHON_OIDC_CLIENT_SECRET go together: set all three or none".to_owned());
                None
            }
        };
        // The development realm's secret is public: never on an address others can reach (O-008).
        if oidc
            .as_ref()
            .is_some_and(|oidc| oidc.client_secret.expose() == DEV_CLIENT_SECRET)
            && !listen_addr.ip().is_loopback()
        {
            env.problems.push("the development sign-in client (its secret is public) may only run on a loopback COLOPHON_LISTEN_ADDR".to_owned());
        }

        (public_origin, extra_trusted_origins, oidc)
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
        let (public_origin, extra_trusted_origins, oidc) = env.sign_in(listen_addr);
        let studio_dir = env.raw("COLOPHON_STUDIO_DIR").map(std::path::PathBuf::from);
        let worker_poll = env.millis("COLOPHON_WORKER_POLL_MS", 500, 10..=60_000);
        let worker_lease = env.millis("COLOPHON_WORKER_LEASE_MS", 30_000, 1_000..=600_000);
        let worker_slots = env.ranged("COLOPHON_WORKER_SLOTS", 8, 1..=64);
        let worker_attempt = env.millis("COLOPHON_WORKER_ATTEMPT_MS", 10_000, 100..=60_000);
        let worker_db_connections = env.ranged("COLOPHON_WORKER_DB_CONNECTIONS", 4, 1..=100);
        let admission = colophon_api::admission::AdmissionSettings {
            per_address: env.rate("COLOPHON_RATE_LIMIT_PER_ADDRESS", "300/600"),
            per_credential: env.rate("COLOPHON_RATE_LIMIT_PER_CREDENTIAL", "150/300"),
            delivery_per_address: env.rate("COLOPHON_RATE_LIMIT_DELIVERY", "1000/2000"),
            trusted_proxies: env.prefixes("COLOPHON_TRUSTED_PROXIES"),
        };
        let worker_location = env
            .raw("COLOPHON_WORKER_LOCATION")
            .unwrap_or_else(|| "local".to_owned());
        if !is_location(&worker_location) {
            env.problems.push(
                "COLOPHON_WORKER_LOCATION must be 1 to 63 lowercase letters, digits and dashes, \
                 starting with a letter or digit"
                    .to_owned(),
            );
        }

        if probe_timeout >= probe_interval {
            env.problems.push(
                "COLOPHON_PROBE_TIMEOUT_MS must be shorter than COLOPHON_PROBE_INTERVAL_MS"
                    .to_owned(),
            );
        }
        if worker_attempt >= worker_lease {
            env.problems.push(
                "COLOPHON_WORKER_ATTEMPT_MS must be shorter than COLOPHON_WORKER_LEASE_MS"
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
                public_origin,
                oidc,
                extra_trusted_origins,
                studio_dir,
                worker_poll,
                worker_lease,
                worker_slots,
                worker_attempt,
                worker_db_connections,
                worker_location,
                admission,
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
        assert_eq!(config.worker_location, "local");
    }

    #[test]
    fn a_worker_location_is_a_plain_label() {
        for good in ["eu-west-1", "laptop", "a", "9"] {
            let config = load(&[
                ("COLOPHON_DATABASE_URL", DB),
                ("COLOPHON_WORKER_LOCATION", good),
            ])
            .expect("valid");
            assert_eq!(config.worker_location, good);
        }
        for bad in ["EU", "-west", "a b", "ä", &"x".repeat(64)] {
            let error = load(&[
                ("COLOPHON_DATABASE_URL", DB),
                ("COLOPHON_WORKER_LOCATION", bad),
            ])
            .expect_err(bad);
            assert!(
                error.to_string().contains("COLOPHON_WORKER_LOCATION"),
                "{bad}"
            );
        }
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
    fn sign_in_settings_go_together() {
        let error = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            (
                "COLOPHON_OIDC_ISSUER",
                "http://localhost:58080/realms/colophon",
            ),
        ])
        .expect_err("issuer alone");
        assert!(error.to_string().contains("go together"));
        let config = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            (
                "COLOPHON_OIDC_ISSUER",
                "http://localhost:58080/realms/colophon",
            ),
            ("COLOPHON_OIDC_CLIENT_ID", "colophon-server"),
            ("COLOPHON_OIDC_CLIENT_SECRET", "s3cret-value"),
        ])
        .expect("all three");
        assert!(config.oidc.is_some());
        assert!(
            !format!("{config:?}").contains("s3cret-value"),
            "the secret is redacted"
        );
    }

    #[test]
    fn the_public_development_client_refuses_to_run_off_loopback() {
        let base = [
            ("COLOPHON_DATABASE_URL", DB),
            (
                "COLOPHON_OIDC_ISSUER",
                "http://localhost:58080/realms/colophon",
            ),
            ("COLOPHON_OIDC_CLIENT_ID", "colophon-server"),
            ("COLOPHON_OIDC_CLIENT_SECRET", "colophon-dev-client-secret"),
        ];
        assert!(load(&base).is_ok(), "fine on loopback");
        let mut public = base.to_vec();
        public.push(("COLOPHON_LISTEN_ADDR", "0.0.0.0:58000"));
        let error = load(&public).expect_err("refused on 0.0.0.0");
        assert!(error.to_string().contains("loopback"), "{error}");
    }

    #[test]
    fn origins_are_exact_and_bare() {
        let config = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_PUBLIC_URL", "https://cms.example.com/"),
            (
                "COLOPHON_TRUSTED_ORIGINS",
                "http://localhost:58101, https://preview.example.com",
            ),
        ])
        .expect("valid");
        assert_eq!(config.public_origin, "https://cms.example.com");
        assert_eq!(
            config.extra_trusted_origins,
            ["http://localhost:58101", "https://preview.example.com"]
        );
        for bad in [
            "https://cms.example.com/path",
            "ftp://cms.example.com",
            "not a url",
        ] {
            assert!(
                load(&[("COLOPHON_DATABASE_URL", DB), ("COLOPHON_PUBLIC_URL", bad)]).is_err(),
                "{bad}"
            );
        }
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
