//! Changesets and their review (docs/modules/01-domain.md, "Editorial workflow"; spec §06;
//! D-044, D-104).
//!
//! A changeset pins exact revisions: for each entry, the revision, its content hash, and the
//! content-type version it was validated against, and whether the changeset publishes that
//! revision or takes it down (D-049: an unpublish item pins the revision that is published, so the
//! reviewer approves taking down exactly what readers see). Its hash is the SHA-256 of [`canonical`], so
//! two changesets with the same items have the same hash whatever order they were added in, and
//! any difference in any item changes it. A review decision records the hash it was made on, and
//! only counts while the changeset still has that hash: refreshing a changeset after an edit
//! re-pins the new revisions, which changes the hash, so earlier approvals no longer count.
//!
//! An environment may also name an ordered list of review [`Stage`]s (milestone E, step E2):
//! then a review names the stage it decides for, and a changeset is approved only once every
//! stage has an approval on the current hash — any stage's request for changes is the whole
//! changeset's. With no stages configured, nothing changes: the single implicit review of
//! D-044, which is a review that names no stage.
//!
//! This module decides; it doesn't hash (the domain has no crypto dependency) or store.

use std::fmt::Write as _;

/// The version line every canonical form starts with; changing the format changes it.
pub const FORMAT: &str = "colophon/changeset v1";

/// How long an approval counts before it must be given again (spec §06: "expired approval forces
/// revalidation"). Seven days, in seconds.
pub const APPROVAL_LIFETIME_SECS: i64 = 7 * 24 * 60 * 60;

/// The most stages one environment's review workflow holds (D-104): each is a gate a person
/// reviews, and every one is read on each decision about a changeset, so the list stays small.
pub const MAX_STAGES: usize = 20;

/// Whether `text` is a stage key: `[a-z][a-z0-9-]{0,62}` — a role key's rule, which the
/// database checks and a review names its stage by.
pub fn is_stage_key(text: &str) -> bool {
    crate::policy::is_role_key(text)
}

/// A stage of an environment's review workflow (D-104): a named gate a submitted changeset
/// needs an approval on, on its current hash, before it may publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stage {
    /// Its key in the environment's ordered list: what a review names it by. Renaming or
    /// reordering a stage keeps its key, so approvals recorded for it still count; removing it
    /// leaves its reviews as history that counts for nothing.
    pub key: String,
    /// Its name, as reviewers see it.
    pub name: String,
    /// Whether its approval must come from someone other than the submitter.
    pub different_reviewer: bool,
}

/// What a changeset does with one of its entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Publish the pinned revision.
    Publish,
    /// Take the pinned revision, the published one, down: out of the release and, at the origin,
    /// out of every earlier release too (D-049).
    Unpublish,
}

impl Kind {
    /// The name stored and used in the API.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Publish => "publish",
            Self::Unpublish => "unpublish",
        }
    }

    /// The kind with this name.
    pub fn parse(name: &str) -> Option<Self> {
        [Self::Publish, Self::Unpublish]
            .into_iter()
            .find(|kind| kind.as_str() == name)
    }
}

/// One entry in a changeset, pinned to an exact revision. Ids and hashes are as the database
/// writes them: lowercase hyphenated UUIDs and lowercase hex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// What the changeset does with it.
    pub kind: Kind,
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
/// contain a space or a newline (ids, slugs, hex and an integer), so the form is unambiguous. An
/// unpublish item has the word `unpublish` after its entry id, where a publish item has its
/// revision id (never that word), so a publish line reads as it did before unpublishing existed
/// and no earlier changeset's hash changed.
pub fn canonical(items: &[Item]) -> String {
    let mut sorted: Vec<&Item> = items.iter().collect();
    sorted.sort_by(|a, b| a.entry.cmp(&b.entry));
    let mut text = String::from(FORMAT);
    for item in sorted {
        // Writing to a String can't fail.
        let _ = write!(text, "\n{}", item.entry);
        if item.kind == Kind::Unpublish {
            text.push_str(" unpublish");
        }
        let _ = write!(
            text,
            " {} {} {} {} {}",
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
    /// The stage they decided for, by key — `None` for the single implicit review of an
    /// environment with no stages configured (D-104).
    pub stage: Option<String>,
    /// Its place in the changeset's decisions, in the order they were recorded: the latest has
    /// the highest. Timestamps can't order them (two reviews can share one).
    pub seq: i64,
    /// When, in Unix seconds: for expiry only.
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
    /// The environment's review stages, in order (D-104): empty for the single implicit
    /// review of D-044.
    pub stages: &'a [Stage],
    /// The release it was published as, if it has been (D-045).
    pub published: Option<i64>,
}

/// Where a changeset stands. Only [`Status::Approved`] may be published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Published as the release with this generation; a changeset is published once, and stays a
    /// record of what that release contained.
    Published {
        /// The release's generation.
        generation: i64,
    },
    /// Being put together; not submitted for review.
    Open,
    /// An entry was edited after its revision was pinned: refresh the changeset (a new hash).
    Stale {
        /// How many entries moved on.
        items: usize,
    },
    /// Submitted, with some required stage still waiting for a decision on the current hash.
    AwaitingReview,
    /// A required stage's latest decision on the current hash asked for changes.
    ChangesRequested {
        /// Who asked — the requester of the latest such decision, when several stages did.
        by: String,
    },
    /// Approved: every required stage has an approval on the current hash, all within their
    /// lifetime.
    Approved {
        /// Who gave the approval that completed the set (the latest one recorded).
        by: String,
        /// Whether they also submitted it, which is allowed unless the project or the stage
        /// forbids it, and always shown.
        self_review: bool,
        /// That approval's place among the changeset's decisions, so a caller can find the
        /// recorded review a release or a schedule binds.
        seq: i64,
    },
    /// Every stage's approval is on the current hash, but at least one is older than
    /// [`APPROVAL_LIFETIME_SECS`].
    ApprovalExpired,
}

/// The latest decision that counts for each required stage, in stage order: the stage's key —
/// `None` for the implicit single review when `facts.stages` is empty — and the decision, or
/// `None` where nothing counts for it yet. A decision counts for the stage it names, made on
/// the current hash; a self-approval never counts where the stage — or the project as a whole
/// — requires a different reviewer, and asking for changes always counts, even for one's own
/// work. This is the whole rule `status` and the per-stage views share (D-104).
pub fn counting<'a>(facts: &'a Facts<'a>) -> Vec<(Option<&'a str>, Option<&'a Review>)> {
    let latest = |stage: Option<&str>, strict: bool| {
        facts
            .reviews
            .iter()
            .filter(|review| {
                review.hash == facts.hash
                    && review.stage.as_deref() == stage
                    && !(strict
                        && review.decision == Decision::Approve
                        && Some(review.reviewer.as_str()) == facts.submitted_by)
            })
            .max_by_key(|review| review.seq)
    };
    if facts.stages.is_empty() {
        vec![(None, latest(None, facts.require_different_reviewer))]
    } else {
        facts
            .stages
            .iter()
            .map(|stage| {
                (
                    Some(stage.key.as_str()),
                    latest(
                        Some(&stage.key),
                        stage.different_reviewer || facts.require_different_reviewer,
                    ),
                )
            })
            .collect()
    }
}

/// Where one configured stage stands on `facts`' changeset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageStatus {
    /// No decision counts for it yet.
    Pending,
    /// Its latest counting decision asked for changes.
    ChangesRequested {
        /// Who asked.
        by: String,
    },
    /// Approved on the current hash.
    Approved {
        /// Who approved.
        by: String,
        /// Whether they also submitted it.
        self_review: bool,
        /// When, in Unix seconds.
        at: i64,
        /// Whether the approval is older than [`APPROVAL_LIFETIME_SECS`].
        expired: bool,
    },
}

/// Where `stage`, one of `facts.stages`, stands on `facts`' changeset: its latest counting
/// decision on the current hash.
pub fn stage_status(facts: &Facts<'_>, stage: &Stage) -> StageStatus {
    let found = counting(facts)
        .into_iter()
        .find(|(key, _)| *key == Some(stage.key.as_str()))
        .and_then(|(_, review)| review);
    match found {
        None => StageStatus::Pending,
        Some(review) if review.decision == Decision::RequestChanges => {
            StageStatus::ChangesRequested {
                by: review.reviewer.clone(),
            }
        }
        Some(review) => StageStatus::Approved {
            by: review.reviewer.clone(),
            self_review: Some(review.reviewer.as_str()) == facts.submitted_by,
            at: review.at,
            expired: facts.now - review.at > APPROVAL_LIFETIME_SECS,
        },
    }
}

/// Decides a changeset's status. Reviews of other hashes are ignored: they were about a
/// different set of revisions. The latest decision (highest `seq`) counting for each required
/// stage wins: a request for changes at any stage is the whole changeset's, and `Approved`
/// needs an approval on the current hash at every stage — the single implicit one included
/// (D-044, D-104).
pub fn status(facts: &Facts<'_>) -> Status {
    if let Some(generation) = facts.published {
        return Status::Published { generation };
    }
    if facts.stale_items > 0 {
        return Status::Stale {
            items: facts.stale_items,
        };
    }
    if facts.submitted_by.is_none() {
        return Status::Open;
    }
    let mut pending = false;
    let mut requested: Option<&Review> = None;
    let mut approval: Option<&Review> = None;
    let mut expired = false;
    for (_, decision) in counting(facts) {
        match decision {
            None => pending = true,
            Some(review) if review.decision == Decision::RequestChanges => {
                if requested.is_none_or(|held| review.seq > held.seq) {
                    requested = Some(review);
                }
            }
            Some(review) => {
                expired |= facts.now - review.at > APPROVAL_LIFETIME_SECS;
                if approval.is_none_or(|held| review.seq > held.seq) {
                    approval = Some(review);
                }
            }
        }
    }
    if let Some(review) = requested {
        return Status::ChangesRequested {
            by: review.reviewer.clone(),
        };
    }
    if pending {
        return Status::AwaitingReview;
    }
    if expired {
        return Status::ApprovalExpired;
    }
    match approval {
        Some(review) => Status::Approved {
            by: review.reviewer.clone(),
            self_review: Some(review.reviewer.as_str()) == facts.submitted_by,
            seq: review.seq,
        },
        // Every required stage decided and none asked for changes, so an approval exists;
        // still, a changeset that can't call itself approved waits for review, never publishes.
        None => Status::AwaitingReview,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(entry: &str, revision: &str) -> Item {
        Item {
            kind: Kind::Publish,
            entry: entry.to_owned(),
            revision: revision.to_owned(),
            collection: "articles".to_owned(),
            content_hash: "ab".repeat(32),
            schema_version: 1,
            definition_hash: "cd".repeat(32),
        }
    }

    /// A review whose sequence number follows its time, as most do, on no stage (the implicit
    /// one).
    fn review(reviewer: &str, decision: Decision, hash: &str, at: i64) -> Review {
        staged(reviewer, decision, hash, None, at)
    }

    /// A review for `stage` (or the implicit one, `None`).
    fn staged(
        reviewer: &str,
        decision: Decision,
        hash: &str,
        stage: Option<&str>,
        at: i64,
    ) -> Review {
        Review {
            reviewer: reviewer.to_owned(),
            decision,
            hash: hash.to_owned(),
            stage: stage.map(str::to_owned),
            seq: at,
            at,
        }
    }

    fn stage(key: &str, different_reviewer: bool) -> Stage {
        Stage {
            key: key.to_owned(),
            name: key.to_owned(),
            different_reviewer,
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
            stages: &[],
            published: None,
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
        let changes: [fn(&mut Item); 6] = [
            |i| i.kind = Kind::Unpublish,
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
    fn an_unpublish_line_names_itself_and_leaves_publish_lines_as_they_were() {
        let mut down = item("e1", "r1");
        down.kind = Kind::Unpublish;
        assert_eq!(
            canonical(&[down, item("e2", "r2")]),
            format!(
                "{FORMAT}\ne1 unpublish r1 articles {} 1 {}\ne2 r2 articles {} 1 {}",
                "ab".repeat(32),
                "cd".repeat(32),
                "ab".repeat(32),
                "cd".repeat(32)
            )
        );
        for kind in [Kind::Publish, Kind::Unpublish] {
            assert_eq!(Kind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(Kind::parse("delete"), None);
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
                self_review: false,
                seq: 20
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
                self_review: false,
                seq: 20
            },
            "the last second still counts"
        );

        let own = [review("ann", Decision::Approve, "h1", 20)];
        assert_eq!(
            status(&facts(&own)),
            Status::Approved {
                by: "ann".to_owned(),
                self_review: true,
                seq: 20
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
    fn the_order_decisions_were_recorded_in_wins_not_their_timestamps() {
        // Asked for changes, then approved within the same second: the approval is the latest.
        let asked = Review {
            seq: 1,
            ..review("bo", Decision::RequestChanges, "h1", 20)
        };
        let approved = Review {
            seq: 2,
            ..review("bo", Decision::Approve, "h1", 20)
        };
        for reviews in [
            [asked.clone(), approved.clone()],
            [approved.clone(), asked.clone()],
        ] {
            assert_eq!(
                status(&facts(&reviews)),
                Status::Approved {
                    by: "bo".to_owned(),
                    self_review: false,
                    seq: 2
                }
            );
        }
        // A transaction that waited for the lock can carry an earlier time than the decision
        // recorded before it; the sequence still says which came last.
        let inverted = [
            Review {
                seq: 1,
                ..review("bo", Decision::Approve, "h1", 30)
            },
            Review {
                seq: 2,
                ..review("cy", Decision::RequestChanges, "h1", 25)
            },
        ];
        assert_eq!(
            status(&facts(&inverted)),
            Status::ChangesRequested {
                by: "cy".to_owned()
            }
        );
    }

    #[test]
    fn a_published_changeset_stays_published() {
        let approved = [review("bo", Decision::Approve, "h1", 20)];
        assert_eq!(
            status(&Facts {
                published: Some(7),
                stale_items: 3,
                ..facts(&approved)
            }),
            Status::Published { generation: 7 },
            "later edits don't change what a release contained"
        );
    }

    #[test]
    fn decision_names_round_trip() {
        for decision in [Decision::Approve, Decision::RequestChanges] {
            assert_eq!(Decision::parse(decision.as_str()), Some(decision));
        }
        assert_eq!(Decision::parse("approved"), None);
    }

    /// Two stages, "editorial" then "legal"; `ann` submitted.
    fn staged_facts<'a>(stages: &'a [Stage], reviews: &'a [Review]) -> Facts<'a> {
        Facts {
            stages,
            ..facts(reviews)
        }
    }

    #[test]
    fn every_configured_stage_needs_an_approval_on_the_current_hash() {
        let stages = [stage("editorial", false), stage("legal", false)];
        let none: [Review; 0] = [];
        assert_eq!(
            status(&staged_facts(&stages, &none)),
            Status::AwaitingReview
        );

        // One stage down, any order: the changeset still waits.
        let one = [staged("bo", Decision::Approve, "h1", Some("legal"), 10)];
        assert_eq!(status(&staged_facts(&stages, &one)), Status::AwaitingReview);

        // The last approval to land completes it; `by` and `seq` name it.
        let both = [
            staged("bo", Decision::Approve, "h1", Some("legal"), 10),
            staged("cy", Decision::Approve, "h1", Some("editorial"), 12),
        ];
        assert_eq!(
            status(&staged_facts(&stages, &both)),
            Status::Approved {
                by: "cy".to_owned(),
                self_review: false,
                seq: 12
            }
        );

        // A stage approval on another hash is about other revisions.
        let moved = [
            staged("bo", Decision::Approve, "h0", Some("legal"), 10),
            staged("cy", Decision::Approve, "h1", Some("editorial"), 12),
        ];
        assert_eq!(
            status(&staged_facts(&stages, &moved)),
            Status::AwaitingReview,
            "a stale-hash approval doesn't count"
        );

        // The implicit review of an unstaged environment counts for no named stage.
        let implicit = [
            review("bo", Decision::Approve, "h1", 10),
            staged("cy", Decision::Approve, "h1", Some("editorial"), 12),
        ];
        assert_eq!(
            status(&staged_facts(&stages, &implicit)),
            Status::AwaitingReview
        );
    }

    #[test]
    fn a_request_for_changes_at_any_stage_blocks_and_a_reapproval_clears_it() {
        let stages = [stage("editorial", false), stage("legal", false)];
        let asked = [
            staged("bo", Decision::Approve, "h1", Some("editorial"), 10),
            staged("cy", Decision::RequestChanges, "h1", Some("legal"), 12),
        ];
        assert_eq!(
            status(&staged_facts(&stages, &asked)),
            Status::ChangesRequested {
                by: "cy".to_owned()
            },
            "one stage's request is the whole changeset's"
        );

        // Another stage approving later doesn't clear it: stage order is display, not a gate.
        let still = [
            staged("bo", Decision::Approve, "h1", Some("editorial"), 10),
            staged("cy", Decision::RequestChanges, "h1", Some("legal"), 12),
            staged("di", Decision::Approve, "h1", Some("editorial"), 14),
        ];
        assert_eq!(
            status(&staged_facts(&stages, &still)),
            Status::ChangesRequested {
                by: "cy".to_owned()
            }
        );

        // A re-approval on the asking stage, recorded after, does.
        let cleared = [
            staged("bo", Decision::Approve, "h1", Some("editorial"), 10),
            staged("cy", Decision::RequestChanges, "h1", Some("legal"), 12),
            staged("el", Decision::Approve, "h1", Some("legal"), 14),
        ];
        assert_eq!(
            status(&staged_facts(&stages, &cleared)),
            Status::Approved {
                by: "el".to_owned(),
                self_review: false,
                seq: 14
            }
        );
    }

    #[test]
    fn a_stage_can_require_someone_other_than_the_submitter() {
        let stages = [stage("legal", true)];
        let own = [staged("ann", Decision::Approve, "h1", Some("legal"), 10)];
        assert_eq!(
            status(&staged_facts(&stages, &own)),
            Status::AwaitingReview,
            "the submitter's approval doesn't count there"
        );

        // The project-level rule, when set, reaches every stage too.
        let loose = [stage("legal", false)];
        assert_eq!(
            status(&Facts {
                require_different_reviewer: true,
                ..staged_facts(&loose, &own)
            }),
            Status::AwaitingReview
        );

        // But asking for changes to one's own work always counts.
        let asked = [staged(
            "ann",
            Decision::RequestChanges,
            "h1",
            Some("legal"),
            10,
        )];
        assert_eq!(
            status(&staged_facts(&stages, &asked)),
            Status::ChangesRequested {
                by: "ann".to_owned()
            }
        );

        // And someone else's approval counts again — even after the submitter's.
        let reviews = [
            staged("ann", Decision::Approve, "h1", Some("legal"), 10),
            staged("bo", Decision::Approve, "h1", Some("legal"), 12),
        ];
        assert_eq!(
            status(&staged_facts(&stages, &reviews)),
            Status::Approved {
                by: "bo".to_owned(),
                self_review: false,
                seq: 12
            }
        );
    }

    #[test]
    fn a_staged_approval_expires_one_stage_at_a_time() {
        let stages = [stage("editorial", false), stage("legal", false)];
        let reviews = [
            staged("bo", Decision::Approve, "h1", Some("editorial"), 20),
            staged("cy", Decision::Approve, "h1", Some("legal"), 30),
        ];
        assert_eq!(
            status(&Facts {
                now: 20 + APPROVAL_LIFETIME_SECS + 1,
                ..staged_facts(&stages, &reviews)
            }),
            Status::ApprovalExpired,
            "the oldest approval lapsing ends it"
        );
        assert_eq!(
            status(&Facts {
                now: 20 + APPROVAL_LIFETIME_SECS,
                ..staged_facts(&stages, &reviews)
            }),
            Status::Approved {
                by: "cy".to_owned(),
                self_review: false,
                seq: 30
            }
        );
    }

    #[test]
    fn each_stage_reports_where_it_stands() {
        let stages = [stage("editorial", false), stage("legal", false)];
        let reviews = [
            staged("bo", Decision::Approve, "h1", Some("editorial"), 20),
            staged("cy", Decision::RequestChanges, "h1", Some("legal"), 25),
        ];
        let inputs = staged_facts(&stages, &reviews);
        assert_eq!(
            stage_status(&inputs, &stages[0]),
            StageStatus::Approved {
                by: "bo".to_owned(),
                self_review: false,
                at: 20,
                expired: false
            }
        );
        assert_eq!(
            stage_status(&inputs, &stages[1]),
            StageStatus::ChangesRequested {
                by: "cy".to_owned()
            }
        );
        assert_eq!(
            stage_status(&staged_facts(&stages, &[]), &stages[0]),
            StageStatus::Pending
        );
        assert_eq!(
            stage_status(
                &Facts {
                    now: 20 + APPROVAL_LIFETIME_SECS + 1,
                    ..staged_facts(&stages, &reviews)
                },
                &stages[0]
            ),
            StageStatus::Approved {
                by: "bo".to_owned(),
                self_review: false,
                at: 20,
                expired: true
            }
        );
    }

    #[test]
    fn stage_keys_are_role_shaped() {
        for key in ["editorial", "legal-2", "a"] {
            assert!(is_stage_key(key), "{key}");
        }
        for key in ["", "Editorial", "legal_2", "-legal", "legal two"] {
            assert!(!is_stage_key(key), "{key}");
        }
    }
}
