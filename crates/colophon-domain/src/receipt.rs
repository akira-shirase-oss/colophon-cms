//! Delivery receipts (docs/modules/06-delivery.md, "Evidence probes"; docs/modules/11-studio.md,
//! "Publishing labels"; spec §05, §14; D-051): what a probe's answer proves, and the one label a
//! release shows.
//!
//! A probe fetches a URL and reads the generation the answer was built from: the
//! `Colophon-Generation` header, or a `<meta name="colophon-generation" content="N">` marker in a
//! page (a static site can't send headers of its own). Seeing release N or a later one proves the
//! release reached that place, as seen from that location, at that moment; nothing more. A release
//! is never just "verified": it is verified at the number of locations that saw it.

use crate::job::{self, Outcome};

/// How long a probe waits at least before looking again when it saw an older release: sites and
/// shared caches take a while to change, and asking sooner proves nothing new.
pub const PROBE_INTERVAL_MS: u64 = 5_000;

/// What a probe's answer says about the release it looked for.
pub fn probe(status: u16, seen: Option<i64>, wanted: i64) -> Outcome {
    match (status, seen) {
        (200..=299, Some(generation)) if generation >= wanted => Outcome::Delivered { status },
        (200..=299, Some(generation)) => Outcome::Retryable {
            reason: format!("saw release {generation}, not yet {wanted}"),
            retry_after_ms: Some(PROBE_INTERVAL_MS),
        },
        (200..=299, None) => Outcome::Retryable {
            reason: "the answer names no release".to_owned(),
            retry_after_ms: Some(PROBE_INTERVAL_MS),
        },
        _ => job::classify(status, None),
    }
}

/// The release a page says it was built from: the `content` of its first
/// `<meta name="colophon-generation" content="N">`, attributes in that order and double-quoted, as
/// the reference site writes it. Anything else is no marker: a probe doesn't guess.
pub fn marker(page: &str) -> Option<i64> {
    const OPEN: &str = r#"<meta name="colophon-generation" content=""#;
    let start = page.find(OPEN)? + OPEN.len();
    let rest = page.get(start..)?;
    let digits = rest.get(..rest.find('"')?)?;
    if digits.is_empty() || digits.len() > 18 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// A destination's kind, as far as a receipt cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Hears about each release.
    Webhook,
    /// Is told what to purge.
    Cdn,
    /// Looks for the release from outside.
    Probe,
}

/// Where one destination stands for a release, from its jobs at that release's generation or
/// later: a later success covers an earlier release too (an announcement of release 7 tells a
/// site about 6's changes as well, a purge covers every release since the last one, and a probe
/// that saw 7 saw at least 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Destination {
    /// Its kind.
    pub kind: Kind,
    /// One of those jobs succeeded.
    pub covered: bool,
    /// One of them is still waiting or running.
    pub open: bool,
}

/// The one label a release shows (spec §05).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label {
    /// Committed; nothing was configured to deliver or check it, or nothing checks the site.
    PublishedInCms,
    /// A delivery or check is still on its way.
    UpdatePending,
    /// Every destination is done, and probes saw it at this many locations.
    VerifiedAt {
        /// How many distinct locations saw this release or a later one.
        locations: u32,
    },
    /// A delivery or check gave up: someone should look.
    NeedsAttention,
}

impl Label {
    /// The name the API writes.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PublishedInCms => "published",
            Self::UpdatePending => "pending",
            Self::VerifiedAt { .. } => "verified",
            Self::NeedsAttention => "needs_attention",
        }
    }
}

/// The label of a release whose destinations stand as `destinations`, with probes having seen it
/// at `locations` distinct locations. A failure anywhere outranks everything; anything still open
/// means pending; only probes can verify, and only where they looked.
pub fn label(destinations: &[Destination], locations: u32) -> Label {
    if destinations
        .iter()
        .any(|destination| !destination.covered && !destination.open)
    {
        return Label::NeedsAttention;
    }
    if destinations.iter().any(|destination| !destination.covered) {
        return Label::UpdatePending;
    }
    let probed = destinations
        .iter()
        .any(|destination| destination.kind == Kind::Probe);
    if probed && locations > 0 {
        Label::VerifiedAt { locations }
    } else {
        Label::PublishedInCms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_probe_passes_only_on_the_release_or_a_later_one() {
        assert_eq!(probe(200, Some(7), 7), Outcome::Delivered { status: 200 });
        assert_eq!(
            probe(200, Some(8), 7),
            Outcome::Delivered { status: 200 },
            "a later release has 7's changes"
        );
        assert_eq!(
            probe(200, Some(6), 7),
            Outcome::Retryable {
                reason: "saw release 6, not yet 7".to_owned(),
                retry_after_ms: Some(PROBE_INTERVAL_MS)
            }
        );
        assert!(matches!(probe(200, None, 7), Outcome::Retryable { .. }));
        assert!(matches!(probe(503, Some(7), 7), Outcome::Retryable { .. }));
        assert!(
            matches!(probe(404, Some(7), 7), Outcome::Terminal { .. }),
            "an error page proves nothing, even with a marker"
        );
    }

    #[test]
    fn a_marker_is_read_exactly_or_not_at_all() {
        let page = r#"<html><head><meta name="colophon-generation" content="42"></head></html>"#;
        assert_eq!(marker(page), Some(42));
        for page in [
            "",
            r#"<meta name="colophon-generation" content="">"#,
            r#"<meta name="colophon-generation" content="4x">"#,
            r#"<meta name="colophon-generation" content="-1">"#,
            r#"<meta content="42" name="colophon-generation">"#,
            r#"<meta name="colophon-generation" content="42"#,
            r#"<meta name="colophon-generation" content="9999999999999999999">"#,
        ] {
            assert_eq!(marker(page), None, "{page:?}");
        }
        let two = r#"<meta name="colophon-generation" content="3"><meta name="colophon-generation" content="9">"#;
        assert_eq!(marker(two), Some(3), "the first one counts");
    }

    #[test]
    fn a_label_never_claims_more_than_was_seen() {
        let done = |kind| Destination {
            kind,
            covered: true,
            open: false,
        };
        let waiting = |kind| Destination {
            kind,
            covered: false,
            open: true,
        };
        let failed = |kind| Destination {
            kind,
            covered: false,
            open: false,
        };
        assert_eq!(
            label(&[], 0),
            Label::PublishedInCms,
            "nothing to deliver or check"
        );
        assert_eq!(
            label(&[done(Kind::Webhook), done(Kind::Cdn)], 0),
            Label::PublishedInCms,
            "delivered, but nothing looked at the site"
        );
        assert_eq!(
            label(&[done(Kind::Webhook), done(Kind::Probe)], 1),
            Label::VerifiedAt { locations: 1 }
        );
        assert_eq!(
            label(&[done(Kind::Probe), waiting(Kind::Cdn)], 1),
            Label::UpdatePending
        );
        assert_eq!(
            label(&[failed(Kind::Cdn), waiting(Kind::Probe)], 0),
            Label::NeedsAttention,
            "a failure outranks what's still going"
        );
        assert_eq!(
            label(&[done(Kind::Probe)], 0),
            Label::PublishedInCms,
            "a probe success with no location on record verifies nothing"
        );
        assert_eq!(Label::VerifiedAt { locations: 2 }.as_str(), "verified");
    }
}
