//! Uploads in flight (D-215). An upload's body is read by its handler, after sign-in and the
//! role's permission, with an idle timeout instead of the request deadline (a body still arriving
//! isn't the server being slow); the deadline then covers the server's own work. Because a slow
//! body can now take minutes, the bytes uploads hold in memory are bounded:
//!
//! - a memory budget shared by every upload (`COLOPHON_UPLOAD_MEMORY_MB`, default 256): each
//!   upload holds what its `Content-Length` declares, or the body limit when it declares none,
//!   until its handler finishes. A full budget is waited on for [`WAIT`], then refused `503
//!   uploads_busy` with `Retry-After`;
//! - at most [`PER_CREDENTIAL`] uploads at once per session or API token, refused `429
//!   too_many_uploads` with `Retry-After`, so one client can't take the whole budget.
//!
//! A declared `Content-Length` over the body limit is refused `413` before any byte is read (the
//! body hygiene layer, D-164). Streaming the body straight into the quarantine object instead of
//! holding it is the later improvement that lifts the memory bound altogether.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

/// How long a full budget is waited on before `503 uploads_busy`.
pub const WAIT: Duration = Duration::from_secs(2);
/// Uploads one session or token may have in flight at once.
pub const PER_CREDENTIAL: usize = 4;
/// Seconds a refused upload is told to wait (`Retry-After`).
pub const RETRY_AFTER_SECONDS: u32 = 5;
/// The longest a body may go without a byte before the read is given up (`408 body_stalled`).
pub const IDLE: Duration = Duration::from_secs(15);
/// The slowest a whole body may arrive on average: the read's overall bound is the body limit at
/// this rate (at least [`MIN_OVERALL`]), about 5.5 minutes for 21 MiB.
pub const MIN_RATE_BYTES_PER_SEC: u64 = 64 * 1024;
/// The overall bound's floor.
pub const MIN_OVERALL: Duration = Duration::from_secs(30);

/// Why an upload wasn't admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The memory budget stayed full for [`WAIT`].
    Busy,
    /// This credential already has [`PER_CREDENTIAL`] uploads in flight.
    TooMany,
}

/// The shared bounds. Cheap to clone.
#[derive(Clone)]
pub struct UploadGate {
    budget: Arc<Semaphore>,
    budget_kib: u32,
    body_limit: usize,
    open: Arc<Mutex<HashMap<Uuid, usize>>>,
    per_credential: usize,
    wait: Duration,
}

/// One admitted upload: its share of the budget and its place in its credential's count, both
/// given back when it drops — on every exit, a cancelled request's included.
pub struct UploadPermit {
    _memory: OwnedSemaphorePermit,
    _place: Place,
}

/// Kibibytes, rounded up, at least one: what a body of `bytes` holds of the budget.
fn kib(bytes: u64) -> u32 {
    u32::try_from(bytes.div_ceil(1024).max(1)).unwrap_or(u32::MAX)
}

impl UploadGate {
    /// A budget of `memory_mb` MiB for bodies of at most `body_limit` bytes; never smaller than one
    /// body at the limit, so the largest upload can always go alone.
    pub fn new(memory_mb: usize, body_limit: usize) -> Self {
        Self::with(memory_mb, body_limit, PER_CREDENTIAL, WAIT)
    }

    /// The gate with its bounds spelled out: for tests that need them to bite quickly.
    pub fn with(
        memory_mb: usize,
        body_limit: usize,
        per_credential: usize,
        wait: Duration,
    ) -> Self {
        let one = kib(body_limit as u64);
        let budget_kib = u32::try_from(memory_mb.saturating_mul(1024))
            .unwrap_or(u32::MAX)
            .max(one);
        Self {
            budget: Arc::new(Semaphore::new(budget_kib as usize)),
            budget_kib,
            body_limit,
            open: Arc::new(Mutex::new(HashMap::new())),
            per_credential: per_credential.max(1),
            wait,
        }
    }

    /// The body limit uploads are read against.
    pub fn body_limit(&self) -> usize {
        self.body_limit
    }

    /// The overall bound on reading one body: the body limit at [`MIN_RATE_BYTES_PER_SEC`], at
    /// least [`MIN_OVERALL`].
    pub fn overall(&self) -> Duration {
        MIN_OVERALL.max(Duration::from_secs(
            self.body_limit as u64 / MIN_RATE_BYTES_PER_SEC,
        ))
    }

    /// Admits one upload for `credential` holding `declared` bytes (the body limit when unknown):
    /// its credential's place first, then its share of the budget, waited on for at most the gate's
    /// wait.
    pub async fn admit(
        &self,
        credential: Uuid,
        declared: Option<u64>,
    ) -> Result<UploadPermit, Refused> {
        {
            let mut open = self
                .open
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let count = open.entry(credential).or_insert(0);
            if *count >= self.per_credential {
                return Err(Refused::TooMany);
            }
            *count += 1;
        }
        // From here the credential's place is held: a guard gives it back if the budget refuses.
        let place = Place {
            credential,
            open: Arc::clone(&self.open),
        };
        let wanted = kib(declared
            .unwrap_or(self.body_limit as u64)
            .min(self.body_limit as u64))
        .min(self.budget_kib);
        let memory = tokio::time::timeout(
            self.wait,
            Arc::clone(&self.budget).acquire_many_owned(wanted),
        )
        .await
        .map_err(|_| Refused::Busy)?
        .map_err(|_| Refused::Busy)?;
        Ok(UploadPermit {
            _memory: memory,
            _place: place,
        })
    }
}

/// A credential's place in its count: given back when dropped, whether the budget refused the upload
/// or its permit finished.
struct Place {
    credential: Uuid,
    open: Arc<Mutex<HashMap<Uuid, usize>>>,
}

impl Drop for Place {
    fn drop(&mut self) {
        let mut open = self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(count) = open.get_mut(&self.credential) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                open.remove(&self.credential);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: usize = 1024 * 1024;

    #[tokio::test]
    async fn the_budget_admits_twelve_bodies_at_the_limit_then_refuses_the_thirteenth() {
        let gate = UploadGate::with(256, 21 * MIB, 100, Duration::from_millis(50));
        let mut held = Vec::new();
        for _ in 0..12 {
            held.push(
                gate.admit(Uuid::now_v7(), None)
                    .await
                    .expect("room for twelve"),
            );
        }
        assert_eq!(
            gate.admit(Uuid::now_v7(), None).await.err(),
            Some(Refused::Busy)
        );
        // Small declared bodies still fit beside them, in what is left (256 - 12 × 21 = 4 MiB).
        assert!(gate.admit(Uuid::now_v7(), Some(1024 * 1024)).await.is_ok());
        held.pop();
        assert!(
            gate.admit(Uuid::now_v7(), None).await.is_ok(),
            "a released share is reused"
        );
    }

    #[tokio::test]
    async fn one_credential_has_at_most_four_while_another_still_uploads() {
        let gate = UploadGate::with(256, 21 * MIB, 4, Duration::from_millis(50));
        let busy = Uuid::now_v7();
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(gate.admit(busy, Some(1024)).await.expect("four"));
        }
        assert_eq!(
            gate.admit(busy, Some(1024)).await.err(),
            Some(Refused::TooMany)
        );
        assert!(
            gate.admit(Uuid::now_v7(), Some(1024)).await.is_ok(),
            "another credential"
        );
        held.pop();
        assert!(
            gate.admit(busy, Some(1024)).await.is_ok(),
            "a finished upload frees its place"
        );
    }

    #[tokio::test]
    async fn a_budget_refusal_gives_the_credential_its_place_back() {
        let gate = UploadGate::with(21, 21 * MIB, 1, Duration::from_millis(20));
        let whole = gate
            .admit(Uuid::now_v7(), None)
            .await
            .expect("the one body the budget holds");
        let caller = Uuid::now_v7();
        assert_eq!(gate.admit(caller, None).await.err(), Some(Refused::Busy));
        // Its place wasn't kept by the refused attempt: with room again it is admitted.
        drop(whole);
        assert!(gate.admit(caller, None).await.is_ok());
    }

    #[test]
    fn the_budget_always_fits_one_body_at_the_limit_and_the_read_bound_scales() {
        let gate = UploadGate::new(1, 21 * MIB);
        assert_eq!(gate.budget_kib, 21 * 1024);
        assert_eq!(gate.overall(), Duration::from_secs(336));
        assert_eq!(UploadGate::new(256, 64 * 1024).overall(), MIN_OVERALL);
    }
}
