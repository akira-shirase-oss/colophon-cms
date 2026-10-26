//! The delivery cache (docs/modules/06-delivery.md, "Cache keys" and "Cache layer"; D-048).
//!
//! Public answers are cached in Valkey under keys that name the generation they were read at
//! (`colophon_domain::cache`). A generation's published content never changes, so an entry can't
//! go stale: a new release is a new generation and so new keys. The active generation itself is
//! resolved from Postgres on every request, never from the cache, so a stale pointer can't bring
//! an older release back (DL3).
//!
//! Misses for one key in one process share a single fill (L04: a thousand callers at expiry cause
//! one database read). When Valkey is down or slow, answers come from Postgres and are marked
//! `bypass`: the cache is disposable, never a source of truth (DL7, L11, F08). Postgres stays
//! bounded either way: fills go through its pool, and a full pool answers 503 with `Retry-After`.

use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use redis::aio::MultiplexedConnection;
use tokio::sync::OnceCell;

/// How long a cached answer lives, before jitter (module 06: 300 seconds with jitter).
const TTL_SECS: u64 = 300;
/// Up to this much is added to each entry's lifetime, so entries made together don't expire
/// together.
const JITTER_SECS: u64 = 60;
/// A cache call that takes longer than this is abandoned: Postgres answers instead.
const DEADLINE: Duration = Duration::from_millis(100);
/// Larger answers are served but not cached, so a few huge pages can't evict everything else.
const MAX_CACHED_BYTES: usize = 1 << 20;
/// After the cache fails, requests go straight to Postgres for this long before anyone tries to
/// reconnect: a dead cache costs one attempt a second, not one per request.
const BACKOFF: Duration = Duration::from_secs(1);

/// Where an answer came from, for the `Colophon-Cache` response header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Served {
    /// From the cache.
    Hit,
    /// Read from Postgres by this request, then cached.
    Miss,
    /// Read from Postgres by another request for the same key, which this one waited for.
    Coalesced,
    /// The cache was off, down or slow, or the answer isn't cacheable: read from Postgres.
    Bypass,
}

impl Served {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Coalesced => "coalesced",
            Self::Bypass => "bypass",
        }
    }
}

/// An answer: its body and the `ETag` computed when it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cached {
    pub(crate) etag: String,
    pub(crate) body: String,
}

impl Cached {
    fn encode(&self) -> String {
        format!("{}\n{}", self.etag, self.body)
    }

    fn decode(stored: &str) -> Option<Self> {
        let (etag, body) = stored.split_once('\n')?;
        Some(Self {
            etag: etag.to_owned(),
            body: body.to_owned(),
        })
    }
}

/// What a fill read from Postgres.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Filled {
    /// An answer to cache and serve.
    Keep(Cached),
    /// An answer to serve but not cache: an empty page, which any made-up collection name in a
    /// URL would get, so caching it would let anyone fill the cache with junk.
    Once(Cached),
    /// Nothing there: 404, not cached.
    Missing,
}

/// Why a fill failed, shared with every request that waited for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refused {
    /// Postgres had no connection free in time: 503 with `Retry-After`.
    Busy,
    /// Anything else, already logged where it happened: 500.
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Cache,
    Database,
}

type Flight = Arc<OnceCell<Result<(Filled, Source), Refused>>>;

/// The connection to the cache, or when to try again after it failed.
#[derive(Default)]
struct Link {
    connection: Option<MultiplexedConnection>,
    retry_at: Option<Instant>,
}

/// The delivery cache: Valkey when configured, plus single-flight for misses.
pub struct DeliveryCache {
    client: Option<redis::Client>,
    /// Held only to clone or replace the connection, never across an await.
    link: Mutex<Link>,
    /// Held while connecting. Others don't wait for it: they read from Postgres instead.
    connecting: tokio::sync::Mutex<()>,
    flights: Mutex<HashMap<String, Flight>>,
}

impl std::fmt::Debug for DeliveryCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeliveryCache")
            .field("enabled", &self.client.is_some())
            .finish_non_exhaustive()
    }
}

/// The cache URL was refused. Never includes the URL: it holds a password.
#[derive(Debug, thiserror::Error)]
#[error("COLOPHON_CACHE_URL is not a valid redis:// URL")]
pub struct InvalidCacheUrl;

impl DeliveryCache {
    /// No cache: every answer comes from Postgres (still with single-flight).
    pub fn disabled() -> Self {
        Self {
            client: None,
            link: Mutex::new(Link::default()),
            connecting: tokio::sync::Mutex::new(()),
            flights: Mutex::new(HashMap::new()),
        }
    }

    /// A cache at `url`. Connects lazily, and again after any failure.
    pub fn new(url: &str) -> Result<Self, InvalidCacheUrl> {
        let client = redis::Client::open(url).map_err(|_| InvalidCacheUrl)?;
        Ok(Self {
            client: Some(client),
            ..Self::disabled()
        })
    }

    fn link(&self) -> std::sync::MutexGuard<'_, Link> {
        self.link.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The live connection, if there is one; or `Err(true)` if it's time to try connecting.
    fn current(&self) -> Result<MultiplexedConnection, bool> {
        let link = self.link();
        match &link.connection {
            Some(connection) => Ok(connection.clone()),
            None => Err(link.retry_at.is_none_or(|at| Instant::now() >= at)),
        }
    }

    async fn connection(&self) -> Option<MultiplexedConnection> {
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

    /// The cache failed or was too slow: drop the connection and leave it alone for a while.
    fn fail(&self) {
        let mut link = self.link();
        link.connection = None;
        link.retry_at = Some(Instant::now() + BACKOFF);
    }

    /// The cached answer for `key`, if the cache has one and answers in time; and whether the
    /// cache answered at all.
    async fn get(&self, key: &str) -> (Option<Cached>, bool) {
        let Some(mut connection) = self.connection().await else {
            return (None, false);
        };
        let reply = tokio::time::timeout(
            DEADLINE,
            redis::cmd("GET")
                .arg(key)
                .query_async::<Option<String>>(&mut connection),
        )
        .await;
        if let Ok(Ok(stored)) = reply {
            (stored.as_deref().and_then(Cached::decode), true)
        } else {
            self.fail();
            (None, false)
        }
    }

    /// Stores `value` under `key` for a jittered lifetime. Failures are ignored: the cache is
    /// disposable.
    async fn put(&self, key: &str, value: &Cached, jitter: u64) {
        if value.body.len() > MAX_CACHED_BYTES {
            return;
        }
        let Some(mut connection) = self.connection().await else {
            return;
        };
        let lifetime = TTL_SECS + jitter % (JITTER_SECS + 1);
        let stored = tokio::time::timeout(
            DEADLINE,
            redis::cmd("SET")
                .arg(key)
                .arg(value.encode())
                .arg("EX")
                .arg(lifetime)
                .query_async::<()>(&mut connection),
        )
        .await;
        if !matches!(stored, Ok(Ok(()))) {
            self.fail();
        }
    }

    /// The answer for `key`: from the cache, or from `fill`, then cached. Concurrent misses for
    /// one key in this process share one fill and its outcome, failures included. `jitter` is any
    /// random number, for the entry's lifetime.
    pub(crate) async fn get_or_fill<F, Fut>(
        &self,
        key: &str,
        jitter: u64,
        fill: F,
    ) -> Result<(Filled, Served), Refused>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Filled, Refused>>,
    {
        let (found, reachable) = self.get(key).await;
        if let Some(cached) = found {
            return Ok((Filled::Keep(cached), Served::Hit));
        }
        let flight = {
            let mut flights = self.flights.lock().unwrap_or_else(PoisonError::into_inner);
            Arc::clone(flights.entry(key.to_owned()).or_default())
        };
        let mut led = false;
        let outcome = flight
            .get_or_init(|| async {
                led = true;
                // The flight before this one may have filled the cache since this request looked.
                if reachable && let (Some(cached), _) = self.get(key).await {
                    return Ok((Filled::Keep(cached), Source::Cache));
                }
                let filled = fill().await?;
                if reachable && let Filled::Keep(cached) = &filled {
                    self.put(key, cached, jitter).await;
                }
                Ok((filled, Source::Database))
            })
            .await
            .clone();
        if led {
            // Later requests look in the cache again rather than reuse this outcome.
            let mut flights = self.flights.lock().unwrap_or_else(PoisonError::into_inner);
            if flights
                .get(key)
                .is_some_and(|current| Arc::ptr_eq(current, &flight))
            {
                flights.remove(key);
            }
        }
        let (filled, source) = outcome?;
        let served = match (led, source, &filled) {
            (false, _, _) => Served::Coalesced,
            (true, Source::Cache, _) => Served::Hit,
            (true, Source::Database, Filled::Keep(_)) if reachable => Served::Miss,
            (true, Source::Database, _) => Served::Bypass,
        };
        Ok((filled, served))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn answer(body: &str) -> Cached {
        Cached {
            etag: "\"e\"".to_owned(),
            body: body.to_owned(),
        }
    }

    #[test]
    fn a_cached_answer_round_trips_with_its_etag() {
        let cached = answer("{\"data\":[]}\nwith a newline");
        assert_eq!(Cached::decode(&cached.encode()), Some(cached));
        assert_eq!(Cached::decode("no newline"), None);
    }

    #[tokio::test]
    async fn concurrent_misses_share_one_fill_without_a_cache() {
        let cache = Arc::new(DeliveryCache::disabled());
        let fills = Arc::new(AtomicUsize::new(0));
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..50 {
            let (cache, fills) = (Arc::clone(&cache), Arc::clone(&fills));
            tasks.spawn(async move {
                cache
                    .get_or_fill("k", 0, || async {
                        fills.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        Ok(Filled::Keep(answer("b")))
                    })
                    .await
            });
        }
        let mut served = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            let (filled, how) = joined.expect("task").expect("filled");
            assert_eq!(filled, Filled::Keep(answer("b")));
            served.push(how);
        }
        assert_eq!(
            fills.load(Ordering::SeqCst),
            1,
            "one fill for fifty callers"
        );
        let count = |wanted| served.iter().filter(|how| **how == wanted).count();
        assert_eq!(
            count(Served::Bypass),
            1,
            "no cache: the one that filled bypassed it"
        );
        assert_eq!(count(Served::Coalesced), 49);
    }

    #[tokio::test]
    async fn waiters_share_the_fills_failure_and_the_next_request_tries_again() {
        let cache = Arc::new(DeliveryCache::disabled());
        let (gate, _) = tokio::sync::broadcast::channel::<()>(1);
        let leader = {
            let (cache, mut opened) = (Arc::clone(&cache), gate.subscribe());
            tokio::spawn(async move {
                cache
                    .get_or_fill("k", 0, || async move {
                        let _ = opened.recv().await;
                        Err(Refused::Busy)
                    })
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        let waiter = {
            let cache = Arc::clone(&cache);
            tokio::spawn(async move {
                cache
                    .get_or_fill("k", 0, || async { Ok(Filled::Keep(answer("never"))) })
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        gate.send(()).expect("the leader is waiting");
        assert_eq!(leader.await.expect("task"), Err(Refused::Busy));
        assert_eq!(
            waiter.await.expect("task"),
            Err(Refused::Busy),
            "the waiter shares it"
        );
        let next = cache
            .get_or_fill("k", 0, || async { Ok(Filled::Missing) })
            .await;
        assert_eq!(
            next,
            Ok((Filled::Missing, Served::Bypass)),
            "a new fill, not the old failure"
        );
    }

    #[tokio::test]
    async fn an_unreachable_cache_is_bypassed_quickly() {
        // Nothing listens on port 9 (discard) on loopback in the test environment.
        let cache = DeliveryCache::new("redis://127.0.0.1:9/").expect("valid URL");
        let started = std::time::Instant::now();
        let outcome = cache
            .get_or_fill("k", 0, || async { Ok(Filled::Keep(answer("b"))) })
            .await;
        assert_eq!(outcome, Ok((Filled::Keep(answer("b")), Served::Bypass)));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(
            cache.current().err(),
            Some(false),
            "no new attempt within the backoff"
        );
    }
}
