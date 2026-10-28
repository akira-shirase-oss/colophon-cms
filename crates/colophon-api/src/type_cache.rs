//! Parsed content types, kept so an entry request doesn't parse its collection's type again
//! (D-078).
//!
//! Every entry read and save needs its collection's current content type. Parsing the stored
//! definition took 2 µs for `articles`, 22 µs for the modelling page and 303 µs for a 100-field
//! type with eight patterns, half of that compiling the patterns (docs/modules/17-verification.md,
//! "First results"). This cache keeps each parsed type under the SHA-256 of the definition text it
//! was parsed from, computed here over the text just read. Parsing is a pure function of that
//! text, so a hit is exactly what parsing would return: an entry can't go stale (a new version is
//! new text) and can't carry anything across projects (the same text parses the same anywhere).
//! The hash stored beside each definition isn't used for this, since the database only checks its
//! length.
//!
//! The cache is bounded by entries and by the bytes of definition text behind them, and the least
//! recently used entry goes first. It is per process and holds nothing that isn't in a stored
//! definition, so losing it costs only the parse.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};

use colophon_domain::schema::ContentType;
use serde_json::Value;

use crate::{ApiError, entries::sha256};

/// At most this many parsed types (D-078): far more than the current types a server's busy
/// projects use at once.
const MAX_ENTRIES: usize = 1024;
/// At most this many bytes of definition text behind them (D-078). A definition larger than this
/// is parsed for each request and never kept.
const MAX_BYTES: usize = 32 << 20;

/// Parsed content types by the SHA-256 of their definition text (D-078).
pub struct ContentTypeCache {
    max_entries: usize,
    max_bytes: usize,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    slots: HashMap<[u8; 32], Slot>,
    /// The definition bytes behind every slot.
    bytes: usize,
    /// Counts uses, so the slot with the smallest `used` is the least recently used.
    clock: u64,
}

struct Slot {
    parsed: Arc<ContentType>,
    bytes: usize,
    used: u64,
}

impl Default for ContentTypeCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ContentTypeCache {
    /// A cache with the server's bounds (D-078).
    pub fn new() -> Self {
        Self::bounded(MAX_ENTRIES, MAX_BYTES)
    }

    /// A cache of at most `max_entries` types and `max_bytes` of their definition text.
    pub fn bounded(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            max_entries,
            max_bytes,
            inner: Mutex::default(),
        }
    }

    /// The content type a stored `definition` describes, parsed at most once while it stays in
    /// the cache. Stored definitions were validated when written, so failing to parse one is a
    /// bug: an internal error, and nothing is kept.
    ///
    /// # Errors
    ///
    /// [`ApiError::Internal`] when `definition` isn't JSON or isn't a valid content type.
    pub fn parse(&self, definition: &str) -> Result<Arc<ContentType>, ApiError> {
        let key = sha256(definition.as_bytes());
        if let Some(parsed) = self.get(&key) {
            return Ok(parsed);
        }
        // Parsed outside the lock: two requests that miss together both parse, and the second
        // insert finds the first's.
        let value: Value = serde_json::from_str(definition).map_err(ApiError::internal)?;
        let parsed = Arc::new(ContentType::parse(&value).map_err(ApiError::internal)?);
        Ok(self.insert(key, parsed, definition.len()))
    }

    /// How many types are cached, and the definition bytes behind them.
    pub fn len_and_bytes(&self) -> (usize, usize) {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        (inner.slots.len(), inner.bytes)
    }

    fn get(&self, key: &[u8; 32]) -> Option<Arc<ContentType>> {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.clock += 1;
        let now = inner.clock;
        let slot = inner.slots.get_mut(key)?;
        slot.used = now;
        Some(Arc::clone(&slot.parsed))
    }

    /// Keeps `parsed` unless it alone exceeds a bound, evicting the least recently used until it
    /// fits; answers the cached copy if another request inserted one first.
    fn insert(&self, key: [u8; 32], parsed: Arc<ContentType>, bytes: usize) -> Arc<ContentType> {
        if self.max_entries == 0 || bytes > self.max_bytes {
            return parsed;
        }
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.clock += 1;
        let now = inner.clock;
        if let Some(slot) = inner.slots.get_mut(&key) {
            slot.used = now;
            return Arc::clone(&slot.parsed);
        }
        // Each eviction scans every slot. Bytes can force several only when slots average more
        // than max_bytes / max_entries (32 KiB), so a definition within the 1 MiB request limit
        // evicts at most about 32 for bytes.
        while inner.slots.len() >= self.max_entries || inner.bytes + bytes > self.max_bytes {
            let Some(oldest) = inner
                .slots
                .iter()
                .min_by_key(|(_, slot)| slot.used)
                .map(|(key, _)| *key)
            else {
                break;
            };
            if let Some(evicted) = inner.slots.remove(&oldest) {
                inner.bytes -= evicted.bytes;
            }
        }
        inner.bytes += bytes;
        inner.slots.insert(
            key,
            Slot {
                parsed: Arc::clone(&parsed),
                bytes,
                used: now,
            },
        );
        parsed
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn definition(name: &str) -> String {
        json!({
            "name": name,
            "title_field": "title",
            "fields": [
                {"id": "title", "label": "Title", "type": "text", "required": true, "max_chars": 200},
                {"id": "code", "label": "Code", "type": "text", "max_chars": 20, "pattern": "[A-Z]{3}-[0-9]{4}"}
            ]
        })
        .to_string()
    }

    #[test]
    fn the_same_text_is_parsed_once_and_answers_what_parsing_would() {
        let cache = ContentTypeCache::new();
        let text = definition("Products");
        let first = cache.parse(&text).expect("valid");
        let second = cache.parse(&text).expect("valid");
        assert!(
            Arc::ptr_eq(&first, &second),
            "the second read is the cached copy"
        );
        let fresh = ContentType::parse(&serde_json::from_str(&text).expect("JSON")).expect("valid");
        assert_eq!(second.canonical_json(), fresh.canonical_json());
        assert_eq!(cache.len_and_bytes(), (1, text.len()));
    }

    #[test]
    fn different_text_is_a_different_type_even_for_the_same_collection() {
        let cache = ContentTypeCache::new();
        let before = cache.parse(&definition("Products")).expect("valid");
        let after = cache.parse(&definition("Goods")).expect("valid");
        assert!(!Arc::ptr_eq(&before, &after));
        assert_eq!(before.name(), "Products");
        assert_eq!(after.name(), "Goods");
    }

    #[test]
    fn the_least_recently_used_type_goes_first() {
        let cache = ContentTypeCache::bounded(2, usize::MAX);
        let a = cache.parse(&definition("A")).expect("valid");
        cache.parse(&definition("B")).expect("valid");
        // A is used again, so B is now the least recently used.
        cache.parse(&definition("A")).expect("valid");
        cache.parse(&definition("C")).expect("valid");
        assert_eq!(cache.len_and_bytes().0, 2);
        assert!(Arc::ptr_eq(
            &a,
            &cache.parse(&definition("A")).expect("valid")
        ));
        let b = cache.parse(&definition("B")).expect("valid");
        assert_eq!(b.name(), "B", "B was evicted, so it was parsed again");
        assert_eq!(cache.len_and_bytes().0, 2);
    }

    #[test]
    fn bytes_bound_the_cache_and_a_definition_over_the_bound_is_never_kept() {
        let small = definition("A");
        let cache = ContentTypeCache::bounded(100, small.len() * 2);
        cache.parse(&definition("A")).expect("valid");
        cache.parse(&definition("B")).expect("valid");
        cache.parse(&definition("C")).expect("valid");
        let (entries, bytes) = cache.len_and_bytes();
        assert_eq!(entries, 2, "the third evicted the first");
        assert!(bytes <= small.len() * 2);

        let tiny = ContentTypeCache::bounded(100, 10);
        let parsed = tiny.parse(&small).expect("valid");
        assert_eq!(parsed.name(), "A", "still answered");
        assert_eq!(tiny.len_and_bytes(), (0, 0), "but not kept");
    }

    #[test]
    fn a_definition_that_does_not_parse_is_an_internal_error_and_is_not_kept() {
        let cache = ContentTypeCache::new();
        for text in ["not json", r#"{"name": "No fields"}"#] {
            assert!(matches!(cache.parse(text), Err(ApiError::Internal(_))));
        }
        assert_eq!(cache.len_and_bytes(), (0, 0));
    }

    #[test]
    fn a_cache_of_no_entries_parses_every_time() {
        let cache = ContentTypeCache::bounded(0, usize::MAX);
        let text = definition("A");
        let first = cache.parse(&text).expect("valid");
        let second = cache.parse(&text).expect("valid");
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(cache.len_and_bytes(), (0, 0));
    }
}
