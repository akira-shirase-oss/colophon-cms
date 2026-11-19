//! Delivery receipts (docs/modules/06-delivery.md, "Evidence probes"; D-051). Under `{P}`:
//!
//! - `GET {P}/releases/{generation}`: one release with its label, each destination's part in
//!   delivering it, and every probe attempt made for it, oldest first;
//! - `GET {P}/releases/{generation}/changes?limit=&cursor=`: what it changed for readers, a page
//!   at a time — each entry it published or took down, with what readers had before — as far as
//!   the caller's role reads them (D-185).
//!
//! A release's label comes from `colophon_domain::receipt::label`: a destination counts as done
//! for a release once one of its jobs at that release or later succeeded, and only probes can
//! verify, scoped to the locations that looked.

use axum::{
    Json,
    extract::{Path, Query, State},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use colophon_db::releases::{self, ChangesAfter, Release};
use colophon_domain::receipt::{self, Destination, Kind, Label};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    changesets::ReleaseView,
    entries::{content_hash, open, open_isolated, rfc3339},
    session::SignedIn,
    shape::{Data, Page},
    visibility::{self, Shown},
};

#[derive(Deserialize)]
pub(crate) struct OnePath {
    project: String,
    environment: String,
    /// Read as text, so a generation that isn't a number answers like one that doesn't exist.
    generation: String,
}

/// A release's generation from its path segment: a positive `i64`, or no such release.
fn generation(raw: &str) -> Result<i64, ApiError> {
    raw.parse::<i64>()
        .ok()
        .filter(|generation| *generation >= 1)
        .ok_or(ApiError::NotFound)
}

/// The label of `release`, from its destinations' states and the locations that saw it.
pub(crate) fn label(release: &Release) -> Label {
    let destinations: Vec<Destination> = release
        .delivery_kinds
        .iter()
        .zip(&release.delivery_covered)
        .zip(&release.delivery_open)
        .map(|((kind, covered), open)| Destination {
            kind: match kind.as_str() {
                "probe" => Kind::Probe,
                "cdn" => Kind::Cdn,
                _ => Kind::Webhook,
            },
            covered: *covered,
            open: *open,
        })
        .collect();
    receipt::label(
        &destinations,
        u32::try_from(release.locations).unwrap_or(u32::MAX),
    )
}

/// One destination's part in delivering a release.
#[derive(Serialize)]
pub(crate) struct DeliveryView {
    destination_id: Uuid,
    name: String,
    kind: String,
    location: Option<String>,
    /// `done` (its job for this release succeeded), `covered` (a later release's did, which
    /// covers this one: see `covered_by`), `pending` or `failed`.
    state: &'static str,
    covered_by: Option<i64>,
    /// Its job for this very release, if it had one.
    status: Option<String>,
    attempts: Option<i32>,
    last_error: Option<String>,
    finished_at: Option<String>,
}

/// One probe attempt.
#[derive(Serialize)]
pub(crate) struct SeenView {
    destination_id: Uuid,
    location: String,
    /// The release the answer said it was built from, if it said.
    observed_generation: Option<i64>,
    http_status: Option<i16>,
    body_hash: Option<String>,
    error: Option<String>,
    observed_at: String,
}

/// One entry the release changed for readers.
#[derive(Serialize)]
pub(crate) struct ChangeView {
    /// `publish` or `unpublish`.
    kind: String,
    entry_id: Uuid,
    collection: String,
    /// The title of the version it published, or took down.
    title: String,
    revision_id: Uuid,
    /// What readers had just before: the version it replaced or took down; `null` when the
    /// entry was new to them.
    previous_revision_id: Option<Uuid>,
    /// Deleted since (D-080).
    entry_deleted: bool,
    /// The caller's role doesn't read this entry's title field, so `title` is blank (D-058).
    title_hidden: bool,
}

#[derive(Serialize)]
pub(crate) struct ReceiptView {
    #[serde(flatten)]
    release: ReleaseView,
    deliveries: Vec<DeliveryView>,
    observations: Vec<SeenView>,
}

/// `GET {P}/releases/{generation}`.
pub(crate) async fn get(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
) -> Result<Json<Data<ReceiptView>>, ApiError> {
    // One snapshot: the release row (which counts the locations that saw it) and the
    // deliveries are separate statements, and a probe finishing between them would show a
    // delivery done with no location that saw it.
    let generation = generation(&path.generation)?;
    let mut tx = open_isolated(
        &state,
        caller,
        &path.project,
        &path.environment,
        colophon_db::scope::Isolation::Snapshot,
    )
    .await?;
    let release = releases::list(&mut tx, releases::Window::At(generation))
        .await
        .map_err(ApiError::from_db)?
        .into_iter()
        .next()
        .ok_or(ApiError::NotFound)?;
    let (deliveries, seen) = releases::receipt(&mut tx, generation)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let generation = release.generation;
    let deliveries = deliveries
        .into_iter()
        .map(|delivery| {
            let state = match delivery.covered_by {
                Some(by) if by == generation => "done",
                Some(_) => "covered",
                None if delivery.open => "pending",
                None => "failed",
            };
            Ok(DeliveryView {
                destination_id: delivery.destination_id,
                name: delivery.name,
                kind: delivery.kind,
                location: delivery.location,
                state,
                covered_by: delivery.covered_by,
                status: delivery.status,
                attempts: delivery.attempts,
                last_error: delivery.last_error,
                finished_at: delivery.finished_at.map(rfc3339).transpose()?,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let observations = seen
        .into_iter()
        .map(|seen| {
            Ok(SeenView {
                destination_id: seen.destination_id,
                location: seen.location,
                observed_generation: seen.observed_generation,
                http_status: seen.http_status,
                body_hash: seen.body_hash.as_deref().map(content_hash),
                error: seen.error,
                observed_at: rfc3339(seen.observed_at)?,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    Ok(Json(Data {
        data: ReceiptView {
            release: ReleaseView::new(release)?,
            deliveries,
            observations,
        },
    }))
}

/// `?limit=` (1–100, default 100) and `?cursor=` on a release's changes.
#[derive(Deserialize)]
pub(crate) struct ChangesQuery {
    limit: Option<i64>,
    cursor: Option<String>,
}

/// A changes cursor names the environment, the release and the `(kind, entry)` the page ended
/// at. A release's changes never change once it committed, so pages neither skip nor repeat.
fn changes_cursor(environment: Uuid, generation: i64, kind: &str, entry: Uuid) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "changes:v1:{environment}:{generation}:{kind}:{entry}"
    ))
}

fn changes_after(
    environment: Uuid,
    generation: i64,
    cursor: &str,
) -> Result<ChangesAfter, ApiError> {
    let prefix = format!("changes:v1:{environment}:{generation}:");
    URL_SAFE_NO_PAD
        .decode(cursor)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| {
            let (kind, entry) = text.strip_prefix(&prefix)?.split_once(':')?;
            let kind = ["publish", "unpublish"]
                .into_iter()
                .find(|known| *known == kind)?;
            Some(ChangesAfter {
                kind: kind.to_owned(),
                entry_id: Uuid::try_parse(entry).ok()?,
            })
        })
        .ok_or(ApiError::InvalidHeader(
            "this cursor is not valid for this list",
        ))
}

/// `GET {P}/releases/{generation}/changes`: what the release changed for readers, a page at a
/// time — what went live (new, or a newer version, with the one it replaced), then what was
/// taken down, each by entry id — as far as the caller's role reads it (D-185, D-058): an entry it
/// can't read is left out, and a title it can't read is blank with `title_hidden`. A page may so
/// hold fewer than `limit`; follow `next_cursor` until it is `null`. The release's `entries` and
/// `unpublished` count every change, read or not.
pub(crate) async fn changes(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
    Query(query): Query<ChangesQuery>,
) -> Result<Json<Page<ChangeView>>, ApiError> {
    let generation = generation(&path.generation)?;
    let limit = query.limit.unwrap_or(releases::MAX_CHANGES_PAGE);
    if !(1..=releases::MAX_CHANGES_PAGE).contains(&limit) {
        return Err(ApiError::InvalidHeader("limit must be between 1 and 100"));
    }
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let environment = tx.environment;
    let after = query
        .cursor
        .as_deref()
        .map(|cursor| changes_after(environment, generation, cursor))
        .transpose()?;
    let (_, latest) = releases::generation(&mut tx)
        .await
        .map_err(ApiError::from_db)?;
    if generation > latest {
        return Err(ApiError::NotFound);
    }
    // One more than the page: whether it came back says whether another page follows.
    let mut rows = releases::changes(&mut tx, generation, after.as_ref(), limit + 1)
        .await
        .map_err(ApiError::from_db)?;
    let page = usize::try_from(limit).unwrap_or(usize::MAX);
    let more = rows.len() > page;
    rows.truncate(page);
    let next_cursor = more
        .then(|| {
            rows.last()
                .map(|row| changes_cursor(environment, generation, &row.kind, row.entry_id))
        })
        .flatten();
    let named: Vec<(&str, Option<Uuid>)> = rows
        .iter()
        .map(|row| (row.collection.as_str(), row.created_by))
        .collect();
    let shown = visibility::shown(&mut tx, &named).await?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let data = rows
        .into_iter()
        .zip(shown)
        .filter(|(_, shown)| *shown != Shown::Hidden)
        .map(|(change, shown)| ChangeView {
            kind: change.kind,
            entry_id: change.entry_id,
            collection: change.collection,
            title: if shown == Shown::Whole {
                change.title
            } else {
                String::new()
            },
            revision_id: change.revision_id,
            previous_revision_id: change.previous_revision_id,
            entry_deleted: change.created_by.is_none(),
            title_hidden: shown == Shown::Untitled,
        })
        .collect();
    Ok(Json(Page { data, next_cursor }))
}
