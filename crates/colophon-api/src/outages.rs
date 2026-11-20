//! Outage logs (D-224). A fault that repeats for as long as an outage lasts (a request refused
//! because no database connection came free, a dependency's failed probe) is said once when it
//! starts, then once per [`WINDOW`] with how often it happened, then once when a whole window
//! passed without it. An outage of any size is a handful of lines, not one per request: 1,941
//! refusals in one drill buried the line an operator needed.
//!
//! The server's summary task closes the windows ([`Outages::close_window`] every [`WINDOW`]).
//! Summary and recovery lines carry counts, never a `code` or `status`: they are not failed
//! requests, and CI's stall attribution (`scripts/stall-signature.sh`) reads every line with a
//! code as a failure in that second.
//!
//! Each refusal is still logged on its own, at DEBUG on the `colophon_api::refusals` target, with
//! its code: off at the default `info`, and the per-second evidence CI turns on.

use std::{
    collections::{HashMap, hash_map::Entry},
    sync::{LazyLock, Mutex, PoisonError},
    time::Duration,
};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::health::Dependency;

/// How often an ongoing outage is summarised, and how long it must stay quiet to be over.
pub const WINDOW: Duration = Duration::from_secs(10);

/// What repeats while an outage lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fault {
    /// A request refused: no database connection came free within the acquire timeout.
    NoConnection,
    /// A dependency's probe failed.
    Down(Dependency),
}

/// One outage in progress.
#[derive(Debug)]
struct Episode {
    since: OffsetDateTime,
    /// Occurrences since the last window closed (the first is said on its own, so not counted).
    in_window: u64,
    total: u64,
}

/// What closing a window found for one fault.
#[derive(Debug, PartialEq, Eq)]
enum Closed {
    /// It happened `count` more times in the window; the outage goes on.
    Continues {
        fault: Fault,
        count: u64,
        since: OffsetDateTime,
    },
    /// A whole window without it: the outage is over after `total` occurrences.
    Over {
        fault: Fault,
        total: u64,
        since: OffsetDateTime,
    },
}

/// The outages in progress, one per [`Fault`]. Bounded: one entry per fault.
#[derive(Debug, Default)]
pub struct Outages {
    episodes: Mutex<HashMap<Fault, Episode>>,
}

/// The process's outages: the request path and the probes report here.
pub static OUTAGES: LazyLock<Outages> = LazyLock::new(Outages::default);

impl Outages {
    /// Notes one occurrence of `fault`. True when it starts an outage: the caller says that one
    /// in full, at once. Every other occurrence is only counted, for the window's summary.
    pub fn begins(&self, fault: Fault) -> bool {
        let mut episodes = self.episodes.lock().unwrap_or_else(PoisonError::into_inner);
        match episodes.entry(fault) {
            Entry::Occupied(mut open) => {
                let open = open.get_mut();
                open.in_window += 1;
                open.total += 1;
                false
            }
            Entry::Vacant(vacant) => {
                vacant.insert(Episode {
                    since: OffsetDateTime::now_utc(),
                    in_window: 0,
                    total: 1,
                });
                true
            }
        }
    }

    /// Closes the current window: a summary for each outage that went on in it, a recovery line
    /// for each that didn't. Logs after the lock is released.
    pub fn close_window(&self) {
        for closed in self.close() {
            say(&closed);
        }
    }

    /// At shutdown: summarises what the open windows counted, without calling anything over.
    pub fn flush(&self) {
        for closed in self.close() {
            if matches!(closed, Closed::Continues { .. }) {
                say(&closed);
            }
        }
    }

    fn close(&self) -> Vec<Closed> {
        let mut episodes = self.episodes.lock().unwrap_or_else(PoisonError::into_inner);
        let mut closed = Vec::with_capacity(episodes.len());
        episodes.retain(|&fault, episode| {
            if episode.in_window > 0 {
                closed.push(Closed::Continues {
                    fault,
                    count: episode.in_window,
                    since: episode.since,
                });
                episode.in_window = 0;
                true
            } else {
                closed.push(Closed::Over {
                    fault,
                    total: episode.total,
                    since: episode.since,
                });
                false
            }
        });
        closed
    }
}

/// A request refused because no database connection came free (503): its own DEBUG line (the
/// per-second evidence), and the outage's first WARN if it starts one.
pub(crate) fn refused(outages: &Outages, code: &str) {
    tracing::debug!(
        target: "colophon_api::refusals",
        code,
        "request refused: no database connection free"
    );
    if outages.begins(Fault::NoConnection) {
        // Operators need to see saturation. The target is the one this line always had.
        tracing::warn!(
            target: "colophon_api::error",
            code,
            "request refused: no database connection free"
        );
    }
}

fn say(closed: &Closed) {
    let window = WINDOW.as_secs();
    match *closed {
        Closed::Continues {
            fault: Fault::NoConnection,
            count,
            since,
        } => {
            let since = stamp(since);
            tracing::warn!(
                refused = count,
                %since,
                "refused {count} requests in the last {window} s: no database connection free, since {since}"
            );
        }
        Closed::Over {
            fault: Fault::NoConnection,
            total,
            since,
        } => {
            let since = stamp(since);
            tracing::info!(
                refused = total,
                %since,
                "requests are answered again: none refused for {window} s (refused {total}: no database connection free, since {since})"
            );
        }
        Closed::Continues {
            fault: Fault::Down(dependency),
            count,
            since,
        } => {
            let since = stamp(since);
            tracing::warn!(
                dependency = dependency.as_str(),
                failed = count,
                %since,
                "dependency is still down: {count} failed probes in the last {window} s, since {since}"
            );
        }
        Closed::Over {
            fault: Fault::Down(dependency),
            total,
            since,
        } => {
            let since = stamp(since);
            tracing::info!(
                dependency = dependency.as_str(),
                failed = total,
                %since,
                "dependency is up: no failed probe for {window} s ({total} failed, since {since})"
            );
        }
    }
}

fn stamp(at: OffsetDateTime) -> String {
    at.format(&Rfc3339)
        .unwrap_or_else(|_| at.unix_timestamp().to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        sync::{Arc, Mutex, PoisonError},
    };

    use tracing_subscriber::fmt::MakeWriter;

    use super::*;

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Captured {
        fn lines(&self) -> Vec<serde_json::Value> {
            let bytes = self
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            String::from_utf8_lossy(&bytes)
                .lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect()
        }
    }

    impl io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Captured {
        type Writer = Self;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// Runs `body` with every line at `filter` captured as JSON, as the server writes them.
    fn capture(filter: &str, body: impl FnOnce()) -> Vec<serde_json::Value> {
        let captured = Captured::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_env_filter(filter)
            .with_writer(captured.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, body);
        captured.lines()
    }

    fn at(lines: &[serde_json::Value], level: &str) -> usize {
        lines.iter().filter(|line| line["level"] == level).count()
    }

    #[test]
    fn a_burst_of_two_thousand_refusals_is_three_lines_at_the_default_level() {
        let outages = Outages::default();
        let lines = capture("info", || {
            for _ in 0..2_000 {
                refused(&outages, "unavailable");
            }
            outages.close_window(); // the burst's window: a summary
            outages.close_window(); // a quiet window: over
        });
        assert_eq!(lines.len(), 3, "{lines:#?}");
        assert_eq!(at(&lines, "WARN"), 2);
        assert_eq!(
            lines[0]["message"],
            "request refused: no database connection free"
        );
        assert_eq!(
            lines[0]["code"], "unavailable",
            "the first is a real refusal, with its code"
        );
        assert_eq!(lines[1]["refused"], 1_999);
        assert!(
            lines[1]["message"]
                .as_str()
                .is_some_and(|text| text.starts_with("refused 1999 requests in the last 10 s")),
            "{}",
            lines[1]
        );
        assert_eq!(lines[2]["level"], "INFO");
        assert_eq!(lines[2]["refused"], 2_000);
        // Not failed requests: CI's stall attribution reads any line with a code or status as one.
        for line in &lines[1..] {
            assert!(
                line.get("code").is_none() && line.get("status").is_none(),
                "{line}"
            );
        }
    }

    #[test]
    fn a_long_outage_is_one_line_a_window_however_many_refusals() {
        let outages = Outages::default();
        let lines = capture("info", || {
            for _ in 0..30 {
                for _ in 0..500 {
                    refused(&outages, "unavailable");
                }
                outages.close_window();
            }
        });
        // 15,000 refusals over 30 windows: the first line and 30 summaries.
        assert_eq!(lines.len(), 31);
        assert_eq!(lines[1]["refused"], 499);
        assert_eq!(lines[30]["refused"], 500);
    }

    #[test]
    fn each_refusal_is_still_evidence_on_its_own_target_when_asked_for() {
        let outages = Outages::default();
        let lines = capture("info,colophon_api::refusals=debug", || {
            for _ in 0..2_000 {
                refused(&outages, "unavailable");
            }
        });
        let evidence = lines
            .iter()
            .filter(|line| {
                line["target"] == "colophon_api::refusals" && line["code"] == "unavailable"
            })
            .count();
        assert_eq!(evidence, 2_000);
        assert_eq!(at(&lines, "WARN"), 1, "the WARN is still said once");
    }

    #[test]
    fn an_outage_over_begins_again_in_full() {
        let outages = Outages::default();
        assert!(outages.begins(Fault::NoConnection));
        assert!(!outages.begins(Fault::NoConnection));
        outages.close_window();
        outages.close_window();
        assert!(
            outages.begins(Fault::NoConnection),
            "a new outage is said in full"
        );
        // Kinds are apart: the cache going down is its own outage.
        assert!(outages.begins(Fault::Down(Dependency::Cache)));
        assert!(!outages.begins(Fault::Down(Dependency::Cache)));
    }

    #[test]
    fn shutdown_summarises_without_calling_an_outage_over() {
        let outages = Outages::default();
        let lines = capture("info", || {
            outages.begins(Fault::Down(Dependency::Database));
            outages.begins(Fault::Down(Dependency::Database));
            outages.flush();
        });
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["dependency"], "database");
        assert_eq!(lines[0]["failed"], 1);
        assert_eq!(lines[0]["level"], "WARN");
    }
}
