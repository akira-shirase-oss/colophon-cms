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

/// Who asks for an undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asker<I> {
    /// A person, by identity.
    Person(I),
    /// An API token (D-111).
    Token,
}

/// What the database says about the release asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release<I> {
    /// Who published it.
    pub publisher: I,
    /// Whether it is younger than [`WINDOW_SECS`], by the database's clock.
    pub in_window: bool,
    /// How many of its changeset's items publish something.
    pub publishes: i64,
    /// How many take an entry down.
    pub takes_down: i64,
}

/// What deciding an undo of release `generation` looks at. `I` is an identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts<I> {
    /// The release asked about.
    pub generation: i64,
    /// The environment's newest release.
    pub latest: i64,
    /// Who asks.
    pub asker: Asker<I>,
    /// The release, when there is one at `generation`.
    pub release: Option<Release<I>>,
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
}

/// Whether the release in `facts` may be undone by its caller.
///
/// # Errors
///
/// The first [`Refusal`] that applies.
pub fn decide<I: PartialEq>(facts: &Facts<I>) -> Result<(), Refusal> {
    let Asker::Person(caller) = &facts.asker else {
        return Err(Refusal::ByToken);
    };
    let release = match &facts.release {
        Some(release) if (1..=facts.latest).contains(&facts.generation) => release,
        _ => {
            return Err(Refusal::NoSuchRelease {
                generation: facts.generation,
            });
        }
    };
    if facts.generation != facts.latest {
        return Err(Refusal::ChangedSince {
            latest: facts.latest,
        });
    }
    if release.publishes > 0 || release.takes_down == 0 {
        return Err(Refusal::NotUndoable);
    }
    if *caller != release.publisher {
        return Err(Refusal::NotYours);
    }
    if !release.in_window {
        return Err(Refusal::Expired);
    }
    // Review is not decided here: the undo's request is approved as a recorded self-review and
    // then published by `publish::publish`, which judges it with the one rule every publish
    // uses (`changeset::status`, D-104). Where review stages or another reviewer are required,
    // that self-review doesn't count, so the publish refuses and nothing is kept (U1).
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
            asker: Asker::Person("ana"),
            release: Some(Release {
                publisher: "ana",
                in_window: true,
                publishes: 0,
                takes_down: 1,
            }),
        }
    }

    /// [`undoable`], with its release changed by `change`.
    fn with_release(change: impl FnOnce(&mut Release<&'static str>)) -> Facts<&'static str> {
        let mut facts = undoable();
        if let Some(release) = facts.release.as_mut() {
            change(release);
        }
        facts
    }

    #[test]
    fn the_newest_takedown_by_its_author_within_the_window_is_undone() {
        assert_eq!(decide(&undoable()), Ok(()));
        assert_eq!(
            decide(&with_release(|release| release.takes_down = 3)),
            Ok(())
        );
    }

    #[test]
    fn each_rule_refuses_on_its_own() {
        let cases = [
            (
                Facts {
                    asker: Asker::Token,
                    ..undoable()
                },
                Refusal::ByToken,
            ),
            (
                Facts {
                    release: None,
                    ..undoable()
                },
                Refusal::NoSuchRelease { generation: 4 },
            ),
            (
                Facts {
                    generation: 0,
                    ..undoable()
                },
                Refusal::NoSuchRelease { generation: 0 },
            ),
            (
                Facts {
                    generation: -1,
                    ..undoable()
                },
                Refusal::NoSuchRelease { generation: -1 },
            ),
            (
                Facts {
                    generation: 5,
                    ..undoable()
                },
                Refusal::NoSuchRelease { generation: 5 },
            ),
            (
                Facts {
                    latest: 5,
                    ..undoable()
                },
                Refusal::ChangedSince { latest: 5 },
            ),
            (
                with_release(|release| release.publishes = 1),
                Refusal::NotUndoable,
            ),
            (
                with_release(|release| release.takes_down = 0),
                Refusal::NotUndoable,
            ),
            (
                Facts {
                    asker: Asker::Person("ben"),
                    ..undoable()
                },
                Refusal::NotYours,
            ),
            (
                with_release(|release| release.in_window = false),
                Refusal::Expired,
            ),
        ];
        for (facts, refusal) in cases {
            assert_eq!(decide(&facts), Err(refusal), "{facts:?}");
        }
    }

    #[test]
    fn the_most_basic_reason_comes_first() {
        // A token asking about an old release someone else published, too late.
        let everything = Facts {
            asker: Asker::Token,
            latest: 9,
            release: Some(Release {
                publisher: "ben",
                in_window: false,
                publishes: 2,
                takes_down: 1,
            }),
            ..undoable()
        };
        assert_eq!(decide(&everything), Err(Refusal::ByToken));
        // A person: not the newest beats not a takedown, not yours and too late.
        let person = Facts {
            asker: Asker::Person("ana"),
            ..everything.clone()
        };
        assert_eq!(decide(&person), Err(Refusal::ChangedSince { latest: 9 }));
    }
}
