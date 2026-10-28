//! What a Colophon build is: its release version, the git commit it was built from and the day
//! it was built (D-062, `docs/modules/19-oss.md`, "Versions").
//!
//! Every Colophon binary carries the workspace's one version. A binary stamps itself with a
//! build script that calls [`stamp::emit`] (this crate as a build-dependency with the `stamp`
//! feature), then reads the stamp with [`build_info!`] and [`long_version!`]:
//!
//! ```ignore
//! // build.rs
//! fn main() {
//!     colophon_version::stamp::emit();
//! }
//!
//! // src/main.rs
//! #[derive(clap::Parser)]
//! #[command(version = colophon_version::long_version!())]
//! struct Cli {}
//! ```
//!
//! Only the binaries have that build script, so a new commit rebuilds them and nothing else.
//! Libraries take a [`BuildInfo`] from the binary that runs them.

#[cfg(any(feature = "stamp", test, doc))]
pub mod stamp;

/// What a build reports about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildInfo {
    /// The release version (`version` in `[workspace.package]`), a semantic version.
    pub version: &'static str,
    /// The full git commit it was built from, or [`UNKNOWN`].
    pub commit: &'static str,
    /// The day it was built, UTC, as `YYYY-MM-DD`, or [`UNKNOWN`].
    pub build_date: &'static str,
}

/// What a build says when it couldn't learn its commit or date: a source tarball built
/// without git and without `COLOPHON_COMMIT`.
pub const UNKNOWN: &str = "unknown";

impl BuildInfo {
    /// A build that knows only its version: for tests and tools that never stamp themselves.
    pub const fn unstamped(version: &'static str) -> Self {
        Self {
            version,
            commit: UNKNOWN,
            build_date: UNKNOWN,
        }
    }
}

impl std::fmt::Display for BuildInfo {
    /// `0.1.0-alpha.1 (commit 50558a7…, built 2026-10-28)`, the same text as [`long_version!`].
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (commit {}, built {})",
            self.version, self.commit, self.build_date
        )
    }
}

/// The calling package's [`BuildInfo`]. It needs the package's build script to have called
/// [`stamp::emit`]; without it, compilation fails on the missing variables.
#[macro_export]
macro_rules! build_info {
    () => {
        $crate::BuildInfo {
            version: env!("CARGO_PKG_VERSION"),
            commit: env!("COLOPHON_COMMIT"),
            build_date: env!("COLOPHON_BUILD_DATE"),
        }
    };
}

/// The calling package's version line as a `&'static str`, for `--version`:
/// `0.1.0-alpha.1 (commit 50558a7…, built 2026-10-28)`.
#[macro_export]
macro_rules! long_version {
    () => {
        concat!(
            env!("CARGO_PKG_VERSION"),
            " (commit ",
            env!("COLOPHON_COMMIT"),
            ", built ",
            env!("COLOPHON_BUILD_DATE"),
            ")"
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_matches_the_long_version_layout() {
        let build = BuildInfo {
            version: "0.1.0-alpha.1",
            commit: "50558a7c0ffee",
            build_date: "2026-10-28",
        };
        assert_eq!(
            build.to_string(),
            "0.1.0-alpha.1 (commit 50558a7c0ffee, built 2026-10-28)"
        );
    }

    #[test]
    fn an_unstamped_build_says_unknown_rather_than_inventing_values() {
        let build = BuildInfo::unstamped("1.2.3");
        assert_eq!(build.to_string(), "1.2.3 (commit unknown, built unknown)");
    }
}
