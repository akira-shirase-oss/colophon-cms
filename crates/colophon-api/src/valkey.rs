//! One lazily opened connection to Valkey, shared by the delivery cache (D-048) and admission
//! (D-076). Every call has [`DEADLINE`]; after a failure nobody tries again for [`BACKOFF`], so a
//! dead Valkey costs one attempt a second, not one per request; and a request never queues behind
//! another's reconnect. Callers fall back to Postgres or to this process's own limits: Valkey is
//! never a source of truth.

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use redis::aio::MultiplexedConnection;

/// A Valkey call that takes longer than this is abandoned.
pub(crate) const DEADLINE: Duration = Duration::from_millis(100);
/// After Valkey fails, nobody tries to reconnect for this long.
const BACKOFF: Duration = Duration::from_secs(1);

/// The connection, or when to try again after it failed.
#[derive(Default)]
struct Link {
    connection: Option<MultiplexedConnection>,
    retry_at: Option<Instant>,
}

/// Valkey at one URL, or nothing.
pub(crate) struct Valkey {
    client: Option<redis::Client>,
    /// Held only to clone or replace the connection, never across an await.
    link: Mutex<Link>,
    /// Held while connecting. Others don't wait for it: they fall back instead.
    connecting: tokio::sync::Mutex<()>,
}

impl Valkey {
    /// No Valkey: [`Self::connection`] is always `None`.
    pub(crate) fn disabled() -> Self {
        Self {
            client: None,
            link: Mutex::new(Link::default()),
            connecting: tokio::sync::Mutex::new(()),
        }
    }

    /// Valkey at `url`, connected when first needed. `None` if the URL isn't a valid one (the
    /// caller says so without the URL, which holds a password).
    pub(crate) fn new(url: &str) -> Option<Self> {
        let client = redis::Client::open(url).ok()?;
        Some(Self {
            client: Some(client),
            ..Self::disabled()
        })
    }

    /// Whether a URL was configured.
    pub(crate) const fn enabled(&self) -> bool {
        self.client.is_some()
    }

    fn link(&self) -> std::sync::MutexGuard<'_, Link> {
        self.link.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The live connection, if there is one; or `Err(true)` if it's time to try connecting.
    pub(crate) fn current(&self) -> Result<MultiplexedConnection, bool> {
        let link = self.link();
        match &link.connection {
            Some(connection) => Ok(connection.clone()),
            None => Err(link.retry_at.is_none_or(|at| Instant::now() >= at)),
        }
    }

    /// A connection, if Valkey is configured, up, and not being connected to by someone else.
    pub(crate) async fn connection(&self) -> Option<MultiplexedConnection> {
        let client = self.client.as_ref()?;
        match self.current() {
            Ok(connection) => return Some(connection),
            Err(false) => return None,
            Err(true) => {}
        }
        // Another request is connecting: don't queue behind it.
        let Ok(_connecting) = self.connecting.try_lock() else {
            return None;
        };
        if let Ok(connection) = self.current() {
            return Some(connection);
        }
        let config = redis::AsyncConnectionConfig::new()
            .set_connection_timeout(Some(DEADLINE))
            .set_response_timeout(Some(DEADLINE));
        let connected = tokio::time::timeout(
            DEADLINE,
            client.get_multiplexed_async_connection_with_config(&config),
        )
        .await;
        let mut link = self.link();
        if let Ok(Ok(connection)) = connected {
            link.connection = Some(connection.clone());
            link.retry_at = None;
            Some(connection)
        } else {
            link.retry_at = Some(Instant::now() + BACKOFF);
            None
        }
    }

    /// Valkey failed or was too slow: drop the connection and leave it alone for a while.
    pub(crate) fn fail(&self) {
        let mut link = self.link();
        link.connection = None;
        link.retry_at = Some(Instant::now() + BACKOFF);
    }
}
