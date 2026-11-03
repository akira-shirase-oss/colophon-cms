//! Object-store keys. The bucket belongs to the installation; keys namespace every object by
//! its stage (`quarantine/` vs `public/`, M4–M5) and its project and environment so a storage
//! browser can't confuse what may be served. The storage name is random at intent time — an
//! uploader never picks an object's key.

use std::sync::Arc;

/// The shared object store: `object_store`'s trait object, an S3 bucket live and
/// `object_store::memory::InMemory` in tests.
pub type Shared = Arc<dyn object_store::ObjectStore>;

/// Where a not-yet-verified upload waits. The random storage name keeps an uploader from
/// choosing a key (or guessing another upload's).
pub fn quarantine_key(project: &str, environment: &str, storage_name: &str) -> String {
    format!("quarantine/{project}/{environment}/{storage_name}")
}

/// Where a verified object is served from: one stable name per asset, written only when the
/// asset turns usable. No `public/` object ever exists for a quarantined or rejected asset.
pub fn public_key(project: &str, environment: &str, asset: &str) -> String {
    format!("public/{project}/{environment}/{asset}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_namespace_stages() {
        assert_eq!(
            quarantine_key("p", "production", "deadbeef"),
            "quarantine/p/production/deadbeef"
        );
        assert_eq!(
            public_key("p", "production", "a1"),
            "public/p/production/a1"
        );
    }
}
