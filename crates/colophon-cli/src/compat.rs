//! Whether this colophon fits the server it talks to, and the plain words for when it doesn't
//! (D-062; the rule is in `docs/modules/08-cli.md`, "Versions").
//!
//! The server says what it is at `GET /version`: its release and the API versions it serves.
//! This colophon fits when, in order:
//!
//! 1. the server serves the API version this colophon speaks ([`API`]);
//! 2. before 1.0, both are on the same minor line (0.1.x with 0.1.x): until 1.0 a minor release
//!    may change the API (SemVer item 4), so neither side can promise more;
//! 3. the server is at least [`OLDEST_SERVER`], the oldest that has everything this colophon
//!    calls.
//!
//! From 1.0 on, `/api/vN` is the contract: a newer server that still serves this colophon's API
//! version fits. Pre-release tags (`-alpha.1`) are ordered by SemVer precedence and otherwise
//! belong to their line.

use std::cmp::Ordering;

use semver::Version;
use serde_json::Value;

/// The API version this colophon speaks.
pub(crate) const API: &str = "v1";

/// The oldest server with everything this colophon calls. Raise it in the change that makes a
/// command use a route or field older servers lack. 0.1.0-alpha.1 is the first server that
/// reports its version.
pub(crate) const OLDEST_SERVER: &str = "0.1.0-alpha.1";

/// Where to get another colophon.
const RELEASES: &str = "https://github.com/akira-shirase-oss/colophon-cms/releases";

/// What a server's `/version` says, read loosely: extra fields are ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Report {
    pub(crate) version: Version,
    pub(crate) api: Vec<String>,
}

impl Report {
    /// The report in a `/version` body; `None` if it isn't one (a missing or malformed
    /// `version`, or an `api` that isn't a list of strings).
    pub(crate) fn read(body: &Value) -> Option<Self> {
        let version = Version::parse(body["version"].as_str()?).ok()?;
        let api = body["api"]
            .as_array()?
            .iter()
            .map(|item| item.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()?;
        Some(Self { version, api })
    }
}

/// Whether a server fits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    Fits,
    Misfit(Reason),
}

/// Why a server doesn't fit. Each reason says which side should change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reason {
    /// The server serves only API versions newer than this colophon's.
    ApiDropped,
    /// The server serves only API versions older than this colophon's.
    ApiNotYet,
    /// Before 1.0: the server is on a later minor line.
    LaterLine,
    /// Before 1.0: the server is on an earlier minor line.
    EarlierLine,
    /// The server is older than [`OLDEST_SERVER`].
    BeforeOldest,
}

impl Reason {
    /// True when this colophon is too old for the server (install a newer colophon); false when
    /// it is too new (upgrade the server, or install an older colophon).
    pub(crate) const fn colophon_too_old(self) -> bool {
        matches!(self, Self::ApiDropped | Self::LaterLine)
    }
}

/// The rule in the module documentation, as a pure function.
pub(crate) fn verdict(
    client: &Version,
    api: &str,
    oldest_server: &Version,
    server: &Report,
) -> Verdict {
    if !server.api.iter().any(|served| served == api) {
        let offered: Vec<u64> = server.api.iter().filter_map(|v| api_number(v)).collect();
        let newer = match api_number(api) {
            Some(ours) if !offered.is_empty() && offered.iter().all(|&theirs| theirs > ours) => {
                true
            }
            Some(ours) if !offered.is_empty() && offered.iter().all(|&theirs| theirs < ours) => {
                false
            }
            // No clear order between the API versions: go by release.
            _ => server.version.cmp_precedence(client) == Ordering::Greater,
        };
        return Verdict::Misfit(if newer {
            Reason::ApiDropped
        } else {
            Reason::ApiNotYet
        });
    }
    if client.major == 0 || server.version.major == 0 {
        match line(&server.version).cmp(&line(client)) {
            Ordering::Greater => return Verdict::Misfit(Reason::LaterLine),
            Ordering::Less => return Verdict::Misfit(Reason::EarlierLine),
            Ordering::Equal => {}
        }
    }
    if server.version.cmp_precedence(oldest_server) == Ordering::Less {
        return Verdict::Misfit(Reason::BeforeOldest);
    }
    Verdict::Fits
}

/// `v1` → 1.
fn api_number(api: &str) -> Option<u64> {
    api.strip_prefix('v')?.parse().ok()
}

/// The release line a version promises compatibility within: its major version, or before 1.0
/// its minor one.
fn line(version: &Version) -> (u64, u64) {
    if version.major == 0 {
        (0, version.minor)
    } else {
        (version.major, 0)
    }
}

fn line_name(version: &Version) -> String {
    let (major, minor) = line(version);
    if major == 0 {
        format!("0.{minor}")
    } else {
        major.to_string()
    }
}

/// Checks the server's report against this colophon; the explanation when it doesn't fit.
pub(crate) fn check(server_url: &str, report: &Report) -> Result<(), String> {
    let (client, oldest) = ours()?;
    match verdict(&client, API, &oldest, report) {
        Verdict::Fits => Ok(()),
        Verdict::Misfit(reason) => Err(explain(reason, &client, &oldest, server_url, report)),
    }
}

/// This colophon's version and the oldest server it needs.
fn ours() -> Result<(Version, Version), String> {
    let parse = |text: &str| {
        Version::parse(text)
            .map_err(|error| format!("this colophon's own version {text:?}: {error}"))
    };
    Ok((parse(env!("CARGO_PKG_VERSION"))?, parse(OLDEST_SERVER)?))
}

/// The explanation for a server that answers `/version` with "no such route": it predates
/// version reports, so it is older than any server this colophon fits.
pub(crate) fn no_version_report(server_url: &str) -> String {
    format!(
        "colophon {} is too new for the server at {server_url}: the server doesn't report its \
         version, so it predates {OLDEST_SERVER}, the oldest server this colophon works with. \
         Upgrade the server.",
        env!("CARGO_PKG_VERSION"),
    )
}

fn explain(
    reason: Reason,
    client: &Version,
    oldest: &Version,
    server_url: &str,
    report: &Report,
) -> String {
    let server = &report.version;
    let apis = if report.api.is_empty() {
        "none".to_owned()
    } else {
        report.api.join(", ")
    };
    let pre_one = |side: &str| {
        format!(
            "before 1.0 each minor release may change the API, so colophon and the server must \
             share a minor version, and the server is on {side} one ({} here, {} there)",
            line_name(client),
            line_name(server)
        )
    };
    let why = match reason {
        Reason::ApiDropped => format!(
            "the server no longer serves API {API}, which this colophon speaks (it serves {apis})"
        ),
        Reason::ApiNotYet => format!(
            "this colophon speaks API {API}, which the server doesn't serve (it serves {apis})"
        ),
        Reason::LaterLine => pre_one("a later"),
        Reason::EarlierLine => pre_one("an earlier"),
        Reason::BeforeOldest => format!("this colophon needs a server of at least {oldest}"),
    };
    if reason.colophon_too_old() {
        format!(
            "colophon {client} is too old for the server at {server_url}, which runs {server}: \
             {why}. Install colophon {server} to match it ({RELEASES})."
        )
    } else {
        format!(
            "colophon {client} is too new for the server at {server_url}, which runs {server}: \
             {why}. Upgrade the server, or install colophon {server} to match it ({RELEASES})."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).expect("a valid version in the test")
    }

    fn server(version: &str, api: &[&str]) -> Report {
        Report {
            version: v(version),
            api: api.iter().map(|&name| name.to_owned()).collect(),
        }
    }

    /// The verdict for a colophon at `client` speaking v1 that needs at least 0.1.0-alpha.1.
    fn judge(client: &str, version: &str, api: &[&str]) -> Verdict {
        verdict(&v(client), "v1", &v("0.1.0-alpha.1"), &server(version, api))
    }

    #[test]
    fn the_same_release_fits() {
        assert_eq!(
            judge("0.1.0-alpha.1", "0.1.0-alpha.1", &["v1"]),
            Verdict::Fits
        );
        assert_eq!(judge("1.4.2", "1.4.2", &["v1"]), Verdict::Fits);
    }

    #[test]
    fn before_1_0_the_minor_line_must_match_in_both_directions() {
        // Pre-releases and patches within a line fit, whichever side is newer.
        assert_eq!(
            judge("0.1.0-alpha.1", "0.1.0-alpha.3", &["v1"]),
            Verdict::Fits
        );
        assert_eq!(judge("0.1.0", "0.1.0-alpha.2", &["v1"]), Verdict::Fits);
        assert_eq!(judge("0.1.0-alpha.1", "0.1.7", &["v1"]), Verdict::Fits);
        assert_eq!(judge("0.1.7", "0.1.0", &["v1"]), Verdict::Fits);
        // Another minor line doesn't, and the verdict says which side should move.
        assert_eq!(
            judge("0.1.0-alpha.1", "0.2.0", &["v1"]),
            Verdict::Misfit(Reason::LaterLine)
        );
        assert_eq!(
            judge("0.3.1", "0.2.9", &["v1"]),
            Verdict::Misfit(Reason::EarlierLine)
        );
        // Crossing 1.0 is a line change too.
        assert_eq!(
            judge("0.9.0", "1.0.0", &["v1"]),
            Verdict::Misfit(Reason::LaterLine)
        );
        assert_eq!(
            judge("1.0.0", "0.9.0", &["v1"]),
            Verdict::Misfit(Reason::EarlierLine)
        );
    }

    #[test]
    fn from_1_0_the_api_version_is_the_contract() {
        assert_eq!(judge("1.0.0", "1.9.0", &["v1"]), Verdict::Fits);
        assert_eq!(judge("1.9.0", "1.0.0", &["v1"]), Verdict::Fits);
        assert_eq!(judge("1.2.0", "2.0.0", &["v1", "v2"]), Verdict::Fits);
        assert_eq!(
            judge("1.2.0", "3.0.0", &["v2"]),
            Verdict::Misfit(Reason::ApiDropped)
        );
        assert_eq!(
            verdict(&v("2.0.0"), "v2", &v("2.0.0"), &server("1.8.0", &["v1"])),
            Verdict::Misfit(Reason::ApiNotYet)
        );
    }

    #[test]
    fn a_server_older_than_the_oldest_supported_is_too_old_for_this_colophon() {
        let oldest = v("1.3.0");
        assert_eq!(
            verdict(&v("1.5.0"), "v1", &oldest, &server("1.2.9", &["v1"])),
            Verdict::Misfit(Reason::BeforeOldest)
        );
        assert_eq!(
            verdict(&v("1.5.0"), "v1", &oldest, &server("1.3.0-rc.1", &["v1"])),
            Verdict::Misfit(Reason::BeforeOldest),
            "a pre-release comes before its release"
        );
        assert_eq!(
            verdict(&v("1.5.0"), "v1", &oldest, &server("1.3.0", &["v1"])),
            Verdict::Fits
        );
    }

    #[test]
    fn the_api_check_comes_first_and_handles_odd_lists() {
        // The API decides even within one line.
        assert_eq!(
            judge("0.1.0", "0.1.5", &["v2"]),
            Verdict::Misfit(Reason::ApiDropped)
        );
        assert_eq!(
            verdict(&v("0.1.0"), "v2", &v("0.1.0"), &server("0.1.5", &["v1"])),
            Verdict::Misfit(Reason::ApiNotYet)
        );
        // Nothing served, or names we can't order: the releases decide the direction.
        assert_eq!(
            judge("0.1.0", "0.1.5", &[]),
            Verdict::Misfit(Reason::ApiDropped)
        );
        assert_eq!(
            judge("0.1.5", "0.1.0", &["beta"]),
            Verdict::Misfit(Reason::ApiNotYet)
        );
        // v0 and v2 around our v1: no clear order either.
        assert_eq!(
            judge("1.0.0", "2.0.0", &["v0", "v2"]),
            Verdict::Misfit(Reason::ApiDropped)
        );
    }

    #[test]
    fn reports_are_read_loosely_but_never_guessed() {
        let body = serde_json::json!({
            "version": "0.1.0-alpha.1", "commit": "abc", "build_date": "2026-10-28",
            "api": ["v1"], "added_later": true
        });
        assert_eq!(Report::read(&body), Some(server("0.1.0-alpha.1", &["v1"])));
        for broken in [
            serde_json::json!({ "api": ["v1"] }),
            serde_json::json!({ "version": "1.0", "api": ["v1"] }),
            serde_json::json!({ "version": "0.1.0" }),
            serde_json::json!({ "version": "0.1.0", "api": "v1" }),
            serde_json::json!({ "version": "0.1.0", "api": [1] }),
            serde_json::json!({ "error": { "code": "route_not_found" } }),
        ] {
            assert_eq!(Report::read(&broken), None, "{broken}");
        }
    }

    #[test]
    fn each_reason_says_which_side_should_move() {
        assert!(Reason::ApiDropped.colophon_too_old());
        assert!(Reason::LaterLine.colophon_too_old());
        assert!(!Reason::ApiNotYet.colophon_too_old());
        assert!(!Reason::EarlierLine.colophon_too_old());
        assert!(!Reason::BeforeOldest.colophon_too_old());
    }

    #[test]
    fn explanations_name_both_versions_and_what_to_do() {
        let url = "https://cms.example";
        let message = explain(
            Reason::LaterLine,
            &v("0.1.0"),
            &v("0.1.0-alpha.1"),
            url,
            &server("0.2.0", &["v1"]),
        );
        assert_eq!(
            message,
            "colophon 0.1.0 is too old for the server at https://cms.example, which runs 0.2.0: \
             before 1.0 each minor release may change the API, so colophon and the server must \
             share a minor version, and the server is on a later one (0.1 here, 0.2 there). \
             Install colophon 0.2.0 to match it \
             (https://github.com/akira-shirase-oss/colophon-cms/releases)."
        );
        let message = explain(
            Reason::BeforeOldest,
            &v("1.5.0"),
            &v("1.3.0"),
            url,
            &server("1.2.0", &["v1"]),
        );
        assert!(message.starts_with("colophon 1.5.0 is too new for the server at https://cms.example, which runs 1.2.0: this colophon needs a server of at least 1.3.0. Upgrade the server"), "{message}");
        let message = explain(
            Reason::ApiDropped,
            &v("1.0.0"),
            &v("1.0.0"),
            url,
            &server("2.0.0", &["v2"]),
        );
        assert!(
            message.contains("no longer serves API v1, which this colophon speaks (it serves v2)"),
            "{message}"
        );
    }

    #[test]
    fn this_colophon_s_own_versions_parse() {
        let (client, oldest) = ours().expect("valid");
        assert!(
            oldest.cmp_precedence(&client) != Ordering::Greater,
            "OLDEST_SERVER ({oldest}) is newer than this colophon ({client})"
        );
    }
}
