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
/// Pre-versioning layout (D-136): the first 50 migrations wrote the original straight here;
/// rows landed since carry their versioned key, so this stays only for backfilled assets.
pub fn public_key(project: &str, environment: &str, asset: &str) -> String {
    format!("public/{project}/{environment}/{asset}")
}

/// Where a verified *version* lives (D-136): the asset's stable prefix, then the stripped
/// bytes' digest — the content names the object, so a new version can never overwrite an
/// older one, and a crashed promote's retry puts the same bytes under the same key. Renditions
/// sit under it (`{version_key}/{name}`). The whole prefix `public/{project}/{environment}/
/// {asset}` is what a delete removes, listing it rather than trusting the recorded rows.
pub fn version_key(project: &str, environment: &str, asset: &str, sha256_hex: &str) -> String {
    format!("public/{project}/{environment}/{asset}/{sha256_hex}")
}

/// The 64 lowercase hex characters a digest spells, in a key or a `…/{asset}/{sha}` URL.
pub fn sha_hex(sha256: &[u8]) -> String {
    use std::fmt::Write as _;

    sha256.iter().fold(String::with_capacity(64), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// Whether `name` spells a content address — the segment of a pinned media URL
/// (`/media/v1/…/{asset}/{sha}`) that selects a version rather than a rendition.
pub fn is_version_sha(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| b.is_ascii_hexdigit())
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
        assert_eq!(
            version_key("p", "production", "a1", "ab12"),
            "public/p/production/a1/ab12"
        );
    }

    #[test]
    fn a_version_sha_is_64_hex() {
        assert!(is_version_sha(&"a".repeat(64)));
        assert!(is_version_sha(&"0123456789abcdef".repeat(4)));
        assert!(!is_version_sha("w320"));
        assert!(!is_version_sha(&"g".repeat(64)));
        assert!(!is_version_sha(&"a".repeat(63)));
    }
}
