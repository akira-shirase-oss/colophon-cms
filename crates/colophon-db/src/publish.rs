//! Publishing an approved changeset as one release (D-045): the one routine both the API's
//! publish and a schedule falling due run (D-060), so both make exactly the same checks, in the
//! same order, under the same locks. The caller holds a [`ProjectTransaction`] as the person
//! publishing: their role's policy is the one checked.

use colophon_domain::{
    changeset::{self, Decision, Facts, Status},
    policy::Action,
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
}

/// The domain's decision for `head`, from its reviews (newest first), `edited` of its entries
/// having moved on, at `now`.
pub fn status(
    head: &changesets::Head,
    edited: usize,
    reviews: &[changesets::Review],
    now: OffsetDateTime,
) -> Status {
    let hash = hex(&head.hash);
    let reviews: Vec<changeset::Review> = reviews
        .iter()
        .filter(|review| review.changeset_hash == head.hash)
        .filter_map(|review| {
            Some(changeset::Review {
                reviewer: review.reviewer_id.to_string(),
                decision: Decision::parse(&review.decision)?,
                hash: hash.clone(),
                seq: i64::from(review.seq),
                at: review.created_at.unix_timestamp(),
            })
        })
        .collect();
    let submitter = head.submitted_by.map(|id| id.to_string());
    changeset::status(&Facts {
        hash: &hash,
        submitted_by: submitter.as_deref(),
        stale_items: edited,
        reviews: &reviews,
        now: now.unix_timestamp(),
        // Projects can't require a different reviewer yet; the rule is in place for when they can.
        require_different_reviewer: false,
        published: head.published,
    })
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
    let items = changesets::items(tx, head.id).await?;
    let reviews = changesets::reviews(tx, head.id, Some(&head.hash)).await?;
    let edited = items.iter().filter(|item| item.moved()).count();
    let decided = status(&head, edited, &reviews, now);
    if !matches!(decided, Status::Approved { .. }) {
        return Ok(Err(Refusal::NotPublishable(decided)));
    }
    // Approved means the newest decision on this hash is an approval: the one it publishes under.
    let approval = reviews
        .first()
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
            Err(error) => return Err(error),
        }
    }
    let (_, current) = releases::generation(tx).await?;
    Ok(Err(Refusal::GenerationMoved { current }))
}
