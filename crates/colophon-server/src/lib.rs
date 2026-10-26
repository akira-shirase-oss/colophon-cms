//! `colophon-server`: the composition root. It builds the adapters from the validated
//! configuration, starts the readiness probes, serves HTTP, and shuts down in order
//! (`docs/ARCHITECTURE.md`, "Process topology"). All logic lives here rather than in
//! `main.rs`, so tests can drive it.

pub mod config;
pub mod probes;
pub mod shutdown;
pub mod telemetry;

use std::{future::Future, time::Duration};

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

/// Runs the server until `signal` resolves, then shuts down.
pub async fn run(
    config: Config,
    signal: impl Future<Output = &'static str> + Send + 'static,
) -> anyhow::Result<ShutdownOutcome> {
    let readiness = Readiness::new(config.role.as_str(), config.cache_url.is_some());

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

    let listener = TcpListener::bind(config.listen_addr)
        .await
        .with_context(|| format!("binding {}", config.listen_addr))?;
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

    let outcome = tokio::select! {
        outcome = shutdown::serve(listener, app, readiness, timing, trigger) => outcome.context("serving")?,
        never = supervisor.watch() => match never {},
    };

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
