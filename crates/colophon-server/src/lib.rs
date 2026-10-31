//! `colophon-server`: the composition root. It builds the adapters from the validated
//! configuration, starts the readiness probes and, for the worker role, the worker, serves HTTP,
//! and shuts down in order (`docs/ARCHITECTURE.md`, "Process topology"). All logic lives here
//! rather than in `main.rs`, so tests can drive it.

pub mod config;
pub mod probes;
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
/// database until `cancel` fires. Private destinations are only reachable from a development
/// server bound to loopback.
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
    let settings = colophon_worker::Settings {
        owner: format!("colophon-{}-{started}", std::process::id()),
        poll: config.worker_poll,
        lease: config.worker_lease,
        attempt: config.worker_attempt,
        slots: config.worker_slots,
        allow_private: config.listen_addr.ip().is_loopback(),
        location: config.worker_location.clone(),
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
    let cancel = cancel.clone();
    Ok(Some(tokio::spawn(async move {
        Arc::new(worker).run(cancel).await;
        worker_pool.close().await;
    })))
}

/// How often the API checks for publishes whose time has come.
const SCHEDULE_POLL: std::time::Duration = std::time::Duration::from_secs(2);

/// How many due publishes one pass may run — a backlog drains in passes, never in one burst.
const SCHEDULE_BATCH: i64 = 10;

/// Runs due scheduled publishes on the API's pool: the commit replays the member's own
/// publish, so it needs the same role that serves the management API.
fn start_scheduler(
    config: &Config,
    pool: colophon_db::PgPool,
    cancel: &CancellationToken,
) -> Option<tokio::task::JoinHandle<()>> {
    if !matches!(config.role, Role::Api | Role::All) {
        return None;
    }
    let cancel = cancel.clone();
    Some(tokio::spawn(async move {
        loop {
            tokio::select! {
                () = cancel.cancelled() => break,
                () = tokio::time::sleep(SCHEDULE_POLL) => {}
            }
            if let Err(error) = colophon_api::scheduled_publish_tick(&pool, SCHEDULE_BATCH).await {
                tracing::warn!(%error, "scheduled publishes could not run this pass");
            }
        }
    }))
}

/// Stops the background tasks, and lets work in flight finish within the cleanup deadline;
/// leases a stopped worker abandons are claimed again once they run out.
async fn stop_tasks(
    cancel: &CancellationToken,
    worker: Option<tokio::task::JoinHandle<()>>,
    scheduler: Option<tokio::task::JoinHandle<()>>,
) {
    cancel.cancel();
    for task in [worker, scheduler].into_iter().flatten() {
        if tokio::time::timeout(CLEANUP_DEADLINE, task).await.is_err() {
            tracing::warn!("a background task did not stop in time");
        }
    }
}

/// The public delivery cache (D-048): Valkey at `COLOPHON_CACHE_URL`, or none.
fn delivery_cache(config: &Config) -> anyhow::Result<colophon_api::DeliveryCache> {
    match &config.cache_url {
        Some(url) => colophon_api::DeliveryCache::new(url.expose())
            .map_err(|error| anyhow::anyhow!(error))
            .context("configuring the delivery cache"),
        None => Ok(colophon_api::DeliveryCache::disabled()),
    }
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
    supervisor.spawn(
        Dependency::Database,
        DatabaseProbe::new(pool.clone(), config.probe_timeout),
        config.probe_interval,
    );
    if let Some(url) = &config.cache_url {
        let probe = CacheProbe::new(url.expose(), config.probe_timeout)
            .map_err(|error| anyhow::anyhow!(error))
            .context("configuring the cache probe")?;
        supervisor.spawn(Dependency::Cache, probe, config.probe_interval);
    }

    let local_addr = listener.local_addr().context("reading the bound address")?;
    tracing::info!(
        %local_addr,
        role = config.role.as_str(),
        version = env!("CARGO_PKG_VERSION"),
        "listening"
    );

    let oidc = match &config.oidc {
        Some(settings) => Some(
            colophon_auth::OidcClient::new(colophon_auth::OidcConfig {
                issuer: settings.issuer.clone(),
                client_id: settings.client_id.clone(),
                client_secret: colophon_auth::ClientSecret::new(settings.client_secret.expose()),
                redirect_uri: format!("{}/auth/callback", config.public_origin),
            })
            .map_err(|error| anyhow::anyhow!(error))
            .context("configuring sign-in")?,
        ),
        None => None,
    };
    let mut trusted_origins = vec![config.public_origin.clone()];
    trusted_origins.extend(config.extra_trusted_origins.iter().cloned());
    let services = colophon_api::Services {
        pool: pool.clone(),
        oidc,
        sessions: colophon_api::SessionSettings::default(),
        trusted_origins,
        allow_private_destinations: config.listen_addr.ip().is_loopback(),
        cache: delivery_cache(&config)?,
    };
    let state = colophon_api::AppState {
        readiness: readiness.clone(),
        services: Some(std::sync::Arc::new(services)),
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
    let scheduler = start_scheduler(&config, pool.clone(), &worker_cancel);

    let outcome = tokio::select! {
        outcome = shutdown::serve(listener, app, readiness, timing, trigger) => outcome.context("serving")?,
        never = supervisor.watch() => match never {},
    };

    stop_tasks(&worker_cancel, worker, scheduler).await;
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
