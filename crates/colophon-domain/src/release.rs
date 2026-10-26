//! Releases and publication history (docs/modules/01-domain.md, "Releases"; spec §14; D-045).
//!
//! An environment's generation counts its committed releases: each release takes exactly the
//! next number, and only if the environment is still at the generation the publisher saw, so two
//! releases prepared on the same base can't both commit. An entry's publication history is a
//! series of half-open intervals of generations, `[from, to)`, that never overlap: publishing a
//! new revision closes the open interval at the new generation and opens another.
//!
//! Unpublishing (D-049) closes the open interval and leaves a tombstone at the new generation.
//! History stays as it was, but the origin no longer delivers any interval that began before a
//! tombstone, at any generation: taking something down takes it out of old release links too
//! (spec §15, DL5). Publishing the entry again later opens an interval after the tombstone, which
//! is delivered from its own generation on.

/// Why a release can't commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// Another release committed since the publisher looked: they must look again.
    #[error(
        "the environment is at generation {current}, not {base}: another release committed first"
    )]
    GenerationMoved {
        /// The generation the publisher saw.
        base: i64,
        /// The generation now.
        current: i64,
    },
}

/// The generation a release prepared on `base` commits as, if the environment is still at
/// `base`.
pub const fn next_generation(base: i64, current: i64) -> Result<i64, Refusal> {
    if base == current {
        Ok(current + 1)
    } else {
        Err(Refusal::GenerationMoved { base, current })
    }
}

/// One stretch of an entry's publication history: `revision` was published from generation
/// `from` until, not including, `to` (still published when `to` is `None`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interval<R> {
    /// The published revision.
    pub revision: R,
    /// The first generation it was published in.
    pub from: i64,
    /// The first generation it no longer was, if any.
    pub to: Option<i64>,
}

impl<R> Interval<R> {
    /// Whether this interval is the published one at `generation`.
    pub fn covers(&self, generation: i64) -> bool {
        self.from <= generation && self.to.is_none_or(|to| generation < to)
    }
}

/// What publishing a revision does to an entry, given its open interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The revision is already the published one: nothing changes.
    Unchanged,
    /// Close the open interval (if any) at the new generation and open one for the revision.
    Publish {
        /// Whether there is an open interval to close.
        close: bool,
    },
}

/// Decides what publishing `revision` changes, given the entry's open interval.
pub fn publish<R: PartialEq>(open: Option<&Interval<R>>, revision: &R) -> Change {
    match open {
        Some(current) if current.revision == *revision => Change::Unchanged,
        Some(_) => Change::Publish { close: true },
        None => Change::Publish { close: false },
    }
}

/// Applies publishing `revision` at `generation` to an entry's history, oldest first, returning
/// the new history. Used by tests and the simulator to check the stored history against.
pub fn apply<R: PartialEq + Clone>(
    history: &[Interval<R>],
    revision: &R,
    generation: i64,
) -> Vec<Interval<R>> {
    let mut next = history.to_vec();
    let open = next.iter_mut().find(|interval| interval.to.is_none());
    match open {
        Some(current) if current.revision == *revision => {}
        Some(current) => {
            current.to = Some(generation);
            next.push(Interval {
                revision: revision.clone(),
                from: generation,
                to: None,
            });
        }
        None => next.push(Interval {
            revision: revision.clone(),
            from: generation,
            to: None,
        }),
    }
    next
}

/// Applies unpublishing an entry at `generation` to its history, oldest first: closes the open
/// interval, if there is one.
pub fn apply_unpublish<R: Clone>(history: &[Interval<R>], generation: i64) -> Vec<Interval<R>> {
    let mut next = history.to_vec();
    if let Some(open) = next.iter_mut().find(|interval| interval.to.is_none()) {
        open.to = Some(generation);
    }
    next
}

/// Whether the origin may deliver an interval that began at `from`, given the generations at
/// which the entry was unpublished: not if any of them came after it began.
pub fn deliverable(from: i64, unpublished: &[i64]) -> bool {
    unpublished.iter().all(|&at| at <= from)
}

/// Whether a history is well formed: intervals in order, none empty, none overlapping, at most
/// one open and only as the last.
pub fn well_formed<R>(history: &[Interval<R>]) -> bool {
    history.iter().enumerate().all(|(index, interval)| {
        let last = index + 1 == history.len();
        let nonempty = interval.to.is_none_or(|to| to > interval.from);
        let open_only_last = interval.to.is_some() || last;
        let follows = index == 0 || history[index - 1].to.is_some_and(|to| to <= interval.from);
        nonempty && open_only_last && follows
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_takes_the_next_generation_only_from_the_base_it_saw() {
        assert_eq!(next_generation(0, 0), Ok(1));
        assert_eq!(next_generation(4, 4), Ok(5));
        assert_eq!(
            next_generation(4, 5),
            Err(Refusal::GenerationMoved {
                base: 4,
                current: 5
            }),
            "two releases on one base: the second can't commit"
        );
    }

    #[test]
    fn history_is_half_open_intervals_that_never_overlap() {
        let mut history: Vec<Interval<&str>> = Vec::new();
        for (revision, generation) in [("r1", 1), ("r1", 2), ("r2", 3), ("r3", 7)] {
            history = apply(&history, &revision, generation);
            assert!(well_formed(&history), "{history:?}");
        }
        assert_eq!(
            history,
            [
                Interval {
                    revision: "r1",
                    from: 1,
                    to: Some(3)
                },
                Interval {
                    revision: "r2",
                    from: 3,
                    to: Some(7)
                },
                Interval {
                    revision: "r3",
                    from: 7,
                    to: None
                },
            ],
            "republishing the same revision changes nothing"
        );
        let at = |generation| {
            history
                .iter()
                .find(|interval| interval.covers(generation))
                .map(|i| i.revision)
        };
        assert_eq!(
            [at(0), at(1), at(2), at(3), at(6), at(7), at(99)],
            [
                None,
                Some("r1"),
                Some("r1"),
                Some("r2"),
                Some("r2"),
                Some("r3"),
                Some("r3")
            ]
        );
    }

    #[test]
    fn publish_says_what_changes() {
        let open = Interval {
            revision: "r1",
            from: 1,
            to: None,
        };
        assert_eq!(publish(Some(&open), &"r1"), Change::Unchanged);
        assert_eq!(publish(Some(&open), &"r2"), Change::Publish { close: true });
        assert_eq!(publish(None, &"r1"), Change::Publish { close: false });
    }

    #[test]
    fn unpublishing_takes_an_entry_out_of_every_earlier_release_until_it_is_published_again() {
        let mut history: Vec<Interval<&str>> = Vec::new();
        history = apply(&history, &"r1", 1);
        history = apply(&history, &"r2", 2);
        history = apply_unpublish(&history, 4);
        assert!(well_formed(&history), "{history:?}");
        assert_eq!(
            apply_unpublish(&history, 5),
            history,
            "nothing published, nothing to close"
        );
        history = apply(&history, &"r3", 6);
        assert!(well_formed(&history), "{history:?}");
        let tombstones = [4];
        let delivered = |generation| {
            history
                .iter()
                .find(|interval| interval.covers(generation))
                .filter(|interval| deliverable(interval.from, &tombstones))
                .map(|interval| interval.revision)
        };
        assert_eq!(
            [1, 2, 3, 4, 5, 6, 9].map(delivered),
            [None, None, None, None, None, Some("r3"), Some("r3")],
            "release 1's and 2's links stop delivering it; the republish delivers from 6"
        );
        assert!(deliverable(6, &[4]));
        assert!(!deliverable(6, &[4, 7]), "unpublished again at 7");
        assert!(deliverable(3, &[]));
    }

    #[test]
    fn malformed_histories_are_caught() {
        let open = |revision, from| Interval {
            revision,
            from,
            to: None,
        };
        let closed = |revision, from, to| Interval {
            revision,
            from,
            to: Some(to),
        };
        assert!(well_formed::<&str>(&[]));
        assert!(!well_formed(&[open("a", 1), open("b", 2)]), "two open");
        assert!(
            !well_formed(&[closed("a", 1, 4), closed("b", 3, 5)]),
            "overlap"
        );
        assert!(!well_formed(&[closed("a", 2, 2)]), "empty");
        assert!(
            !well_formed(&[open("a", 1), closed("b", 2, 3)]),
            "open not last"
        );
    }
}
