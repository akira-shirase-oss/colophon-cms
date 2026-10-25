//! `/livez` and `/readyz` (`docs/modules/18-operations.md`, "Health").
//!
//! - `/livez`: the process responds. It checks nothing else, because restarting a process
//!   cannot fix a database outage.
//! - `/readyz`: this process can safely serve now. Background probes update [`Readiness`];
//!   the handler only reads it. A flood of `/readyz` requests therefore costs no database
//!   or cache work at all, and a slow dependency can't make the probe endpoint slow.
//!
//! | Database | Cache | Status | HTTP |
//! |---|---|---|---|
//! | not probed yet | any | `starting` | 503 |
//! | down | any | `not_ready` | 503 |
//! | up | up or not configured | `ready` | 200 |
//! | up | down or not probed yet | `degraded` | 200 |
//!
//! Once draining begins (shutdown), the status is `draining` (503) whatever the probes say.
//! The cache is a disposable accelerator: losing it slows Colophon down but never makes it
//! unsafe, so it can only degrade readiness, never remove it.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

use axum::{
    Json,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;

/// The state of one dependency, as the last probe saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    /// Not probed yet.
    Unknown,
    /// The last probe succeeded within its deadline.
    Up,
    /// The last probe failed or missed its deadline.
    Down,
    /// This install doesn't use the dependency.
    NotConfigured,
}

impl CheckState {
    const fn to_u8(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::Up => 1,
            Self::Down => 2,
            Self::NotConfigured => 3,
        }
    }

    const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Up,
            2 => Self::Down,
            3 => Self::NotConfigured,
            _ => Self::Unknown,
        }
    }
}

/// The dependencies readiness depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dependency {
    /// PostgreSQL: the only source of truth. Required.
    Database,
    /// The Redis-protocol cache (Valkey): optional and disposable.
    Cache,
}

impl Dependency {
    /// The name used in logs and in the readiness report.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Database => "database",
            Self::Cache => "cache",
        }
    }
}

/// Overall readiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The database has not been probed yet.
    Starting,
    /// Everything this process needs is up.
    Ready,
    /// Safe to serve, but the cache is unavailable, so some reads are slower.
    Degraded,
    /// Not safe to serve: the database is down.
    NotReady,
    /// Shutting down: finishing in-flight requests, accepting no new traffic.
    Draining,
}

impl Status {
    /// Whether a load balancer should send traffic here.
    pub const fn accepts_traffic(self) -> bool {
        matches!(self, Self::Ready | Self::Degraded)
    }

    /// Derives the overall status. A pure function, so every combination can be tested.
    pub const fn derive(draining: bool, database: CheckState, cache: CheckState) -> Self {
        if draining {
            return Self::Draining;
        }
        match database {
            CheckState::Unknown => Self::Starting,
            CheckState::Down | CheckState::NotConfigured => Self::NotReady,
            CheckState::Up => match cache {
                CheckState::Up | CheckState::NotConfigured => Self::Ready,
                CheckState::Down | CheckState::Unknown => Self::Degraded,
            },
        }
    }
}

/// The per-dependency part of the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Checks {
    /// PostgreSQL.
    pub database: CheckState,
    /// The cache.
    pub cache: CheckState,
}

/// The `/readyz` body. It names states only: never hosts, ports or error messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ReadinessReport {
    /// Overall status.
    pub status: Status,
    /// The role this process runs (`api`, `worker` or `all`).
    pub role: &'static str,
    /// Each dependency.
    pub checks: Checks,
}

#[derive(Debug)]
struct Inner {
    role: &'static str,
    draining: AtomicBool,
    database: AtomicU8,
    cache: AtomicU8,
}

/// Shared readiness: written by the probes and the shutdown sequence, read by `/readyz`.
/// Cloning is one reference-count increment.
#[derive(Debug, Clone)]
pub struct Readiness(Arc<Inner>);

impl Readiness {
    /// Readiness for a process running `role`. Without a configured cache, the cache check is
    /// `not_configured` from the start.
    pub fn new(role: &'static str, cache_configured: bool) -> Self {
        let cache = if cache_configured {
            CheckState::Unknown
        } else {
            CheckState::NotConfigured
        };
        Self(Arc::new(Inner {
            role,
            draining: AtomicBool::new(false),
            database: AtomicU8::new(CheckState::Unknown.to_u8()),
            cache: AtomicU8::new(cache.to_u8()),
        }))
    }

    fn slot(&self, dependency: Dependency) -> &AtomicU8 {
        match dependency {
            Dependency::Database => &self.0.database,
            Dependency::Cache => &self.0.cache,
        }
    }

    /// Records a probe result and returns the previous state, so the caller can log
    /// transitions instead of every probe.
    pub fn record(&self, dependency: Dependency, up: bool) -> CheckState {
        let next = if up { CheckState::Up } else { CheckState::Down };
        // Each value is independent and read without ordering against the others, so
        // `Relaxed` is enough: a report may mix two probes' results, never a torn value.
        CheckState::from_u8(self.slot(dependency).swap(next.to_u8(), Ordering::Relaxed))
    }

    /// Starts draining. Terminal: no probe result can make the process ready again.
    pub fn begin_drain(&self) {
        self.0.draining.store(true, Ordering::Relaxed);
    }

    /// The current report.
    pub fn report(&self) -> ReadinessReport {
        let database = CheckState::from_u8(self.0.database.load(Ordering::Relaxed));
        let cache = CheckState::from_u8(self.0.cache.load(Ordering::Relaxed));
        let draining = self.0.draining.load(Ordering::Relaxed);
        ReadinessReport {
            status: Status::derive(draining, database, cache),
            role: self.0.role,
            checks: Checks { database, cache },
        }
    }
}

#[derive(Serialize)]
struct Liveness {
    status: &'static str,
}

fn no_store(mut response: Response) -> Response {
    // Health answers are about this instant; no cache or proxy may keep them.
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// `GET /livez`.
pub(crate) async fn livez() -> Response {
    no_store(Json(Liveness { status: "live" }).into_response())
}

/// `GET /readyz`: 200 when the process accepts traffic, 503 otherwise, with the report.
pub(crate) async fn readyz(State(readiness): State<Readiness>) -> Response {
    let report = readiness.report();
    let status = if report.status.accepts_traffic() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    no_store((status, Json(report)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [CheckState; 4] = [
        CheckState::Unknown,
        CheckState::Up,
        CheckState::Down,
        CheckState::NotConfigured,
    ];

    #[test]
    fn every_combination_has_the_documented_status() {
        for database in ALL {
            for cache in ALL {
                let expected = match (database, cache) {
                    (CheckState::Unknown, _) => Status::Starting,
                    (CheckState::Down | CheckState::NotConfigured, _) => Status::NotReady,
                    (CheckState::Up, CheckState::Up | CheckState::NotConfigured) => Status::Ready,
                    (CheckState::Up, CheckState::Down | CheckState::Unknown) => Status::Degraded,
                };
                assert_eq!(
                    Status::derive(false, database, cache),
                    expected,
                    "{database:?}/{cache:?}"
                );
                assert_eq!(Status::derive(true, database, cache), Status::Draining);
            }
        }
    }

    #[test]
    fn only_ready_and_degraded_accept_traffic() {
        assert!(Status::Ready.accepts_traffic());
        assert!(Status::Degraded.accepts_traffic());
        for status in [Status::Starting, Status::NotReady, Status::Draining] {
            assert!(!status.accepts_traffic(), "{status:?}");
        }
    }

    #[test]
    fn check_states_survive_the_atomic_round_trip() {
        for state in ALL {
            assert_eq!(CheckState::from_u8(state.to_u8()), state);
        }
    }

    #[test]
    fn record_returns_the_previous_state() {
        let readiness = Readiness::new("all", true);
        assert_eq!(
            readiness.record(Dependency::Database, true),
            CheckState::Unknown
        );
        assert_eq!(
            readiness.record(Dependency::Database, false),
            CheckState::Up
        );
        assert_eq!(readiness.report().checks.database, CheckState::Down);
    }

    #[test]
    fn an_unconfigured_cache_never_degrades_readiness() {
        let readiness = Readiness::new("api", false);
        readiness.record(Dependency::Database, true);
        let report = readiness.report();
        assert_eq!(report.status, Status::Ready);
        assert_eq!(report.checks.cache, CheckState::NotConfigured);
    }

    #[test]
    fn draining_is_terminal() {
        let readiness = Readiness::new("all", true);
        readiness.record(Dependency::Database, true);
        readiness.record(Dependency::Cache, true);
        readiness.begin_drain();
        readiness.record(Dependency::Database, true);
        assert_eq!(readiness.report().status, Status::Draining);
    }

    #[test]
    fn the_report_has_a_stable_wire_shape() {
        let readiness = Readiness::new("all", true);
        readiness.record(Dependency::Database, true);
        readiness.record(Dependency::Cache, false);
        let json = serde_json::to_value(readiness.report()).expect("serializable");
        assert_eq!(
            json,
            serde_json::json!({
                "status": "degraded",
                "role": "all",
                "checks": { "database": "up", "cache": "down" }
            })
        );
    }
}
