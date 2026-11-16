//! Password credentials: Argon2id in the password-hash crate's PHC format — the algorithm
//! choice, memory and rounds all travel inside the stored string, so parameters can be
//! raised later without a migration. Hashes are only ever compared, never returned.

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};

use crate::DbError;

/// A real, parseable PHC string a sign-in verifies against when the account doesn't exist:
/// a wrong password and a missing account take the same amount of work to refuse.
const DUMMY: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$c29tZS1zYWx0LXNhbHQ$c3BzRmFrZUhhc2hGb3JUaW1pbmdFcXVhbGl0eQ";

/// Argon2id at the crate's defaults, which are the OWASP minimums for memory-bound hashing
/// (19 MiB, two rounds, one lane): a sign-in costs tens of milliseconds for the person.
fn argon() -> Argon2<'static> {
    Argon2::default()
}

/// Hashes `password` for storage. Never fails on valid input; an error means the algorithm
/// or its random source broke, which is an internal problem, not the caller's.
pub fn hash(password: &str) -> Result<String, DbError> {
    let mut salt_bytes = [0u8; 16];
    aws_lc_rs::rand::fill(&mut salt_bytes)
        .map_err(|_| DbError::Corrupt("the random source failed"))?;
    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|_| DbError::Corrupt("password hashing failed"))?;
    Ok(argon()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| DbError::Corrupt("password hashing failed"))?
        .to_string())
}

/// `hash` on the blocking pool: Argon2's memory work doesn't belong on an async worker.
pub async fn hashed(password: &str) -> Result<String, DbError> {
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || hash(&password))
        .await
        .map_err(|_| DbError::Corrupt("the hashing task was cancelled"))?
}

/// Whether `password` is the one `phc` stored — false also when `phc` can't be read, so a
/// corrupt row refuses rather than admits.
pub fn verify(password: &str, phc: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(phc) else {
        return false;
    };
    argon()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// `verify` on the blocking pool, against `phc` — or the dummy hash when there is none, so a
/// missing account costs the same work and gets the same refusal.
pub async fn verify_in(password: &str, phc: Option<&str>) -> bool {
    let password = password.to_owned();
    let phc = phc.map_or_else(|| DUMMY.to_owned(), str::to_owned);
    tokio::task::spawn_blocking(move || verify(&password, &phc))
        .await
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{DUMMY, hash, verify};

    #[test]
    fn a_password_verifies_against_its_own_hash_only() {
        let stored = hash("correct horse battery staple").expect("hashed");
        assert!(verify("correct horse battery staple", &stored));
        assert!(!verify("wrong", &stored));
        assert!(!verify("correct horse battery staple", "not a phc string"));
        assert!(!verify("anything", DUMMY));
    }
}
