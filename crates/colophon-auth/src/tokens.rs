//! Random tokens, their hashes, and PKCE. One crypto library: aws-lc-rs, the same one the TLS
//! stack uses.

use aws_lc_rs::{constant_time, digest, rand::SecureRandom as _};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

/// 256 bits from the operating system's secure random source, as 43 URL-safe characters. Used for
/// session and CSRF tokens, OIDC state and nonce, and PKCE verifiers.
pub fn random_token() -> Result<String, aws_lc_rs::error::Unspecified> {
    let mut bytes = [0_u8; 32];
    aws_lc_rs::rand::SystemRandom::new().fill(&mut bytes)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// SHA-256 of a token: what the database stores instead of the token.
pub fn hash(token: &str) -> [u8; 32] {
    let digest = digest::digest(&digest::SHA256, token.as_bytes());
    let mut out = [0_u8; 32];
    out.copy_from_slice(digest.as_ref());
    out
}

/// The PKCE `S256` challenge for a verifier (RFC 7636 §4.2).
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(hash(verifier))
}

/// Compares two secrets in time that doesn't depend on where they differ.
pub fn same_secret(a: &[u8], b: &[u8]) -> bool {
    constant_time::verify_slices_are_equal(a, b).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_the_rfc_7636_example() {
        // RFC 7636, Appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn tokens_are_43_url_safe_characters_and_unique() {
        let a = random_token().expect("random source");
        let b = random_token().expect("random source");
        assert_eq!(a.len(), 43);
        assert!(
            a.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "{a}"
        );
        assert_ne!(a, b);
    }

    #[test]
    fn hashes_are_sha256() {
        // SHA-256("abc"), FIPS 180-2 example.
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let hex = hash("abc").iter().fold(String::new(), |mut hex, byte| {
            use std::fmt::Write as _;
            let _infallible = write!(hex, "{byte:02x}");
            hex
        });
        assert_eq!(hex, expected);
    }

    #[test]
    fn secrets_compare_exactly() {
        assert!(same_secret(b"same", b"same"));
        assert!(!same_secret(b"same", b"sane"));
        assert!(!same_secret(b"short", b"longer"));
    }
}
