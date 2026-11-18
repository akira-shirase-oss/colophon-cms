//! `colophon-server`: the composition root. It builds the adapters from the validated
//! configuration, starts the readiness probes and, for the worker role, the worker, serves HTTP,
//! and shuts down in order (`docs/ARCHITECTURE.md`, "Process topology"). All logic lives here
//! rather than in `main.rs`, so tests can drive it.

pub mod config;
pub mod events;
mod mail;
pub mod probes;
pub mod setup_code;
pub mod shutdown;
pub mod telemetry;

use std::{future::Future, sync::Arc, time::Duration};

use anyhow::Context as _;
use colophon_api::{Dependency, HttpLimits, Readiness};
use colophon_db::PoolSettings;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

pub use config::{Config, Role};
use probes::{CacheProbe, DatabaseProbe, Supervisor};
pub use shutdown::ShutdownOutcome;
use shutdown::ShutdownTiming;

/// How long stopping the probes and closing the pool may take after the server has drained.
const CLEANUP_DEADLINE: Duration = Duration::from_secs(5);

/// Starts the worker for the worker and all roles (D-046): it claims delivery jobs from the same
/// database, runs schedules as they fall due (D-060), and, with an object store, sweeps expired
/// assets and orphaned quarantine objects (D-099), until `cancel` fires. Private destinations
/// are reachable only when `COLOPHON_ALLOW_PRIVATE_DESTINATIONS` opts in (D-149).
fn start_worker(
    config: &Config,
    cancel: &CancellationToken,
) -> anyhow::Result<Option<tokio::task::JoinHandle<()>>> {
    if !matches!(config.role, Role::Worker | Role::All) {
        return Ok(None);
    }
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let assets = asset_store(config)?;
    let settings = colophon_worker::Settings {
        owner: format!("colophon-{}-{started}", std::process::id()),
        poll: config.worker_poll,
        lease: config.worker_lease,
        attempt: config.worker_attempt,
        slots: config.worker_slots,
        allow_private: config.allow_private_destinations,
        location: config.worker_location.clone(),
        assets: assets.clone(),
    };
    // Its own pool: deliveries can never take the connections editors' requests need.
    let worker_pool = colophon_db::connect_lazy(
        config.database_url.expose(),
        PoolSettings {
            max_connections: config.worker_db_connections,
            ..PoolSettings::default()
        },
    )
    .context("configuring the worker's database pool")?;
    let worker = colophon_worker::Worker::new(worker_pool.clone(), settings)
        .context("starting the worker")?;
    // Schedules (D-060) run beside the deliveries, on the same pool and poll; the media
    // retention sweep (D-099) beside both, only where there is a store to keep; and the
    // table-retention sweep (D-135) on the same pool, always — the bounds it keeps live in
    // the database, not the object store.
    let scheduler =
        colophon_worker::schedules::Scheduler::new(worker_pool.clone(), config.worker_poll);
    let sweeper = assets.map(|store| {
        colophon_worker::retention::Sweeper::new(
            worker_pool.clone(),
            store,
            config.asset_sweep,
            config.asset_expiry,
        )
    });
    let tables = colophon_worker::tables::Sweeper::new(worker_pool.clone(), config.table_sweep);
    let cancel = cancel.clone();
    Ok(Some(tokio::spawn(async move {
        let sweep_cancel = cancel.clone();
        let sweep = async move {
            if let Some(sweeper) = sweeper {
                sweeper.run(sweep_cancel).await;
            }
        };
        tokio::join!(
            Arc::new(worker).run(cancel.clone()),
            scheduler.run(cancel.clone()),
            sweep,
            tables.run(cancel.clone())
        );
        worker_pool.close().await;
    })))
}

/// The tasks a serving process runs beside the listener, stopped once it has drained.
struct Background {
    worker_cancel: CancellationToken,
    worker: Option<tokio::task::JoinHandle<()>>,
    setup_cancel: CancellationToken,
    setup: Option<tokio::task::JoinHandle<()>>,
    events_cancel: CancellationToken,
    hints: Vec<tokio::task::JoinHandle<()>>,
}

/// Stops the worker, the setup code and the change hints, in that order.
async fn stop_background(tasks: Background) {
    stop_worker(&tasks.worker_cancel, tasks.worker).await;
    stop_setup_code(&tasks.setup_cancel, tasks.setup).await;
    stop_events(&tasks.events_cancel, tasks.hints).await;
}

/// Stops the worker claiming, and lets attempts in flight finish within the cleanup deadline;
/// any it abandons are claimed again once their leases run out.
async fn stop_worker(cancel: &CancellationToken, worker: Option<tokio::task::JoinHandle<()>>) {
    cancel.cancel();
    if let Some(task) = worker
        && tokio::time::timeout(CLEANUP_DEADLINE, task).await.is_err()
    {
        tracing::warn!("the worker did not stop in time; its leases will run out");
    }
}

/// Issues and prints a fresh installation's setup code (D-061), from the processes that serve
/// the Studio; a no-op task once the installation has an administrator.
fn start_setup_code(
    config: &Config,
    pool: &colophon_db::PgPool,
    cancel: &CancellationToken,
) -> Option<tokio::task::JoinHandle<()>> {
    matches!(config.role, Role::Api | Role::All).then(|| {
        tokio::spawn(setup_code::run(
            pool.clone(),
            format!("{}/studio/", config.public_origin),
            cancel.clone(),
        ))
    })
}

/// Stops issuing the setup code, if it is still trying.
async fn stop_setup_code(cancel: &CancellationToken, task: Option<tokio::task::JoinHandle<()>>) {
    cancel.cancel();
    if let Some(task) = task
        && tokio::time::timeout(CLEANUP_DEADLINE, task).await.is_err()
    {
        tracing::warn!("issuing the setup code did not stop in time");
    }
}

/// Starts the change hints' two tasks: the pump in every role (D-180) — writers queue their
/// hints in `change_hints` and some running Colophon process must send them, whichever pump
/// gets there first — and, for the api and all roles, the listener (D-108): one `LISTEN`
/// connection feeding the bus the `{P}/events` streams subscribe to. The worker role alone has
/// no streams to serve and gets no listener.
fn start_events(
    config: &Config,
    pool: &colophon_db::PgPool,
    bus: &Arc<colophon_api::events::EventBus>,
    cancel: &CancellationToken,
) -> Vec<tokio::task::JoinHandle<()>> {
    let mut tasks = vec![tokio::spawn(events::pump(pool.clone(), cancel.clone()))];
    if matches!(config.role, Role::Api | Role::All) {
        tasks.push(tokio::spawn(events::run(
            bus.clone(),
            config.database_url.expose().to_owned(),
            cancel.clone(),
        )));
    }
    tasks
}

/// Stops the pump and the listener; open streams end when the process does anyway, so there is
/// no drain, and hints still queued are sent by the next process's pump.
async fn stop_events(cancel: &CancellationToken, tasks: Vec<tokio::task::JoinHandle<()>>) {
    cancel.cancel();
    for task in tasks {
        if tokio::time::timeout(CLEANUP_DEADLINE, task).await.is_err() {
            tracing::warn!("a change-hint task did not stop in time");
        }
    }
}

/// The public delivery cache (D-048): Valkey at `COLOPHON_CACHE_URL`, or none. The entry
/// lifetimes come from `COLOPHON_CACHE_TTL_MS`/`COLOPHON_CACHE_JITTER_MS` — the production
/// defaults unless a load run shortens them to watch expiries (L04, D-128).
fn delivery_cache(config: &Config) -> anyhow::Result<colophon_api::DeliveryCache> {
    match &config.cache_url {
        Some(url) => colophon_api::DeliveryCache::new(url.expose())
            .map(|cache| cache.with_lifetime(config.cache_ttl, config.cache_jitter))
            .map_err(|error| anyhow::anyhow!(error))
            .context("configuring the delivery cache"),
        None => Ok(colophon_api::DeliveryCache::disabled()),
    }
}

/// The API's `Services`, from the validated configuration.
fn api_services(
    config: &Config,
    pool: &colophon_db::PgPool,
    events: Arc<colophon_api::events::EventBus>,
) -> anyhow::Result<colophon_api::Services> {
    let mut trusted_origins = vec![config.public_origin.clone()];
    trusted_origins.extend(config.extra_trusted_origins.iter().cloned());
    Ok(colophon_api::Services {
        pool: pool.clone(),
        oidc: oidc_client(config)?,
        sessions: colophon_api::SessionSettings::default(),
        trusted_origins,
        allow_private_destinations: config.allow_private_destinations,
        dev_mail: config.dev_mail,
        mail: config.mail.as_ref().map(|settings| {
            Arc::new(mail::Resend::new(
                settings.api_key.clone(),
                settings.from.clone(),
                settings.endpoint.clone(),
            )) as Arc<dyn colophon_api::mail::Mail>
        }),
        cache: delivery_cache(config)?,
        content_types: colophon_api::ContentTypeCache::new(),
        assets: asset_store(config)?,
        events,
        mcp: colophon_api::mcp::Shared::new(config.mcp),
        passwords: colophon_db::passwords::PasswordGate::new(config.password_concurrency),
        magic_links: config.magic_links,
        media_origin: config.media_origin.clone(),
        install_kind: config.install_kind,
    })
}

/// The uploads' object store (07-media): the S3 endpoint in `COLOPHON_S3_*`, or `None` — uploads
/// then answer "not configured" and the worker has no asset jobs to run.
fn asset_store(config: &Config) -> anyhow::Result<Option<colophon_media::Shared>> {
    let Some(settings) = &config.s3 else {
        return Ok(None);
    };
    let store = object_store::aws::AmazonS3Builder::new()
        .with_endpoint(&settings.endpoint)
        .with_region(&settings.region)
        .with_bucket_name(&settings.bucket)
        .with_access_key_id(&settings.access_key)
        .with_secret_access_key(settings.secret_key.expose())
        .with_allow_http(settings.allow_http)
        .build()
        .context("configuring the object store")?;
    Ok(Some(std::sync::Arc::new(store)))
}

/// The OpenID Connect client, when sign-in is configured.
fn oidc_client(
    config: &Config,
) -> anyhow::Result<Option<std::sync::Arc<colophon_auth::OidcClient>>> {
    let Some(settings) = &config.oidc else {
        return Ok(None);
    };
    colophon_auth::OidcClient::new(colophon_auth::OidcConfig {
        issuer: settings.issuer.clone(),
        client_id: settings.client_id.clone(),
        client_secret: colophon_auth::ClientSecret::new(settings.client_secret.expose()),
        redirect_uri: format!("{}/auth/callback", config.public_origin),
    })
    .map(Some)
    .map_err(|error| anyhow::anyhow!(error))
    .context("configuring sign-in")
}

/// Runs the server until `signal` resolves, then shuts down.
pub async fn run(
    config: Config,
    signal: impl Future<Output = &'static str> + Send + 'static,
) -> anyhow::Result<ShutdownOutcome> {
    let readiness = Readiness::new(config.role.as_str(), config.cache_url.is_some());

    // Bind before anything connects out: an outgoing connection opened first (the probes', the
    // pool's) can be given this very port as its local one, and the bind then fails with
    // "address in use" (seen once in CI). Once bound, the port isn't handed out that way.
    let listener = TcpListener::bind(config.listen_addr)
        .await
        .with_context(|| format!("binding {}", config.listen_addr))?;

    let pool = colophon_db::connect_lazy(
        config.database_url.expose(),
        PoolSettings {
            max_connections: config.db_max_connections,
            ..PoolSettings::default()
        },
    )
    .context("configuring the database pool")?;

    let cancel = CancellationToken::new();
    let mut supervisor = Supervisor::new(readiness.clone(), cancel.child_token());
    let database_probe = DatabaseProbe::new(config.database_url.expose(), config.probe_timeout)
        .map_err(|error| anyhow::anyhow!(error))
        .context("configuring the database probe")?;
    supervisor.spawn(Dependency::Database, database_probe, config.probe_interval);
    if let Some(url) = &config.cache_url {
        let probe = CacheProbe::new(url.expose(), config.probe_timeout)
            .map_err(|error| anyhow::anyhow!(error))
            .context("configuring the cache probe")?;
        supervisor.spawn(Dependency::Cache, probe, config.probe_interval);
    }

    // Said once at start, at INFO: the load runs treat any WARN as a defect, and the setting is
    // the development stack's own (deploy/local.env).
    if config.allow_private_destinations {
        tracing::info!(
            "COLOPHON_ALLOW_PRIVATE_DESTINATIONS is on: destinations may reach private, \
             loopback and link-local addresses — a development setting, never for production"
        );
    }
    let local_addr = listener.local_addr().context("reading the bound address")?;
    let build = colophon_version::build_info!();
    tracing::info!(
        %local_addr,
        role = config.role.as_str(),
        version = build.version,
        commit = build.commit,
        "listening"
    );

    let event_bus = colophon_api::events::EventBus::new(config.events);
    let services = api_services(&config, &pool, event_bus.clone())?;
    let admission = colophon_api::admission::Admission::new(
        config.admission.clone(),
        config.cache_url.as_ref().map(config::Secret::expose),
    )
    .map_err(|error| anyhow::anyhow!(error))
    .context("configuring rate limits")?;
    let state = colophon_api::AppState {
        readiness: readiness.clone(),
        services: Some(std::sync::Arc::new(services)),
        build,
        admission: Some(std::sync::Arc::new(admission)),
    };
    let mut routes = colophon_api::routes(state);
    if let Some(dir) = &config.studio_dir {
        colophon_api::studio::check(dir).context("COLOPHON_STUDIO_DIR")?;
        routes = colophon_api::studio::mount(routes, dir);
    }
    // The Studio gets the same limits, request ids and error envelope as the API.
    let app = colophon_api::middleware::apply(
        routes,
        HttpLimits {
            request_timeout: config.request_timeout,
            max_body_bytes: config.max_body_bytes,
        },
        build,
    );
    let timing = ShutdownTiming {
        drain_delay: config.drain_delay,
        grace: config.shutdown_grace,
    };
    let trigger = async move {
        let name = signal.await;
        tracing::info!(signal = name, "shutdown signal received");
    };

    let worker_cancel = cancel.child_token();
    let worker = start_worker(&config, &worker_cancel)?;
    let setup_cancel = cancel.child_token();
    let setup = start_setup_code(&config, &pool, &setup_cancel);
    let events_cancel = cancel.child_token();
    let hints = start_events(&config, &pool, &event_bus, &events_cancel);

    let outcome = tokio::select! {
        outcome = shutdown::serve(listener, app, readiness, timing, trigger) => outcome.context("serving")?,
        never = supervisor.watch() => match never {},
    };

    stop_background(Background {
        worker_cancel,
        worker,
        setup_cancel,
        setup,
        events_cancel,
        hints,
    })
    .await;
    supervisor.shutdown(CLEANUP_DEADLINE).await;
    if tokio::time::timeout(CLEANUP_DEADLINE, pool.close())
        .await
        .is_err()
    {
        tracing::warn!("database pool did not close in time");
    }
    cancel.cancel();
    Ok(outcome)
}

#[cfg(test)]
mod stop_tests {
    use std::time::Duration;

    use tokio_util::sync::CancellationToken;

    use super::{Background, stop_background};

    /// `TimeoutStopSec` in the production unit: systemd kills the process this long after SIGTERM.
    fn systemd_stop_timeout() -> Duration {
        let unit = include_str!("../../../deploy/production/colophon.service");
        let seconds = unit
            .lines()
            .find_map(|line| line.strip_prefix("TimeoutStopSec="))
            .and_then(|value| value.trim().parse().ok())
            .expect("invariant: the unit sets TimeoutStopSec in whole seconds");
        Duration::from_secs(seconds)
    }

    /// Tasks that never stop, however they're asked: the worst case a stop has to bound.
    fn stuck() -> tokio::task::JoinHandle<()> {
        tokio::spawn(std::future::pending())
    }

    /// The default grace (25 s) plus everything after it ends inside systemd's stop timeout, with
    /// room left for the probes and the pool: background tasks that never stop are abandoned in
    /// time, so systemd never has to kill a process mid-cleanup.
    #[tokio::test(start_paused = true)]
    async fn stuck_background_tasks_are_abandoned_before_systemd_kills() {
        let default_grace = Duration::from_millis(25_000);
        // The probes' and the pool's share, and a second's slack for the process to exit.
        let rest = Duration::from_secs(3);
        let allowed = systemd_stop_timeout()
            .checked_sub(default_grace + rest)
            .expect("invariant: the unit's timeout is longer than the default grace");
        let started = tokio::time::Instant::now();
        stop_background(Background {
            worker_cancel: CancellationToken::new(),
            worker: Some(stuck()),
            setup_cancel: CancellationToken::new(),
            setup: Some(stuck()),
            events_cancel: CancellationToken::new(),
            hints: vec![stuck(), stuck()],
        })
        .await;
        let took = started.elapsed();
        assert!(
            took <= allowed,
            "stopping stuck background tasks took {took:?}; after a 25 s grace only {allowed:?} is left before systemd's {:?}",
            systemd_stop_timeout()
        );
    }
}
