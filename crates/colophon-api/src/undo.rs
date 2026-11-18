//! `POST {P}/releases/{generation}/undo` (D-184): puts back exactly what a takedown release took
//! down, in one request and one transaction, or nothing.
//!
//! The Studio's Undo after Unpublish used to be four to six requests from the browser (make a
//! request, submit it, approve it, publish it, and save the old version and then the newer one
//! again when the draft had moved on). Each step was checked, but between them another release
//! could go out, and a lost answer could leave an approved request or a draft holding the old
//! content. Here the whole undo commits together:
//!
//! 1. [`colophon_domain::undo::decide`] on facts the database reads: the release is the newest,
//!    it only took entries down, the caller published it, it is younger than the window by the
//!    database's own clock, and a person asks (D-111). Review is not re-judged here (U1): see 3.
//! 2. The same permission checks the four one-click calls make: the caller may change every entry
//!    (as `POST …/changesets` asks), review content in every collection (as `…/reviews` asks)
//!    and publish it (as `…/publish` asks, which [`publish::publish`] checks again under its
//!    locks).
//! 3. A changeset pinning, for each entry the release took down, the revision readers had —
//!    the release before is the publication set it restores, so it is marked `rollback_generation
//!    = N - 1` (D-097's history pins, which a newer draft can't make stale) and `undo_of = N`
//!    (migration 0071), which is how receipts and lists say "Undo of release N". It is submitted,
//!    approved as a recorded self-review, and published by [`publish::publish`] on base `N`, which
//!    judges it with the one review rule every publish uses (`changeset::status`, D-104): where
//!    review stages or another reviewer are required, a self-review doesn't count, so it refuses
//!    as `needs_review` and nothing is kept, and an undo is never a way around review. The
//!    same locks, type checks (D-055), deleted-entry check (D-080), slug and unique-value checks,
//!    asset gate and delivery outbox as every release. If another release committed first, the
//!    environment's conditional update refuses it and nothing is kept (`changed_since`).
//!
//! The draft is never touched: an entry whose draft had moved on is live again with its old
//! version and shows "changed". The answer is stored under the caller's idempotency key, so a
//! double click or a retried request replays the one release.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use colophon_db::{
    changesets::{self, Pin},
    idempotency::{self, Claim},
    publish::{self, Base, Refusal},
    releases,
    scope::ProjectTransaction,
};
use colophon_domain::{
    changeset::Kind,
    policy::Action,
    undo::{self, Asker, Facts, Release},
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    changesets::{hash_of, may_change_entries, refused},
    entries::{content_hash, idempotency_key, json, open, request_hash},
    members::require,
    session::{Credential, SignedIn},
    shape::Data,
};

/// What an undo's request hash starts with.
const UNDO: &str = "release.undo";
/// The idempotency scope an undo claims its key in: publishing's own. An undo is a publish, and
/// sharing the scope means one key can't name both a publish and an undo (the request hashes
/// differ, so the second is refused as a reused key). It also keeps the operation list's CHECK
/// (`idempotency_records_operation_check`) as it is: that list is replaced whole by migrations,
/// and another branch replaces it too.
const SCOPE: &str = "changeset.publish";

#[derive(Deserialize)]
pub(crate) struct UndoPath {
    project: String,
    environment: String,
    generation: i64,
}

/// What an undo answers: the release that put things back.
#[derive(Serialize)]
pub(crate) struct UndoneView {
    /// The new release.
    generation: i64,
    epoch: i64,
    /// The release it undid.
    undo_of: i64,
    changeset_id: Uuid,
    changeset_hash: String,
    /// How many entries readers get again.
    entries_restored: u64,
}

/// The longest title a changeset takes (`changesets::title`).
const TITLE_CHARS: usize = 200;

/// "Undo of release N: <the takedown's title>", cut to fit a changeset title.
fn titled(generation: i64, title: &str) -> String {
    let full = format!("Undo of release {generation}: {title}");
    if full.chars().count() <= TITLE_CHARS {
        return full;
    }
    let mut cut: String = full.chars().take(TITLE_CHARS - 1).collect();
    cut.push('…');
    cut
}

/// The answer to a refused undo, with what to do instead.
fn refusal(refused: undo::Refusal) -> ApiError {
    match refused {
        undo::Refusal::ByToken => ApiError::forbidden(
            "agent_self_review",
            "an API token can't undo a release: that would approve its own request with no person in the loop",
        ),
        undo::Refusal::NoSuchRelease { .. } => ApiError::InvalidField {
            reason: "invalid_generation",
            pointer: "/generation",
            message: "there is no release with this number in this environment".to_owned(),
        },
        undo::Refusal::ChangedSince { .. } => changed_since(),
        undo::Refusal::NotUndoable => ApiError::conflict(
            "not_undoable",
            "only a release that took entries down, and nothing else, can be undone",
        ),
        undo::Refusal::NotYours => ApiError::forbidden(
            "not_your_release",
            "only the person who published a release can undo it; publish the entries again instead",
        ),
        undo::Refusal::Expired => ApiError::conflict(
            "undo_expired",
            "the time to undo this release has passed; publish the entries again instead",
        ),
    }
}

fn changed_since() -> ApiError {
    ApiError::conflict(
        "changed_since",
        "another release went out after this one, so it can no longer be undone; publish the entries again instead",
    )
}

fn needs_review() -> ApiError {
    ApiError::conflict(
        "needs_review",
        "publishing here goes through review, so an undo can't approve itself; make a publish request instead",
    )
}

/// Reads what deciding an undo of `generation` needs and decides it; the release's facts.
async fn decided(
    tx: &mut ProjectTransaction,
    generation: i64,
    by_token: bool,
) -> Result<colophon_db::undo::Facts, ApiError> {
    // A publish's first locks, first (D-131, D-135): `publish::publish` takes the admission pair
    // before the environment row, so an undo holding that row while waiting for the pair would
    // queue in a circle with a publish holding the pair — a deadlock the race test caught.
    colophon_db::work::admission(tx)
        .await
        .map_err(ApiError::from_db)?;
    let (_, latest) = releases::generation(tx).await.map_err(ApiError::from_db)?;
    let read = if (1..=latest).contains(&generation) {
        colophon_db::undo::facts(tx, generation, undo::WINDOW_SECS)
            .await
            .map_err(ApiError::from_db)?
    } else {
        None
    };
    let facts = Facts {
        generation,
        latest,
        asker: if by_token {
            Asker::Token
        } else {
            Asker::Person(tx.identity)
        },
        release: read.as_ref().map(|read| Release {
            publisher: read.published_by,
            in_window: read.in_window,
            publishes: read.publishes,
            takes_down: read.takes_down,
        }),
    };
    undo::decide(&facts).map_err(refusal)?;
    let Some(read) = read else {
        return Err(ApiError::internal(std::io::Error::other(
            "an undoable release has no row",
        )));
    };

    Ok(read)
}

/// The pins that put back what the release's changeset took down: for each entry, the revision
/// readers had (an unpublish item pins it, D-049), now to publish. Refused when an entry has been
/// deleted since, or when the caller may not change, review or publish what it would restore.
async fn restorable(tx: &mut ProjectTransaction, changeset: Uuid) -> Result<Vec<Pin>, ApiError> {
    let taken = changesets::items(tx, changeset)
        .await
        .map_err(ApiError::from_db)?;
    if taken.iter().any(|item| item.created_by.is_none()) {
        return Err(ApiError::conflict(
            "entry_deleted",
            "an entry this release took down has been deleted since, so it can't be put back",
        ));
    }
    let pins: Vec<Pin> = taken
        .into_iter()
        .map(|item| Pin {
            kind: Kind::Publish.as_str().to_owned(),
            ..item.pin
        })
        .collect();
    let entries: Vec<Uuid> = pins.iter().map(|pin| pin.entry_id).collect();
    may_change_entries(tx, &entries).await?;
    for collection in pins.iter().map(|pin| pin.collection.as_str()) {
        require(
            &tx.policy,
            Action::ContentReview,
            Some(collection),
            "your role can't review changes to this content",
        )?;
        require(
            &tx.policy,
            Action::ContentPublish,
            Some(collection),
            "your role can't publish this content",
        )?;
    }

    Ok(pins)
}

/// Makes the undo's changeset, approves it as a recorded self-review and publishes it on base
/// `generation`; its id and the release.
async fn commit(
    tx: &mut ProjectTransaction,
    generation: i64,
    title: &str,
    pins: &[Pin],
) -> Result<(Uuid, publish::Published), ApiError> {
    let id = Uuid::now_v7();
    let hash = hash_of(pins);
    let title = titled(generation, title);
    changesets::create_undo(tx, id, &title, pins, &hash, generation)
        .await
        .map_err(ApiError::from_db)?;
    changesets::submit(tx, id)
        .await
        .map_err(ApiError::from_db)?;
    changesets::review(
        tx,
        id,
        &hash,
        "approve",
        &format!(
            "Undo of release {generation}, by the person who published it, while it was the newest release."
        ),
        None,
    )
    .await
    .map_err(ApiError::from_db)?;
    let published = publish::publish(
        tx,
        id,
        Some(&hash),
        Base::Seen(generation),
        OffsetDateTime::now_utc(),
        std::time::Duration::from_secs(colophon_domain::job::LIFETIME_SECS.unsigned_abs()),
    )
    .await
    .map_err(ApiError::from_db)?
    .map_err(|refusal| match refusal {
        // Another release committed between the check above and this one (D-045).
        Refusal::GenerationMoved { .. } => changed_since(),
        Refusal::NotPublishable(_) => needs_review(),
        other => refused(other),
    })?;
    Ok((id, published))
}

/// `POST {P}/releases/{generation}/undo`, with an `Idempotency-Key` and no body.
pub(crate) async fn undo(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<UndoPath>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let key = idempotency_key(&headers)?;
    let by_token = caller.credential == Credential::ApiToken;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let request = request_hash(&[UNDO, &path.generation.to_string()]);
    match idempotency::claim(&mut tx, SCOPE, key, &request)
        .await
        .map_err(ApiError::from_db)?
    {
        Claim::Fresh => {}
        Claim::Mismatch => return Err(ApiError::IdempotencyKeyReused),
        Claim::Replay { status, response } => {
            let status = StatusCode::from_u16(status).map_err(ApiError::internal)?;
            return Ok(json(status, response, None, true));
        }
    }
    let read = decided(&mut tx, path.generation, by_token).await?;
    let pins = restorable(&mut tx, read.changeset_id).await?;
    let (id, published) = commit(&mut tx, path.generation, &read.title, &pins).await?;
    colophon_db::audit::record_in(
        &mut tx.transaction,
        tx.project,
        tx.identity,
        "release.undone",
        &path.generation.to_string(),
        &serde_json::json!({
            "generation": published.generation,
            "undo_of": path.generation,
            "entries": published.changed,
            "title": read.title,
        })
        .to_string(),
    )
    .await
    .map_err(ApiError::from_db)?;
    tracing::info!(
        project = %tx.project,
        generation = published.generation,
        undo_of = path.generation,
        "release undone"
    );
    let text = serde_json::to_string(&Data {
        data: UndoneView {
            generation: published.generation,
            epoch: published.epoch,
            undo_of: path.generation,
            changeset_id: id,
            changeset_hash: content_hash(&published.hash),
            entries_restored: published.changed,
        },
    })
    .map_err(ApiError::internal)?;
    idempotency::complete(&mut tx, SCOPE, key, StatusCode::CREATED.as_u16(), &text)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(json(StatusCode::CREATED, text, None, false))
}

#[cfg(test)]
mod tests {
    use super::titled;

    #[test]
    fn an_undo_is_named_for_the_release_it_undoes_and_fits_a_title() {
        assert_eq!(
            titled(3, "Unpublish: Tides"),
            "Undo of release 3: Unpublish: Tides"
        );
        let long = "a".repeat(500);
        let cut = titled(12, &long);
        assert_eq!(cut.chars().count(), 200);
        assert!(cut.starts_with("Undo of release 12: aaa"));
        assert!(cut.ends_with('…'));
        // Characters, not bytes: a title of multi-byte characters is cut on a boundary.
        let wide = titled(1, &"é".repeat(300));
        assert_eq!(wide.chars().count(), 200);
    }
}
