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

/// The S3-compatible object store for uploads (Garage locally, a managed bucket live; D-016).
/// Only the S3 API is spoken, so the store can be swapped.
#[derive(Debug, Clone)]
pub struct S3Settings {
    /// `COLOPHON_S3_ENDPOINT`: the API's URL, e.g. `http://127.0.0.1:53900`.
    pub endpoint: String,
    /// `COLOPHON_S3_REGION`, default `garage`.
    pub region: String,
    /// `COLOPHON_S3_BUCKET`.
    pub bucket: String,
    /// `COLOPHON_S3_ACCESS_KEY`.
    pub access_key: String,
    /// `COLOPHON_S3_SECRET_KEY`.
    pub secret_key: Secret,
    /// Plain HTTP rather than HTTPS: only for a loopback endpoint (the local stack's Garage).
    pub allow_http: bool,
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
    /// PostgreSQL URL (holds a password). `COLOPHON_DATABASE_URL`, or the file
    /// `COLOPHON_DATABASE_URL_FILE` names; required. Every secret below takes `_FILE` too (D-173).
    pub database_url: Secret,
    /// Redis-protocol cache URL. `COLOPHON_CACHE_URL`, optional: without it, readiness reports
    /// the cache as `not_configured`.
    pub cache_url: Option<Secret>,
    /// A cached delivery answer's lifetime (`COLOPHON_CACHE_TTL_MS`, default five minutes) and
    /// the bound on the jitter added to each entry (`COLOPHON_CACHE_JITTER_MS`, default one
    /// minute; `0` makes every entry live exactly the TTL — what a stampede load run needs for
    /// its expiries to be deterministic, L04).
    pub cache_ttl: Duration,
    /// See `cache_ttl`.
    pub cache_jitter: Duration,
    /// `COLOPHON_LOG_FORMAT`: `json` (default) or `pretty`.
    pub log_format: LogFormat,
    /// `EnvFilter` directives. `COLOPHON_LOG`, default `info`.
    pub log_filter: String,
    /// `COLOPHON_REQUEST_TIMEOUT_MS`, default 10 s.
    pub request_timeout: Duration,
    /// `COLOPHON_HEADER_TIMEOUT_MS`, default 10 s (100 ms to 60 s): how long a connection has to
    /// send a request's headers before it is closed (D-230).
    pub header_timeout: Duration,
    /// `COLOPHON_MAX_BODY_BYTES`, default 21 MiB: JSON requests stay small, but the upload
    /// endpoint legitimately carries a 20 MiB image (its own intent check bounds it tighter).
    pub max_body_bytes: usize,
    /// `COLOPHON_UPLOAD_MEMORY_MB`, default 256: the memory uploads in flight may hold between them
    /// (D-215); at the 21 MiB body limit that is 12 at once, and never less than one.
    pub upload_memory_mb: usize,
    /// `COLOPHON_DB_MAX_CONNECTIONS`, default 16.
    pub db_max_connections: u32,
    /// Slow statements, in two tiers (D-224): logged at INFO from `COLOPHON_DB_SLOW_STATEMENT_INFO_MS`
    /// (default 100), the evidence that names a stall's statements (D-221), and at WARN from
    /// `COLOPHON_DB_SLOW_STATEMENT_MS` (default 1000, sqlx's own), an alert. The INFO tier can't be
    /// above the WARN tier.
    pub db_slow_statements: colophon_db::SlowStatements,
    /// Time between readiness probes. `COLOPHON_PROBE_INTERVAL_MS`, default 2 s.
    pub probe_interval: Duration,
    /// Deadline for one probe; shorter than the interval. `COLOPHON_PROBE_TIMEOUT_MS`, default 1 s.
    pub probe_timeout: Duration,
    /// How long readiness says `draining` before the listener closes. `COLOPHON_DRAIN_DELAY_MS`,
    /// default 5 s.
    pub drain_delay: Duration,
    /// How long requests have to finish after the shutdown signal; background cleanup then has 5 s
    /// more (D-197), so a supervisor's stop timeout must exceed this plus 5 s.
    /// `COLOPHON_SHUTDOWN_GRACE_MS`, default 25 s.
    pub shutdown_grace: Duration,
    /// The origin browsers reach this server at (`COLOPHON_PUBLIC_URL`, default
    /// `http://localhost:58000`): the sign-in redirect URI and the first trusted origin.
    pub public_origin: String,
    /// The origin delivery's media URLs name (`COLOPHON_MEDIA_URL`, default the public origin;
    /// D-178): a CDN or a separate asset host in front of `/media/v1`.
    pub media_origin: String,
    /// Sign-in; `None` when the three `COLOPHON_OIDC_*` settings are absent.
    pub oidc: Option<OidcSettings>,
    /// The object store uploads are quarantined in and served from (07-media); `None` when the
    /// `COLOPHON_S3_*` settings are absent.
    pub s3: Option<S3Settings>,
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
    /// How old an unreferenced upload must be before the retention sweep expires it (07-media,
    /// spec §18's 24 hours; D-099). `COLOPHON_ASSET_EXPIRY_MS`, default `86_400_000`.
    pub asset_expiry: Duration,
    /// How often the retention sweep looks for collectable assets and orphaned quarantine
    /// objects (D-099). `COLOPHON_ASSET_SWEEP_MS`, default `300_000` (five minutes).
    pub asset_sweep: Duration,
    /// How often the table-retention sweep runs its bounded deletes over finished work,
    /// the settled ledger, dead flow state and the audit log (spec §27, D-135).
    /// `COLOPHON_TABLE_SWEEP_MS`, default `3_600_000` (one hour).
    pub table_sweep: Duration,
    /// Rate limits (D-076): `COLOPHON_RATE_LIMIT_PER_ADDRESS` (default `300/600`, requests a
    /// second and burst, or `off`), `COLOPHON_RATE_LIMIT_PER_CREDENTIAL` (default `150/300`),
    /// `COLOPHON_RATE_LIMIT_DELIVERY` (public delivery per address, default `1000/2000`),
    /// `COLOPHON_RATE_LIMIT_PASSWORD` (password work — sign-in, sign-up — per address on top of
    /// the management limits, IPv6 by its /64, default `5/10`; D-213), and
    /// `COLOPHON_TRUSTED_PROXIES` (comma-separated addresses or networks whose
    /// `X-Forwarded-For` names the client; default none). The per-token request quota's
    /// window (D-118) is `COLOPHON_QUOTA_WINDOW_MS`, default `60_000` — the minute
    /// `requests_per_minute` names; a test shortens it rather than waiting a minute.
    pub admission: colophon_api::admission::AdmissionSettings,
    /// Live-hint stream bounds (D-108): `COLOPHON_EVENTS_PER_CREDENTIAL` (8),
    /// `COLOPHON_EVENTS_PER_PROJECT` (256) and `COLOPHON_EVENTS_MAX_STREAMS` (2048) cap the
    /// streams; `COLOPHON_EVENTS_CHANNEL_CAPACITY` (256) buffers hints per project;
    /// `COLOPHON_EVENTS_KEEPALIVE_MS` (15 s) warms idle connections;
    /// `COLOPHON_EVENTS_LIFETIME_MS` (30 min) ends a stream; and
    /// `COLOPHON_EVENTS_REVALIDATE_MS` (5 min; `0` disables) re-runs the membership check.
    /// The public delivery streams have their own budget (D-159):
    /// `COLOPHON_EVENTS_PUBLIC_PER_PROJECT` (256), `COLOPHON_EVENTS_PUBLIC_MAX_STREAMS` (2048)
    /// and `COLOPHON_EVENTS_PUBLIC_PER_ADDRESS` (8).
    pub events: colophon_api::events::EventLimits,
    /// Remote-MCP session bounds (D-121): `COLOPHON_MCP_PER_TOKEN` (8),
    /// `COLOPHON_MCP_PER_PROJECT` (256) and `COLOPHON_MCP_MAX_SESSIONS` (2048) cap the
    /// sessions; `COLOPHON_MCP_IDLE_MS` (30 min) forgets one untouched for that long.
    pub mcp: colophon_api::mcp::Limits,
    /// Argon2id jobs that may run at once (D-150): `COLOPHON_PASSWORD_CONCURRENCY`, one
    /// fewer than the cores by default, leaving one for everything else (D-213; 1–256). Past it a password job waits briefly, then the request is
    /// shed with `503` and `Retry-After`.
    pub password_concurrency: usize,
    /// Magic-link request bounds (D-161): `COLOPHON_MAGIC_LINKS_PER_ADDRESS` links one client
    /// address may ask for in one project per ten minutes (5), and
    /// `COLOPHON_MAGIC_LINKS_PER_PROJECT` a project may record per minute (300).
    pub magic_links: colophon_db::audience::MagicLinkLimits,
    /// Whether destinations may name private, loopback and link-local addresses and plain
    /// http: `COLOPHON_ALLOW_PRIVATE_DESTINATIONS=1` (D-149). Off by default and never
    /// inferred from the listen address — a production server behind a local reverse proxy
    /// binds loopback too. The development stack opts in (`deploy/local.env`).
    pub allow_private_destinations: bool,
    /// Whether a magic-link request hands the link back in `dev_link` instead of emailing it:
    /// `COLOPHON_DEV_MAIL=1` (D-140). A development affordance, off by default.
    pub dev_mail: bool,
    /// How this server was installed, when the installer says so: `COLOPHON_INSTALL_KIND`,
    /// only `container` today (the image sets it, D-174). The Studio's first-run screen uses it
    /// to show where the setup code is — `docker compose logs`, not a terminal.
    pub install_kind: Option<&'static str>,
    /// Transactional mail for sign-in links (D-145): `COLOPHON_MAIL_PROVIDER=resend` with
    /// `COLOPHON_MAIL_FROM` and `COLOPHON_RESEND_API_KEY`; `COLOPHON_RESEND_ENDPOINT` overrides
    /// the API address (a deployment's own relay, or a test receiver). `None` without a
    /// provider — links are then recorded but not delivered, as before.
    pub mail: Option<MailSettings>,
    /// Self-serve accounts (D-235): registration, the project cap, the per-account quotas, the
    /// sign-up cap per address, the signed-in hint's domain and Turnstile's site key.
    pub selfserve: colophon_api::selfserve::SelfServe,
    /// Cloudflare Turnstile's secret key (`COLOPHON_TURNSTILE_SECRET[_FILE]`), with
    /// `COLOPHON_TURNSTILE_SITE_KEY`: both or neither.
    pub turnstile_secret: Option<Secret>,
    /// Where Turnstile tokens are verified (`COLOPHON_TURNSTILE_VERIFY_URL`): Cloudflare's
    /// `siteverify` unless set, and only ever set to a loopback address (a test's stub), so
    /// configuration can't send the secret and visitors' addresses anywhere else.
    pub turnstile_verify_url: Option<String>,
    /// The installation's key for keyed digests in the staff auth log (`COLOPHON_AUTH_EVENT_KEY[_FILE]`):
    /// a failed sign-in for an address with no account is recorded by HMAC-SHA256 under it, so
    /// the log can't confirm a guessed address without the key. `None` records no digest.
    pub auth_event_key: Option<Secret>,
}

/// Transactional mail configuration (D-145); `provider` is the seam further providers hang off.
#[derive(Debug, Clone)]
pub struct MailSettings {
    /// The provider this delivers through.
    pub provider: MailProvider,
    /// The `From` header, as the provider wants it (`The Draft <hello@thedraft.example>`).
    pub from: String,
    /// The provider's API key.
    pub api_key: Secret,
    /// The endpoint mails POST to.
    pub endpoint: String,
}

/// A mail provider this build knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailProvider {
    /// Resend: one `POST /emails` with a bearer key.
    Resend,
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

/// Whether `origin` (as [`parse_origin`] wrote it) is one only this machine can use: `localhost`
/// or a `*.localhost` name, or a loopback address. The development affordances need one (D-163):
/// behind a reverse proxy a production server listens on loopback too, but serves a public
/// origin.
pub(crate) fn is_loopback_origin(origin: &str) -> bool {
    match url::Url::parse(origin)
        .ok()
        .as_ref()
        .and_then(url::Url::host)
    {
        Some(url::Host::Domain(name)) => {
            name.eq_ignore_ascii_case("localhost")
                || name.to_ascii_lowercase().ends_with(".localhost")
        }
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
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
        // The development realm's secret is public: never on an address others can reach
        // (O-008), nor behind a public origin, which a loopback listener behind a reverse proxy
        // serves too (D-163).
        if oidc
            .as_ref()
            .is_some_and(|oidc| oidc.client_secret.expose() == DEV_CLIENT_SECRET)
            && !(listen_addr.ip().is_loopback() && is_loopback_origin(&public_origin))
        {
            env.problems.push("the development sign-in client (its secret is public) may only run on a loopback COLOPHON_LISTEN_ADDR with a localhost COLOPHON_PUBLIC_URL".to_owned());
        }

        (public_origin, extra_trusted_origins, oidc)
    }

    /// An opt-in switch: `1` or `true` (any case) turns it on; unset, `0` or `false` leave it
    /// off, and anything else is a problem rather than a silent default.
    fn flag(&mut self, key: &str) -> bool {
        match self.raw(key) {
            None => false,
            Some(value) if value == "1" || value.eq_ignore_ascii_case("true") => true,
            Some(value) if value == "0" || value.eq_ignore_ascii_case("false") => false,
            Some(_) => {
                self.problems.push(format!(
                    "{key} is 1 or true to turn it on, 0 or false to leave it off"
                ));
                false
            }
        }
    }

    /// Transactional mail (D-145): `COLOPHON_MAIL_PROVIDER=resend` with its key and a `From`,
    /// or nothing at all. `COLOPHON_RESEND_ENDPOINT` moves the API address — a deployment's
    /// own relay, or a test's receiver.
    fn mail(&mut self) -> Option<MailSettings> {
        let provider = self.raw("COLOPHON_MAIL_PROVIDER");
        let from = self.raw("COLOPHON_MAIL_FROM");
        let api_key = self.secret("COLOPHON_RESEND_API_KEY", false);
        let endpoint = self.raw("COLOPHON_RESEND_ENDPOINT");
        match (provider.as_deref(), from, api_key, endpoint) {
            (None, None, None, None) => None,
            (Some("resend"), Some(from), Some(api_key), endpoint) => Some(MailSettings {
                provider: MailProvider::Resend,
                from,
                api_key,
                endpoint: endpoint.unwrap_or_else(|| "https://api.resend.com/emails".to_owned()),
            }),
            (Some(other), _, _, _) => {
                self.problems.push(format!(
                    "COLOPHON_MAIL_PROVIDER: {other:?} is not one this build knows ('resend')"
                ));
                None
            }
            _ => {
                self.problems.push(
                    "COLOPHON_MAIL_PROVIDER, COLOPHON_MAIL_FROM and COLOPHON_RESEND_API_KEY go \
                     together: set all three or none (COLOPHON_RESEND_ENDPOINT only moves \
                     the provider's address)"
                        .to_owned(),
                );
                None
            }
        }
    }

    /// Self-serve accounts (D-235, `colophon_api::selfserve`), and Turnstile's secret: the site
    /// key and the secret go together, and the hint domain must cover the public host.
    fn selfserve(
        &mut self,
        public_origin: &str,
    ) -> (colophon_api::selfserve::SelfServe, Option<Secret>) {
        use colophon_api::selfserve::{Quotas, Registration, SelfServe, hint_domain_covers};
        let registration = match self.raw("COLOPHON_REGISTRATION") {
            None => Registration::default(),
            Some(value) => Registration::parse(&value).unwrap_or_else(|problem| {
                self.problems.push(problem);
                Registration::default()
            }),
        };
        let projects_per_account = self.ranged("COLOPHON_PROJECTS_PER_ACCOUNT", 3, 1..=10_000);
        let quotas = Quotas {
            entries: self.ranged(
                "COLOPHON_ACCOUNT_MAX_ENTRIES",
                Quotas::DEFAULT.entries,
                0..=1_000_000_000,
            ),
            assets: self.ranged(
                "COLOPHON_ACCOUNT_MAX_ASSETS",
                Quotas::DEFAULT.assets,
                0..=1_000_000_000,
            ),
            storage_bytes: self.ranged(
                "COLOPHON_ACCOUNT_MAX_STORAGE_BYTES",
                Quotas::DEFAULT.storage_bytes,
                0..=(1 << 50),
            ),
        };
        let signups_per_address_hour =
            self.ranged("COLOPHON_SIGNUPS_PER_ADDRESS_HOUR", 10, 1..=100_000);
        let hint_domain = self.raw("COLOPHON_SIGNED_IN_HINT_DOMAIN").map(|domain| {
            let host = url::Url::parse(public_origin)
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned))
                .unwrap_or_default();
            if !hint_domain_covers(&domain, &host) {
                self.problems.push(format!(
                    "COLOPHON_SIGNED_IN_HINT_DOMAIN: {domain:?} must be COLOPHON_PUBLIC_URL's host \
                     ({host:?}) or a parent of it, of two labels at least"
                ));
            }
            domain.trim_start_matches('.').to_ascii_lowercase()
        });
        let site_key = self.raw("COLOPHON_TURNSTILE_SITE_KEY");
        let secret = self.secret("COLOPHON_TURNSTILE_SECRET", false);
        let (turnstile_site_key, turnstile_secret) = match (site_key, secret) {
            (Some(site_key), Some(secret)) => (Some(site_key), Some(secret)),
            (None, None) => (None, None),
            _ => {
                self.problems.push(
                    "COLOPHON_TURNSTILE_SITE_KEY and COLOPHON_TURNSTILE_SECRET (or its _FILE) go \
                     together: set both or neither"
                        .to_owned(),
                );
                (None, None)
            }
        };
        (
            SelfServe {
                registration,
                projects_per_account,
                quotas,
                signups_per_address_hour,
                hint_domain,
                turnstile_site_key,
            },
            turnstile_secret,
        )
    }

    /// The upload store's settings; all five `COLOPHON_S3_*` values or none, and plain HTTP only
    /// when the endpoint is loopback (a public store over plain HTTP would leak uploads and keys).
    fn s3(&mut self) -> Option<S3Settings> {
        let endpoint = self.raw("COLOPHON_S3_ENDPOINT");
        let bucket = self.raw("COLOPHON_S3_BUCKET");
        let access_key = self.raw("COLOPHON_S3_ACCESS_KEY");
        let secret_key = self.secret("COLOPHON_S3_SECRET_KEY", false);
        let any =
            endpoint.is_some() || bucket.is_some() || access_key.is_some() || secret_key.is_some();
        let (Some(endpoint), Some(bucket), Some(access_key), Some(secret_key)) =
            (endpoint, bucket, access_key, secret_key)
        else {
            if any {
                self.problems.push(
                    "COLOPHON_S3_ENDPOINT, COLOPHON_S3_BUCKET, COLOPHON_S3_ACCESS_KEY and \
                     COLOPHON_S3_SECRET_KEY go together: set all four or none"
                        .to_owned(),
                );
            }
            return None;
        };
        let mut allow_http = false;
        match url::Url::parse(&endpoint) {
            Err(_) => self
                .problems
                .push(format!("COLOPHON_S3_ENDPOINT: cannot parse {endpoint:?}")),
            Ok(parsed) if parsed.scheme() == "http" => {
                allow_http = parsed.host_str().is_some_and(|host| {
                    host == "localhost"
                        || host.ends_with(".localhost")
                        || host
                            .parse::<std::net::IpAddr>()
                            .is_ok_and(|ip| ip.is_loopback())
                });
                if !allow_http {
                    self.problems.push(
                        "COLOPHON_S3_ENDPOINT may be plain HTTP only for a loopback address"
                            .to_owned(),
                    );
                }
            }
            Ok(parsed) if parsed.scheme() != "https" => self.problems.push(format!(
                "COLOPHON_S3_ENDPOINT: unsupported scheme {:?}",
                parsed.scheme()
            )),
            _ => {}
        }
        let region = self
            .raw("COLOPHON_S3_REGION")
            .unwrap_or_else(|| "garage".to_owned());
        Some(S3Settings {
            endpoint,
            region,
            bucket,
            access_key,
            secret_key,
            allow_http,
        })
    }

    /// The rate limits (D-076) and the token quota's window (D-118).
    fn admission(&mut self) -> colophon_api::admission::AdmissionSettings {
        colophon_api::admission::AdmissionSettings {
            per_address: self.rate("COLOPHON_RATE_LIMIT_PER_ADDRESS", "300/600"),
            per_credential: self.rate("COLOPHON_RATE_LIMIT_PER_CREDENTIAL", "150/300"),
            delivery_per_address: self.rate("COLOPHON_RATE_LIMIT_DELIVERY", "1000/2000"),
            password_per_address: self.rate("COLOPHON_RATE_LIMIT_PASSWORD", "5/10"),
            trusted_proxies: self.prefixes("COLOPHON_TRUSTED_PROXIES"),
            // A window under 10 ms is a broken test, not a configuration: the floor keeps
            // one; the ceiling is an hour, far past the minute the column names.
            quota_window_ms: self.ranged("COLOPHON_QUOTA_WINDOW_MS", 60_000, 10..=3_600_000),
        }
    }

    /// The delivery probe's cadence and deadline; `from_lookup` refuses a timeout at or over
    /// the interval.
    fn probe(&mut self) -> (Duration, Duration) {
        (
            self.millis("COLOPHON_PROBE_INTERVAL_MS", 2_000, 100..=60_000),
            self.millis("COLOPHON_PROBE_TIMEOUT_MS", 1_000, 50..=30_000),
        )
    }

    /// The post-listen drain delay and the shutdown grace; `from_lookup` refuses a delay at
    /// or over the grace.
    fn drain(&mut self) -> (Duration, Duration) {
        (
            self.millis("COLOPHON_DRAIN_DELAY_MS", 5_000, 0..=60_000),
            self.millis("COLOPHON_SHUTDOWN_GRACE_MS", 25_000, 1_000..=600_000),
        )
    }

    /// A cached delivery answer's TTL and jitter bound. A one-second TTL floor keeps `EX`
    /// valid; the hour ceiling is far past module 06's five minutes. Zero jitter is allowed —
    /// a stampede run's expiries must be exact (L04).
    fn cache_lifetimes(&mut self) -> (Duration, Duration) {
        (
            self.millis("COLOPHON_CACHE_TTL_MS", 300_000, 1_000..=3_600_000),
            self.millis("COLOPHON_CACHE_JITTER_MS", 60_000, 0..=600_000),
        )
    }

    /// The live-hint stream bounds (D-108); `COLOPHON_EVENTS_REVALIDATE_MS` of `0` turns the
    /// membership re-check off.
    fn events(&mut self) -> colophon_api::events::EventLimits {
        let revalidate = self.ranged("COLOPHON_EVENTS_REVALIDATE_MS", 300_000, 0..=3_600_000);
        colophon_api::events::EventLimits {
            per_credential: self.ranged("COLOPHON_EVENTS_PER_CREDENTIAL", 8, 1..=1_024),
            per_project: self.ranged("COLOPHON_EVENTS_PER_PROJECT", 256, 1..=16_384),
            max_streams: self.ranged("COLOPHON_EVENTS_MAX_STREAMS", 2_048, 1..=65_536),
            channel_capacity: self.ranged("COLOPHON_EVENTS_CHANNEL_CAPACITY", 256, 8..=8_192),
            keep_alive: self.millis("COLOPHON_EVENTS_KEEPALIVE_MS", 15_000, 1_000..=120_000),
            lifetime: self.millis("COLOPHON_EVENTS_LIFETIME_MS", 1_800_000, 1_000..=86_400_000),
            revalidate: (revalidate > 0).then(|| Duration::from_millis(revalidate)),
            public_per_project: self.ranged("COLOPHON_EVENTS_PUBLIC_PER_PROJECT", 256, 1..=16_384),
            public_max_streams: self.ranged(
                "COLOPHON_EVENTS_PUBLIC_MAX_STREAMS",
                2_048,
                1..=65_536,
            ),
            public_per_address: self.ranged("COLOPHON_EVENTS_PUBLIC_PER_ADDRESS", 8, 1..=1_024),
        }
    }

    /// The remote-MCP session bounds (D-121).
    fn mcp(&mut self) -> colophon_api::mcp::Limits {
        colophon_api::mcp::Limits {
            per_token: self.ranged("COLOPHON_MCP_PER_TOKEN", 8, 1..=1_024),
            per_project: self.ranged("COLOPHON_MCP_PER_PROJECT", 256, 1..=16_384),
            max_sessions: self.ranged("COLOPHON_MCP_MAX_SESSIONS", 2_048, 1..=65_536),
            idle: self.millis("COLOPHON_MCP_IDLE_MS", 1_800_000, 60_000..=86_400_000),
        }
    }

    /// A secret: the value is never echoed, not even in problems. It comes from `KEY`, or from
    /// the file `KEY_FILE` names — a mounted secret, so the value is never in the environment
    /// (D-173). Setting both is a problem, not a precedence rule: one of them is a mistake.
    fn secret(&mut self, key: &str, required: bool) -> Option<Secret> {
        let file_key = format!("{key}_FILE");
        let in_env = (self.lookup)(key).is_some();
        let path = self.raw(&file_key);
        if in_env && path.is_some() {
            self.problems
                .push(format!("{key} and {file_key} are both set: use one"));
            return None;
        }
        if let Some(path) = path {
            return match read_secret_file(std::path::Path::new(&path)) {
                Ok(value) => Some(Secret::new(value)),
                Err(problem) => {
                    self.problems.push(format!("{file_key}: {path}: {problem}"));
                    None
                }
            };
        }
        let value = self.raw(key);
        if value.is_none() && required && !in_env && (self.lookup)(&file_key).is_none() {
            self.problems.push(format!(
                "{key} is required (or {file_key}, a file holding it)"
            ));
        }
        value.map(Secret::new)
    }
}

/// The most bytes a secret file may hold: a URL with a password is far shorter, and a bound
/// keeps a wrong path (a log, a device) from being read whole.
const SECRET_FILE_MAX_BYTES: u64 = 16 * 1024;

/// Reads a mounted secret: UTF-8, at most [`SECRET_FILE_MAX_BYTES`], surrounding whitespace
/// trimmed (the newline `echo` and editors add). The problem never quotes the contents. Read
/// once at startup, before the server does anything else, so a blocking read is fine here.
fn read_secret_file(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read as _;
    let file = std::fs::File::open(path).map_err(|error| format!("cannot open it: {error}"))?;
    let mut bytes = Vec::new();
    file.take(SECRET_FILE_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read it: {error}"))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > SECRET_FILE_MAX_BYTES {
        return Err(format!(
            "it is over {SECRET_FILE_MAX_BYTES} bytes, too big for a secret"
        ));
    }
    let text = String::from_utf8(bytes).map_err(|_| "it is not UTF-8 text".to_owned())?;
    let value = text.trim();
    if value.is_empty() {
        return Err("it is empty".to_owned());
    }
    Ok(value.to_owned())
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
    #[allow(
        clippy::too_many_lines,
        reason = "one stanza per environment variable; the function's length is the count \
                  of settings, not complexity"
    )]
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
        // The TLS rule (D-156), at start-up: a database elsewhere must prove who it is.
        if let Some(url) = &database_url
            && let Err(error) = colophon_db::check_url(url.expose())
        {
            env.problems.push(format!("COLOPHON_DATABASE_URL: {error}"));
        }
        let cache_url = env.secret("COLOPHON_CACHE_URL", false);
        let (cache_ttl, cache_jitter) = env.cache_lifetimes();
        let log_format = env.parse("COLOPHON_LOG_FORMAT", LogFormat::Json);
        let log_filter = env.raw("COLOPHON_LOG").unwrap_or_else(|| "info".to_owned());
        let request_timeout = env.millis("COLOPHON_REQUEST_TIMEOUT_MS", 10_000, 100..=120_000);
        let header_timeout = env.millis("COLOPHON_HEADER_TIMEOUT_MS", 10_000, 100..=60_000);
        let max_body_bytes = env.ranged("COLOPHON_MAX_BODY_BYTES", 21 * MIB, KIB..=64 * MIB);
        let upload_memory_mb = env.ranged("COLOPHON_UPLOAD_MEMORY_MB", 256, 1..=16 * 1024);
        let db_max_connections = env.ranged("COLOPHON_DB_MAX_CONNECTIONS", 16, 1..=1_000);
        let db_slow_statements = colophon_db::SlowStatements {
            info: env.millis("COLOPHON_DB_SLOW_STATEMENT_INFO_MS", 100, 1..=600_000),
            warn: env.millis("COLOPHON_DB_SLOW_STATEMENT_MS", 1_000, 1..=600_000),
        };
        if db_slow_statements.info > db_slow_statements.warn {
            env.problems.push(format!(
                "COLOPHON_DB_SLOW_STATEMENT_INFO_MS ({} ms) is above COLOPHON_DB_SLOW_STATEMENT_MS ({} ms): evidence starts below the alert",
                db_slow_statements.info.as_millis(),
                db_slow_statements.warn.as_millis()
            ));
        }
        let (probe_interval, probe_timeout) = env.probe();
        let (drain_delay, shutdown_grace) = env.drain();
        let (public_origin, extra_trusted_origins, oidc) = env.sign_in(listen_addr);
        let media_origin = match env.raw("COLOPHON_MEDIA_URL") {
            None => public_origin.clone(),
            Some(url) => parse_origin(&url).unwrap_or_else(|| {
                env.problems.push(format!(
                    "COLOPHON_MEDIA_URL: {url:?} is not an http(s) origin like https://media.example.com"
                ));
                String::new()
            }),
        };
        let s3 = env.s3();
        let studio_dir = env.raw("COLOPHON_STUDIO_DIR").map(std::path::PathBuf::from);
        let worker_poll = env.millis("COLOPHON_WORKER_POLL_MS", 500, 10..=60_000);
        let worker_lease = env.millis("COLOPHON_WORKER_LEASE_MS", 30_000, 1_000..=600_000);
        let worker_slots = env.ranged("COLOPHON_WORKER_SLOTS", 8, 1..=64);
        let worker_attempt = env.millis("COLOPHON_WORKER_ATTEMPT_MS", 10_000, 100..=60_000);
        let worker_db_connections = env.ranged("COLOPHON_WORKER_DB_CONNECTIONS", 4, 1..=100);
        let admission = env.admission();
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
        // A year of headroom over the spec's 24 h; the sweep itself can run as often as the
        // tests need and as rarely as an hourly pass.
        let asset_expiry = env.millis("COLOPHON_ASSET_EXPIRY_MS", 86_400_000, 1..=31_536_000_000);
        let asset_sweep = env.millis("COLOPHON_ASSET_SWEEP_MS", 300_000, 50..=3_600_000);
        // Hourly by default: retention is a bound, not a latency promise, so a pass an hour
        // keeps every table's size where the ages say without spending worker time on it.
        let table_sweep = env.millis("COLOPHON_TABLE_SWEEP_MS", 3_600_000, 1_000..=86_400_000);
        let events = env.events();
        let mcp = env.mcp();
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
        let dev_mail = env.flag("COLOPHON_DEV_MAIL");
        // Development mail answers a sign-in link in the response itself: wherever others can
        // reach it, it is an account-takeover switch, so like the dev client's secret it needs
        // a loopback listener (D-140) and a localhost public origin — a production server
        // behind a reverse proxy listens on loopback too (D-163).
        if dev_mail && !(listen_addr.ip().is_loopback() && is_loopback_origin(&public_origin)) {
            env.problems.push(
                "COLOPHON_DEV_MAIL hands sign-in links back in responses: it may only run on a \
                 loopback COLOPHON_LISTEN_ADDR with a localhost COLOPHON_PUBLIC_URL"
                    .to_owned(),
            );
        }
        let allow_private_destinations = env.flag("COLOPHON_ALLOW_PRIVATE_DESTINATIONS");
        let magic_links = colophon_db::audience::MagicLinkLimits {
            per_address: env.ranged("COLOPHON_MAGIC_LINKS_PER_ADDRESS", 5, 1..=100_000),
            per_project: env.ranged("COLOPHON_MAGIC_LINKS_PER_PROJECT", 300, 1..=1_000_000),
            ..colophon_db::audience::MagicLinkLimits::DEFAULT
        };
        let password_concurrency = env.ranged(
            "COLOPHON_PASSWORD_CONCURRENCY",
            colophon_db::passwords::PasswordGate::default_slots(),
            1..=256,
        );
        let mail = env.mail();
        let (selfserve, turnstile_secret) = env.selfserve(&public_origin);
        let turnstile_verify_url = env.raw("COLOPHON_TURNSTILE_VERIFY_URL");
        if let Some(raw) = turnstile_verify_url.as_deref()
            && !url::Url::parse(raw)
                .is_ok_and(|url| colophon_auth::turnstile::allowed_verify_url(&url))
        {
            env.problems.push(format!(
                "COLOPHON_TURNSTILE_VERIFY_URL: {raw:?} must be Cloudflare's siteverify or a \
                 loopback address (tests only)"
            ));
        }
        let auth_event_key = env.secret("COLOPHON_AUTH_EVENT_KEY", false);
        let install_kind = match env.raw("COLOPHON_INSTALL_KIND").as_deref() {
            None => None,
            Some("container") => Some("container"),
            Some(other) => {
                env.problems.push(format!(
                    "COLOPHON_INSTALL_KIND: {other:?} is not one this build knows ('container')"
                ));
                None
            }
        };
        if let Some(database_url) = database_url
            && env.problems.is_empty()
        {
            Ok(Self {
                role,
                listen_addr,
                database_url,
                cache_url,
                cache_ttl,
                cache_jitter,
                log_format,
                log_filter,
                request_timeout,
                header_timeout,
                max_body_bytes,
                upload_memory_mb,
                db_max_connections,
                db_slow_statements,
                probe_interval,
                probe_timeout,
                drain_delay,
                shutdown_grace,
                public_origin,
                media_origin,
                oidc,
                s3,
                extra_trusted_origins,
                studio_dir,
                worker_poll,
                worker_lease,
                worker_slots,
                worker_attempt,
                worker_db_connections,
                worker_location,
                asset_expiry,
                asset_sweep,
                table_sweep,
                admission,
                events,
                mcp,
                password_concurrency,
                magic_links,
                allow_private_destinations,
                dev_mail,
                install_kind,
                mail,
                selfserve,
                turnstile_secret,
                turnstile_verify_url,
                auth_event_key,
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
        assert_eq!(config.header_timeout, Duration::from_secs(10));
        assert_eq!(config.max_body_bytes, 21 * MIB);
        assert_eq!(config.probe_interval, Duration::from_secs(2));
        assert!(config.cache_url.is_none());
        assert_eq!(config.cache_ttl, Duration::from_secs(300));
        assert_eq!(config.cache_jitter, Duration::from_secs(60));
        assert_eq!(config.log_format, LogFormat::Json);
        assert_eq!(config.worker_location, "local");
    }

    #[test]
    fn slow_statements_log_in_two_tiers() {
        let config = load(&[("COLOPHON_DATABASE_URL", DB)]).expect("valid");
        assert_eq!(
            config.db_slow_statements,
            colophon_db::SlowStatements {
                info: Duration::from_millis(100),
                warn: Duration::from_secs(1)
            }
        );
        let config = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_DB_SLOW_STATEMENT_INFO_MS", "50"),
            ("COLOPHON_DB_SLOW_STATEMENT_MS", "500"),
        ])
        .expect("both moved");
        assert_eq!(config.db_slow_statements.info, Duration::from_millis(50));
        assert_eq!(config.db_slow_statements.warn, Duration::from_millis(500));
        let error = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_DB_SLOW_STATEMENT_INFO_MS", "2000"),
        ])
        .expect_err("evidence above the alert");
        assert!(
            error
                .to_string()
                .contains("COLOPHON_DB_SLOW_STATEMENT_INFO_MS"),
            "{error}"
        );
    }

    #[test]
    fn cache_lifetimes_parse_and_floor() {
        let config = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_CACHE_TTL_MS", "4000"),
            ("COLOPHON_CACHE_JITTER_MS", "0"),
        ])
        .expect("a stampede run's lifetimes");
        assert_eq!(config.cache_ttl, Duration::from_secs(4));
        assert_eq!(config.cache_jitter, Duration::ZERO);
        for (key, value) in [
            ("COLOPHON_CACHE_TTL_MS", "500"),
            ("COLOPHON_CACHE_TTL_MS", "x"),
        ] {
            let error = load(&[("COLOPHON_DATABASE_URL", DB), (key, value)])
                .expect_err("below the floor or not a number");
            assert!(
                error.to_string().contains(key),
                "{key} missing from: {error}"
            );
        }
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
        assert_eq!(
            error.problems,
            [
                "COLOPHON_DATABASE_URL is required (or COLOPHON_DATABASE_URL_FILE, a file holding it)"
            ]
        );
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
    fn the_upload_store_is_all_or_none_and_https_only_off_loopback() {
        // Unset: no store, no problem.
        let config = load(&[("COLOPHON_DATABASE_URL", DB)]).expect("valid");
        assert!(config.s3.is_none());
        // One without the rest: a problem, still no store.
        let error = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_S3_BUCKET", "colophon-dev"),
        ])
        .expect_err("a lone bucket");
        assert!(
            error
                .to_string()
                .contains("go together: set all four or none")
        );
        // Plain HTTP off loopback is refused; on loopback it is the local stack's Garage.
        let base = [
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_S3_BUCKET", "colophon-dev"),
            ("COLOPHON_S3_ACCESS_KEY", "GK-test"),
            ("COLOPHON_S3_SECRET_KEY", "s3cret"),
        ];
        let mut public = Vec::from(base);
        public.push(("COLOPHON_S3_ENDPOINT", "http://s3.example.test"));
        let error = load(&public).expect_err("plain http off loopback");
        assert!(error.to_string().contains("loopback"));
        let mut local = Vec::from(base);
        local.push(("COLOPHON_S3_ENDPOINT", "http://127.0.0.1:53900"));
        let config = load(&local).expect("loopback http");
        assert!(config.s3.as_ref().is_some_and(|s3| s3.allow_http));
        assert!(!format!("{config:?}").contains("s3cret"));
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
    fn self_serve_defaults_to_invite_with_the_approved_bounds() {
        let config = load(&[("COLOPHON_DATABASE_URL", DB)]).expect("defaults load");
        let selfserve = &config.selfserve;
        assert_eq!(
            selfserve.registration,
            colophon_api::selfserve::Registration::Invite
        );
        assert_eq!(selfserve.projects_per_account, 3);
        assert_eq!(selfserve.quotas, colophon_api::selfserve::Quotas::DEFAULT);
        assert_eq!(selfserve.signups_per_address_hour, 10);
        assert_eq!(selfserve.hint_domain, None);
        assert_eq!(selfserve.turnstile_site_key, None);
        assert!(config.turnstile_secret.is_none() && config.auth_event_key.is_none());
    }

    #[test]
    fn self_serve_settings_are_read_and_bad_ones_refused() {
        let config = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_PUBLIC_URL", "https://colophonapp.404human.dev"),
            ("COLOPHON_LISTEN_ADDR", "127.0.0.1:58000"),
            ("COLOPHON_REGISTRATION", "open"),
            ("COLOPHON_PROJECTS_PER_ACCOUNT", "5"),
            ("COLOPHON_ACCOUNT_MAX_ENTRIES", "0"),
            ("COLOPHON_ACCOUNT_MAX_STORAGE_BYTES", "2048"),
            ("COLOPHON_SIGNUPS_PER_ADDRESS_HOUR", "20"),
            ("COLOPHON_SIGNED_IN_HINT_DOMAIN", "404human.dev"),
            ("COLOPHON_TURNSTILE_SITE_KEY", "1x00000000000000000000AA"),
            (
                "COLOPHON_TURNSTILE_SECRET",
                "1x0000000000000000000000000000000AA",
            ),
        ])
        .expect("loads");
        let selfserve = &config.selfserve;
        assert_eq!(
            selfserve.registration,
            colophon_api::selfserve::Registration::Open
        );
        assert_eq!(selfserve.projects_per_account, 5);
        assert_eq!(selfserve.quotas.entries, 0, "0 is no bound");
        assert_eq!(selfserve.quotas.storage_bytes, 2048);
        assert_eq!(selfserve.signups_per_address_hour, 20);
        assert_eq!(selfserve.hint_domain.as_deref(), Some("404human.dev"));
        assert!(config.turnstile_secret.is_some());

        for (pairs, wanted) in [
            (
                vec![("COLOPHON_REGISTRATION", "public")],
                "COLOPHON_REGISTRATION",
            ),
            (
                vec![("COLOPHON_PROJECTS_PER_ACCOUNT", "0")],
                "COLOPHON_PROJECTS_PER_ACCOUNT",
            ),
            (
                vec![("COLOPHON_TURNSTILE_SITE_KEY", "1x00000000000000000000AA")],
                "COLOPHON_TURNSTILE_SITE_KEY",
            ),
            (
                vec![(
                    "COLOPHON_TURNSTILE_SECRET",
                    "1x0000000000000000000000000000000AA",
                )],
                "COLOPHON_TURNSTILE_SITE_KEY",
            ),
            (
                vec![("COLOPHON_SIGNED_IN_HINT_DOMAIN", "example.com")],
                "COLOPHON_SIGNED_IN_HINT_DOMAIN",
            ),
            (
                vec![("COLOPHON_SIGNED_IN_HINT_DOMAIN", "dev")],
                "COLOPHON_SIGNED_IN_HINT_DOMAIN",
            ),
        ] {
            let mut env = vec![
                ("COLOPHON_DATABASE_URL", DB),
                ("COLOPHON_PUBLIC_URL", "https://colophonapp.404human.dev"),
                ("COLOPHON_LISTEN_ADDR", "127.0.0.1:58000"),
            ];
            env.extend(pairs.iter().copied());
            let error = load(&env).expect_err(&format!("{pairs:?} refused"));
            assert!(error.to_string().contains(wanted), "{pairs:?}: {error}");
        }
    }

    #[test]
    fn development_mail_refuses_to_run_off_loopback() {
        let base = [("COLOPHON_DATABASE_URL", DB), ("COLOPHON_DEV_MAIL", "1")];
        let config = load(&base).expect("fine on loopback");
        assert!(config.dev_mail);
        let mut public = base.to_vec();
        public.push(("COLOPHON_LISTEN_ADDR", "0.0.0.0:58000"));
        let error = load(&public).expect_err("refused on 0.0.0.0");
        assert!(error.to_string().contains("COLOPHON_DEV_MAIL"), "{error}");
    }

    #[test]
    fn development_affordances_refuse_a_public_origin_even_on_loopback() {
        // Behind a reverse proxy a production server listens on loopback too (D-163): what
        // says "development" is a public origin nobody else can use, not the listen address.
        let behind_a_proxy = [
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_LISTEN_ADDR", "127.0.0.1:58000"),
            ("COLOPHON_PUBLIC_URL", "https://cms.example.com"),
        ];
        let mut mail = behind_a_proxy.to_vec();
        mail.push(("COLOPHON_DEV_MAIL", "1"));
        let error = load(&mail).expect_err("development mail behind a public origin");
        assert!(error.to_string().contains("COLOPHON_DEV_MAIL"), "{error}");
        let mut client = behind_a_proxy.to_vec();
        client.extend([
            (
                "COLOPHON_OIDC_ISSUER",
                "https://id.example.com/realms/colophon",
            ),
            ("COLOPHON_OIDC_CLIENT_ID", "colophon-server"),
            ("COLOPHON_OIDC_CLIENT_SECRET", "colophon-dev-client-secret"),
        ]);
        let error = load(&client).expect_err("the public client secret behind a public origin");
        assert!(
            error.to_string().contains("development sign-in client"),
            "{error}"
        );
        // A loopback origin — localhost, *.localhost, 127.0.0.0/8 or [::1] — is development.
        for origin in [
            "http://localhost:58000",
            "http://colophon.localhost:58000",
            "http://127.0.0.2:58000",
            "http://[::1]:58000",
        ] {
            let mut local = mail.clone();
            local.retain(|(name, _)| *name != "COLOPHON_PUBLIC_URL");
            local.push(("COLOPHON_PUBLIC_URL", origin));
            assert!(load(&local).is_ok_and(|config| config.dev_mail), "{origin}");
        }
    }

    #[tokio::test]
    async fn a_tls_database_url_is_taken_as_given() {
        // Neon's URLs, with `sslmode=verify-full` (D-156): the configuration keeps the URL
        // whole — the driver, built with rustls and the Mozilla roots, does the TLS and checks
        // the certificate names the host — and never prints it.
        let url = "postgres://app:s3cret@ep-quiet-1.eu-central-1.aws.neon.tech/colophon?sslmode=verify-full";
        let config = load(&[("COLOPHON_DATABASE_URL", url)]).expect("valid");
        assert_eq!(config.database_url.expose(), url);
        assert!(!format!("{config:?}").contains("s3cret"));
        assert!(
            colophon_db::connect_lazy(url, colophon_db::PoolSettings::default()).is_ok(),
            "the driver accepts the verify-full URL"
        );
    }

    #[test]
    fn an_unverified_remote_database_url_stops_the_server_at_start() {
        // `require` takes any certificate, `verify-ca` any publicly trusted one, and the driver
        // ignores `channel_binding` (D-156): the server refuses to start on any of them — Neon's
        // copied URL carries the first and the last — naming the variable and the fix, never
        // the URL.
        let neon = "postgres://app:s3cret@ep-quiet-1.eu-central-1.aws.neon.tech/colophon";
        for query in [
            "?sslmode=require",
            "?sslmode=require&channel_binding=require",
            "?sslmode=verify-ca&sslrootcert=/etc/colophon/db-ca.pem",
            "?sslmode=verify-full&channel_binding=require",
        ] {
            let url = format!("{neon}{query}");
            let error = load(&[("COLOPHON_DATABASE_URL", url.as_str())]).expect_err("refused");
            let text = error.to_string();
            assert!(text.contains("COLOPHON_DATABASE_URL"), "{query}: {text}");
            assert!(text.contains("database URL refused"), "{query}: {text}");
            assert!(
                text.contains("sslmode=verify-full") && !text.contains("s3cret"),
                "{query}: {text}"
            );
        }
    }

    #[test]
    fn private_destinations_are_an_explicit_opt_in_never_the_listen_address() {
        // Loopback is the default listen address, and still no private destinations: a
        // server behind a local reverse proxy binds loopback in production too.
        let config = load(&[("COLOPHON_DATABASE_URL", DB)]).expect("valid");
        assert!(config.listen_addr.ip().is_loopback());
        assert!(!config.allow_private_destinations);
        for (value, expected) in [("1", true), ("true", true), ("0", false), ("false", false)] {
            let config = load(&[
                ("COLOPHON_DATABASE_URL", DB),
                ("COLOPHON_ALLOW_PRIVATE_DESTINATIONS", value),
            ])
            .expect("valid");
            assert_eq!(config.allow_private_destinations, expected, "{value}");
        }
        let error = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_ALLOW_PRIVATE_DESTINATIONS", "yes"),
        ])
        .expect_err("not a flag");
        assert!(
            error
                .to_string()
                .contains("COLOPHON_ALLOW_PRIVATE_DESTINATIONS"),
            "{error}"
        );
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
            config.media_origin, "https://cms.example.com",
            "media URLs default to the public origin"
        );
        assert_eq!(
            config.extra_trusted_origins,
            ["http://localhost:58101", "https://preview.example.com"]
        );
        let media = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_MEDIA_URL", "https://media.example.com/"),
        ])
        .expect("valid");
        assert_eq!(media.media_origin, "https://media.example.com");
        assert!(
            load(&[
                ("COLOPHON_DATABASE_URL", DB),
                ("COLOPHON_MEDIA_URL", "ftp://media.example.com")
            ])
            .is_err(),
            "a media origin is http(s)"
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

    #[test]
    fn mail_needs_the_whole_resend_setup_or_nothing() {
        // Nothing set: no mail, like before.
        let config = load(&[("COLOPHON_DATABASE_URL", DB)]).expect("valid");
        assert!(config.mail.is_none());

        // The complete setup, endpoint defaulted.
        let config = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_MAIL_PROVIDER", "resend"),
            ("COLOPHON_MAIL_FROM", "The Draft <hello@thedraft.example>"),
            ("COLOPHON_RESEND_API_KEY", "re_live_secret"),
        ])
        .expect("a full resend setup");
        let mail = config.mail.as_ref().expect("mail configured");
        assert_eq!(mail.provider, MailProvider::Resend);
        assert_eq!(mail.from, "The Draft <hello@thedraft.example>");
        assert_eq!(mail.endpoint, "https://api.resend.com/emails");
        // The key is a secret: it never shows in a Debug dump.
        assert!(!format!("{mail:?}").contains("re_live_secret"));
        assert!(!format!("{config:?}").contains("re_live_secret"));

        // The endpoint override moves the provider's address.
        let config = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_MAIL_PROVIDER", "resend"),
            ("COLOPHON_MAIL_FROM", "hello@thedraft.example"),
            ("COLOPHON_RESEND_API_KEY", "re_test"),
            ("COLOPHON_RESEND_ENDPOINT", "http://127.0.0.1:9999/hook"),
        ])
        .expect("an endpoint override");
        assert_eq!(
            config.mail.expect("mail").endpoint,
            "http://127.0.0.1:9999/hook"
        );

        // Every partial combination refuses, and so does a provider this build
        // doesn't know.
        for extra in [
            vec![("COLOPHON_MAIL_PROVIDER", "resend")],
            vec![
                ("COLOPHON_MAIL_PROVIDER", "resend"),
                ("COLOPHON_MAIL_FROM", "hello@thedraft.example"),
            ],
            vec![
                ("COLOPHON_MAIL_FROM", "hello@thedraft.example"),
                ("COLOPHON_RESEND_API_KEY", "re_test"),
            ],
            vec![("COLOPHON_RESEND_ENDPOINT", "http://127.0.0.1:9/hook")],
            vec![("COLOPHON_MAIL_PROVIDER", "smtp")],
        ] {
            let mut pairs = vec![("COLOPHON_DATABASE_URL", DB)];
            pairs.extend(extra.iter().copied());
            let error = load(&pairs).expect_err(&format!("{extra:?}"));
            assert!(
                error.to_string().contains("COLOPHON_MAIL_")
                    || error.to_string().contains("COLOPHON_RESEND_"),
                "{extra:?}: {error}"
            );
        }
    }

    /// A scratch directory of this test's own, removed when it drops.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "colophon-config-{name}-{}-{}",
                std::process::id(),
                uuid::Uuid::now_v7().simple()
            ));
            std::fs::create_dir_all(&dir).expect("a scratch directory");
            Self(dir)
        }

        fn file(&self, name: &str, contents: &[u8]) -> String {
            let path = self.0.join(name);
            std::fs::write(&path, contents).expect("a scratch file");
            path.to_string_lossy().into_owned()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn secrets_come_from_files_trimmed_and_never_quoted() {
        let scratch = Scratch::new("files");
        let database = scratch.file("database_url", format!("{DB}\n").as_bytes());
        let cache = scratch.file("cache_url", b"  redis://:hunter3@127.0.0.1:6379\r\n");
        let s3 = scratch.file("s3", b"s3cret-from-file\n");
        let config = load(&[
            ("COLOPHON_DATABASE_URL_FILE", &database),
            ("COLOPHON_CACHE_URL_FILE", &cache),
            ("COLOPHON_S3_ENDPOINT", "http://127.0.0.1:3900"),
            ("COLOPHON_S3_BUCKET", "colophon"),
            ("COLOPHON_S3_ACCESS_KEY", "GK-test"),
            ("COLOPHON_S3_SECRET_KEY_FILE", &s3),
        ])
        .expect("every secret from a file");
        assert_eq!(config.database_url.expose(), DB, "the newline is trimmed");
        assert_eq!(
            config.cache_url.as_ref().map(Secret::expose),
            Some("redis://:hunter3@127.0.0.1:6379")
        );
        assert_eq!(
            config.s3.as_ref().map(|s3| s3.secret_key.expose()),
            Some("s3cret-from-file")
        );
        let debug = format!("{config:?}");
        for secret in ["hunter2", "hunter3", "s3cret-from-file"] {
            assert!(!debug.contains(secret), "{secret} leaked into {debug}");
        }
    }

    #[test]
    fn a_secret_and_its_file_together_is_a_problem_not_a_precedence_rule() {
        let scratch = Scratch::new("both");
        let database = scratch.file("database_url", DB.as_bytes());
        let error = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_DATABASE_URL_FILE", &database),
        ])
        .expect_err("both set");
        assert_eq!(
            error.problems,
            ["COLOPHON_DATABASE_URL and COLOPHON_DATABASE_URL_FILE are both set: use one"]
        );
        assert!(!error.to_string().contains("hunter2"));
    }

    #[test]
    fn a_secret_file_that_is_missing_empty_huge_or_binary_is_refused_without_its_contents() {
        let scratch = Scratch::new("bad");
        let missing = scratch.0.join("absent").to_string_lossy().into_owned();
        let empty = scratch.file("empty", b" \n\n");
        let huge = scratch.file("huge", &vec![b'h'; 16 * 1024 + 1]);
        let binary = scratch.file("binary", &[0xff, 0xfe, b'h', b'u', b'n', b't', b'e', b'r']);
        for (path, says) in [
            (&missing, "cannot open it"),
            (&empty, "it is empty"),
            (&huge, "too big for a secret"),
            (&binary, "not UTF-8"),
        ] {
            let error = load(&[("COLOPHON_DATABASE_URL_FILE", path)]).expect_err(says);
            let text = error.to_string();
            assert!(text.contains("COLOPHON_DATABASE_URL_FILE"), "{text}");
            assert!(text.contains(says), "{says} missing from: {text}");
            assert!(!text.contains("hhhh") && !text.contains("unter"), "{text}");
        }
        // Exactly at the bound is still a secret.
        let at_bound = scratch.file("at-bound", &vec![b'h'; 16 * 1024]);
        let error = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_OIDC_ISSUER", "https://id.example.com"),
            ("COLOPHON_OIDC_CLIENT_ID", "colophon"),
            ("COLOPHON_OIDC_CLIENT_SECRET_FILE", &at_bound),
        ]);
        assert!(error.is_ok(), "{error:?}");
    }

    #[test]
    fn the_install_kind_is_container_or_nothing() {
        let config = load(&[("COLOPHON_DATABASE_URL", DB)]).expect("valid");
        assert_eq!(config.install_kind, None);
        let config = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_INSTALL_KIND", "container"),
        ])
        .expect("the image's value");
        assert_eq!(config.install_kind, Some("container"));
        let error = load(&[
            ("COLOPHON_DATABASE_URL", DB),
            ("COLOPHON_INSTALL_KIND", "snap"),
        ])
        .expect_err("unknown");
        assert!(
            error.to_string().contains("COLOPHON_INSTALL_KIND"),
            "{error}"
        );
    }
}
