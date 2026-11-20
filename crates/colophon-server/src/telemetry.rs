//! Logging: JSON lines in production, pretty locally, one `EnvFilter` for both, and a
//! fallible setup so a bad filter fails startup loudly. Slow statements are said in two tiers
//! (D-224, [`SlowStatementTiers`]). Beside the log layers, the database's begin tripwire (D-226)
//! watches sqlx's backend notices with a filter of its own, so the log filter (`COLOPHON_LOG`)
//! can't blind it: that filter applies to the log layers, not globally.

use std::time::Duration;

use tracing::{
    Event, Level, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{
    EnvFilter,
    filter::{FilterExt as _, ParseError},
    fmt,
    layer::{Context, Layer, SubscriberExt as _},
    util::{SubscriberInitExt as _, TryInitError},
};

use crate::config::LogFormat;

/// Why logging could not be set up.
#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    /// `COLOPHON_LOG` did not parse.
    #[error("COLOPHON_LOG: invalid filter {filter:?}")]
    Filter {
        /// The rejected directives.
        filter: String,
        /// The parser's error.
        #[source]
        source: ParseError,
    },
    /// A global subscriber was already installed.
    #[error("a global tracing subscriber is already installed")]
    AlreadyInstalled(#[from] TryInitError),
}

/// Parses filter directives such as `info,colophon_server=debug`.
pub fn filter(directives: &str) -> Result<EnvFilter, TelemetryError> {
    EnvFilter::try_new(directives).map_err(|source| TelemetryError::Filter {
        filter: directives.to_owned(),
        source,
    })
}

/// Installs the global subscriber. Call once, before anything logs.
pub fn init(format: LogFormat, directives: &str) -> Result<(), TelemetryError> {
    let filter = filter(directives)?;
    // `Option<Layer>` is a `Layer` and `None` is a no-op: one registry type for both formats.
    let (json, pretty) = match format {
        LogFormat::Json => (
            Some(
                fmt::layer()
                    .json()
                    .flatten_event(true)
                    .with_current_span(true)
                    .with_span_list(false),
            ),
            None,
        ),
        LogFormat::Pretty => (None, Some(fmt::layer().pretty())),
    };
    tracing_subscriber::registry()
        // Before the log layers and the tripwire: an INFO statement under the INFO tier is dropped
        // for every layer (D-224). The tripwire reads backend notices, never `sqlx::query`.
        .with(SlowStatementTiers::default())
        // `Layer::and_then` named in full: `Option` has an `and_then` of its own.
        // The startup self-test's deliberate nested BEGIN logs no WARN (D-226): its notice is
        // dropped from the log layers by where it was raised; a real one still logs.
        .with(
            tracing_subscriber::Layer::and_then(json, pretty)
                .with_filter(filter.and(colophon_db::transactions::hide_the_self_test_notice())),
        )
        .with(colophon_db::transactions::begin_tripwire())
        .try_init()?;
    Ok(())
}

/// The lower tier of D-224's slow statements. With the tiers set, sqlx offers every statement at INFO
/// (target `sqlx::query`, with its `elapsed_secs`) and says one past the WARN tier at WARN, "slow
/// statement". This keeps an INFO statement only from the INFO tier on (100 ms by default): the
/// evidence that names a stall's statements, never every statement. A layer can drop an event but
/// not say another in its place (tracing gives a nested event the no-op dispatcher), so the tiers
/// are sqlx's two levels and this filter.
#[derive(Debug, Default)]
pub struct SlowStatementTiers {
    /// The INFO tier; `None` reads the database layer's, as the server set it at start.
    info: Option<Duration>,
}

impl SlowStatementTiers {
    /// With its own INFO tier, for tests.
    pub fn with_info(info: Duration) -> Self {
        Self { info: Some(info) }
    }
}

/// A statement's `elapsed_secs`, the one field the filter reads.
#[derive(Debug, Default)]
struct Elapsed(Option<f64>);

impl Visit for Elapsed {
    fn record_f64(&mut self, field: &Field, value: f64) {
        if field.name() == "elapsed_secs" {
            self.0 = Some(value);
        }
    }

    fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}
}

impl<S: Subscriber> Layer<S> for SlowStatementTiers {
    fn event_enabled(&self, event: &Event<'_>, _ctx: Context<'_, S>) -> bool {
        let metadata = event.metadata();
        if *metadata.level() != Level::INFO || metadata.target() != "sqlx::query" {
            return true;
        }
        let mut elapsed = Elapsed::default();
        event.record(&mut elapsed);
        let info = self
            .info
            .unwrap_or_else(|| colophon_db::slow_statements().info);
        elapsed.0.is_some_and(|secs| secs >= info.as_secs_f64())
    }
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

    /// What the server writes for statements of `elapsed` seconds, offered as sqlx offers them with the
    /// tiers set (1 s for WARN): (level, message, `elapsed_secs`) per line.
    fn logged(directives: &str, elapsed: &[f64]) -> Vec<(String, String, f64)> {
        let captured = Captured::default();
        let subscriber = tracing_subscriber::registry()
            .with(filter(directives).expect("a valid filter"))
            .with(SlowStatementTiers::with_info(Duration::from_millis(100)))
            .with(
                fmt::layer()
                    .json()
                    .flatten_event(true)
                    .with_writer(captured.clone()),
            );
        tracing::subscriber::with_default(subscriber, || {
            for &elapsed_secs in elapsed {
                // As sqlx-core's logger says them: past the WARN tier at WARN with its message, the
                // rest at the statements' level (INFO) without one.
                if elapsed_secs >= 1.0 {
                    tracing::warn!(
                        target: "sqlx::query",
                        summary = "COMMIT",
                        db.statement = "",
                        rows_affected = 0_u64,
                        rows_returned = 0_u64,
                        elapsed_secs,
                        "slow statement: execution time exceeded alert threshold"
                    );
                } else {
                    tracing::info!(
                        target: "sqlx::query",
                        summary = "COMMIT",
                        db.statement = "",
                        rows_affected = 0_u64,
                        rows_returned = 0_u64,
                        elapsed_secs,
                    );
                }
            }
        });
        let bytes = captured
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        String::from_utf8_lossy(&bytes)
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .map(|line| {
                (
                    line["level"].as_str().unwrap_or_default().to_owned(),
                    line["message"].as_str().unwrap_or_default().to_owned(),
                    line["elapsed_secs"].as_f64().unwrap_or(-1.0),
                )
            })
            .collect()
    }

    fn levels(lines: &[(String, String, f64)]) -> Vec<(&str, f64)> {
        lines
            .iter()
            .map(|(level, _, secs)| (level.as_str(), *secs))
            .collect()
    }

    #[test]
    fn info_keeps_statements_from_the_info_tier_and_warn_past_the_warn_tier() {
        let lines = logged("info", &[0.002, 0.099, 0.1, 0.217, 0.999, 1.4]);
        assert_eq!(
            levels(&lines),
            [
                ("INFO", 0.1),
                ("INFO", 0.217),
                ("INFO", 0.999),
                ("WARN", 1.4)
            ],
            "every statement under 100 ms is dropped"
        );
        assert!(lines[3].1.starts_with("slow statement"));
    }

    #[test]
    fn at_warn_only_the_alerts_show() {
        let lines = logged("warn", &[0.002, 0.217, 0.999, 1.4]);
        assert_eq!(levels(&lines), [("WARN", 1.4)]);
    }

    #[test]
    fn evidence_at_info_for_sqlx_alone_beside_warn() {
        // CI's smokes job: warn, with the statements' evidence (D-224).
        let lines = logged("warn,sqlx::query=info", &[0.002, 0.217, 1.4]);
        assert_eq!(levels(&lines), [("INFO", 0.217), ("WARN", 1.4)]);
    }

    #[test]
    fn accepts_typical_filters() {
        for directives in [
            "info",
            "warn,colophon_server=debug",
            "info,tower_http=debug",
        ] {
            assert!(filter(directives).is_ok(), "{directives}");
        }
    }

    #[test]
    fn rejects_a_bad_filter_and_names_it() {
        let error = filter("info,tower_http=loud").expect_err("`loud` is not a level");
        assert!(error.to_string().contains("tower_http=loud"));
    }
}
