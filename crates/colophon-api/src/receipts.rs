//! Delivery receipts (docs/modules/06-delivery.md, "Evidence probes"; D-051). Under `{P}`:
//!
//! - `GET {P}/releases/{generation}`: one release with its label, what it changed for readers
//!   (each entry it published or took down, with what readers had before), each destination's
//!   part in delivering it, and every probe attempt made for it, oldest first.
//!
//! A release's label comes from `colophon_domain::receipt::label`: a destination counts as done
//! for a release once one of its jobs at that release or later succeeded, and only probes can
//! verify, scoped to the locations that looked.

use axum::{
    Json,
    extract::{Path, State},
};
use colophon_db::releases::{self, Release};
use colophon_domain::receipt::{self, Destination, Kind, Label};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    changesets::ReleaseView,
    entries::{content_hash, open_isolated, rfc3339},
    session::SignedIn,
    shape::Data,
};

#[derive(Deserialize)]
pub(crate) struct OnePath {
    project: String,
    environment: String,
    generation: i64,
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
}

#[derive(Serialize)]
pub(crate) struct ReceiptView {
    #[serde(flatten)]
    release: ReleaseView,
    /// What went live and what was taken down, up to 500.
    changes: Vec<ChangeView>,
    deliveries: Vec<DeliveryView>,
    observations: Vec<SeenView>,
}

/// `GET {P}/releases/{generation}`.
pub(crate) async fn get(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
) -> Result<Json<Data<ReceiptView>>, ApiError> {
    // One snapshot: the release row (which counts the locations that saw it and the entries it
    // changed), its changes and the deliveries are separate statements, and a probe finishing
    // between them would show a delivery done with no location that saw it.
    let mut tx = open_isolated(
        &state,
        caller,
        &path.project,
        &path.environment,
        colophon_db::scope::Isolation::Snapshot,
    )
    .await?;
    let release = releases::list(&mut tx, releases::Window::At(path.generation))
        .await
        .map_err(ApiError::from_db)?
        .into_iter()
        .next()
        .ok_or(ApiError::NotFound)?;
    let (deliveries, seen) = releases::receipt(&mut tx, path.generation)
        .await
        .map_err(ApiError::from_db)?;
    let changes = releases::changes(&mut tx, path.generation)
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
            changes: changes
                .into_iter()
                .map(|change| ChangeView {
                    kind: change.kind,
                    entry_id: change.entry_id,
                    collection: change.collection,
                    title: change.title,
                    revision_id: change.revision_id,
                    previous_revision_id: change.previous_revision_id,
                    entry_deleted: change.entry_deleted,
                })
                .collect(),
            deliveries,
            observations,
        },
    }))
}
