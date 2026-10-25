//! Logging: JSON lines in production, pretty locally, one `EnvFilter` for both, and a
//! fallible setup so a bad filter fails startup loudly.

use tracing_subscriber::{
    EnvFilter,
    filter::ParseError,
    fmt,
    layer::SubscriberExt as _,
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
        .with(filter)
        .with(json)
        .with(pretty)
        .try_init()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
