//! Undoing a takedown (D-184): when a release that took entries down may be reversed in one step.
//!
//! An undo is an explicit, short-lived reversal by the person who made the takedown, while the
//! takedown is still the newest release. It puts back exactly the revisions readers had, through
//! the ordinary publish path. That is why it doesn't conflict with D-097's rule that a rollback
//! never restores a takedown: a rollback is a bulk "go back to release N" anyone with the right
//! role may ask for, long after; an undo names one release, by its own author, within the window.
//!
//! Every rule that decides it is here, without I/O: the caller reads the facts (the release's
//! age is measured by the database against its own clock) and turns a refusal into an answer.

/// How long after a takedown its author may undo it, in seconds. The Studio offers Undo for ten;
/// the server's window only has to be finite and a little longer than a slow round trip, so that
/// a stale tab or a replayed request can't bring content back minutes later.
pub const WINDOW_SECS: i64 = 60;

/// What deciding an undo of release `generation` looks at. `I` is an identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts<I> {
    /// The release asked about.
    pub generation: i64,
    /// The environment's newest release.
    pub latest: i64,
    /// Whether the release exists (generation within `1..=latest` and its row read).
    pub exists: bool,
    /// Whether the caller is an API token rather than a person (D-111).
    pub by_token: bool,
    /// Who asks.
    pub caller: I,
    /// Who published the release.
    pub publisher: I,
    /// Whether the release is younger than [`WINDOW_SECS`], by the database's clock.
    pub in_window: bool,
    /// How many of its changeset's items publish something.
    pub publishes: i64,
    /// How many take an entry down.
    pub takes_down: i64,
    /// How many review stages the environment configures (D-104).
    pub stages: usize,
    /// Whether the project requires someone other than the submitter to approve.
    pub different_reviewer: bool,
}

/// Why a release can't be undone, in the order the checks run: the first that applies is the
/// answer, so a caller learns the most basic reason first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// An API token asks: approving its own request would put no person in the loop (D-111).
    #[error("an API token can't undo a release; a person has to")]
    ByToken,
    /// There is no such release.
    #[error("there is no release {generation}")]
    NoSuchRelease {
        /// The release asked about.
        generation: i64,
    },
    /// Another release went out after it, so putting things back would undo more than it.
    #[error("release {latest} went out after it; only the newest release can be undone")]
    ChangedSince {
        /// The newest release now.
        latest: i64,
    },
    /// It published something: only a takedown can be undone.
    #[error("only a release that took entries down can be undone")]
    NotUndoable,
    /// Someone else published it.
    #[error("only the person who published a release can undo it")]
    NotYours,
    /// The window passed.
    #[error("the time to undo it has passed")]
    Expired,
    /// The environment's publishing goes through review stages, or needs someone else's approval:
    /// an undo would approve its own request, so it would be a way around review.
    #[error("putting it back needs a review here; make a publish request")]
    NeedsReview,
}

/// Whether the release in `facts` may be undone by its caller.
///
/// # Errors
///
/// The first [`Refusal`] that applies.
pub fn decide<I: PartialEq>(facts: &Facts<I>) -> Result<(), Refusal> {
    if facts.by_token {
        return Err(Refusal::ByToken);
    }
    if !facts.exists || facts.generation < 1 || facts.generation > facts.latest {
        return Err(Refusal::NoSuchRelease {
            generation: facts.generation,
        });
    }
    if facts.generation != facts.latest {
        return Err(Refusal::ChangedSince {
            latest: facts.latest,
        });
    }
    if facts.publishes > 0 || facts.takes_down == 0 {
        return Err(Refusal::NotUndoable);
    }
    if facts.caller != facts.publisher {
        return Err(Refusal::NotYours);
    }
    if !facts.in_window {
        return Err(Refusal::Expired);
    }
    if facts.stages > 0 || facts.different_reviewer {
        return Err(Refusal::NeedsReview);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A takedown its author may undo: release 4, the newest, one entry down, a minute not up.
    fn undoable() -> Facts<&'static str> {
        Facts {
            generation: 4,
            latest: 4,
            exists: true,
            by_token: false,
            caller: "ana",
            publisher: "ana",
            in_window: true,
            publishes: 0,
            takes_down: 1,
            stages: 0,
            different_reviewer: false,
        }
    }

    #[test]
    fn the_newest_takedown_by_its_author_within_the_window_is_undone() {
        assert_eq!(decide(&undoable()), Ok(()));
        let several = Facts {
            takes_down: 3,
            ..undoable()
        };
        assert_eq!(decide(&several), Ok(()));
    }

    #[test]
    fn each_rule_refuses_on_its_own() {
        let cases = [
            (Facts { by_token: true, ..undoable() }, Refusal::ByToken),
            (Facts { exists: false, ..undoable() }, Refusal::NoSuchRelease { generation: 4 }),
            (Facts { generation: 0, ..undoable() }, Refusal::NoSuchRelease { generation: 0 }),
            (Facts { generation: -1, ..undoable() }, Refusal::NoSuchRelease { generation: -1 }),
            (Facts { generation: 5, ..undoable() }, Refusal::NoSuchRelease { generation: 5 }),
            (Facts { latest: 5, ..undoable() }, Refusal::ChangedSince { latest: 5 }),
            (Facts { publishes: 1, ..undoable() }, Refusal::NotUndoable),
            (Facts { takes_down: 0, ..undoable() }, Refusal::NotUndoable),
            (Facts { caller: "ben", ..undoable() }, Refusal::NotYours),
            (Facts { in_window: false, ..undoable() }, Refusal::Expired),
            (Facts { stages: 2, ..undoable() }, Refusal::NeedsReview),
            (Facts { different_reviewer: true, ..undoable() }, Refusal::NeedsReview),
        ];
        for (facts, refusal) in cases {
            assert_eq!(decide(&facts), Err(refusal), "{facts:?}");
        }
    }

    #[test]
    fn the_most_basic_reason_comes_first() {
        // A token asking about an old release someone else published: the token is the answer.
        let everything = Facts {
            by_token: true,
            latest: 9,
            caller: "ben",
            in_window: false,
            stages: 1,
            publishes: 2,
            ..undoable()
        };
        assert_eq!(decide(&everything), Err(Refusal::ByToken));
        // Not the newest beats not a takedown, not yours, too late and review.
        let person = Facts {
            by_token: false,
            ..everything.clone()
        };
        assert_eq!(decide(&person), Err(Refusal::ChangedSince { latest: 9 }));
        // Too late is said before review, so an expired undo never suggests a request instead.
        let late = Facts {
            in_window: false,
            stages: 2,
            ..undoable()
        };
        assert_eq!(decide(&late), Err(Refusal::Expired));
    }

}
