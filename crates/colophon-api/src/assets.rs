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

/// A rendition's path: the asset's, plus the rendition name the worker wrote.
#[derive(Deserialize)]
pub(crate) struct RenditionPath {
    project: String,
    environment: String,
    asset: String,
    name: String,
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
    // Who may upload and what the object's key is are reads: done and closed before the store
    // sees a byte, so a refused call never leaves an object (and no transaction spans the
    // write). Keys name the environment by id, as the public key does.
    let seen = open(&state, caller, &path.project, &path.environment).await?;
    // Assets belong to the project, not one collection: the whole-project form of the grant.
    crate::members::require(
        &seen.policy,
        colophon_domain::policy::Action::ContentCreate,
        None,
        "your role can't add assets to this project",
    )?;
    let environment = seen.environment;
    seen.commit().await.map_err(ApiError::from_db)?;
    let asset = Uuid::now_v7();
    let storage_name = format!("{asset}-{}", &Uuid::now_v7().simple().to_string()[..8]);
    let quarantine_key = quarantine_key(&path.project, &environment.to_string(), &storage_name);
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

/// `GET /media/v1/{project}/{environment}/{asset}`: the asset origin — the verified bytes,
/// publicly (media M5). A `public/` object exists exactly when the worker verified it, so the
/// store is the whole authority: no session, no row read, only the environment slug resolved
/// to the id the key names. The stored `Content-Type` (written at promote) is served back, and
/// the object is immutable by construction — a changed image is a new asset — so it caches
/// forever. Quarantined and rejected assets have no `public/` object: they are 404s here, like
/// everything that never was.
pub(crate) async fn serve(
    State(state): State<AppState>,
    Path(path): Path<OnePath>,
) -> Result<Response, ApiError> {
    stream(&state, path.project, path.environment, path.asset, None).await
}

/// `GET /media/v1/{project}/{environment}/{asset}/{name}`: a rendition — the same store-read as
/// the original, under the name the worker wrote (`w320`, `w1280`). Any other name 404s because
/// no object answers it.
pub(crate) async fn serve_rendition(
    State(state): State<AppState>,
    Path(path): Path<RenditionPath>,
) -> Result<Response, ApiError> {
    stream(
        &state,
        path.project,
        path.environment,
        path.asset,
        Some(path.name),
    )
    .await
}

/// The origin's one read: the `public/` object the key names, streamed with its stored type and
/// an immutable cache (the bytes under an id never change).
async fn stream(
    state: &AppState,
    project: String,
    environment: String,
    asset: String,
    name: Option<String>,
) -> Result<Response, ApiError> {
    let services = services(state)?;
    let store = services.assets.clone().ok_or(ApiError::Unavailable(
        "uploads are not configured on this server",
    ))?;
    let project = Uuid::try_parse(&project).map_err(|_| ApiError::NotFound)?;
    let asset = Uuid::try_parse(&asset).map_err(|_| ApiError::NotFound)?;
    // A rendition name is lowercase letters and digits only — anything else can never name an
    // object the worker wrote.
    if name.as_ref().is_some_and(|name| {
        name.len() > 16
            || !name
                .chars()
                .all(|letter| letter.is_ascii_lowercase() || letter.is_ascii_digit())
    }) {
        return Err(ApiError::NotFound);
    }
    let environment = colophon_db::delivery::environment(&services.pool, project, &environment)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let mut key = colophon_media::public_key(
        &project.to_string(),
        &environment.id.to_string(),
        &asset.to_string(),
    );
    if let Some(name) = name {
        key = format!("{key}/{name}");
    }
    let object = store
        .get(&object_store::path::Path::from(key))
        .await
        .map_err(|error| match error {
            object_store::Error::NotFound { .. } => ApiError::NotFound,
            other => {
                tracing::warn!(error = %other, "asset origin read failed");
                ApiError::Unavailable("the object store is unreachable")
            }
        })?;
    let mut headers = HeaderMap::new();
    let content_type = object
        .attributes
        .get(&object_store::Attribute::ContentType)
        .map_or_else(
            || "application/octet-stream".to_owned(),
            |value| value.as_ref().to_owned(),
        );
    headers.insert(
        header::CONTENT_TYPE,
        content_type.parse().map_err(ApiError::internal)?,
    );
    headers.insert(
        header::CONTENT_LENGTH,
        object
            .meta
            .size
            .to_string()
            .parse()
            .map_err(ApiError::internal)?,
    );
    headers.insert(
        header::CACHE_CONTROL,
        "public, max-age=31536000, immutable"
            .parse()
            .map_err(ApiError::internal)?,
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        "nosniff".parse().map_err(ApiError::internal)?,
    );
    if let Some(etag) = object.meta.e_tag.as_deref() {
        headers.insert(header::ETAG, etag.parse().map_err(ApiError::internal)?);
    }
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .body(axum::body::Body::from_stream(object.into_stream()))
        .map_err(ApiError::internal)?;
    response.headers_mut().extend(headers);
    Ok(response)
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
        "derivatives": asset.derivatives,
        "created_at": asset
            .created_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
    })
}
