//! Scheduled publishing (D-060; spec §06 "Scheduled publishing", §17, case X07).
//!
//! A schedule keeps what its author chose, a local date and time in an IANA time zone, together
//! with the instant that resolves to and the approved hash it publishes. A local time the clocks
//! show twice (when they go back) or never (when they go forward) needs an explicit choice
//! between its two candidate instants; nothing is guessed. When a schedule falls due, the
//! platform checks everything again and either publishes it once or records why it didn't.

use jiff::{
    civil::DateTime,
    tz::{AmbiguousOffset, Disambiguation, TimeZone},
};

/// How far ahead a schedule may be: a year and a day.
pub const MAX_AHEAD_SECS: i64 = 366 * 24 * 60 * 60;

/// How late a schedule may start and still count as on time: a worker checks every few seconds,
/// so anything later than this means it couldn't run (the platform was down or overloaded).
pub const DELAY_GRACE_SECS: i64 = 60;

/// Which of two candidate instants a local time means, when it means two (or none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// The earlier instant.
    Earlier,
    /// The later instant.
    Later,
}

impl Choice {
    /// The name stored and used in the API.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Earlier => "earlier",
            Self::Later => "later",
        }
    }

    /// The choice with this name.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "earlier" => Some(Self::Earlier),
            "later" => Some(Self::Later),
            _ => None,
        }
    }
}

/// Where a schedule stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Waiting for its time.
    Scheduled,
    /// Published, as one release.
    Published,
    /// Its time came and something refused it (an edit, an expired approval, a role that no
    /// longer may publish); the reason is kept, and nothing was published.
    Blocked,
    /// Cancelled before it ran.
    Cancelled,
}

impl State {
    /// The name stored and used in the API.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scheduled => "scheduled",
            Self::Published => "published",
            Self::Blocked => "blocked",
            Self::Cancelled => "cancelled",
        }
    }

    /// The state with this name.
    pub fn parse(name: &str) -> Option<Self> {
        [
            Self::Scheduled,
            Self::Published,
            Self::Blocked,
            Self::Cancelled,
        ]
        .into_iter()
        .find(|state| state.as_str() == name)
    }
}

/// A local time resolved in its time zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The instant, in seconds since the Unix epoch.
    pub at: i64,
    /// The local date and time, normalized (`2026-11-01T01:30:00`).
    pub local: String,
    /// The IANA time zone, as given.
    pub zone: String,
    /// The choice it needed, if the local time was ambiguous or skipped.
    pub choice: Option<Choice>,
}

/// Why a schedule was refused. Stable codes for the API's details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleError {
    /// Not a date and time like `2026-11-03T09:00`.
    #[error("a local time is a date and a time, like 2026-11-03T09:00")]
    InvalidLocalTime,
    /// Not an IANA time zone name.
    #[error("a time zone is an IANA name, like Europe/London or America/New_York")]
    UnknownZone,
    /// The clocks show this local time twice; the candidates are both instants (seconds).
    #[error(
        "the clocks show this time twice in this time zone, as they go back; choose the earlier or the later one"
    )]
    Ambiguous {
        /// The earlier instant.
        earlier: i64,
        /// The later instant.
        later: i64,
    },
    /// The clocks skip this local time; the candidates are the instants either side.
    #[error(
        "the clocks skip this time in this time zone, as they go forward; choose the earlier or the later one"
    )]
    Skipped {
        /// The earlier candidate.
        earlier: i64,
        /// The later candidate.
        later: i64,
    },
    /// Not in the future.
    #[error("a schedule is for a time in the future")]
    InThePast,
    /// More than [`MAX_AHEAD_SECS`] ahead.
    #[error("a schedule is at most a year and a day ahead")]
    TooFar,
    /// After the approval it publishes under expires.
    #[error(
        "the approval expires before that time; schedule it earlier, or have it approved closer to the time"
    )]
    AfterApproval {
        /// When the approval expires (seconds).
        expires: i64,
    },
}

impl ScheduleError {
    /// The code clients branch on.
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidLocalTime => "invalid_local_time",
            Self::UnknownZone => "unknown_time_zone",
            Self::Ambiguous { .. } => "ambiguous_local_time",
            Self::Skipped { .. } => "skipped_local_time",
            Self::InThePast => "in_the_past",
            Self::TooFar => "too_far_ahead",
            Self::AfterApproval { .. } => "after_approval_expires",
        }
    }
}

/// Resolves `local` (`YYYY-MM-DDTHH:MM`, seconds optional) in the IANA time zone `zone`. A time
/// the clocks show twice or skip needs `choice`; anywhere else, a choice is ignored.
pub fn resolve(local: &str, zone: &str, choice: Option<Choice>) -> Result<Resolved, ScheduleError> {
    let civil: DateTime = local
        .parse()
        .map_err(|_| ScheduleError::InvalidLocalTime)?;
    let tz = TimeZone::get(zone).map_err(|_| ScheduleError::UnknownZone)?;
    let ambiguous = tz.to_ambiguous_zoned(civil);
    let at = |how: Disambiguation| {
        ambiguous
            .clone()
            .disambiguate(how)
            .map(|zoned| zoned.timestamp().as_second())
            .map_err(|_| ScheduleError::InvalidLocalTime)
    };
    let both = || -> Result<(i64, i64), ScheduleError> {
        let (one, other) = (at(Disambiguation::Earlier)?, at(Disambiguation::Later)?);
        Ok((one.min(other), one.max(other)))
    };
    let (instant, choice) = match ambiguous.offset() {
        AmbiguousOffset::Unambiguous { .. } => (at(Disambiguation::Compatible)?, None),
        AmbiguousOffset::Fold { .. } => {
            let (earlier, later) = both()?;
            match choice {
                Some(Choice::Earlier) => (earlier, choice),
                Some(Choice::Later) => (later, choice),
                None => return Err(ScheduleError::Ambiguous { earlier, later }),
            }
        }
        AmbiguousOffset::Gap { .. } => {
            let (earlier, later) = both()?;
            match choice {
                Some(Choice::Earlier) => (earlier, choice),
                Some(Choice::Later) => (later, choice),
                None => return Err(ScheduleError::Skipped { earlier, later }),
            }
        }
    };
    Ok(Resolved {
        at: instant,
        local: civil.strftime("%Y-%m-%dT%H:%M:%S").to_string(),
        zone: zone.to_owned(),
        choice,
    })
}

/// Refuses an instant (seconds) that isn't after `now`, is more than [`MAX_AHEAD_SECS`] ahead, or
/// comes after the approval it publishes under expires.
pub const fn check_window(at: i64, now: i64, approval_expires: i64) -> Result<(), ScheduleError> {
    if at <= now {
        Err(ScheduleError::InThePast)
    } else if at - now > MAX_AHEAD_SECS {
        Err(ScheduleError::TooFar)
    } else if at >= approval_expires {
        Err(ScheduleError::AfterApproval {
            expires: approval_expires,
        })
    } else {
        Ok(())
    }
}

/// Whether a schedule that started at `started` (seconds) for `at` is labelled delayed.
pub const fn delayed(at: i64, started: i64) -> bool {
    started - at > DELAY_GRACE_SECS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seconds since the epoch of an RFC 3339 instant.
    fn utc(text: &str) -> i64 {
        text.parse::<jiff::Timestamp>()
            .expect("a test instant")
            .as_second()
    }

    #[test]
    fn an_ordinary_local_time_resolves_in_its_zone() {
        let resolved = resolve("2026-11-03T09:00", "Asia/Kolkata", None).expect("resolves");
        assert_eq!(resolved.at, utc("2026-11-03T03:30:00Z"));
        assert_eq!(resolved.local, "2026-11-03T09:00:00");
        assert_eq!(resolved.choice, None);
        // A choice where none is needed is ignored.
        let ignored = resolve("2026-11-03T09:00:30", "Europe/London", Some(Choice::Later))
            .expect("resolves");
        assert_eq!(ignored.at, utc("2026-11-03T09:00:30Z"));
        assert_eq!(ignored.choice, None);
    }

    #[test]
    fn a_time_the_clocks_show_twice_needs_a_choice() {
        // New York goes back from 02:00 EDT to 01:00 EST on 1 November 2026.
        let refused = resolve("2026-11-01T01:30", "America/New_York", None);
        let (earlier, later) = (utc("2026-11-01T05:30:00Z"), utc("2026-11-01T06:30:00Z"));
        assert_eq!(refused, Err(ScheduleError::Ambiguous { earlier, later }));
        assert_eq!(
            resolve("2026-11-01T01:30", "America/New_York", Some(Choice::Earlier))
                .map(|r| (r.at, r.choice)),
            Ok((earlier, Some(Choice::Earlier)))
        );
        assert_eq!(
            resolve("2026-11-01T01:30", "America/New_York", Some(Choice::Later)).map(|r| r.at),
            Ok(later)
        );
        // London goes back on 25 October 2026.
        assert!(matches!(
            resolve("2026-10-25T01:15", "Europe/London", None),
            Err(ScheduleError::Ambiguous { .. })
        ));
    }

    #[test]
    fn a_time_the_clocks_skip_needs_a_choice_between_either_side() {
        // New York goes forward from 02:00 EST to 03:00 EDT on 8 March 2026: 02:30 never happens.
        let refused = resolve("2026-03-08T02:30", "America/New_York", None);
        let (earlier, later) = (utc("2026-03-08T06:30:00Z"), utc("2026-03-08T07:30:00Z"));
        assert_eq!(refused, Err(ScheduleError::Skipped { earlier, later }));
        assert_eq!(
            resolve("2026-03-08T02:30", "America/New_York", Some(Choice::Later)).map(|r| r.at),
            Ok(later)
        );
        assert!(matches!(
            resolve("2026-03-29T01:30", "Europe/London", None),
            Err(ScheduleError::Skipped { .. })
        ));
    }

    #[test]
    fn nonsense_is_refused_by_name() {
        for (local, zone, error) in [
            ("2026-13-01T09:00", "Europe/London", ScheduleError::InvalidLocalTime),
            ("tomorrow at nine", "Europe/London", ScheduleError::InvalidLocalTime),
            ("2026-11-03T09:00", "Mars/Olympus_Mons", ScheduleError::UnknownZone),
            ("2026-11-03T09:00", "", ScheduleError::UnknownZone),
        ] {
            assert_eq!(resolve(local, zone, None), Err(error), "{local} in {zone:?}");
        }
        assert_eq!(Choice::parse("earlier"), Some(Choice::Earlier));
        assert_eq!(Choice::parse("Earlier"), None);
        for state in [State::Scheduled, State::Published, State::Blocked, State::Cancelled] {
            assert_eq!(State::parse(state.as_str()), Some(state));
        }
    }

    #[test]
    fn the_window_is_the_future_within_a_year_and_the_approval() {
        let now = utc("2026-10-27T12:00:00Z");
        let expires = now + 7 * 24 * 60 * 60;
        assert_eq!(check_window(now, now, expires), Err(ScheduleError::InThePast));
        assert_eq!(check_window(now + 1, now, expires), Ok(()));
        assert_eq!(
            check_window(expires, now, expires),
            Err(ScheduleError::AfterApproval { expires })
        );
        assert_eq!(
            check_window(now + MAX_AHEAD_SECS + 1, now, i64::MAX),
            Err(ScheduleError::TooFar)
        );
        assert!(!delayed(now, now + DELAY_GRACE_SECS));
        assert!(delayed(now, now + DELAY_GRACE_SECS + 1));
    }
}
