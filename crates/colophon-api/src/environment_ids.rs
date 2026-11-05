//! Environment ids by `(project, slug)`, remembered so a public media read doesn't spend a
//! Postgres round trip on a mapping that cannot change.
//!
//! `/media/v1` object keys name the environment *id*, so the handler resolves the URL's
//! environment slug to the id on every request (`colophon_db::delivery::environment`). The
//! mapping itself is immutable: an environment's row is written once, the paths that touch it
//! later only ever change `epoch`, `generation` and `last_unpublish`, and nothing deletes an
//! environment or gives a slug a different id. A remembered answer is therefore exactly what
//! Postgres would answer, forever; the fields that do move are deliberately not what this
//! cache holds — `/content/v1` still reads them fresh on every request.
//!
//! Misses are never kept: an unknown environment costs its lookup every time, so made-up
//! slugs can't fill the map and an environment created after a miss answers on its very next
//! request. The map is bounded by entries and evicts the least recently used, and it is per
//! process — losing it costs only the next lookup.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use uuid::Uuid;

use colophon_db::{DbError, PgPool, delivery};

/// At most this many `(project, slug)` pairs: environments are a handful per project, and a
/// remembered pair — two ids and a slug of at most 63 characters — is about a hundred bytes.
const MAX_ENTRIES: usize = 1024;

/// The environment ids a media read has already resolved.
pub struct EnvironmentIds {
    max_entries: usize,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    slots: HashMap<Key, Slot>,
    /// Counts uses, so the slot with the smallest `used` is the least recently used.
    clock: u64,
}

#[derive(PartialEq, Eq, Hash)]
struct Key {
    project: Uuid,
    slug: Box<str>,
}

struct Slot {
    environment: Uuid,
    used: u64,
}

impl Default for EnvironmentIds {
    fn default() -> Self {
        Self::new()
    }
}

impl EnvironmentIds {
    /// A cache with the server's bound.
    pub fn new() -> Self {
        Self::bounded(MAX_ENTRIES)
    }

    /// A cache of at most `max_entries` pairs.
    pub fn bounded(max_entries: usize) -> Self {
        Self {
            max_entries,
            inner: Mutex::default(),
        }
    }

    /// The id `slug` names in `project`, remembered after the first lookup so only that one
    /// costs Postgres. `None` when the environment doesn't exist — nothing is kept, so the
    /// same made-up slug asks again next time rather than squatting in the map.
    ///
    /// # Errors
    ///
    /// [`DbError`] when the lookup's own query fails.
    pub async fn resolve(
        &self,
        pool: &PgPool,
        project: Uuid,
        slug: &str,
    ) -> Result<Option<Uuid>, DbError> {
        if let Some(environment) = self.known(project, slug) {
            return Ok(Some(environment));
        }
        let Some(found) = delivery::environment(pool, project, slug).await? else {
            return Ok(None);
        };
        Ok(Some(self.remember(project, slug, found.id)))
    }

    /// How many pairs are remembered; tests only — nothing answers differently on the count.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .slots
            .len()
    }

    /// The remembered id, if this `(project, slug)` was resolved before. Looking up keys the
    /// map by an owned `Key` — one small string box against the Postgres round trip it saves.
    fn known(&self, project: Uuid, slug: &str) -> Option<Uuid> {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.clock += 1;
        let now = inner.clock;
        let slot = inner.slots.get_mut(&Key {
            project,
            slug: slug.into(),
        })?;
        slot.used = now;
        Some(slot.environment)
    }

    /// Keeps `environment` as `slug`'s id in `project`, evicting the least recently used while
    /// the map is full; answers another request's copy when one inserted first.
    fn remember(&self, project: Uuid, slug: &str, environment: Uuid) -> Uuid {
        if self.max_entries == 0 {
            return environment;
        }
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.clock += 1;
        let now = inner.clock;
        if let Some(slot) = inner.slots.get_mut(&Key {
            project,
            slug: slug.into(),
        }) {
            slot.used = now;
            return slot.environment;
        }
        // Each eviction scans every slot — at most the bound's count, and only while the map
        // stays full.
        while inner.slots.len() >= self.max_entries {
            let Some(oldest) =
                inner
                    .slots
                    .iter()
                    .min_by_key(|(_, slot)| slot.used)
                    .map(|(key, _)| Key {
                        project: key.project,
                        slug: key.slug.clone(),
                    })
            else {
                break;
            };
            inner.slots.remove(&oldest);
        }
        inner.slots.insert(
            Key {
                project,
                slug: slug.into(),
            },
            Slot {
                environment,
                used: now,
            },
        );
        environment
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(number: u8) -> Uuid {
        Uuid::from_u128(u128::from(number))
    }

    #[test]
    fn a_remembered_slug_answers_its_environment() {
        let cache = EnvironmentIds::new();
        let environment = Uuid::now_v7();
        cache.remember(project(1), "production", environment);
        assert_eq!(cache.known(project(1), "production"), Some(environment));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn the_same_slug_in_another_project_is_a_different_environment() {
        let cache = EnvironmentIds::new();
        let one = Uuid::now_v7();
        let two = Uuid::now_v7();
        cache.remember(project(1), "production", one);
        cache.remember(project(2), "production", two);
        assert_eq!(cache.known(project(1), "production"), Some(one));
        assert_eq!(cache.known(project(2), "production"), Some(two));
        assert_eq!(cache.known(project(1), "staging"), None);
    }

    #[test]
    fn the_least_recently_used_pair_goes_first() {
        let cache = EnvironmentIds::bounded(2);
        let a = Uuid::now_v7();
        cache.remember(project(1), "a", a);
        cache.remember(project(1), "b", Uuid::now_v7());
        // `a` is asked for again, so `b` is now the least recently used.
        assert!(cache.known(project(1), "a").is_some());
        cache.remember(project(1), "c", Uuid::now_v7());
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.known(project(1), "a"), Some(a));
        assert_eq!(
            cache.known(project(1), "b"),
            None,
            "b was evicted, so it is resolved again"
        );
    }

    #[test]
    fn a_cache_of_no_entries_resolves_every_time() {
        let cache = EnvironmentIds::bounded(0);
        cache.remember(project(1), "production", Uuid::now_v7());
        assert_eq!(cache.known(project(1), "production"), None);
        assert_eq!(cache.len(), 0);
    }
}
