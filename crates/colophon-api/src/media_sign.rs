//! Short-lived signed media addresses (D-229). Media is private until it is published, so an answer that shows
//! unpublished or members-only content — a preview, a signed-in site member's read — signs the media addresses it
//! names: `?expires=<unix seconds>&signature=<base64url HMAC-SHA256>` over the address's path and its expiry, with
//! the installation's one key (migration 0082, read once per process). An address works for
//! `SIGNED_FOR_SECS`, then answers 404 like any unknown asset; its bytes are served `private, no-store`, so no
//! shared cache keeps them.

use aws_lc_rs::hmac;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use colophon_db::PgPool;

use crate::error::ApiError;

/// How long a signed address works: long enough to render a preview and load its images, short enough that a
/// copied link stops working the same day.
pub(crate) const SIGNED_FOR_SECS: i64 = 15 * 60;

/// The installation's signing key, read from the database on first use and kept: every server reads the same row,
/// so an address one server signs, another verifies.
#[derive(Default)]
pub struct Keys(tokio::sync::OnceCell<std::sync::Arc<hmac::Key>>);

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keys([REDACTED])")
    }
}

impl Keys {
    pub(crate) async fn key(&self, pool: &PgPool) -> Result<&std::sync::Arc<hmac::Key>, ApiError> {
        self.0
            .get_or_try_init(|| async {
                let bytes = colophon_db::delivery::media_signing_key(pool)
                    .await
                    .map_err(ApiError::from_db)?;
                Ok(std::sync::Arc::new(hmac::Key::new(
                    hmac::HMAC_SHA256,
                    &bytes,
                )))
            })
            .await
    }
}

fn message(path: &str, expires: i64) -> String {
    format!("colophon-media-v1\n{path}\n{expires}")
}

/// The query string that signs `path` (`/media/v1/…`, exactly as it is requested) until `expires`.
pub(crate) fn query(key: &hmac::Key, path: &str, expires: i64) -> String {
    let tag = hmac::sign(key, message(path, expires).as_bytes());
    format!(
        "expires={expires}&signature={}",
        URL_SAFE_NO_PAD.encode(tag.as_ref())
    )
}

/// Whether `raw_query` signs `path` and hasn't expired at `now`: the expiry no further ahead than an address is
/// ever signed for, the signature checked in constant time.
pub(crate) fn verifies(key: &hmac::Key, path: &str, raw_query: Option<&str>, now: i64) -> bool {
    let Some(raw) = raw_query else {
        return false;
    };
    let mut expires = None;
    let mut signature = None;
    for (name, value) in url::form_urlencoded::parse(raw.as_bytes()) {
        match name.as_ref() {
            "expires" if expires.is_none() => expires = value.parse::<i64>().ok(),
            "signature" if signature.is_none() => {
                signature = URL_SAFE_NO_PAD.decode(value.as_bytes()).ok();
            }
            _ => {}
        }
    }
    let (Some(expires), Some(signature)) = (expires, signature) else {
        return false;
    };
    if expires < now || expires > now.saturating_add(SIGNED_FOR_SECS + 60) {
        return false;
    }
    hmac::verify(key, message(path, expires).as_bytes(), &signature).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> hmac::Key {
        hmac::Key::new(hmac::HMAC_SHA256, &[byte; 32])
    }

    const PATH: &str = "/media/v1/0192a8f0-0000-7000-8000-000000000001/production/0192a8f0-0000-7000-8000-000000000002";

    #[test]
    fn a_signed_address_works_until_it_expires_and_only_for_its_path() {
        let now = 1_800_000_000;
        let signed = query(&key(1), PATH, now + SIGNED_FOR_SECS);
        assert!(verifies(&key(1), PATH, Some(&signed), now));
        assert!(verifies(
            &key(1),
            PATH,
            Some(&signed),
            now + SIGNED_FOR_SECS
        ));
        // Expired, another path, a rendition of it, another key, or no signature at all.
        assert!(!verifies(
            &key(1),
            PATH,
            Some(&signed),
            now + SIGNED_FOR_SECS + 1
        ));
        assert!(!verifies(
            &key(1),
            &format!("{PATH}/w320"),
            Some(&signed),
            now
        ));
        assert!(!verifies(&key(2), PATH, Some(&signed), now));
        assert!(!verifies(&key(1), PATH, None, now));
        assert!(!verifies(&key(1), PATH, Some("expires=1"), now));
    }

    #[test]
    fn an_expiry_can_be_neither_moved_nor_set_far_ahead() {
        let now = 1_800_000_000;
        let signed = query(&key(1), PATH, now + 60);
        let moved = signed.replace(&(now + 60).to_string(), &(now + 600).to_string());
        assert!(!verifies(&key(1), PATH, Some(&moved), now));
        // Signed for a year with the real key: still refused, so a leaked key's addresses can't outlive it long.
        let year = query(&key(1), PATH, now + 365 * 24 * 3600);
        assert!(!verifies(&key(1), PATH, Some(&year), now));
        // A second `signature` or `expires` doesn't replace the first.
        let doubled = format!("{signed}&signature=AAAA&expires={}", now + 600);
        assert!(verifies(&key(1), PATH, Some(&doubled), now));
    }
}
