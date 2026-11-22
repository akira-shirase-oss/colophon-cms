//! Short-lived signed media addresses (D-229, D-233). Media is private until it is published, so an answer that
//! shows unpublished or members-only content — a preview, a signed-in site member's read — signs the media
//! addresses it names: `?kid=<key id>&expires=<unix seconds>&signature=<base64url HMAC-SHA256>` over the key id,
//! the address's purpose, its path and its expiry. An address works for `SIGNED_FOR_SECS`, then answers 404 like
//! any unknown asset; its bytes are served `private, no-store`, so no shared cache keeps them.
//!
//! The purpose is signed, not sent: a preview's address may name the bare address, which serves the asset's
//! current version, draft or not; a site member's may name only a version a release recorded. The origin checks
//! a bare address against the preview purpose alone, so a member's signature never opens the newest upload
//! (review S1, 2026-11-21).
//!
//! Keys rotate (D-233, migration 0086): the current key (the highest id) signs, it and the one before it verify.
//! Each server keeps the live keys for `KEYS_FRESH_FOR`, so a rotation reaches every server's signing, and a
//! retired key leaves every server's verifying, within a minute; an address naming a key id this server hasn't
//! seen reloads at once, at most once per `UNKNOWN_KID_EVERY`, so a burst of forged ids costs Postgres one read a
//! second (and may delay a just-made key's first links on another server by up to that second).

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use aws_lc_rs::hmac;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use colophon_db::PgPool;

use crate::error::ApiError;

/// How long a signed address works: long enough to render a preview and load its images, short enough that a
/// copied link stops working the same day.
pub(crate) const SIGNED_FOR_SECS: i64 = 15 * 60;

/// How long a server keeps the keys it read before reading them again: the bound on a rotation reaching it.
pub(crate) const KEYS_FRESH_FOR: Duration = Duration::from_secs(60);

/// How often an unknown key id may make a server read the keys again.
pub(crate) const UNKNOWN_KID_EVERY: Duration = Duration::from_secs(1);

/// One live key: its id and the HMAC key.
pub(crate) struct SigningKey {
    pub(crate) id: u32,
    key: hmac::Key,
}

/// The live keys, the current one (the one that signs) first. Never empty.
pub(crate) struct KeySet(Vec<SigningKey>);

impl KeySet {
    pub(crate) fn new(keys: Vec<(u32, Vec<u8>)>) -> Option<Self> {
        let mut keys: Vec<SigningKey> = keys
            .into_iter()
            .map(|(id, bytes)| SigningKey {
                id,
                key: hmac::Key::new(hmac::HMAC_SHA256, &bytes),
            })
            .collect();
        keys.sort_by_key(|key| std::cmp::Reverse(key.id));
        (!keys.is_empty()).then_some(Self(keys))
    }

    /// The key that signs.
    pub(crate) fn current(&self) -> &SigningKey {
        &self.0[0]
    }

    fn by_id(&self, id: u32) -> Option<&SigningKey> {
        self.0.iter().find(|key| key.id == id)
    }

    fn ids(&self) -> Vec<u32> {
        self.0.iter().map(|key| key.id).collect()
    }
}

struct Loaded {
    keys: Arc<KeySet>,
    /// When `keys` last changed or was read for its age: what `KEYS_FRESH_FOR` counts from.
    fresh_at: Instant,
    /// The read sequence number `keys` came from: a slower, older read never replaces a newer one.
    sequence: u64,
}

/// The installation's live signing keys, read from the database and kept a minute (D-233): every server reads
/// the same rows, so an address one server signs, another verifies.
#[derive(Default)]
pub struct Keys {
    loaded: Mutex<Option<Loaded>>,
    /// The next read's sequence number.
    reads: std::sync::atomic::AtomicU64,
    /// When an unknown key id last made this server read the keys.
    unknown_at: Mutex<Option<Instant>>,
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keys([REDACTED])")
    }
}

impl Keys {
    /// The live keys, read again once they are `KEYS_FRESH_FOR` old.
    pub(crate) async fn current(&self, pool: &PgPool) -> Result<Arc<KeySet>, ApiError> {
        if let Some(keys) = self.fresh(Instant::now()) {
            return Ok(keys);
        }
        self.read(pool, true).await
    }

    /// The keys again, for an address naming a key id they don't hold: at most once per `UNKNOWN_KID_EVERY`, and a
    /// read that finds the same keys doesn't count as fresh, so forged ids can't keep a retired key alive.
    pub(crate) async fn for_unknown_kid(
        &self,
        pool: &PgPool,
    ) -> Result<Option<Arc<KeySet>>, ApiError> {
        if !self.unknown_allowed(Instant::now()) {
            return Ok(None);
        }
        self.read(pool, false).await.map(Some)
    }

    /// Whether an unknown key id may read the keys at `now`: once per `UNKNOWN_KID_EVERY`, per server.
    fn unknown_allowed(&self, now: Instant) -> bool {
        let mut last = lock(&self.unknown_at);
        if last.is_some_and(|at| now.duration_since(at) < UNKNOWN_KID_EVERY) {
            return false;
        }
        *last = Some(now);
        true
    }

    fn fresh(&self, now: Instant) -> Option<Arc<KeySet>> {
        lock(&self.loaded)
            .as_ref()
            .filter(|loaded| now.duration_since(loaded.fresh_at) < KEYS_FRESH_FOR)
            .map(|loaded| Arc::clone(&loaded.keys))
    }

    /// Reads the keys; `ages` says the read restarts the freshness clock even when nothing changed (the minute's
    /// own read does, an unknown id's doesn't).
    async fn read(&self, pool: &PgPool, ages: bool) -> Result<Arc<KeySet>, ApiError> {
        self.read_with(ages, || async {
            let rows = colophon_db::delivery::media_signing_keys(pool)
                .await
                .map_err(ApiError::from_db)?;
            Ok(rows
                .into_iter()
                .filter_map(|row| Some((u32::try_from(row.id).ok()?, row.key)))
                .collect())
        })
        .await
    }

    /// [`Self::read`] with the database read passed in (tests stand in for it).
    async fn read_with<F, Fut>(&self, ages: bool, fetch: F) -> Result<Arc<KeySet>, ApiError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Vec<(u32, Vec<u8>)>, ApiError>>,
    {
        let sequence = self
            .reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let read = KeySet::new(fetch().await?)
            .ok_or(ApiError::Unavailable("no media signing key is configured"))?;
        Ok(self.store(Arc::new(read), sequence, ages, Instant::now()))
    }

    fn store(&self, read: Arc<KeySet>, sequence: u64, ages: bool, now: Instant) -> Arc<KeySet> {
        let mut loaded = lock(&self.loaded);
        match loaded.as_mut() {
            // A newer read already landed: keep it.
            Some(newer) if newer.sequence > sequence => Arc::clone(&newer.keys),
            Some(held) => {
                if ages || held.keys.ids() != read.ids() {
                    held.fresh_at = now;
                }
                held.keys = Arc::clone(&read);
                held.sequence = sequence;
                read
            }
            None => {
                *loaded = Some(Loaded {
                    keys: Arc::clone(&read),
                    fresh_at: now,
                    sequence,
                });
                read
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What a signed address is for, and so what it may open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Purpose {
    /// A preview's (a signed-in previewer's answer): any address, the bare one's current version included.
    Preview,
    /// A site member's delivery answer: only a version's own address, which a release recorded.
    Member,
}

impl Purpose {
    const fn name(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Member => "member",
        }
    }
}

fn message(kid: u32, purpose: Purpose, path: &str, expires: i64) -> String {
    format!(
        "colophon-media-v2\n{kid}\n{}\n{path}\n{expires}",
        purpose.name()
    )
}

/// The query string that signs `path` (`/media/v1/…`, exactly as it is requested) with `key` for `purpose` until
/// `expires`.
pub(crate) fn query(key: &SigningKey, purpose: Purpose, path: &str, expires: i64) -> String {
    let tag = hmac::sign(&key.key, message(key.id, purpose, path, expires).as_bytes());
    format!(
        "kid={}&expires={expires}&signature={}",
        key.id,
        URL_SAFE_NO_PAD.encode(tag.as_ref())
    )
}

/// What a signed address's query says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Signed by a live key, for one of the purposes asked, and not expired.
    Valid,
    /// Not a valid signature, whatever the keys.
    Invalid,
    /// Well formed, but naming a key id these keys don't hold: worth one read of the keys.
    UnknownKey(u32),
}

/// A decimal key id as signing writes it: digits only, no leading zero, within a `u32`.
fn kid(text: &str) -> Option<u32> {
    if text.is_empty() || text.len() > 10 || text.starts_with('0') {
        return None;
    }
    if !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Whether `raw_query` signs `path` for one of `purposes` with one of `keys` and hasn't expired at `now`: one `kid`
/// (a second is refused), the first `expires` and `signature`, the expiry no further ahead than an address is
/// ever signed for, each signature checked in constant time. An address without a `kid` (one signed before D-233)
/// is refused.
pub(crate) fn verifies(
    keys: &KeySet,
    purposes: &[Purpose],
    path: &str,
    raw_query: Option<&str>,
    now: i64,
) -> Verdict {
    let Some(raw) = raw_query else {
        return Verdict::Invalid;
    };
    let mut key_id: Option<Option<u32>> = None;
    let mut expires = None;
    let mut signature = None;
    for (name, value) in url::form_urlencoded::parse(raw.as_bytes()) {
        match name.as_ref() {
            "kid" if key_id.is_some() => return Verdict::Invalid,
            "kid" => key_id = Some(kid(&value)),
            "expires" if expires.is_none() => expires = value.parse::<i64>().ok(),
            "signature" if signature.is_none() => {
                signature = URL_SAFE_NO_PAD.decode(value.as_bytes()).ok();
            }
            _ => {}
        }
    }
    let (Some(Some(key_id)), Some(expires), Some(signature)) = (key_id, expires, signature) else {
        return Verdict::Invalid;
    };
    if expires < now || expires > now.saturating_add(SIGNED_FOR_SECS + 60) {
        return Verdict::Invalid;
    }
    let Some(key) = keys.by_id(key_id) else {
        return Verdict::UnknownKey(key_id);
    };
    let valid = purposes.iter().any(|purpose| {
        hmac::verify(
            &key.key,
            message(key_id, *purpose, path, expires).as_bytes(),
            &signature,
        )
        .is_ok()
    });
    if valid {
        Verdict::Valid
    } else {
        Verdict::Invalid
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn keys(ids: &[(u32, u8)]) -> KeySet {
        KeySet::new(
            ids.iter()
                .map(|(id, byte)| (*id, vec![*byte; 32]))
                .collect(),
        )
        .expect("keys")
    }

    const PATH: &str = "/media/v1/0192a8f0-0000-7000-8000-000000000001/production/0192a8f0-0000-7000-8000-000000000002";
    const PREVIEW: &[Purpose] = &[Purpose::Preview];
    const ANY: &[Purpose] = &[Purpose::Preview, Purpose::Member];
    const NOW: i64 = 1_800_000_000;

    fn signed(set: &KeySet, purpose: Purpose, expires: i64) -> String {
        query(set.current(), purpose, PATH, expires)
    }

    #[test]
    fn a_signed_address_works_until_it_expires_and_only_for_its_path() {
        let set = keys(&[(1, 1)]);
        let address = signed(&set, Purpose::Preview, NOW + SIGNED_FOR_SECS);
        assert!(address.starts_with("kid=1&"), "{address}");
        let check = |set: &KeySet, path: &str, query: Option<&str>, now: i64| {
            verifies(set, PREVIEW, path, query, now)
        };
        assert_eq!(check(&set, PATH, Some(&address), NOW), Verdict::Valid);
        assert_eq!(
            check(&set, PATH, Some(&address), NOW + SIGNED_FOR_SECS),
            Verdict::Valid
        );
        // Expired, another path, a rendition of it, another key under the same id, or no signature at all.
        assert_eq!(
            check(&set, PATH, Some(&address), NOW + SIGNED_FOR_SECS + 1),
            Verdict::Invalid
        );
        assert_eq!(
            check(&set, &format!("{PATH}/w320"), Some(&address), NOW),
            Verdict::Invalid
        );
        assert_eq!(
            check(&keys(&[(1, 2)]), PATH, Some(&address), NOW),
            Verdict::Invalid
        );
        assert_eq!(check(&set, PATH, None, NOW), Verdict::Invalid);
        assert_eq!(
            check(&set, PATH, Some("kid=1&expires=1"), NOW),
            Verdict::Invalid
        );
    }

    /// Review S1 (2026-11-21): a member's signature is not a preview's. Where only a preview may open an
    /// address (the bare one, which serves the newest upload), a member's signature on it is refused.
    #[test]
    fn a_signature_opens_only_what_its_purpose_may() {
        let set = keys(&[(1, 1)]);
        let member = signed(&set, Purpose::Member, NOW + 60);
        assert_eq!(
            verifies(&set, PREVIEW, PATH, Some(&member), NOW),
            Verdict::Invalid
        );
        assert_eq!(
            verifies(&set, ANY, PATH, Some(&member), NOW),
            Verdict::Valid
        );
        let preview = signed(&set, Purpose::Preview, NOW + 60);
        assert_eq!(
            verifies(&set, ANY, PATH, Some(&preview), NOW),
            Verdict::Valid
        );
        assert_eq!(
            verifies(&set, &[Purpose::Member], PATH, Some(&preview), NOW),
            Verdict::Invalid
        );
        assert_eq!(
            verifies(&set, &[], PATH, Some(&preview), NOW),
            Verdict::Invalid
        );
    }

    #[test]
    fn an_expiry_can_be_neither_moved_nor_set_far_ahead() {
        let set = keys(&[(1, 1)]);
        let address = signed(&set, Purpose::Preview, NOW + 60);
        let moved = address.replace(&(NOW + 60).to_string(), &(NOW + 600).to_string());
        assert_eq!(
            verifies(&set, PREVIEW, PATH, Some(&moved), NOW),
            Verdict::Invalid
        );
        // Signed for a year with the real key: still refused, so a leaked key's addresses can't outlive it long.
        let year = signed(&set, Purpose::Preview, NOW + 365 * 24 * 3600);
        assert_eq!(
            verifies(&set, PREVIEW, PATH, Some(&year), NOW),
            Verdict::Invalid
        );
        // A second `signature` or `expires` doesn't replace the first.
        let doubled = format!("{address}&signature=AAAA&expires={}", NOW + 600);
        assert_eq!(
            verifies(&set, PREVIEW, PATH, Some(&doubled), NOW),
            Verdict::Valid
        );
    }

    /// D-233: the current key signs; it and the one before it verify; a key that left the set doesn't.
    #[test]
    fn a_rotation_keeps_the_previous_key_verifying_and_retires_the_one_before() {
        let before = keys(&[(1, 1)]);
        let old = signed(&before, Purpose::Preview, NOW + 60);
        let rotated = keys(&[(1, 1), (2, 2)]);
        assert_eq!(rotated.current().id, 2, "the highest id signs");
        assert!(signed(&rotated, Purpose::Preview, NOW + 60).starts_with("kid=2&"));
        assert_eq!(
            verifies(&rotated, PREVIEW, PATH, Some(&old), NOW),
            Verdict::Valid
        );
        // Rotated again (or `--retire-previous`): key 1 is gone, and its address names an id nobody holds.
        let again = keys(&[(2, 2), (3, 3)]);
        assert_eq!(
            verifies(&again, PREVIEW, PATH, Some(&old), NOW),
            Verdict::UnknownKey(1)
        );
    }

    /// A key id is read as signing writes it, once: no leading zero, digits only, within a `u32`, never twice,
    /// and never missing (an address signed before D-233 isn't read as key 1).
    #[test]
    fn a_key_id_is_read_strictly() {
        let set = keys(&[(1, 1), (12, 2)]);
        let good = signed(&set, Purpose::Preview, NOW + 60);
        assert_eq!(
            verifies(&set, PREVIEW, PATH, Some(&good), NOW),
            Verdict::Valid
        );
        let rest = good.strip_prefix("kid=12").expect("kid first");
        for bad in [
            format!("kid=012{rest}"),
            format!("kid=+12{rest}"),
            format!("kid=12.0{rest}"),
            format!("kid={rest}"),
            format!("kid=99999999999{rest}"),
            format!("kid=12{rest}&kid=12"),
            format!("kid=12&kid=1{rest}"),
            rest.trim_start_matches('&').to_owned(),
        ] {
            assert_eq!(
                verifies(&set, PREVIEW, PATH, Some(&bad), NOW),
                Verdict::Invalid,
                "{bad}"
            );
        }
        // A well-formed id this server doesn't hold is worth one read of the keys, nothing more.
        let unknown = good.replacen("kid=12", "kid=13", 1);
        assert_eq!(
            verifies(&set, PREVIEW, PATH, Some(&unknown), NOW),
            Verdict::UnknownKey(13)
        );
        // The id is signed: another live key's id on this signature is refused.
        let swapped = good.replacen("kid=12", "kid=1", 1);
        assert_eq!(
            verifies(&set, PREVIEW, PATH, Some(&swapped), NOW),
            Verdict::Invalid
        );
    }

    #[test]
    fn an_unknown_key_id_reads_the_keys_at_most_once_a_second() {
        let cache = Keys::default();
        let start = Instant::now();
        assert!(cache.unknown_allowed(start));
        assert!(!cache.unknown_allowed(start + Duration::from_millis(10)));
        assert!(!cache.unknown_allowed(start + Duration::from_millis(999)));
        assert!(cache.unknown_allowed(start + UNKNOWN_KID_EVERY));
    }

    /// A read that started earlier never replaces one that started later; a read for an unknown id that finds
    /// the same keys doesn't restart the minute, so forged ids can't keep a retired key verifying.
    #[test]
    fn the_cache_keeps_the_newest_read_and_ages_out_on_time() {
        let cache = Keys::default();
        let start = Instant::now();
        let one = Arc::new(keys(&[(1, 1)]));
        let two = Arc::new(keys(&[(1, 1), (2, 2)]));
        cache.store(Arc::clone(&two), 5, true, start);
        let kept = cache.store(Arc::clone(&one), 4, true, start);
        assert_eq!(
            kept.current().id,
            2,
            "an older read lost the race and was dropped"
        );
        assert_eq!(cache.fresh(start).map(|set| set.current().id), Some(2));
        // An unknown id's read finding the same keys: the clock still runs from `start`.
        cache.store(
            Arc::new(keys(&[(1, 1), (2, 2)])),
            6,
            false,
            start + Duration::from_secs(59),
        );
        assert!(
            cache.fresh(start + KEYS_FRESH_FOR).is_none(),
            "aged out on time"
        );
        // A read that finds a change counts as fresh, whatever asked for it.
        cache.store(
            Arc::new(keys(&[(2, 2), (3, 3)])),
            7,
            false,
            start + Duration::from_secs(61),
        );
        assert_eq!(
            cache
                .fresh(start + Duration::from_secs(62))
                .map(|set| set.current().id),
            Some(3)
        );
    }

    /// White-hat (D-233): a read's snapshot is taken when its query runs, not when it was asked for. Read A, asked
    /// first, waits for a connection; read B, asked second, runs at once (before a `--retire-previous` rotation)
    /// and returns last. B must not put the retired keys back, fresh for another minute, over A's newer read.
    #[tokio::test]
    async fn a_read_never_puts_back_keys_an_earlier_snapshot_held() {
        use tokio::sync::Notify;

        let cache = Arc::new(Keys::default());
        let database = Arc::new(Mutex::new(vec![(1_u32, vec![1_u8; 32]), (2, vec![2; 32])]));
        let a_runs = Arc::new(Notify::new());
        let b_returns = Arc::new(Notify::new());
        let settle = || async {
            for _ in 0..16 {
                tokio::task::yield_now().await;
            }
        };

        let a = tokio::spawn({
            let (cache, database, a_runs) = (
                Arc::clone(&cache),
                Arc::clone(&database),
                Arc::clone(&a_runs),
            );
            async move {
                cache
                    .read_with(true, || async move {
                        a_runs.notified().await;
                        let rows = lock(&database).clone();
                        Ok(rows)
                    })
                    .await
            }
        });
        settle().await;
        let b = tokio::spawn({
            let (cache, database, b_returns) = (
                Arc::clone(&cache),
                Arc::clone(&database),
                Arc::clone(&b_returns),
            );
            async move {
                cache
                    .read_with(true, || async move {
                        let rows = lock(&database).clone();
                        b_returns.notified().await;
                        Ok(rows)
                    })
                    .await
            }
        });
        settle().await;
        // `colophon admin media-key rotate --retire-previous`: keys 1 and 2 are gone.
        *lock(&database) = vec![(3, vec![3; 32])];
        a_runs.notify_one();
        settle().await;
        b_returns.notify_one();
        a.await.expect("a joined").expect("a read");
        b.await.expect("b joined").expect("b read");

        let held = cache.fresh(Instant::now()).expect("fresh keys");
        assert_eq!(held.ids(), [3], "a retired key came back");
        assert!(held.by_id(2).is_none() && held.by_id(1).is_none());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        /// Never panics on any query string or path a client sends, and never accepts one it didn't sign.
        #[test]
        fn hostile_queries_never_panic_or_pass(raw in ".{0,300}", path in "/[ -~]{0,120}", now in any::<i64>()) {
            prop_assert_ne!(verifies(&keys(&[(1, 1)]), ANY, &path, Some(&raw), now), Verdict::Valid);
        }

        /// A real signature with one byte of its query changed is refused, wherever the byte is.
        #[test]
        fn a_changed_byte_is_refused(at in 0_usize..200, byte in any::<u8>()) {
            let set = keys(&[(1, 1), (2, 2)]);
            let address = signed(&set, Purpose::Member, NOW + 60);
            let mut bytes = address.clone().into_bytes();
            let at = at % bytes.len();
            prop_assume!(bytes[at] != byte);
            bytes[at] = byte;
            let changed = String::from_utf8_lossy(&bytes).into_owned();
            // A byte that decodes to the same query (an `&` that only adds an empty pair) may still pass;
            // anything that changes what is verified may not.
            let same = url::form_urlencoded::parse(changed.as_bytes()).collect::<Vec<_>>()
                == url::form_urlencoded::parse(address.as_bytes()).collect::<Vec<_>>();
            prop_assert!(same || verifies(&set, ANY, PATH, Some(&changed), NOW) != Verdict::Valid);
        }
    }
}
