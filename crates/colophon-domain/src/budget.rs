//! Budget reservations (docs/modules/09-automation.md, "Budgets"; spec §17, §33; D-113,
//! invariant AU3): what a principal's work may reserve, and where a reservation stands.
//!
//! The rules are pure so the accepting transaction (which counts and refuses), the outcome's
//! commit (which settles or releases) and the worker's reconciler (which frees the dead and
//! flags the stuck) all apply the same ones. Postgres is the authority: these functions decide,
//! `colophon_db::budgets` stores.
//!
//! A reservation's story: `held` while the work is in flight → `settled` with what it actually
//! consumed, in the same commit that records the outcome, or `released` when the work can never
//! happen → or `uncertain` when a held row outlives what could still be running, kept (never
//! silently dropped) until a reconciler or a person resolves it.

use crate::job::Status;

/// What a reservation bounds: the dimensions `api_token_limits` names, plus the provider
/// budgets the optional assistant would hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Entry writes: creates, saves, localizations, imported items and the mirror writes their
    /// references cause — one per document written.
    Documents,
    /// Delivery jobs in flight: one reservation per job a publish dispatches.
    Jobs,
    /// Management requests: stored for the hard quota slot; nothing consumes it yet (D-113).
    Requests,
    /// Provider tokens, for the optional assistant (reserved for later work).
    Tokens,
    /// Provider cost, for the optional assistant (reserved for later work).
    Cost,
}

impl Kind {
    /// Every kind, for tests and listings.
    pub const ALL: [Self; 5] = [
        Self::Documents,
        Self::Jobs,
        Self::Requests,
        Self::Tokens,
        Self::Cost,
    ];

    /// The name stored in the database.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Documents => "documents",
            Self::Jobs => "jobs",
            Self::Requests => "requests",
            Self::Tokens => "tokens",
            Self::Cost => "cost",
        }
    }

    /// The kind with this name.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == name)
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Where a reservation stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The allowance is taken; the work is in flight (or its outcome hasn't committed).
    Held,
    /// The outcome landed with what it consumed, in its commit.
    Settled,
    /// The work can never happen; the allowance is freed.
    Released,
    /// Held past what could still be running — kept until reconciled, counted conservatively.
    Uncertain,
}

impl State {
    /// Every state, for tests.
    pub const ALL: [Self; 4] = [Self::Held, Self::Settled, Self::Released, Self::Uncertain];

    /// The name stored in the database.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Held => "held",
            Self::Settled => "settled",
            Self::Released => "released",
            Self::Uncertain => "uncertain",
        }
    }

    /// The state with this name.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.as_str() == name)
    }

    /// Whether the reservation might still be running: it counts toward concurrency budgets,
    /// and toward a window quota until it's known never to have happened. `released` is the
    /// only state that frees its allowance.
    pub const fn is_open(self) -> bool {
        !matches!(self, Self::Released)
    }
}

/// The most a `documents_per_day` limit may be (also the database's CHECK bound).
pub const MAX_DOCUMENTS_PER_DAY: i64 = 1_000_000;
/// The most `max_concurrent_jobs` may be (also the database's CHECK bound).
pub const MAX_CONCURRENT_JOBS: i64 = 64;
/// The most `requests_per_minute` may be (also the database's CHECK bound).
pub const MAX_REQUESTS_PER_MINUTE: i64 = 100_000;

/// Why a proposed limits row was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitError {
    /// A value is outside `1..=MAX`; the field name, as the body and the column call it.
    Range(&'static str),
    /// Every field is NULL: a limits row must bound something.
    Empty,
}

/// A token's ceilings: the `api_token_limits` row, every field `None` = unconstrained.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limits {
    /// Entry writes per rolling day.
    pub documents_per_day: Option<i64>,
    /// Delivery jobs held at once.
    pub max_concurrent_jobs: Option<i64>,
    /// Management requests per rolling minute; the request-time quota throttles over it
    /// (D-118), outside this ledger.
    pub requests_per_minute: Option<i64>,
}

impl Limits {
    /// `limits`, if every set field is inside `1..=MAX` and at least one is set.
    pub fn check(self) -> Result<Self, LimitError> {
        for (value, field, max) in [
            (
                self.documents_per_day,
                "documents_per_day",
                MAX_DOCUMENTS_PER_DAY,
            ),
            (
                self.max_concurrent_jobs,
                "max_concurrent_jobs",
                MAX_CONCURRENT_JOBS,
            ),
            (
                self.requests_per_minute,
                "requests_per_minute",
                MAX_REQUESTS_PER_MINUTE,
            ),
        ] {
            if let Some(value) = value
                && !(1..=max).contains(&value)
            {
                return Err(LimitError::Range(field));
            }
        }
        if self == Self::default() {
            return Err(LimitError::Empty);
        }
        Ok(self)
    }

    /// The ceiling `kind` has here; `None` means unconstrained.
    pub const fn ceiling(self, kind: Kind) -> Option<i64> {
        match kind {
            Kind::Documents => self.documents_per_day,
            Kind::Jobs => self.max_concurrent_jobs,
            Kind::Requests => self.requests_per_minute,
            // No column bounds these yet: a limits row alone can't constrain provider budgets.
            Kind::Tokens | Kind::Cost => None,
        }
    }
}

/// Whether `used` plus `more` fits under `limit` (`None` is unconstrained).
pub const fn fits(used: i64, more: i64, limit: Option<i64>) -> bool {
    match limit {
        Some(limit) => used + more <= limit,
        None => true,
    }
}

/// How an outcome closes a held reservation (D-113).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Close {
    /// The work ran: settle with what it consumed.
    Settle,
    /// The work can never happen: release the allowance.
    Release,
}

/// What `status` does to the job's reservation: a delivered or given-up job consumed its slot
/// (`Settle`); a superseded one never ran (`Release`). Anything else closes nothing — a
/// `retrying` job's reservation stays held.
pub fn close_for(status: Status) -> Option<Close> {
    match status {
        Status::Succeeded | Status::Failed => Some(Close::Settle),
        Status::Superseded => Some(Close::Release),
        Status::Pending | Status::Running | Status::Retrying => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_states_round_trip() {
        for kind in Kind::ALL {
            assert_eq!(Kind::parse(kind.as_str()), Some(kind));
        }
        for unknown in ["", "document", "DOCUMENTS", "job", "credits"] {
            assert_eq!(Kind::parse(unknown), None, "{unknown:?} is not a kind");
        }
        for state in State::ALL {
            assert_eq!(State::parse(state.as_str()), Some(state));
        }
        for unknown in ["", "hold", "done", "cancelled"] {
            assert_eq!(State::parse(unknown), None, "{unknown:?} is not a state");
        }
    }

    #[test]
    fn only_released_frees_the_allowance() {
        assert!(State::Held.is_open());
        assert!(State::Settled.is_open());
        assert!(State::Uncertain.is_open());
        assert!(!State::Released.is_open());
    }

    #[test]
    fn limits_check_bounds_every_field() {
        assert_eq!(Limits::default().check(), Err(LimitError::Empty));
        let valid = Limits {
            documents_per_day: Some(100),
            max_concurrent_jobs: Some(2),
            requests_per_minute: None,
        };
        assert_eq!(valid.check(), Ok(valid));
        for (field, limits) in [
            (
                "documents_per_day",
                Limits {
                    documents_per_day: Some(0),
                    ..Limits::default()
                },
            ),
            (
                "documents_per_day",
                Limits {
                    documents_per_day: Some(MAX_DOCUMENTS_PER_DAY + 1),
                    ..Limits::default()
                },
            ),
            (
                "max_concurrent_jobs",
                Limits {
                    max_concurrent_jobs: Some(0),
                    ..Limits::default()
                },
            ),
            (
                "max_concurrent_jobs",
                Limits {
                    max_concurrent_jobs: Some(MAX_CONCURRENT_JOBS + 1),
                    ..Limits::default()
                },
            ),
            (
                "requests_per_minute",
                Limits {
                    requests_per_minute: Some(-3),
                    ..Limits::default()
                },
            ),
            (
                "requests_per_minute",
                Limits {
                    requests_per_minute: Some(MAX_REQUESTS_PER_MINUTE + 1),
                    ..Limits::default()
                },
            ),
        ] {
            assert_eq!(limits.check(), Err(LimitError::Range(field)));
        }
        // The bounds themselves are legal.
        assert_eq!(
            Limits {
                documents_per_day: Some(MAX_DOCUMENTS_PER_DAY),
                ..Limits::default()
            }
            .check()
            .map(|limits| limits.documents_per_day),
            Ok(Some(MAX_DOCUMENTS_PER_DAY))
        );
    }

    #[test]
    fn ceilings_match_the_columns() {
        let limits = Limits {
            documents_per_day: Some(10),
            max_concurrent_jobs: None,
            requests_per_minute: Some(60),
        };
        assert_eq!(limits.ceiling(Kind::Documents), Some(10));
        assert_eq!(limits.ceiling(Kind::Jobs), None);
        assert_eq!(limits.ceiling(Kind::Requests), Some(60));
        assert_eq!(limits.ceiling(Kind::Tokens), None);
        assert_eq!(limits.ceiling(Kind::Cost), None);
    }

    #[test]
    fn fits_only_counts_what_could_happen() {
        assert!(fits(0, 1, Some(1)));
        assert!(fits(99, 1, Some(100)));
        assert!(!fits(99, 2, Some(100)));
        assert!(!fits(0, 1, Some(0)));
        // No ceiling: anything fits.
        for used in [0, 1_000_000, i64::MAX - 1] {
            assert!(fits(used, 1, None));
        }
    }

    #[test]
    fn close_for_matches_the_job_state_machine() {
        assert_eq!(close_for(Status::Succeeded), Some(Close::Settle));
        assert_eq!(close_for(Status::Failed), Some(Close::Settle));
        assert_eq!(close_for(Status::Superseded), Some(Close::Release));
        assert_eq!(close_for(Status::Pending), None);
        assert_eq!(close_for(Status::Running), None);
        assert_eq!(close_for(Status::Retrying), None);
    }
}
