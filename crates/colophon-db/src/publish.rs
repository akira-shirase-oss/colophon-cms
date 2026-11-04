//! Publishing an approved changeset as one release (D-045): the one routine both the API's
//! publish and a schedule falling due run (D-060), so both make exactly the same checks, in the
//! same order, under the same locks. The caller holds a [`ProjectTransaction`] as the person
//! publishing: their role's policy is the one checked.

use colophon_domain::{
    changeset::{self, Decision, Status},
    policy::Action,
    schema::ContentType,
};
use sqlx::types::time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    DbError, changesets, content_types,
    releases::{self, Committed},
    scope::ProjectTransaction,
};

/// Which generation a release follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    /// The one the publisher saw: if another release committed since, nothing is published (the
    /// API's publish, D-045).
    Seen(i64),
    /// Whatever is current when it commits (a schedule: its author chose a time, not a base).
    Latest,
}

/// A release that committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    /// Its generation.
    pub generation: i64,
    /// The environment's epoch.
    pub epoch: i64,
    /// The changeset's hash, as published.
    pub hash: Vec<u8>,
    /// Entries whose published revision changed.
    pub changed: u64,
    /// Entries it took down.
    pub unpublished: u64,
}

/// Why nothing was published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// No such changeset in this environment.
    NotFound,
    /// The changeset's hash isn't the one expected.
    HashMoved {
        /// Its hash now.
        current: Vec<u8>,
    },
    /// The publisher's role can't publish in every collection the changeset touches.
    NotPermitted,
    /// Not approved, on its current hash, within the approval's lifetime: its status says why.
    NotPublishable(Status),
    /// Another release committed after the base the publisher saw.
    GenerationMoved {
        /// The generation now.
        current: i64,
    },
    /// An entry's slug is another entry's published slug (D-054). The transaction is aborted:
    /// the caller rolls it back.
    SlugTaken,
    /// A value of an entry's unique field is one another entry still publishes (D-070). The
    /// transaction is aborted: the caller rolls it back.
    ValueTaken,
    /// It would leave readers a reference to an entry they can't get (D-065). The caller rolls
    /// the transaction back.
    BrokenReference(releases::BrokenReference),
    /// It would publish content naming an asset readers can't get: still quarantined or
    /// rejected (media M1). The caller rolls the transaction back.
    UnusableAsset {
        /// The entry being published.
        entry: Uuid,
        /// The field naming the asset.
        field: String,
        /// The asset it names.
        asset: Uuid,
        /// The asset's state (`quarantined` or `rejected`).
        state: String,
    },
    /// It would publish an entry that has been deleted (D-080): the database refused the
    /// publication. The transaction is aborted: the caller rolls it back.
    EntryDeleted {
        /// The entry, as the database reported it.
        entry: Option<Uuid>,
    },
    /// It would publish a revision written under an older definition that no longer fits the
    /// one in force: a type change can move on after the revision's publication closed, and a
    /// rollback pins such history on purpose (D-055, D-097).
    Outdated {
        /// The entry the revision belongs to.
        entry: Uuid,
        /// The pinned revision's title.
        title: String,
        /// The domain refusal's code.
        reason: &'static str,
        /// The field that fails.
        pointer: String,
        /// What fails.
        message: String,
    },
}

/// Whether a project needs someone other than the submitter to approve (D-044). Projects can't
/// require it yet: the rule is in place in the domain for when project settings can — a stage
/// of the review workflow can require it instead (D-104). The project view reports it, so the
/// Studio's one-click publish follows the same rule as a publish, by hand or on a schedule
/// (D-059).
pub const REQUIRE_DIFFERENT_REVIEWER: bool = false;

/// The fields `changeset::Facts` borrows, owned (D-104): built once from stored rows so a
/// caller asking both the status and the per-stage decisions decides on the same data.
struct Inputs {
    hash: String,
    submitted_by: Option<String>,
    reviews: Vec<changeset::Review>,
    stages: Vec<changeset::Stage>,
}

impl Inputs {
    /// Converts `head`'s stored reviews on its current hash and the environment's `stages`
    /// into the domain's types.
    fn new(
        head: &changesets::Head,
        reviews: &[changesets::Review],
        stages: &[changesets::Stage],
    ) -> Self {
        let hash = hex(&head.hash);
        Self {
            submitted_by: head.submitted_by.map(|id| id.to_string()),
            reviews: reviews
                .iter()
                .filter(|review| review.changeset_hash == head.hash)
                .filter_map(|review| {
                    Some(changeset::Review {
                        reviewer: review.reviewer_id.to_string(),
                        decision: Decision::parse(&review.decision)?,
                        hash: hash.clone(),
                        stage: review.stage.clone(),
                        seq: i64::from(review.seq),
                        at: review.created_at.unix_timestamp(),
                    })
                })
                .collect(),
            stages: stages
                .iter()
                .map(|stage| changeset::Stage {
                    key: stage.key.clone(),
                    name: stage.name.clone(),
                    different_reviewer: stage.different_reviewer,
                })
                .collect(),
            hash,
        }
    }

    /// The domain facts borrowing these inputs, for `edited` moved items at `now`.
    fn facts(&self, edited: usize, now: i64, published: Option<i64>) -> changeset::Facts<'_> {
        changeset::Facts {
            hash: &self.hash,
            submitted_by: self.submitted_by.as_deref(),
            stale_items: edited,
            reviews: &self.reviews,
            now,
            require_different_reviewer: REQUIRE_DIFFERENT_REVIEWER,
            stages: &self.stages,
            published,
        }
    }
}

/// The domain's decision for `head`, from its reviews (newest first), `edited` of its entries
/// having moved on, the environment's `stages`, at `now`.
pub fn status(
    head: &changesets::Head,
    edited: usize,
    reviews: &[changesets::Review],
    stages: &[changesets::Stage],
    now: OffsetDateTime,
) -> Status {
    let inputs = Inputs::new(head, reviews, stages);
    changeset::status(&inputs.facts(edited, now.unix_timestamp(), head.published))
}

/// Each required stage's latest counting decision on `head`'s current hash, in stage order —
/// the stage's key, or `None` for the implicit review of an environment without stages — as
/// the stored review it is, `None` where nothing counts for it yet (D-104).
pub fn counting<'a>(
    head: &changesets::Head,
    reviews: &'a [changesets::Review],
    stages: &[changesets::Stage],
) -> Vec<(Option<String>, Option<&'a changesets::Review>)> {
    let inputs = Inputs::new(head, reviews, stages);
    // `counting` reads none of stale items, now or publication; the real ones are unused.
    let facts = inputs.facts(0, 0, head.published);
    changeset::counting(&facts)
        .into_iter()
        .map(|(stage, decided)| {
            (
                stage.map(str::to_owned),
                decided.and_then(|decided| {
                    reviews
                        .iter()
                        .find(|stored| i64::from(stored.seq) == decided.seq)
                }),
            )
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut text, byte| {
            // Writing to a String can't fail.
            let _ = write!(text, "{byte:02x}");
            text
        })
}

/// How many times a release on [`Base::Latest`] retries when another release commits first.
const LATEST_ATTEMPTS: usize = 3;

/// The first pinned revision of `changeset` that predates its collection's version and no
/// longer fits the definition in force, as an [`Refusal::Outdated`].
async fn outdated(
    tx: &mut ProjectTransaction,
    changeset: Uuid,
) -> Result<Option<Refusal>, DbError> {
    for pin in changesets::outdated_pins(tx, changeset).await? {
        let definition: serde_json::Value = serde_json::from_str(&pin.definition)
            .map_err(|_| DbError::Corrupt("a stored definition is not JSON"))?;
        let current = ContentType::parse(&definition)
            .map_err(|_| DbError::Corrupt("a stored definition no longer parses"))?;
        if let Err(problem) = current.accepts(&pin.content) {
            return Ok(Some(Refusal::Outdated {
                entry: pin.entry_id,
                title: pin.title,
                reason: problem.code,
                pointer: problem.pointer,
                message: problem.message,
            }));
        }
    }
    Ok(None)
}

/// Publishes changeset `id` if it is still at `expected` (when given), approved at `now`, and the
/// transaction's role may publish everything in it. Locks the changeset, then its collections'
/// content-type locks in order (no content type changes under it, D-055), checks, and commits.
pub async fn publish(
    tx: &mut ProjectTransaction,
    id: Uuid,
    expected: Option<&[u8]>,
    base: Base,
    now: OffsetDateTime,
    job_lifetime: std::time::Duration,
) -> Result<Result<Published, Refusal>, DbError> {
    let Some(head) = changesets::lock(tx, id).await? else {
        return Ok(Err(Refusal::NotFound));
    };
    if expected.is_some_and(|expected| expected != head.hash) {
        return Ok(Err(Refusal::HashMoved { current: head.hash }));
    }
    let collections = changesets::collections(tx, head.id).await?;
    for collection in &collections {
        content_types::lock_for_write(tx, collection).await?;
    }
    if !collections
        .iter()
        .all(|collection| tx.policy.allows(Action::ContentPublish, Some(collection)))
    {
        return Ok(Err(Refusal::NotPermitted));
    }
    // A pin made before its collection's current version must still fit the definition in
    // force, under the locks above: drafts are checked when a type changes, but closed
    // publications are not, and a rollback can pin one (D-055, D-097).
    if let Some(refused) = outdated(tx, head.id).await? {
        return Ok(Err(refused));
    }
    let items = changesets::items(tx, head.id).await?;
    let reviews = changesets::reviews(tx, head.id, Some(&head.hash)).await?;
    // The stage list this decides against can't move under it: a stage-list replace queues on
    // the locks, so the release commits under the rules it was checked against (D-104).
    let stages = changesets::stages_locked(tx).await?;
    let edited = items.iter().filter(|item| item.moved()).count();
    let decided = status(&head, edited, &reviews, &stages, now);
    let seq = match decided {
        Status::Approved { seq, .. } => seq,
        other => return Ok(Err(Refusal::NotPublishable(other))),
    };
    // Approved means every required stage's latest decision on this hash is an approval; the
    // recorded one that completed the set is the approval the release binds (D-104).
    let approval = reviews
        .iter()
        .find(|review| i64::from(review.seq) == seq)
        .ok_or(DbError::Corrupt("a changeset is approved without a review"))?;
    let mut generation = match base {
        Base::Seen(generation) => generation,
        Base::Latest => releases::generation(tx).await?.1,
    };
    for _ in 0..LATEST_ATTEMPTS {
        let committed = releases::commit(
            tx,
            generation,
            head.id,
            &head.hash,
            approval.id,
            job_lifetime,
        )
        .await;
        match committed {
            Ok(Committed::Done {
                generation,
                epoch,
                changed,
                unpublished,
            }) => {
                return Ok(Ok(Published {
                    generation,
                    epoch,
                    hash: head.hash,
                    changed,
                    unpublished,
                }));
            }
            Ok(Committed::Moved { current }) => match base {
                Base::Seen(_) => return Ok(Err(Refusal::GenerationMoved { current })),
                Base::Latest => generation = current,
            },
            // Checked above under the changeset's lock: a bug, and nothing is committed.
            Ok(Committed::Unmatched) => {
                return Err(DbError::Corrupt(
                    "an entry to unpublish no longer publishes its pinned revision",
                ));
            }
            Err(DbError::SlugTaken) => return Ok(Err(Refusal::SlugTaken)),
            Err(DbError::ValueTaken) => return Ok(Err(Refusal::ValueTaken)),
            Err(DbError::EntryDeleted { entry }) => {
                return Ok(Err(Refusal::EntryDeleted { entry }));
            }
            Ok(Committed::Broken(broken)) => return Ok(Err(Refusal::BrokenReference(broken))),
            Ok(Committed::UnusableAsset {
                entry,
                field,
                asset,
                state,
            }) => {
                return Ok(Err(Refusal::UnusableAsset {
                    entry,
                    field,
                    asset,
                    state,
                }));
            }
            Err(error) => return Err(error),
        }
    }
    let (_, current) = releases::generation(tx).await?;
    Ok(Err(Refusal::GenerationMoved { current }))
}
