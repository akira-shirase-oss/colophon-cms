//! Uploads (docs/modules/07-media.md). `POST …/assets` takes the bytes themselves: the server
//! writes them to a random quarantine key, then records the asset and its verify job in one
//! transaction — `202` means accepted for checking, nothing more (truthful states). The object
//! lands before the transaction opens so no transaction ever spans an object-store call; an
//! upload whose record never commits leaves an orphan quarantine object, which is never served
//! and is what the expiry job (P6) exists for.
//!
//! `GET …/assets` lists them and `GET …/assets/{asset}` reads one, for the picker's list and
//! for clients polling `quarantined` → `usable`/`rejected`.

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::Response,
};
use object_store::ObjectStoreExt;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use colophon_db::{assets, idempotency};
use colophon_media::{Intent, IntentError, quarantine_key};

use crate::{
    ApiError,
    entries::{idempotency_key, json, open, request_hash},
    session::{SignedIn, services},
    state::AppState,
};

/// Uploads are claimed under this operation.
const UPLOAD: &str = "asset.upload";

/// The environment segment of an asset path.
#[derive(Deserialize)]
pub(crate) struct ListPath {
    project: String,
    environment: String,
}

/// One asset's path.
#[derive(Deserialize)]
pub(crate) struct OnePath {
    project: String,
    environment: String,
    asset: String,
}

/// `POST {P}/assets`: the upload's bytes as the body, its name and claimed type in headers.
/// Answers `202` with the quarantined asset; the verify job decides what it becomes.
pub(crate) async fn upload(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ListPath>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let store = services(&state)?
        .assets
        .clone()
        .ok_or(ApiError::Unavailable(
            "uploads are not configured on this server",
        ))?;
    let key = idempotency_key(&headers)?;
    let file_name = headers
        .get("x-colophon-file-name")
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::InvalidHeader(
            "uploads need an X-Colophon-File-Name header with the file's name",
        ))?;
    let declared = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::InvalidHeader(
            "uploads need a Content-Type naming the claimed image type",
        ))?;
    let intent =
        Intent::new(file_name, declared, body.len() as u64).map_err(|error| field_error(&error))?;
    // The object lands first: no transaction may span this call, and an object whose record
    // never commits can only wait in quarantine, where nothing serves it.
    let asset = Uuid::now_v7();
    let storage_name = format!("{asset}-{}", &Uuid::now_v7().simple().to_string()[..8]);
    let quarantine_key = quarantine_key(&path.project, &path.environment, &storage_name);
    store
        .put(
            &object_store::path::Path::from(quarantine_key.clone()),
            body.into(),
        )
        .await
        .map_err(|error| {
            tracing::warn!(error = %error, "quarantine write failed");
            ApiError::Unavailable("the object store refused the upload")
        })?;
    let hash = request_hash(&[
        UPLOAD,
        intent.file_name.as_str(),
        intent.declared_type.as_str(),
        &intent.size.to_string(),
    ]);
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    // Assets belong to the project, not one collection: the whole-project form of the grant.
    crate::members::require(
        &tx.policy,
        colophon_domain::policy::Action::ContentCreate,
        None,
        "your role can't add assets to this project",
    )?;
    match idempotency::claim(&mut tx, UPLOAD, key, &hash)
        .await
        .map_err(ApiError::from_db)?
    {
        idempotency::Claim::Fresh => {}
        idempotency::Claim::Mismatch => return Err(ApiError::IdempotencyKeyReused),
        idempotency::Claim::Replay { status, response } => {
            return Ok(json(
                StatusCode::from_u16(status).map_err(ApiError::internal)?,
                response,
                None,
                true,
            ));
        }
    }
    let job = Uuid::now_v7();
    let deadline = time::OffsetDateTime::now_utc()
        + time::Duration::seconds(colophon_domain::job::LIFETIME_SECS);
    assets::insert(
        &mut tx,
        asset,
        &intent.file_name,
        &intent.declared_type,
        i64::try_from(intent.size).unwrap_or(i64::MAX),
        &quarantine_key,
        caller.identity,
        job,
        key,
        deadline,
    )
    .await
    .map_err(ApiError::from_db)?;
    let response = serde_json::to_string(&json!({"data": {
        "id": asset,
        "state": "quarantined",
        "file_name": intent.file_name,
        "declared_type": intent.declared_type,
        "size_bytes": intent.size,
    }}))
    .map_err(ApiError::internal)?;
    idempotency::complete(&mut tx, UPLOAD, key, 202, &response)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(json(StatusCode::ACCEPTED, response, None, false))
}

/// `GET {P}/assets`: the environment's assets, newest first.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ListPath>,
) -> Result<Response, ApiError> {
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_read(&tx)?;
    let items = assets::list(&mut tx).await.map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(json(
        StatusCode::OK,
        serde_json::to_string(&json!({
            "data": items.iter().map(view).collect::<Vec<_>>(),
        }))
        .map_err(ApiError::internal)?,
        None,
        false,
    ))
}

/// `GET {P}/assets/{asset}`: one asset, or 404.
pub(crate) async fn get(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
) -> Result<Response, ApiError> {
    let asset = Uuid::try_parse(&path.asset).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_read(&tx)?;
    let asset = assets::get(&mut tx, asset)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(json(
        StatusCode::OK,
        serde_json::to_string(&json!({"data": view(&asset)})).map_err(ApiError::internal)?,
        None,
        false,
    ))
}

/// The read side's permission: project-wide `ContentRead`, matching upload's `ContentCreate`.
fn may_read(tx: &colophon_db::scope::ProjectTransaction) -> Result<(), ApiError> {
    crate::members::require(
        &tx.policy,
        colophon_domain::policy::Action::ContentRead,
        None,
        "your role can't see this project's assets",
    )
}

/// An intent error as a field refusal, pointing at what it checked.
fn field_error(error: &IntentError) -> ApiError {
    let (pointer, message) = match error {
        IntentError::FileName => ("/x-colophon-file-name", error.to_string()),
        IntentError::FileType => ("/content-type", error.to_string()),
        IntentError::FileSize => ("/body", error.to_string()),
    };
    ApiError::InvalidField {
        reason: "invalid_upload",
        pointer,
        message,
    }
}

/// The public shape of an asset row.
fn view(asset: &assets::Asset) -> serde_json::Value {
    json!({
        "id": asset.id,
        "state": asset.state,
        "file_name": asset.file_name,
        "declared_type": asset.declared_type,
        "detected_type": asset.detected_type,
        "size_bytes": asset.size_bytes,
        "width": asset.width,
        "height": asset.height,
        "rejected_reason": asset.rejected_reason,
        "created_at": asset
            .created_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
    })
}
