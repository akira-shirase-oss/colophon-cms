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
//!
//! A key this process stored is filled again within its lifetime only if the cache lost the answer
//! (an eviction, a flush, a lost store). Such fills are counted per kind of key ([`REFILLS`]) and said
//! once a window by the server's summary task, outside any request, on the `colophon_api::cache_refill`
//! target: at INFO when the window had any, at DEBUG when it had none (the load runs' proof the count
//! was taken). L10 gates their count at zero (D-232).

use std::{
    collections::HashMap,
    future::Future,
    sync::{
        Arc, LazyLock, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use tokio::sync::OnceCell;

use crate::valkey::{DEADLINE, Valkey};

/// How long a cached answer lives, before jitter (module 06: 300 seconds with jitter).
const TTL: Duration = Duration::from_secs(300);
/// Up to this much is added to each entry's lifetime, so entries made together don't expire
/// together.
const JITTER: Duration = Duration::from_secs(60);
/// Larger answers are served but not cached, so a few huge pages can't evict everything else.
const MAX_CACHED_BYTES: usize = 1 << 20;
/// The most stored keys the duplicate-fill check remembers. Past it, it drops the ones whose lifetime
/// has passed, and all of them when that leaves more than three quarters: it can miss a duplicate,
/// never report one that wasn't.
const MAX_TRACKED_FILLS: usize = 16_384;

/// Fills of keys this process had stored less than a lifetime before (D-232), per kind of key, for
/// the current window. Bounded: four counters.
#[derive(Debug, Default)]
pub struct Refills {
    list: AtomicU64,
    entry: AtomicU64,
    query: AtomicU64,
    other: AtomicU64,
}

/// The process's refill counts: the cache adds to them, the server's summary task says them.
pub static REFILLS: LazyLock<Refills> = LazyLock::new(Refills::default);

impl Refills {
    /// Counts one refill of `key`, by its kind. Logs nothing: a refill happens inside a request,
    /// whose span names the project and entry, and in production each eviction would be a line.
    pub fn note(&self, key: &str) {
        let counter = match key_kind(key) {
            "list" => &self.list,
            "entry" => &self.entry,
            "query" => &self.query,
            _ => &self.other,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Closes the window: one line with its counts, at INFO when there were any, at DEBUG when there
    /// were none (so a load run can tell a count of zero from a count that wasn't taken).
    pub fn close_window(&self) {
        self.say(true);
    }

    /// At shutdown: says what the open window counted, if anything.
    pub fn flush(&self) {
        self.say(false);
    }

    fn say(&self, even_none: bool) {
        let list = self.list.swap(0, Ordering::Relaxed);
        let entry = self.entry.swap(0, Ordering::Relaxed);
        let query = self.query.swap(0, Ordering::Relaxed);
        let other = self.other.swap(0, Ordering::Relaxed);
        let total = list + entry + query + other;
        if total > 0 {
            tracing::info!(
                target: "colophon_api::cache_refill",
                list,
                entry,
                query,
                other,
                total,
                "delivery cache: keys filled again within their lifetime"
            );
        } else if even_none {
            tracing::debug!(
                target: "colophon_api::cache_refill",
                list,
                entry,
                query,
                other,
                total,
                "delivery cache: keys filled again within their lifetime"
            );
        }
    }
}

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
    /// Taken down (D-049): 410, not cached.
    Gone,
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

/// The delivery cache: Valkey when configured, plus single-flight for misses.
pub struct DeliveryCache {
    /// Valkey, when configured; a call that fails or is slow falls back to Postgres.
    valkey: Valkey,
    flights: Mutex<HashMap<String, Flight>>,
    /// When this process last stored each key it filled, to tell a fill the cache should have
    /// answered (the key was stored less than a lifetime ago) from a first one (D-232).
    stored: Mutex<HashMap<String, Instant>>,
    /// How long a stored answer lives.
    ttl: Duration,
    /// Up to this much extra each store draws, so entries made together don't expire together.
    jitter: Duration,
}

impl std::fmt::Debug for DeliveryCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeliveryCache")
            .field("enabled", &self.valkey.enabled())
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
            valkey: Valkey::disabled(),
            flights: Mutex::new(HashMap::new()),
            stored: Mutex::new(HashMap::new()),
            ttl: TTL,
            jitter: JITTER,
        }
    }

    /// A cache at `url`. Connects lazily, and again after any failure.
    pub fn new(url: &str) -> Result<Self, InvalidCacheUrl> {
        Ok(Self {
            valkey: Valkey::new(url).ok_or(InvalidCacheUrl)?,
            ..Self::disabled()
        })
    }

    /// Entry lifetimes other than the defaults (`COLOPHON_CACHE_TTL_MS` and
    /// `COLOPHON_CACHE_JITTER_MS`): production keeps the defaults; a load run shortens them so
    /// expiries actually land inside its window (L04's stampede). A zero jitter makes every
    /// entry live exactly `ttl`, which the run's expiry arithmetic relies on.
    #[must_use]
    pub fn with_lifetime(mut self, ttl: Duration, jitter: Duration) -> Self {
        self.ttl = ttl;
        self.jitter = jitter;
        self
    }

    /// The cached answer for `key`, if the cache has one and answers in time; and whether the
    /// cache answered at all.
    async fn get(&self, key: &str) -> (Option<Cached>, bool) {
        let Some(mut connection) = self.valkey.connection().await else {
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
            self.valkey.fail();
            (None, false)
        }
    }

    /// Stores `value` under `key` for a jittered lifetime. When Valkey took it, the moment the SET was
    /// sent, which is no later than when its lifetime began there; `None` when it didn't (too big, no
    /// connection, an error or the deadline), and a store that wasn't taken is never remembered.
    async fn put(&self, key: &str, value: &Cached, jitter: u64) -> Option<Instant> {
        if value.body.len() > MAX_CACHED_BYTES {
            return None;
        }
        let mut connection = self.valkey.connection().await?;
        // `PX`, not `EX`: the configured lifetimes are milliseconds (COLOPHON_CACHE_TTL_MS),
        // and a whole-second store would silently round a 1,500-ms TTL down to one.
        let lifetime = u64::try_from(self.ttl.as_millis().max(1)).unwrap_or(u64::MAX)
            + jitter % (u64::try_from(self.jitter.as_millis()).unwrap_or(u64::MAX) + 1);
        let sent = Instant::now();
        let stored = tokio::time::timeout(
            DEADLINE,
            redis::cmd("SET")
                .arg(key)
                .arg(value.encode())
                .arg("PX")
                .arg(lifetime)
                .query_async::<()>(&mut connection),
        )
        .await;
        if matches!(stored, Ok(Ok(()))) {
            Some(sent)
        } else {
            self.valkey.fail();
            None
        }
    }

    /// Records that this process stored `key` at `at`, and counts a refill when it had already
    /// stored it less than the shortest lifetime before: the cache lost an answer it was meant to keep.
    fn stored(&self, key: &str, at: Instant) {
        let (again, forgotten) = {
            let mut stored = self.stored.lock().unwrap_or_else(PoisonError::into_inner);
            record_fill(&mut stored, key, at, self.ttl)
        };
        // A full record is freed after the lock, not under it.
        drop(forgotten);
        if again.is_some() {
            REFILLS.note(key);
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
                if reachable
                    && let Filled::Keep(cached) = &filled
                    && let Some(at) = self.put(key, cached, jitter).await
                {
                    self.stored(key, at);
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

/// Records a store of `key` at `now`: the age of the previous store when it was less than `lifetime`
/// before (a fill the cache should have answered), and the whole record when it was full of live
/// keys and had to be forgotten, for the caller to drop after its lock. Bounded by
/// [`MAX_TRACKED_FILLS`].
fn record_fill(
    stored: &mut HashMap<String, Instant>,
    key: &str,
    now: Instant,
    lifetime: Duration,
) -> (Option<Duration>, Option<HashMap<String, Instant>>) {
    if let Some(at) = stored.get_mut(key) {
        let age = now.saturating_duration_since(*at);
        *at = now;
        return (Some(age).filter(|age| *age < lifetime), None);
    }
    let mut forgotten = None;
    if stored.len() >= MAX_TRACKED_FILLS {
        stored.retain(|_, at| now.saturating_duration_since(*at) < lifetime);
        // Mostly live keys: forget them all rather than scan nearly every one again at the next key.
        if stored.len() > MAX_TRACKED_FILLS / 4 * 3 {
            forgotten = Some(std::mem::take(stored));
        }
    }
    stored.insert(key.to_owned(), now);
    (None, forgotten)
}

/// Which kind of answer a key holds, for the duplicate-fill line (never the key: it names projects).
fn key_kind(key: &str) -> &'static str {
    if key.contains(":list:") {
        "list"
    } else if key.contains(":entry:") {
        "entry"
    } else if key.contains(":query:") {
        "query"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    use super::*;

    fn answer(body: &str) -> Cached {
        Cached {
            etag: "\"e\"".to_owned(),
            body: body.to_owned(),
        }
    }

    /// The refill answer alone (the tests that don't reach the cap).
    fn again(stored: &mut HashMap<String, Instant>, key: &str, now: Instant) -> Option<Duration> {
        let (again, forgotten) = record_fill(stored, key, now, Duration::from_secs(300));
        assert!(forgotten.is_none(), "nothing is forgotten below the cap");
        again
    }

    #[test]
    fn a_key_stored_again_within_its_lifetime_is_a_duplicate_fill_and_after_it_is_not() {
        let start = Instant::now();
        let mut stored = HashMap::new();
        assert_eq!(again(&mut stored, "k", start), None, "a first fill");
        let soon = start + Duration::from_secs(2);
        assert_eq!(
            again(&mut stored, "k", soon),
            Some(Duration::from_secs(2)),
            "refilled 2 s after it was stored: the cache lost it"
        );
        assert_eq!(again(&mut stored, "other", soon), None, "another key");
        // Measured from the latest store: a lifetime after the 2-s refill, the key had expired.
        let later = soon + Duration::from_secs(300);
        assert_eq!(again(&mut stored, "k", later), None, "expired, not lost");
    }

    #[test]
    fn the_fill_record_is_bounded_and_only_ever_forgets() {
        let lifetime = Duration::from_secs(300);
        let start = Instant::now();
        let mut stored = HashMap::new();
        for n in 0..MAX_TRACKED_FILLS {
            assert_eq!(again(&mut stored, &format!("k{n}"), start), None);
        }
        assert_eq!(stored.len(), MAX_TRACKED_FILLS);
        // Full of live keys: a new one forgets the record (handed back to drop after the lock).
        let soon = start + Duration::from_secs(1);
        let (refill, forgotten) = record_fill(&mut stored, "new", soon, lifetime);
        assert_eq!(refill, None);
        assert_eq!(forgotten.map(|all| all.len()), Some(MAX_TRACKED_FILLS));
        assert_eq!(stored.len(), 1);
        // Forgetting can miss a duplicate but never invents one: k0 was stored, now it reads as first.
        assert_eq!(again(&mut stored, "k0", soon), None);
        // Full again, of keys past their lifetime: retain drops those and nothing is forgotten.
        let mut stored = HashMap::new();
        for n in 0..MAX_TRACKED_FILLS {
            again(&mut stored, &format!("old{n}"), start);
        }
        let stored_live = start + lifetime + Duration::from_millis(999);
        let past = start + lifetime + Duration::from_secs(1);
        let (refill, forgotten) = record_fill(&mut stored, "live", stored_live, lifetime);
        assert_eq!((refill, forgotten.is_none()), (None, true));
        assert_eq!(
            stored.len(),
            1,
            "every old key expired, so retain dropped them"
        );
        assert_eq!(
            again(&mut stored, "live", past),
            Some(Duration::from_millis(1)),
            "the live key is still remembered"
        );
    }

    #[test]
    fn refills_are_counted_by_kind_and_a_window_starts_from_zero() {
        let scope = "colophon:delivery:r1:m0:p0:e0:x0:g3:u0";
        let refills = Refills::default();
        refills.note(&format!("{scope}:entry:00ff:articles"));
        refills.note(&format!("{scope}:entry:0100:articles"));
        refills.note(&format!("{scope}:list:50:-:articles"));
        assert_eq!(refills.entry.load(Ordering::Relaxed), 2);
        assert_eq!(refills.list.load(Ordering::Relaxed), 1);
        refills.close_window();
        for counter in [
            &refills.list,
            &refills.entry,
            &refills.query,
            &refills.other,
        ] {
            assert_eq!(
                counter.load(Ordering::Relaxed),
                0,
                "the next window starts from zero"
            );
        }
    }

    #[test]
    fn a_duplicate_fill_names_the_kind_of_key_never_the_key() {
        let scope = "colophon:delivery:r1:m0:p0:e0:x0:g3:u0";
        assert_eq!(key_kind(&format!("{scope}:list:50:-:articles")), "list");
        assert_eq!(key_kind(&format!("{scope}:entry:00ff:articles")), "entry");
        assert_eq!(key_kind(&format!("{scope}:query:ab:-:articles")), "query");
        assert_eq!(key_kind("something else"), "other");
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
            cache.valkey.current().err(),
            Some(false),
            "no new attempt within the backoff"
        );
    }
}
