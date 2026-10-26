//! Delivery jobs (docs/modules/05-worker.md; spec §16; D-046): what an attempt's outcome means,
//! and when to try again. The worker supplies the clock and the randomness, so the simulator can
//! drive the same rules.

/// Each external attempt's deadline (spec §16).
pub const ATTEMPT_DEADLINE_SECS: u64 = 10;
/// The most attempts a job gets.
pub const MAX_ATTEMPTS: u32 = 5;
/// How long after it was created a job may still be attempted.
pub const LIFETIME_SECS: i64 = 60 * 60;
/// The first retry waits up to this long; each later one up to twice the last.
pub const BACKOFF_BASE_MS: u64 = 1_000;
/// No retry waits longer than this, whatever a `Retry-After` asks for.
pub const BACKOFF_CAP_MS: u64 = 60_000;

/// Where a job stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Waiting for its first attempt.
    Pending,
    /// Leased by a worker, attempt in flight.
    Running,
    /// Waiting to be tried again.
    Retrying,
    /// Delivered.
    Succeeded,
    /// Given up: refused for good, out of attempts or out of time.
    Failed,
    /// A newer release for the same destination made it obsolete; never sent after that one.
    Superseded,
}

impl Status {
    /// Every status, for listings and tests.
    pub const ALL: [Self; 6] = [
        Self::Pending,
        Self::Running,
        Self::Retrying,
        Self::Succeeded,
        Self::Failed,
        Self::Superseded,
    ];

    /// The name stored in the database and used in the API.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Retrying => "retrying",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Superseded => "superseded",
        }
    }

    /// The status with this name.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|status| status.as_str() == name)
    }

    /// Whether nothing more will happen to the job.
    pub const fn is_final(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Superseded)
    }
}

/// What one attempt came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The destination accepted it.
    Delivered {
        /// Its HTTP status.
        status: u16,
    },
    /// It might work later: a timeout, a dropped connection, throttling, a server error.
    Retryable {
        /// What happened, for the job's record.
        reason: String,
        /// How long the destination asked us to wait, if it did.
        retry_after_ms: Option<u64>,
    },
    /// It won't work until someone fixes something: a refusal, a bad address.
    Terminal {
        /// What happened.
        reason: String,
    },
}

/// What an HTTP status means for a delivery. Redirects aren't followed, so they're terminal: a
/// destination that moved must be reconfigured, not chased.
pub fn classify(status: u16, retry_after_ms: Option<u64>) -> Outcome {
    match status {
        200..=299 => Outcome::Delivered { status },
        408 | 425 | 429 | 500 | 502 | 503 | 504 => Outcome::Retryable {
            reason: format!("HTTP {status}"),
            retry_after_ms,
        },
        _ => Outcome::Terminal {
            reason: format!("HTTP {status}"),
        },
    }
}

/// What to do after an attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Record it delivered.
    Succeeded,
    /// Try again after this long.
    Retry {
        /// The wait.
        after_ms: u64,
    },
    /// Record it failed.
    Failed,
}

/// The wait before retry number `attempt` (1 for the first retry): full jitter, a random point
/// between zero and `min(cap, base × 2^(attempt − 1))`, from `entropy` (any random 64 bits).
pub fn backoff_ms(attempt: u32, entropy: u64) -> u64 {
    let ceiling = BACKOFF_BASE_MS
        .saturating_mul(1_u64 << attempt.saturating_sub(1).min(32))
        .min(BACKOFF_CAP_MS);
    entropy % (ceiling + 1)
}

/// Decides what follows `outcome`, the result of attempt number `attempts`, at `now_ms`, for a
/// job whose lifetime ends at `deadline_ms` (milliseconds since the Unix epoch).
pub fn after_attempt(
    outcome: &Outcome,
    attempts: u32,
    now_ms: i64,
    deadline_ms: i64,
    entropy: u64,
) -> Next {
    match outcome {
        Outcome::Delivered { .. } => Next::Succeeded,
        Outcome::Terminal { .. } => Next::Failed,
        Outcome::Retryable { retry_after_ms, .. } => {
            if attempts >= MAX_ATTEMPTS {
                return Next::Failed;
            }
            let jittered = backoff_ms(attempts, entropy);
            let asked = retry_after_ms.map_or(0, |ms| ms.min(BACKOFF_CAP_MS));
            let after_ms = jittered.max(asked);
            let when = now_ms.saturating_add(i64::try_from(after_ms).unwrap_or(i64::MAX));
            if when > deadline_ms {
                Next::Failed
            } else {
                Next::Retry { after_ms }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_mean_what_they_say() {
        let expectations = [
            (200, "delivered"),
            (204, "delivered"),
            (301, "terminal"),
            (400, "terminal"),
            (401, "terminal"),
            (404, "terminal"),
            (408, "retryable"),
            (429, "retryable"),
            (500, "retryable"),
            (501, "terminal"),
            (503, "retryable"),
        ];
        for (status, kind) in expectations {
            let actual = match classify(status, None) {
                Outcome::Delivered { .. } => "delivered",
                Outcome::Retryable { .. } => "retryable",
                Outcome::Terminal { .. } => "terminal",
            };
            assert_eq!(actual, kind, "HTTP {status}");
        }
        for status in Status::ALL {
            assert_eq!(Status::parse(status.as_str()), Some(status));
        }
    }

    #[test]
    fn backoff_is_jittered_doubling_and_capped() {
        for attempt in 1..=40 {
            let ceiling = (BACKOFF_BASE_MS << (attempt - 1).min(32)).min(BACKOFF_CAP_MS);
            for entropy in [0, 1, 999, u64::MAX / 3, u64::MAX] {
                assert!(backoff_ms(attempt, entropy) <= ceiling, "attempt {attempt}");
            }
        }
        assert_eq!(backoff_ms(1, 0), 0, "full jitter reaches zero");
        assert_eq!(backoff_ms(1, 1_000), 1_000, "and the ceiling");
    }

    #[test]
    fn retries_stop_at_the_attempt_budget_and_the_deadline() {
        let retry = Outcome::Retryable {
            reason: "HTTP 503".to_owned(),
            retry_after_ms: None,
        };
        let deadline = 3_600_000;
        assert!(matches!(
            after_attempt(&retry, 1, 0, deadline, 7),
            Next::Retry { .. }
        ));
        assert_eq!(
            after_attempt(&retry, MAX_ATTEMPTS, 0, deadline, 7),
            Next::Failed
        );
        assert_eq!(
            after_attempt(&retry, 1, deadline, deadline, 500),
            Next::Failed,
            "past its lifetime"
        );
        let throttled = Outcome::Retryable {
            reason: "HTTP 429".to_owned(),
            retry_after_ms: Some(30_000),
        };
        assert_eq!(
            after_attempt(&throttled, 1, 0, deadline, 0),
            Next::Retry { after_ms: 30_000 },
            "a Retry-After is respected"
        );
        let greedy = Outcome::Retryable {
            reason: "HTTP 429".to_owned(),
            retry_after_ms: Some(86_400_000),
        };
        assert_eq!(
            after_attempt(&greedy, 1, 0, deadline, 0),
            Next::Retry {
                after_ms: BACKOFF_CAP_MS
            },
            "within bounds"
        );
        assert_eq!(
            after_attempt(&Outcome::Delivered { status: 200 }, 5, 0, 0, 0),
            Next::Succeeded
        );
        let refused = Outcome::Terminal {
            reason: "HTTP 401".to_owned(),
        };
        assert_eq!(after_attempt(&refused, 1, 0, deadline, 0), Next::Failed);
    }
}
