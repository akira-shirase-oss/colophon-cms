//! Readiness probes: one background task per dependency, each probing on a fixed interval
//! with a deadline and recording the result in [`Readiness`]. Only transitions are logged,
//! so an outage produces one line, not one line every two seconds.
//!
//! A probe task that dies would freeze readiness at its last value, possibly "up" forever.
//! [`Supervisor`] watches the tasks and marks a dependency down if its probe ever stops
//! without being asked to.

use std::{convert::Infallible, error::Error as StdError, future::Future, time::Duration};

use colophon_api::{CheckState, Dependency, Readiness};
use colophon_db::PgPool;
use redis::aio::MultiplexedConnection;
use tokio::{
    task::{Id, JoinSet},
    time::MissedTickBehavior,
};
use tokio_util::sync::CancellationToken;

/// Why a probe failed. Logged, never returned to clients.
pub type ProbeError = Box<dyn StdError + Send + Sync>;

/// One dependency check, bounded by its own deadline.
pub trait Probe: Send + 'static {
    /// Checks the dependency once.
    fn check(&mut self) -> impl Future<Output = Result<(), ProbeError>> + Send;
}

/// PostgreSQL: `SELECT 1` through the shared pool.
#[derive(Debug)]
pub struct DatabaseProbe {
    pool: PgPool,
    deadline: Duration,
}

impl DatabaseProbe {
    /// Probes through `pool`, giving up after `deadline`.
    pub fn new(pool: PgPool, deadline: Duration) -> Self {
        Self { pool, deadline }
    }
}

impl Probe for DatabaseProbe {
    async fn check(&mut self) -> Result<(), ProbeError> {
        colophon_db::ping(&self.pool, self.deadline).await?;
        Ok(())
    }
}

/// The cache: `PING` over one reused connection, reconnecting after any failure. Moves into
/// the cache adapter in `colophon-delivery` when that crate exists (milestone P3).
pub struct CacheProbe {
    client: redis::Client,
    connection: Option<MultiplexedConnection>,
    deadline: Duration,
}

#[derive(Debug, thiserror::Error)]
enum CacheProbeError {
    #[error("cache did not answer within {0:?}")]
    Timeout(Duration),
    #[error("cache answered {0:?} instead of PONG")]
    UnexpectedReply(String),
}

impl CacheProbe {
    /// Parses the URL now, so a malformed one is a startup error. The URL holds a password,
    /// so the error never includes it.
    pub fn new(url: &str, deadline: Duration) -> Result<Self, ProbeError> {
        let client = redis::Client::open(url)
            .map_err(|_| "COLOPHON_CACHE_URL is not a valid redis:// URL")?;
        Ok(Self {
            client,
            connection: None,
            deadline,
        })
    }

    async fn ping(&mut self) -> Result<(), ProbeError> {
        let connection = match &mut self.connection {
            Some(connection) => connection,
            empty @ None => {
                let config = redis::AsyncConnectionConfig::new()
                    .set_connection_timeout(Some(self.deadline))
                    .set_response_timeout(Some(self.deadline));
                empty.insert(
                    self.client
                        .get_multiplexed_async_connection_with_config(&config)
                        .await?,
                )
            }
        };
        let reply: String = redis::cmd("PING").query_async(connection).await?;
        if reply == "PONG" {
            Ok(())
        } else {
            Err(CacheProbeError::UnexpectedReply(reply).into())
        }
    }
}

impl Probe for CacheProbe {
    async fn check(&mut self) -> Result<(), ProbeError> {
        let result = match tokio::time::timeout(self.deadline, self.ping()).await {
            Ok(result) => result,
            Err(_elapsed) => Err(CacheProbeError::Timeout(self.deadline).into()),
        };
        if result.is_err() {
            // Start from a fresh connection next time.
            self.connection = None;
        }
        result
    }
}

/// Probes `dependency` every `interval` until `cancel` fires.
pub async fn run(
    dependency: Dependency,
    readiness: Readiness,
    mut probe: impl Probe,
    interval: Duration,
    cancel: CancellationToken,
) {
    let name = dependency.as_str();
    let mut ticker = tokio::time::interval(interval);
    // After a slow probe, wait a full interval rather than probing in a burst to catch up.
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        let result = tokio::select! {
            () = cancel.cancelled() => return,
            _ = ticker.tick() => tokio::select! {
                () = cancel.cancelled() => return,
                result = probe.check() => result,
            },
        };
        let previous = readiness.record(dependency, result.is_ok());
        match (previous, result) {
            (CheckState::Up | CheckState::Unknown, Err(error)) => {
                let cause = error_chain(error.as_ref());
                tracing::warn!(dependency = name, %cause, "dependency is down");
            }
            (CheckState::Down | CheckState::Unknown | CheckState::NotConfigured, Ok(())) => {
                tracing::info!(dependency = name, "dependency is up");
            }
            _ => {}
        }
    }
}

fn error_chain(error: &(dyn StdError + 'static)) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// Owns the probe tasks: spawns them, notices if one ends on its own, and stops them all.
pub struct Supervisor {
    tasks: JoinSet<Dependency>,
    /// Which dependency each task probes, so a panic can be attributed.
    owners: Vec<(Id, Dependency)>,
    readiness: Readiness,
    cancel: CancellationToken,
}

impl Supervisor {
    /// A supervisor whose tasks stop when `cancel` fires.
    pub fn new(readiness: Readiness, cancel: CancellationToken) -> Self {
        Self {
            tasks: JoinSet::new(),
            owners: Vec::new(),
            readiness,
            cancel,
        }
    }

    /// Starts probing `dependency`.
    pub fn spawn(&mut self, dependency: Dependency, probe: impl Probe, interval: Duration) {
        let (readiness, cancel) = (self.readiness.clone(), self.cancel.clone());
        let handle = self.tasks.spawn(async move {
            run(dependency, readiness, probe, interval, cancel).await;
            dependency
        });
        self.owners.push((handle.id(), dependency));
    }

    /// Waits until a probe task ends without being cancelled (it panicked, or returned), marks
    /// that dependency down for good, and logs it. Never returns.
    pub async fn watch(&mut self) -> Infallible {
        loop {
            match self.tasks.join_next().await {
                None => std::future::pending::<()>().await,
                Some(_) if self.cancel.is_cancelled() => {}
                Some(Ok(dependency)) => {
                    self.readiness.record(dependency, false);
                    tracing::error!(
                        dependency = dependency.as_str(),
                        "probe stopped unexpectedly; dependency marked down"
                    );
                }
                Some(Err(error)) => {
                    // Attribute the panic to its dependency. An unknown task can't happen (every
                    // task is registered at spawn), but if it did, the safe choice is the
                    // database: it is required, so the process stops taking traffic.
                    let dependency = self
                        .owners
                        .iter()
                        .find(|(id, _)| *id == error.id())
                        .map_or(Dependency::Database, |(_, dependency)| *dependency);
                    self.readiness.record(dependency, false);
                    tracing::error!(dependency = dependency.as_str(), %error, "probe task panicked; dependency marked down");
                }
            }
        }
    }

    /// Stops every probe and waits up to `deadline` for them to finish.
    pub async fn shutdown(mut self, deadline: Duration) {
        self.cancel.cancel();
        let drained = tokio::time::timeout(deadline, async {
            while let Some(result) = self.tasks.join_next().await {
                if let Err(error) = result {
                    tracing::error!(%error, "a probe task failed during shutdown");
                }
            }
        })
        .await;
        if drained.is_err() {
            tracing::warn!("probe tasks did not stop in time; abandoning them");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use colophon_api::Status;

    use super::*;

    /// A scripted probe: answers from a list, then keeps answering the last value.
    struct Scripted {
        answers: Vec<bool>,
        calls: Arc<AtomicUsize>,
    }

    impl Probe for Scripted {
        #[expect(
            clippy::unused_async_trait_impl,
            reason = "the trait method is async; this fake answers at once"
        )]
        async fn check(&mut self) -> Result<(), ProbeError> {
            let call = self.calls.fetch_add(1, Ordering::Relaxed);
            let up = self
                .answers
                .get(call)
                .or(self.answers.last())
                .copied()
                .unwrap_or(false);
            if up {
                Ok(())
            } else {
                Err("scripted failure".into())
            }
        }
    }

    struct Panics;

    impl Probe for Panics {
        #[expect(
            clippy::unused_async_trait_impl,
            reason = "the trait method is async; this fake panics at once"
        )]
        async fn check(&mut self) -> Result<(), ProbeError> {
            panic!("probe bug")
        }
    }

    #[tokio::test(start_paused = true)]
    async fn readiness_follows_the_probe_results() {
        let readiness = Readiness::new("all", false);
        let calls = Arc::new(AtomicUsize::new(0));
        let probe = Scripted {
            answers: vec![true, false, true],
            calls: Arc::clone(&calls),
        };
        let cancel = CancellationToken::new();
        let task = tokio::spawn(run(
            Dependency::Database,
            readiness.clone(),
            probe,
            Duration::from_secs(2),
            cancel.clone(),
        ));

        // The first tick fires at once.
        tokio::time::sleep(Duration::from_millis(1)).await;
        assert_eq!(readiness.report().status, Status::Ready);
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(readiness.report().status, Status::NotReady);
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(readiness.report().status, Status::Ready);

        cancel.cancel();
        task.await
            .expect("the probe loop ends cleanly when cancelled");
        assert_eq!(calls.load(Ordering::Relaxed), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn a_panicking_cache_probe_degrades_instead_of_freezing_readiness() {
        let readiness = Readiness::new("all", true);
        readiness.record(Dependency::Database, true);
        readiness.record(Dependency::Cache, true);
        let mut supervisor = Supervisor::new(readiness.clone(), CancellationToken::new());
        supervisor.spawn(Dependency::Cache, Panics, Duration::from_secs(2));
        tokio::time::timeout(Duration::from_secs(1), supervisor.watch())
            .await
            .expect_err("watch never returns; it should time out after recording the panic");
        assert_eq!(readiness.report().status, Status::Degraded);
        assert_eq!(readiness.report().checks.database, CheckState::Up);
    }

    #[tokio::test(start_paused = true)]
    async fn a_panicking_database_probe_forces_not_ready_instead_of_freezing_readiness() {
        let readiness = Readiness::new("all", false);
        readiness.record(Dependency::Database, true);
        let mut supervisor = Supervisor::new(readiness.clone(), CancellationToken::new());
        supervisor.spawn(Dependency::Database, Panics, Duration::from_secs(2));
        tokio::time::timeout(Duration::from_secs(1), supervisor.watch())
            .await
            .expect_err("watch never returns; it should time out after recording the panic");
        assert_eq!(readiness.report().status, Status::NotReady);
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_stops_every_probe() {
        let readiness = Readiness::new("all", true);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut supervisor = Supervisor::new(readiness, CancellationToken::new());
        for dependency in [Dependency::Database, Dependency::Cache] {
            let probe = Scripted {
                answers: vec![true],
                calls: Arc::clone(&calls),
            };
            supervisor.spawn(dependency, probe, Duration::from_secs(2));
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
        supervisor.shutdown(Duration::from_secs(1)).await;
        let after_shutdown = calls.load(Ordering::Relaxed);
        tokio::time::sleep(Duration::from_secs(10)).await;
        assert_eq!(
            calls.load(Ordering::Relaxed),
            after_shutdown,
            "no probe runs after shutdown"
        );
    }

    #[tokio::test]
    async fn the_cache_probe_reports_a_down_cache_within_its_deadline() {
        let mut probe = CacheProbe::new("redis://:pw@127.0.0.1:1", Duration::from_millis(300))
            .expect("a valid URL");
        let started = std::time::Instant::now();
        assert!(probe.check().await.is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn a_malformed_cache_url_is_rejected_without_echoing_it() {
        let error = CacheProbe::new("http://:hunter2@nowhere", Duration::from_secs(1))
            .err()
            .expect("not a redis URL");
        assert!(!error.to_string().contains("hunter2"));
    }

    #[tokio::test]
    #[ignore = "needs the local stack: scripts/stack.sh up"]
    async fn the_cache_probe_succeeds_against_the_local_stack() {
        let url = std::env::var("COLOPHON_CACHE_URL")
            .unwrap_or_else(|_| "redis://:colophon-dev@127.0.0.1:56379".to_owned());
        let mut probe = CacheProbe::new(&url, Duration::from_secs(2)).expect("a valid URL");
        probe.check().await.expect("the stack's cache answers");
        probe
            .check()
            .await
            .expect("and answers again on the reused connection");
    }
}
