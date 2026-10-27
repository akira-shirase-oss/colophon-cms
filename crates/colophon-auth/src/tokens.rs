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

/// Crockford's base-32 alphabet: digits and capitals without I, L, O and U, so a code read off a
/// terminal can't be mistyped between look-alikes.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Characters in a setup code: 20 of Crockford's alphabet, 100 random bits.
pub const SETUP_CODE_CHARS: usize = 20;

/// A one-time setup code (D-060): 100 bits from the operating system's secure random source, as
/// four groups of five characters, `7K3QD-…`, for a person to copy from the server's terminal.
pub fn setup_code() -> Result<String, aws_lc_rs::error::Unspecified> {
    let mut bytes = [0_u8; SETUP_CODE_CHARS];
    aws_lc_rs::rand::SystemRandom::new().fill(&mut bytes)?;
    let mut code = String::with_capacity(SETUP_CODE_CHARS + 3);
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 && index % 5 == 0 {
            code.push('-');
        }
        // 256 is a multiple of 32, so the low five bits of a uniform byte are uniform.
        code.push(char::from(CROCKFORD[usize::from(byte & 31)]));
    }
    Ok(code)
}

/// The SHA-256 of a setup code as a person typed it, or `None` if it can't be one. Case, spaces
/// and dashes don't matter, and the look-alikes Crockford's alphabet leaves out are read as what
/// they look like (O as 0, I and L as 1).
pub fn setup_code_hash(typed: &str) -> Option<[u8; 32]> {
    // A bound before any work: a code is 20 characters, however it is spaced.
    if typed.len() > 64 {
        return None;
    }
    let mut canonical = String::with_capacity(SETUP_CODE_CHARS);
    for character in typed.chars() {
        let character = match character.to_ascii_uppercase() {
            ' ' | '-' | '\t' => continue,
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        };
        if !character.is_ascii() || !CROCKFORD.contains(&(character as u8)) {
            return None;
        }
        canonical.push(character);
    }
    (canonical.len() == SETUP_CODE_CHARS).then(|| hash(&canonical))
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
    fn setup_codes_are_four_groups_of_crockford_characters_and_unique() {
        let a = setup_code().expect("random source");
        let b = setup_code().expect("random source");
        assert_eq!(a.len(), SETUP_CODE_CHARS + 3, "{a}");
        let groups: Vec<_> = a.split('-').collect();
        assert_eq!(groups.len(), 4, "{a}");
        assert!(groups.iter().all(|group| group.len() == 5), "{a}");
        assert!(
            a.bytes()
                .all(|byte| byte == b'-' || CROCKFORD.contains(&byte)),
            "{a}"
        );
        assert_ne!(a, b);
        assert!(setup_code_hash(&a).is_some(), "a code reads back: {a}");
    }

    #[test]
    fn setup_codes_read_back_however_they_are_typed() {
        let code = "7K3QD-0ZX1V-ABCDE-FGHJK";
        let expected = setup_code_hash(code).expect("a code");
        assert_eq!(expected, hash("7K3QD0ZX1VABCDEFGHJK"));
        for typed in [
            "7k3qd-0zx1v-abcde-fghjk",
            "7K3QD 0ZX1V ABCDE FGHJK",
            "7K3QDOZXIVABCDEFGHJK",
            "  7K3QD-0ZXLV-ABCDE-FGHJK ",
        ] {
            assert_eq!(setup_code_hash(typed), Some(expected), "{typed:?}");
        }
        for wrong in [
            "",
            "7K3QD-0ZX1V-ABCDE-FGHJ",
            "7K3QD-0ZX1V-ABCDE-FGHJKM",
            "7K3QD-0ZX1V-ABCDE-FGHJU",
            "7K3QD-0ZX1V-ABCDE-FGHJ!",
            "7K3QD-0ZX1V-ABCDE-FGHJé",
            &"-".repeat(65),
        ] {
            assert_eq!(setup_code_hash(wrong), None, "{wrong:?}");
        }
    }

    #[test]
    fn secrets_compare_exactly() {
        assert!(same_secret(b"same", b"same"));
        assert!(!same_secret(b"same", b"sane"));
        assert!(!same_secret(b"short", b"longer"));
    }
}
