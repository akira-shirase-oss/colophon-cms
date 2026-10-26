//! Changesets and their review (docs/modules/01-domain.md, "Editorial workflow"; spec §06; D-044).
//!
//! A changeset pins exact revisions: for each entry, the revision, its content hash, and the
//! content-type version it was validated against. Its hash is the SHA-256 of [`canonical`], so
//! two changesets with the same items have the same hash whatever order they were added in, and
//! any difference in any item changes it. A review decision records the hash it was made on, and
//! only counts while the changeset still has that hash: refreshing a changeset after an edit
//! re-pins the new revisions, which changes the hash, so earlier approvals no longer count.
//!
//! This module decides; it doesn't hash (the domain has no crypto dependency) or store.

use std::fmt::Write as _;

/// The version line every canonical form starts with; changing the format changes it.
pub const FORMAT: &str = "colophon/changeset v1";

/// How long an approval counts before it must be given again (spec §06: "expired approval forces
/// revalidation"). Seven days, in seconds.
pub const APPROVAL_LIFETIME_SECS: i64 = 7 * 24 * 60 * 60;

/// One entry in a changeset, pinned to an exact revision. Ids and hashes are as the database
/// writes them: lowercase hyphenated UUIDs and lowercase hex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The entry.
    pub entry: String,
    /// The exact revision of it.
    pub revision: String,
    /// Its collection (content type slug).
    pub collection: String,
    /// The revision's content hash.
    pub content_hash: String,
    /// The content-type version the revision was validated against.
    pub schema_version: i32,
    /// That version's definition hash.
    pub definition_hash: String,
}

/// The exact text whose SHA-256 is a changeset's hash: the format line, then one line per item,
/// sorted by entry id, each field in a fixed order separated by spaces. None of the fields can
/// contain a space or a newline (ids, slugs, hex and an integer), so the form is unambiguous.
pub fn canonical(items: &[Item]) -> String {
    let mut sorted: Vec<&Item> = items.iter().collect();
    sorted.sort_by(|a, b| a.entry.cmp(&b.entry));
    let mut text = String::from(FORMAT);
    for item in sorted {
        // Writing to a String can't fail.
        let _ = write!(
            text,
            "\n{} {} {} {} {} {}",
            item.entry,
            item.revision,
            item.collection,
            item.content_hash,
            item.schema_version,
            item.definition_hash
        );
    }
    text
}

/// A reviewer's decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Approved the changeset with the hash the review records.
    Approve,
    /// Asked for changes to it.
    RequestChanges,
}

impl Decision {
    /// The name stored and used in the API.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::RequestChanges => "request_changes",
        }
    }

    /// The decision with this name.
    pub fn parse(name: &str) -> Option<Self> {
        [Self::Approve, Self::RequestChanges]
            .into_iter()
            .find(|decision| decision.as_str() == name)
    }
}

/// A recorded review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    /// Who reviewed.
    pub reviewer: String,
    /// What they decided.
    pub decision: Decision,
    /// The changeset hash they decided on.
    pub hash: String,
    /// When, in Unix seconds.
    pub at: i64,
}

/// What a changeset's status is decided from.
#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    /// The changeset's current hash.
    pub hash: &'a str,
    /// Who submitted it for review, if anyone has.
    pub submitted_by: Option<&'a str>,
    /// How many of its entries have a newer revision than the one pinned.
    pub stale_items: usize,
    /// Its reviews, in any order.
    pub reviews: &'a [Review],
    /// Now, in Unix seconds.
    pub now: i64,
    /// Whether the project requires a reviewer other than the submitter.
    pub require_different_reviewer: bool,
}

/// Where a changeset stands. Only [`Status::Approved`] may be published (step 2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Being put together; not submitted for review.
    Open,
    /// An entry was edited after its revision was pinned: refresh the changeset (a new hash).
    Stale {
        /// How many entries moved on.
        items: usize,
    },
    /// Submitted, with no decision on its current hash.
    AwaitingReview,
    /// The latest decision on its current hash asked for changes.
    ChangesRequested {
        /// Who asked.
        by: String,
    },
    /// Approved, on its current hash, within the approval's lifetime.
    Approved {
        /// Who approved.
        by: String,
        /// Whether they also submitted it, which is allowed unless the project forbids it, and
        /// always shown.
        self_review: bool,
    },
    /// The approval on its current hash is older than [`APPROVAL_LIFETIME_SECS`].
    ApprovalExpired,
}

/// Decides a changeset's status. Reviews of other hashes are ignored: they were about a
/// different set of revisions. The latest decision on the current hash wins, except that a
/// self-approval doesn't count where the project requires a different reviewer.
pub fn status(facts: &Facts<'_>) -> Status {
    if facts.stale_items > 0 {
        return Status::Stale {
            items: facts.stale_items,
        };
    }
    let Some(submitter) = facts.submitted_by else {
        return Status::Open;
    };
    let counts = |review: &&Review| {
        review.hash == facts.hash
            && !(facts.require_different_reviewer
                && review.decision == Decision::Approve
                && review.reviewer == submitter)
    };
    let latest = facts
        .reviews
        .iter()
        .filter(counts)
        .max_by(|a, b| a.at.cmp(&b.at));
    match latest {
        None => Status::AwaitingReview,
        Some(review) if review.decision == Decision::RequestChanges => Status::ChangesRequested {
            by: review.reviewer.clone(),
        },
        Some(review) if facts.now - review.at > APPROVAL_LIFETIME_SECS => Status::ApprovalExpired,
        Some(review) => Status::Approved {
            by: review.reviewer.clone(),
            self_review: review.reviewer == submitter,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(entry: &str, revision: &str) -> Item {
        Item {
            entry: entry.to_owned(),
            revision: revision.to_owned(),
            collection: "articles".to_owned(),
            content_hash: "ab".repeat(32),
            schema_version: 1,
            definition_hash: "cd".repeat(32),
        }
    }

    fn review(reviewer: &str, decision: Decision, hash: &str, at: i64) -> Review {
        Review {
            reviewer: reviewer.to_owned(),
            decision,
            hash: hash.to_owned(),
            at,
        }
    }

    fn facts(reviews: &[Review]) -> Facts<'_> {
        Facts {
            hash: "h1",
            submitted_by: Some("ann"),
            stale_items: 0,
            reviews,
            now: 1_000,
            require_different_reviewer: false,
        }
    }

    #[test]
    fn the_canonical_form_ignores_order_and_sees_every_field() {
        let (a, b) = (item("e1", "r1"), item("e2", "r2"));
        let base = canonical(&[a.clone(), b.clone()]);
        assert_eq!(
            canonical(&[b.clone(), a.clone()]),
            base,
            "order doesn't matter"
        );
        assert_eq!(
            base,
            format!(
                "{FORMAT}\ne1 r1 articles {} 1 {}\ne2 r2 articles {} 1 {}",
                "ab".repeat(32),
                "cd".repeat(32),
                "ab".repeat(32),
                "cd".repeat(32)
            )
        );
        let changes: [fn(&mut Item); 5] = [
            |i| i.revision = "r9".to_owned(),
            |i| i.collection = "posts".to_owned(),
            |i| i.content_hash = "ef".repeat(32),
            |i| i.schema_version = 2,
            |i| i.definition_hash = "01".repeat(32),
        ];
        for change in changes {
            let mut edited = a.clone();
            change(&mut edited);
            assert_ne!(canonical(&[edited, b.clone()]), base);
        }
        assert_ne!(
            canonical(std::slice::from_ref(&a)),
            base,
            "a missing item changes it"
        );
        assert_eq!(canonical(&[]), FORMAT);
    }

    #[test]
    fn a_changeset_moves_through_review_on_its_own_hash() {
        let none: [Review; 0] = [];
        assert_eq!(
            status(&Facts {
                submitted_by: None,
                ..facts(&none)
            }),
            Status::Open
        );
        assert_eq!(status(&facts(&none)), Status::AwaitingReview);

        let asked = [review("bo", Decision::RequestChanges, "h1", 10)];
        assert_eq!(
            status(&facts(&asked)),
            Status::ChangesRequested {
                by: "bo".to_owned()
            }
        );

        let approved = [
            review("bo", Decision::RequestChanges, "h1", 10),
            review("bo", Decision::Approve, "h1", 20),
        ];
        assert_eq!(
            status(&facts(&approved)),
            Status::Approved {
                by: "bo".to_owned(),
                self_review: false
            },
            "the latest decision wins"
        );

        let other_hash = [review("bo", Decision::Approve, "h0", 20)];
        assert_eq!(
            status(&facts(&other_hash)),
            Status::AwaitingReview,
            "an approval of another hash is about other revisions"
        );
    }

    #[test]
    fn edits_expiry_and_policy_force_revalidation() {
        let approved = [review("bo", Decision::Approve, "h1", 20)];
        assert_eq!(
            status(&Facts {
                stale_items: 2,
                ..facts(&approved)
            }),
            Status::Stale { items: 2 },
            "an entry edited after pinning"
        );
        assert_eq!(
            status(&Facts {
                now: 20 + APPROVAL_LIFETIME_SECS + 1,
                ..facts(&approved)
            }),
            Status::ApprovalExpired
        );
        assert_eq!(
            status(&Facts {
                now: 20 + APPROVAL_LIFETIME_SECS,
                ..facts(&approved)
            }),
            Status::Approved {
                by: "bo".to_owned(),
                self_review: false
            },
            "the last second still counts"
        );

        let own = [review("ann", Decision::Approve, "h1", 20)];
        assert_eq!(
            status(&facts(&own)),
            Status::Approved {
                by: "ann".to_owned(),
                self_review: true
            },
            "allowed by default, and shown"
        );
        assert_eq!(
            status(&Facts {
                require_different_reviewer: true,
                ..facts(&own)
            }),
            Status::AwaitingReview,
            "a project can require someone else"
        );
        let own_request = [review("ann", Decision::RequestChanges, "h1", 20)];
        assert_eq!(
            status(&Facts {
                require_different_reviewer: true,
                ..facts(&own_request)
            }),
            Status::ChangesRequested {
                by: "ann".to_owned()
            },
            "asking for changes to your own work always counts"
        );
    }

    #[test]
    fn decision_names_round_trip() {
        for decision in [Decision::Approve, Decision::RequestChanges] {
            assert_eq!(Decision::parse(decision.as_str()), Some(decision));
        }
        assert_eq!(Decision::parse("approved"), None);
    }
}
