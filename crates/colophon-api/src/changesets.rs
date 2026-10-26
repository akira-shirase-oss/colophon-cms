//! Changesets and reviews (docs/modules/04-api.md, "Route families"; D-044). Under
//! `{P}` = `/api/v1/projects/{project}/environments/{environment}`:
//!
//! - `GET  {P}/changesets`: the newest 100, each with its status;
//! - `POST {P}/changesets`: `{"title", "entries": [ids]}` (`Idempotency-Key` required): pins
//!   each entry's current revision;
//! - `GET  {P}/changesets/{changeset}`: items, status and reviews, with `ETag: "sha256:…"`;
//! - `POST {P}/changesets/{changeset}/submit`, `…/refresh` and `…/reviews` (`{"decision",
//!   "comment"}`): each needs `If-Match` with the hash the caller looked at. A refresh re-pins
//!   edited entries, which gives the changeset a new hash; reviews are for owners and publishers;
//! - `POST {P}/changesets/{changeset}/publish`: `{"base_generation"}` (`If-Match` and
//!   `Idempotency-Key` required): commits an approved changeset as the next release (D-045).
//!
//! A review is stored with the hash it was made on, under a lock on the changeset, and counts only
//! while the changeset keeps that hash (`colophon_domain::changeset::status`).

use std::collections::HashSet;

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
};
use colophon_db::{
    changesets::{self, Head, Pin},
    idempotency::{self, Claim},
    releases::{self, Committed},
    scope::ProjectTransaction,
};
use colophon_domain::{
    changeset::{self, Decision, Facts, Status},
    policy::Action,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    entries::{content_hash, idempotency_key, json, open, request_hash, rfc3339, sha256},
    members::require,
    session::SignedIn,
    shape::{Data, Page},
};

const CREATE: &str = "changeset.create";
const PUBLISH: &str = "changeset.publish";

#[derive(Deserialize)]
pub(crate) struct ListPath {
    project: String,
    environment: String,
}

#[derive(Deserialize)]
pub(crate) struct OnePath {
    project: String,
    environment: String,
    changeset: String,
}

/// `{"title": "Launch posts", "entries": ["…", "…"]}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewChangeset {
    title: String,
    entries: Vec<String>,
}

/// `{"base_generation": 4}`: the generation the publisher saw.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewRelease {
    base_generation: i64,
}

/// `{"decision": "approve", "comment": "…"}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewReview {
    decision: String,
    #[serde(default)]
    comment: String,
}

fn hex(bytes: &[u8]) -> String {
    content_hash(bytes)
        .strip_prefix("sha256:")
        .unwrap_or_default()
        .to_owned()
}

/// What the domain hashes and decides about, from what the database pins.
fn domain_item(pin: &Pin) -> changeset::Item {
    changeset::Item {
        entry: pin.entry_id.to_string(),
        revision: pin.revision_id.to_string(),
        collection: pin.collection.clone(),
        content_hash: hex(&pin.content_hash),
        schema_version: pin.schema_version,
        definition_hash: hex(&pin.definition_hash),
    }
}

/// The changeset hash of `pins`.
fn hash_of(pins: &[Pin]) -> [u8; 32] {
    let items: Vec<changeset::Item> = pins.iter().map(domain_item).collect();
    sha256(changeset::canonical(&items).as_bytes())
}

#[derive(Serialize)]
pub(crate) struct ItemView {
    entry_id: Uuid,
    collection: String,
    revision_id: Uuid,
    title: String,
    content_hash: String,
    schema_version: i32,
    current_revision_id: Uuid,
    /// The entry has a newer revision than the one pinned.
    edited: bool,
}

#[derive(Serialize)]
pub(crate) struct ReviewView {
    id: Uuid,
    reviewer_id: Uuid,
    reviewer_name: Option<String>,
    decision: String,
    changeset_hash: String,
    /// Made on the changeset's current hash, so it still counts.
    current: bool,
    comment: String,
    created_at: String,
}

/// Where a changeset stands, as the API writes it.
#[derive(Serialize)]
pub(crate) struct StatusView {
    /// `open`, `stale`, `awaiting_review`, `changes_requested`, `approved`, `approval_expired` or
    /// `published`.
    state: &'static str,
    /// For `published`: the release's generation.
    generation: Option<i64>,
    /// Who asked for changes or approved.
    by: Option<String>,
    /// The approver also submitted it.
    self_review: bool,
    /// Entries edited since they were pinned.
    edited_items: usize,
}

impl StatusView {
    fn new(status: Status, edited_items: usize) -> Self {
        let generation = match status {
            Status::Published { generation } => Some(generation),
            _ => None,
        };
        let (state, by, self_review) = match status {
            Status::Published { .. } => ("published", None, false),
            Status::Open => ("open", None, false),
            Status::Stale { .. } => ("stale", None, false),
            Status::AwaitingReview => ("awaiting_review", None, false),
            Status::ChangesRequested { by } => ("changes_requested", Some(by), false),
            Status::Approved { by, self_review } => ("approved", Some(by), self_review),
            Status::ApprovalExpired => ("approval_expired", None, false),
        };
        Self {
            state,
            generation,
            by,
            self_review,
            edited_items,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct ChangesetView {
    id: Uuid,
    title: String,
    hash: String,
    status: StatusView,
    created_by: Uuid,
    created_at: String,
    submitted_by: Option<Uuid>,
    submitted_at: Option<String>,
    updated_at: String,
    items: Vec<ItemView>,
    reviews: Vec<ReviewView>,
}

#[derive(Serialize)]
pub(crate) struct SummaryView {
    id: Uuid,
    title: String,
    hash: String,
    status: StatusView,
    items: i64,
    submitted_at: Option<String>,
    updated_at: String,
}

fn unix(at: OffsetDateTime) -> i64 {
    at.unix_timestamp()
}

/// The domain's decision for `head`, from its reviews on its current hash (newest first).
fn decide(head: &Head, edited: usize, reviews: &[colophon_db::changesets::Review]) -> Status {
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
                at: unix(review.created_at),
            })
        })
        .collect();
    let submitter = head.submitted_by.map(|id| id.to_string());
    changeset::status(&Facts {
        hash: &hash,
        submitted_by: submitter.as_deref(),
        stale_items: edited,
        reviews: &reviews,
        now: unix(OffsetDateTime::now_utc()),
        // Projects can't require a different reviewer yet; the rule is in place for when they can.
        require_different_reviewer: false,
        published: head.published,
    })
}

/// Names the approver or requester by display name where the database knows it.
fn named(status: Status, reviews: &[colophon_db::changesets::Review]) -> Status {
    let name = |id: &str| {
        reviews
            .iter()
            .find(|review| review.reviewer_id.to_string() == id)
            .and_then(|review| review.reviewer_name.clone())
            .unwrap_or_else(|| id.to_owned())
    };
    match status {
        Status::ChangesRequested { by } => Status::ChangesRequested { by: name(&by) },
        Status::Approved { by, self_review } => Status::Approved {
            by: name(&by),
            self_review,
        },
        other => other,
    }
}

/// The full view of the changeset `head`, read in `tx`.
async fn view(tx: &mut ProjectTransaction, head: Head) -> Result<ChangesetView, ApiError> {
    let items = changesets::items(tx, head.id)
        .await
        .map_err(ApiError::from_db)?;
    let reviews = changesets::reviews(tx, head.id, None)
        .await
        .map_err(ApiError::from_db)?;
    let edited = items
        .iter()
        .filter(|item| item.current_revision_id != item.pin.revision_id)
        .count();
    let status = named(decide(&head, edited, &reviews), &reviews);
    Ok(ChangesetView {
        id: head.id,
        title: head.title,
        hash: content_hash(&head.hash),
        status: StatusView::new(status, edited),
        created_by: head.created_by,
        created_at: rfc3339(head.created_at)?,
        submitted_by: head.submitted_by,
        submitted_at: head.submitted_at.map(rfc3339).transpose()?,
        updated_at: rfc3339(head.updated_at)?,
        items: items
            .into_iter()
            .map(|item| ItemView {
                entry_id: item.pin.entry_id,
                collection: item.pin.collection,
                revision_id: item.pin.revision_id,
                title: item.title,
                content_hash: content_hash(&item.pin.content_hash),
                schema_version: item.pin.schema_version,
                current_revision_id: item.current_revision_id,
                edited: item.current_revision_id != item.pin.revision_id,
            })
            .collect(),
        reviews: reviews
            .into_iter()
            .map(|review| {
                Ok(ReviewView {
                    id: review.id,
                    reviewer_id: review.reviewer_id,
                    reviewer_name: review.reviewer_name,
                    decision: review.decision,
                    current: review.changeset_hash == head.hash,
                    changeset_hash: content_hash(&review.changeset_hash),
                    comment: review.comment,
                    created_at: rfc3339(review.created_at)?,
                })
            })
            .collect::<Result<_, ApiError>>()?,
    })
}

fn respond(status: StatusCode, body: String, hash: &str, replayed: bool) -> Response {
    let mut response = json(status, body, None, replayed);
    if let Ok(etag) = HeaderValue::from_str(&format!("\"{hash}\"")) {
        response.headers_mut().insert(header::ETAG, etag);
    }
    response
}

fn body(view: &ChangesetView) -> Result<String, ApiError> {
    serde_json::to_string(&Data { data: view }).map_err(ApiError::internal)
}

/// The hash in `If-Match`, as `sha256:<hex>`.
fn if_match(headers: &HeaderMap) -> Result<String, ApiError> {
    let value = headers.get(header::IF_MATCH).ok_or(ApiError::PreconditionRequired(
        "this needs an If-Match header with the changeset's ETag, so you act on what you looked at",
    ))?;
    value
        .to_str()
        .ok()
        .and_then(|text| text.trim().strip_prefix('"')?.strip_suffix('"'))
        .filter(|hash| {
            hash.strip_prefix("sha256:").is_some_and(|hex| {
                hex.len() == 64
                    && hex
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        })
        .map(str::to_owned)
        .ok_or(ApiError::InvalidHeader(
            "If-Match must be one changeset ETag, such as \"sha256:…\"",
        ))
}

/// Locks the changeset and checks it still has the hash the caller looked at.
async fn locked(tx: &mut ProjectTransaction, id: &str, expected: &str) -> Result<Head, ApiError> {
    let id = Uuid::try_parse(id).map_err(|_| ApiError::NotFound)?;
    let head = changesets::lock(tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let current = content_hash(&head.hash);
    if current != expected {
        return Err(ApiError::StaleChangeset { current });
    }
    Ok(head)
}

fn may_write(tx: &ProjectTransaction) -> Result<(), ApiError> {
    require(
        &tx.role,
        Action::WriteContent,
        "you can't change content in this project",
    )
}

/// `GET {P}/changesets`.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ListPath>,
) -> Result<Json<Page<SummaryView>>, ApiError> {
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let rows = changesets::list(&mut tx).await.map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let data = rows
        .into_iter()
        .map(|row| {
            let edited = usize::try_from(row.stale).unwrap_or(usize::MAX);
            let latest: Vec<_> = row.latest.into_iter().collect();
            let status = named(decide(&row.head, edited, &latest), &latest);
            Ok(SummaryView {
                id: row.head.id,
                title: row.head.title,
                hash: content_hash(&row.head.hash),
                status: StatusView::new(status, edited),
                items: row.items,
                submitted_at: row.head.submitted_at.map(rfc3339).transpose()?,
                updated_at: rfc3339(row.head.updated_at)?,
            })
        })
        .collect::<Result<_, ApiError>>()?;
    Ok(Json(Page {
        data,
        next_cursor: None,
    }))
}

/// `POST {P}/changesets`.
pub(crate) async fn create(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ListPath>,
    headers: HeaderMap,
    body_json: Result<Json<NewChangeset>, JsonRejection>,
) -> Result<Response, ApiError> {
    let key = idempotency_key(&headers)?;
    let Json(new) = body_json?;
    let title = new.title.trim();
    if !(1..=200).contains(&title.chars().count()) || title.chars().any(char::is_control) {
        return Err(ApiError::InvalidField {
            reason: "invalid_title",
            pointer: "/title",
            message: "a title is 1 to 200 characters, without control characters".to_owned(),
        });
    }
    let entries: Vec<Uuid> = new
        .entries
        .iter()
        .map(|id| Uuid::try_parse(id))
        .collect::<Result<_, _>>()
        .map_err(|_| ApiError::InvalidField {
            reason: "unknown_entry",
            pointer: "/entries",
            message: "every entry is an entry id in this environment".to_owned(),
        })?;
    let distinct: HashSet<Uuid> = entries.iter().copied().collect();
    if entries.is_empty()
        || entries.len() > changesets::MAX_ITEMS
        || distinct.len() != entries.len()
    {
        return Err(ApiError::InvalidField {
            reason: "invalid_entries",
            pointer: "/entries",
            message: format!(
                "a changeset holds 1 to {} different entries",
                changesets::MAX_ITEMS
            ),
        });
    }
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_write(&tx)?;
    let mut sorted: Vec<String> = entries.iter().map(Uuid::to_string).collect();
    sorted.sort();
    let request = request_hash(&[CREATE, title, &sorted.join(",")]);
    match idempotency::claim(&mut tx, CREATE, key, &request)
        .await
        .map_err(ApiError::from_db)?
    {
        Claim::Fresh => {}
        Claim::Mismatch => return Err(ApiError::IdempotencyKeyReused),
        Claim::Replay { status, response } => {
            let status = StatusCode::from_u16(status).map_err(ApiError::internal)?;
            let hash = serde_json::from_str::<Value>(&response)
                .ok()
                .and_then(|body| body["data"]["hash"].as_str().map(str::to_owned))
                .unwrap_or_default();
            return Ok(respond(status, response, &hash, true));
        }
    }
    let pins = changesets::pins(&mut tx, &entries)
        .await
        .map_err(ApiError::from_db)?;
    if pins.len() != entries.len() {
        return Err(ApiError::InvalidField {
            reason: "unknown_entry",
            pointer: "/entries",
            message: "every entry is an entry id in this environment".to_owned(),
        });
    }
    let id = Uuid::now_v7();
    changesets::create(&mut tx, id, title, &pins, &hash_of(&pins))
        .await
        .map_err(ApiError::from_db)?;
    let head = changesets::get(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or_else(|| ApiError::internal(std::io::Error::other("a changeset vanished")))?;
    let created = view(&mut tx, head).await?;
    let text = body(&created)?;
    idempotency::complete(&mut tx, CREATE, key, StatusCode::CREATED.as_u16(), &text)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(respond(StatusCode::CREATED, text, &created.hash, false))
}

/// `GET {P}/changesets/{changeset}`.
pub(crate) async fn get(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
) -> Result<Response, ApiError> {
    let id = Uuid::try_parse(&path.changeset).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let head = changesets::get(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let shown = view(&mut tx, head).await?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(respond(StatusCode::OK, body(&shown)?, &shown.hash, false))
}

/// `POST {P}/changesets/{changeset}/submit`: asks for review; a no-op if already submitted.
pub(crate) async fn submit(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let expected = if_match(&headers)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_write(&tx)?;
    let head = locked(&mut tx, &path.changeset, &expected).await?;
    changesets::submit(&mut tx, head.id)
        .await
        .map_err(ApiError::from_db)?;
    finish(tx, head.id).await
}

/// `POST {P}/changesets/{changeset}/refresh`: re-pins entries edited since they were pinned.
pub(crate) async fn refresh(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let expected = if_match(&headers)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_write(&tx)?;
    let head = locked(&mut tx, &path.changeset, &expected).await?;
    let entries: Vec<Uuid> = changesets::items(&mut tx, head.id)
        .await
        .map_err(ApiError::from_db)?
        .into_iter()
        .map(|item| item.pin.entry_id)
        .collect();
    let pins = changesets::pins(&mut tx, &entries)
        .await
        .map_err(ApiError::from_db)?;
    let hash = hash_of(&pins);
    if hash[..] != head.hash[..] {
        changesets::repin(&mut tx, head.id, &pins, &hash)
            .await
            .map_err(ApiError::from_db)?;
    }
    finish(tx, head.id).await
}

/// `POST {P}/changesets/{changeset}/reviews`: approves or asks for changes to the exact hash in
/// `If-Match`.
pub(crate) async fn review(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
    headers: HeaderMap,
    body_json: Result<Json<NewReview>, JsonRejection>,
) -> Result<Response, ApiError> {
    let expected = if_match(&headers)?;
    let Json(new) = body_json?;
    let decision = Decision::parse(&new.decision).ok_or_else(|| ApiError::InvalidField {
        reason: "invalid_decision",
        pointer: "/decision",
        message: "a decision is approve or request_changes".to_owned(),
    })?;
    if new.comment.chars().count() > 2000 {
        return Err(ApiError::InvalidField {
            reason: "invalid_comment",
            pointer: "/comment",
            message: "a comment is at most 2,000 characters".to_owned(),
        });
    }
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    require(
        &tx.role,
        Action::Review,
        "only a project's owners and publishers can review changesets",
    )?;
    let head = locked(&mut tx, &path.changeset, &expected).await?;
    if head.submitted_by.is_none() {
        return Err(ApiError::Conflict(
            "this changeset hasn't been submitted for review yet",
        ));
    }
    let edited = changesets::items(&mut tx, head.id)
        .await
        .map_err(ApiError::from_db)?
        .iter()
        .any(|item| item.current_revision_id != item.pin.revision_id);
    if edited {
        return Err(ApiError::Conflict(
            "an entry was edited after this changeset pinned it; refresh the changeset, then review it",
        ));
    }
    changesets::review(
        &mut tx,
        head.id,
        &head.hash,
        decision.as_str(),
        new.comment.trim(),
    )
    .await
    .map_err(ApiError::from_db)?;
    finish(tx, head.id).await
}

#[derive(Serialize)]
pub(crate) struct PublishedView {
    generation: i64,
    epoch: i64,
    changeset_id: Uuid,
    changeset_hash: String,
    /// Entries whose published revision changed.
    entries_changed: u64,
}

/// Refuses to publish anything but an approved changeset, saying what it needs instead.
fn publishable(status: &Status) -> Result<(), ApiError> {
    match status {
        Status::Approved { .. } => Ok(()),
        Status::Published { .. } => Err(ApiError::Conflict(
            "this changeset has already been published",
        )),
        Status::Stale { .. } => Err(ApiError::Conflict(
            "an entry was edited after this changeset pinned it; refresh and review it again before publishing",
        )),
        Status::ApprovalExpired => Err(ApiError::Conflict(
            "the approval has expired; it needs reviewing again before publishing",
        )),
        Status::Open | Status::AwaitingReview | Status::ChangesRequested { .. } => Err(
            ApiError::Conflict("only an approved changeset can be published"),
        ),
    }
}

/// `POST {P}/changesets/{changeset}/publish`: commits the approved changeset as the release after
/// `base_generation`, if the environment is still there.
pub(crate) async fn publish(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
    headers: HeaderMap,
    body_json: Result<Json<NewRelease>, JsonRejection>,
) -> Result<Response, ApiError> {
    let expected = if_match(&headers)?;
    let key = idempotency_key(&headers)?;
    let Json(new) = body_json?;
    if new.base_generation < 0 {
        return Err(ApiError::InvalidField {
            reason: "invalid_generation",
            pointer: "/base_generation",
            message: "a generation is 0 or more".to_owned(),
        });
    }
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    require(
        &tx.role,
        Action::Publish,
        "only a project's owners and publishers can publish",
    )?;
    let request = request_hash(&[
        PUBLISH,
        &path.changeset,
        &expected,
        &new.base_generation.to_string(),
    ]);
    match idempotency::claim(&mut tx, PUBLISH, key, &request)
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
    let head = locked(&mut tx, &path.changeset, &expected).await?;
    let items = changesets::items(&mut tx, head.id)
        .await
        .map_err(ApiError::from_db)?;
    let reviews = changesets::reviews(&mut tx, head.id, Some(&head.hash))
        .await
        .map_err(ApiError::from_db)?;
    let edited = items
        .iter()
        .filter(|item| item.current_revision_id != item.pin.revision_id)
        .count();
    publishable(&decide(&head, edited, &reviews))?;
    // Approved means the newest decision on this hash is an approval: the one it publishes under.
    let approval = reviews
        .first()
        .ok_or_else(|| ApiError::internal(std::io::Error::other("approved without a review")))?;
    let committed = releases::commit(
        &mut tx,
        new.base_generation,
        head.id,
        &head.hash,
        approval.id,
    )
    .await
    .map_err(ApiError::from_db)?;
    let (generation, epoch, changed) = match committed {
        Committed::Done {
            generation,
            epoch,
            changed,
        } => (generation, epoch, changed),
        Committed::Moved { current } => return Err(ApiError::GenerationMoved { current }),
    };
    tracing::info!(project = %tx.project, generation, changeset = %head.id, "release committed");
    let text = serde_json::to_string(&Data {
        data: PublishedView {
            generation,
            epoch,
            changeset_id: head.id,
            changeset_hash: content_hash(&head.hash),
            entries_changed: changed,
        },
    })
    .map_err(ApiError::internal)?;
    idempotency::complete(&mut tx, PUBLISH, key, StatusCode::CREATED.as_u16(), &text)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(json(StatusCode::CREATED, text, None, false))
}

#[derive(Serialize)]
pub(crate) struct ReleaseView {
    generation: i64,
    epoch: i64,
    changeset_id: Uuid,
    title: String,
    changeset_hash: String,
    published_by: Uuid,
    published_by_name: Option<String>,
    published_at: String,
    /// Entries whose published revision changed in this release.
    entries: i64,
}

/// `GET {P}/releases`: the newest 100, with the environment's current generation.
pub(crate) async fn releases(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ListPath>,
) -> Result<Response, ApiError> {
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let (_, generation) = releases::generation(&mut tx)
        .await
        .map_err(ApiError::from_db)?;
    let rows = releases::list(&mut tx).await.map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let data = rows
        .into_iter()
        .map(|row| {
            Ok(ReleaseView {
                generation: row.generation,
                epoch: row.epoch,
                changeset_id: row.changeset_id,
                title: row.title,
                changeset_hash: content_hash(&row.changeset_hash),
                published_by: row.published_by,
                published_by_name: row.published_by_name,
                published_at: rfc3339(row.published_at)?,
                entries: row.entries,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let body = serde_json::to_string(&Page {
        data,
        next_cursor: None,
    })
    .map_err(ApiError::internal)?;
    let mut response = json(StatusCode::OK, body, None, false);
    response
        .headers_mut()
        .insert(crate::delivery::GENERATION, HeaderValue::from(generation));
    Ok(response)
}

/// Reads the changeset back, commits, and answers with it.
async fn finish(mut tx: ProjectTransaction, id: Uuid) -> Result<Response, ApiError> {
    let head = changesets::get(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let shown = view(&mut tx, head).await?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(respond(StatusCode::OK, body(&shown)?, &shown.hash, false))
}
