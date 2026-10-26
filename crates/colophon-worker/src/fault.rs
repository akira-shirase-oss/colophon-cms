//! Fault injection for the crash tests (docs/modules/17-verification.md, F03). Only a build with
//! the `fault-injection` feature has it; in every other build `point` is empty.
//!
//! With the feature, `COLOPHON_FAULT=<name>` makes the process exit at the named point, as a
//! crash would.

/// A named point where a crash test can stop the process.
#[cfg(feature = "fault-injection")]
pub(crate) fn point(name: &str) {
    use std::sync::LazyLock;

    static FAULT: LazyLock<Option<String>> = LazyLock::new(|| std::env::var("COLOPHON_FAULT").ok());
    if FAULT.as_deref() == Some(name) {
        eprintln!("fault injected at {name}: exiting");
        std::process::exit(86);
    }
}

/// A named point where a crash test can stop the process (a no-op in this build).
#[cfg(not(feature = "fault-injection"))]
#[inline(always)]
pub(crate) fn point(_name: &str) {}
