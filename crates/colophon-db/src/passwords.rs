//! Password credentials: Argon2id in the password-hash crate's PHC format — the algorithm
//! choice, memory and rounds all travel inside the stored string, so parameters can be
//! raised later without a migration. Hashes are only ever compared, never returned.
//!
//! Argon2id is the one thing an unauthenticated caller can make the server do that costs
//! 19 MiB and tens of milliseconds of a core, so every request path hashes and verifies
//! through a [`PasswordGate`] (D-150): a fixed number of Argon2 jobs at once, a bounded
//! waiting room with a short deadline, and [`DbError::Busy`] — the API's `503` with
//! `Retry-After` — for everything past that. [`hashed`] stays ungated for one-shot tools
//! (the CLI), which run one hash and exit.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::DbError;

/// A real, parseable PHC string a sign-in verifies against when the account doesn't exist:
/// a wrong password and a missing account take the same amount of work to refuse.
const DUMMY: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$c29tZS1zYWx0LXNhbHQ$c3BzRmFrZUhhc2hGb3JUaW1pbmdFcXVhbGl0eQ";

/// The longest password anything accepts, in bytes: sign-up, the CLI and sign-in alike, so a
/// giant body can never become a giant Argon2 input (D-150).
pub const MAX_PASSWORD_BYTES: usize = 512;

/// How long a password job may wait for a free slot before its request is shed.
pub const WAIT: Duration = Duration::from_secs(2);

/// Requests that may wait for a slot, per slot: a burst queues for up to [`WAIT`], a flood
/// past this is refused at once rather than piling up.
pub const WAITING_PER_SLOT: usize = 32;

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

/// `hash` on the blocking pool, ungated: for one-shot tools such as `colophon admin account`.
/// Request paths use [`PasswordGate::hash`].
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

/// The bound on Argon2 work every request path shares (D-150): `slots` jobs run on the
/// blocking pool at once; a job that finds none free waits up to `wait` among at most
/// `max_waiting` others, and past either bound the caller gets [`DbError::Busy`]. The permit
/// moves into the blocking job, so a request cancelled mid-hash still holds its slot until the
/// work it started has actually stopped — the count is of work, not of requests.
#[derive(Debug, Clone)]
pub struct PasswordGate {
    permits: Arc<Semaphore>,
    waiting: Arc<AtomicUsize>,
    max_waiting: usize,
    wait: Duration,
}

/// Counts one waiter for as long as it waits — dropped on every exit, cancellation included.
struct Waiting(Arc<AtomicUsize>);

impl Drop for Waiting {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl PasswordGate {
    /// `slots` Argon2 jobs at once (at least one), [`WAITING_PER_SLOT`] waiters a slot,
    /// waiting at most [`WAIT`].
    pub fn new(slots: usize) -> Self {
        let slots = slots.max(1);
        Self::with_waiting(slots, slots.saturating_mul(WAITING_PER_SLOT), WAIT)
    }

    /// The gate with its waiting room spelled out: for tests that need a bound to bite.
    pub fn with_waiting(slots: usize, max_waiting: usize, wait: Duration) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(slots.max(1))),
            waiting: Arc::new(AtomicUsize::new(0)),
            max_waiting,
            wait,
        }
    }

    /// The default number of slots: one a core (at least one, at most 64). Argon2 then never
    /// has more jobs than cores to run them on — the bound is on memory (19 MiB a job) and on
    /// blocking threads, while the scheduler still shares the cores with delivery, which since
    /// D-150 never waits behind a verify for a database connection.
    pub fn default_slots() -> usize {
        std::thread::available_parallelism()
            .map_or(2, std::num::NonZeroUsize::get)
            .clamp(1, 64)
    }

    /// A slot: at once if one is free, else after waiting — or [`DbError::Busy`].
    async fn permit(&self) -> Result<OwnedSemaphorePermit, DbError> {
        if let Ok(permit) = Arc::clone(&self.permits).try_acquire_owned() {
            return Ok(permit);
        }
        if self.waiting.fetch_add(1, Ordering::AcqRel) >= self.max_waiting {
            self.waiting.fetch_sub(1, Ordering::AcqRel);
            return Err(DbError::Busy);
        }
        let _waiting = Waiting(Arc::clone(&self.waiting));
        match tokio::time::timeout(self.wait, Arc::clone(&self.permits).acquire_owned()).await {
            Ok(Ok(permit)) => Ok(permit),
            // The semaphore is never closed; a timeout is the bound biting.
            Ok(Err(_)) | Err(_) => Err(DbError::Busy),
        }
    }

    /// A slot for one password job, taken before anything else a sign-in does: a request
    /// the bound sheds then costs nothing — no database work, no counted attempt.
    pub async fn slot(&self) -> Result<PasswordSlot, DbError> {
        self.permit().await.map(PasswordSlot)
    }

    /// Hashes `password` for storage, inside the bound.
    pub async fn hash(&self, password: &str) -> Result<String, DbError> {
        self.slot().await?.hash(password).await
    }

    /// [`PasswordSlot::verify`], taking the slot first.
    pub async fn verify(&self, password: &str, phc: Option<&str>) -> Result<bool, DbError> {
        Ok(self.slot().await?.verify(password, phc).await)
    }

    /// Slots free right now: for tests and diagnostics.
    pub fn available(&self) -> usize {
        self.permits.available_permits()
    }
}

/// One held slot of a [`PasswordGate`]: spent by the one job it runs.
#[derive(Debug)]
pub struct PasswordSlot(OwnedSemaphorePermit);

impl PasswordSlot {
    /// Hashes `password` for storage on the blocking pool; the slot frees when the hash does.
    pub async fn hash(self, password: &str) -> Result<String, DbError> {
        let permit = self.0;
        let password = password.to_owned();
        tokio::task::spawn_blocking(move || {
            let _slot = permit;
            hash(&password)
        })
        .await
        .map_err(|_| DbError::Corrupt("the hashing task was cancelled"))?
    }

    /// Whether `password` matches `phc` — or, with no `phc`, the same work against the dummy
    /// hash and `false`, so a missing account costs what a wrong password does.
    pub async fn verify(self, password: &str, phc: Option<&str>) -> bool {
        let permit = self.0;
        let password = password.to_owned();
        let (phc, real) = match phc {
            Some(phc) => (phc.to_owned(), true),
            None => (DUMMY.to_owned(), false),
        };
        let matched = tokio::task::spawn_blocking(move || {
            let _slot = permit;
            verify(&password, &phc)
        })
        .await
        .unwrap_or(false);
        real && matched
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{DUMMY, PasswordGate, hash, verify};
    use crate::DbError;

    #[test]
    fn a_password_verifies_against_its_own_hash_only() {
        let stored = hash("correct horse battery staple").expect("hashed");
        assert!(verify("correct horse battery staple", &stored));
        assert!(!verify("wrong", &stored));
        assert!(!verify("correct horse battery staple", "not a phc string"));
        assert!(!verify("anything", DUMMY));
    }

    #[tokio::test]
    async fn the_gate_runs_its_slots_lets_a_few_wait_and_sheds_the_rest() {
        // One slot, one waiter, a deadline far longer than one hash: of four jobs started
        // together, one runs, one waits its turn, and two are refused at once — none piles up.
        let gate = PasswordGate::with_waiting(1, 1, Duration::from_secs(30));
        let stored = hash("correct horse battery staple").expect("hashed");
        let jobs = (0..4).map(|_| {
            let gate = gate.clone();
            let stored = stored.clone();
            tokio::spawn(async move {
                gate.verify("correct horse battery staple", Some(&stored))
                    .await
            })
        });
        let mut verified = 0;
        let mut busy = 0;
        for job in jobs.collect::<Vec<_>>() {
            match job.await.expect("joined") {
                Ok(true) => verified += 1,
                Err(DbError::Busy) => busy += 1,
                other => panic!("unexpected {other:?}"),
            }
        }
        assert_eq!((verified, busy), (2, 2));
        assert_eq!(gate.available(), 1, "every slot came back");
    }

    #[tokio::test]
    async fn a_waiter_past_the_deadline_is_shed() {
        let gate = PasswordGate::with_waiting(1, 8, Duration::from_millis(1));
        let held = std::sync::Arc::clone(&gate.permits)
            .try_acquire_owned()
            .expect("the one slot");
        assert!(matches!(gate.verify("x", None).await, Err(DbError::Busy)));
        drop(held);
        assert_eq!(
            gate.verify("x", None).await.ok(),
            Some(false),
            "the dummy never matches"
        );
    }
}
