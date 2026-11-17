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
    Json,
    body::Bytes,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode, header},
    response::Response,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use object_store::ObjectStoreExt;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use colophon_db::{assets, idempotency, scope::ProjectTransaction};
use colophon_domain::policy::{Action, Policy};
use colophon_media::{Intent, IntentError, quarantine_key};

use crate::{
    ApiError,
    entries::{idempotency_key, json, open, request_hash},
    error::StillNeeded,
    session::{SignedIn, services},
    state::AppState,
};

/// Uploads are claimed under this operation.
const UPLOAD: &str = "asset.upload";
/// Byte replacements under this one (D-136).
const REPLACE: &str = "asset.replace";
/// Deletion requests under this one.
const DELETE: &str = "asset.delete";

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

/// A rendition's path: the asset's, plus one more segment — a rendition name the worker wrote
/// (`w320`), or a version's content sha pinning the original under it (D-136).
#[derive(Deserialize)]
pub(crate) struct RenditionPath {
    project: String,
    environment: String,
    asset: String,
    name: String,
}

/// A pinned version's rendition path: the asset's, the version's content sha, then the
/// rendition name (D-136).
#[derive(Deserialize)]
pub(crate) struct VersionedPath {
    project: String,
    environment: String,
    asset: String,
    sha: String,
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
    let intent = read_intent(&headers, body.len() as u64)?;
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

/// The upload contract's two headers — the file's name and its claimed type — checked into an
/// [`Intent`] against the body's size; uploads and replacements share it.
fn read_intent(headers: &HeaderMap, size: u64) -> Result<Intent, ApiError> {
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
    Intent::new(file_name, declared, size).map_err(|error| field_error(&error))
}

/// `POST {P}/assets/{asset}/replace`: new bytes for an existing asset (D-136) — the same
/// headers and body as an upload, but the verified bytes land at a new content-addressed key
/// under the asset's prefix and the current version swings; the old version's objects stay
/// where they are, so what a published release or a pinned URL serves never changes under it.
/// `202` means the reverify job is queued; the asset keeps serving its current version until
/// the verdict lands — a refused replacement changes nothing.
pub(crate) async fn replace(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
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
    let intent = read_intent(&headers, body.len() as u64)?;
    let asset = Uuid::try_parse(&path.asset).map_err(|_| ApiError::NotFound)?;
    // Same ordering as upload: permission and environment resolved and committed before the
    // store sees a byte.
    let seen = open(&state, caller, &path.project, &path.environment).await?;
    crate::members::require(
        &seen.policy,
        colophon_domain::policy::Action::ContentUpdate,
        None,
        "your role can't replace assets in this project",
    )?;
    let environment = seen.environment;
    seen.commit().await.map_err(ApiError::from_db)?;
    // The replacement's quarantine key names the asset it replaces: the object's prefix lists
    // what to remove if the asset goes first, and the job's row — not the asset's — holds the
    // key so the first upload's quarantine record stays put.
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
        REPLACE,
        &asset.to_string(),
        intent.file_name.as_str(),
        intent.declared_type.as_str(),
        &intent.size.to_string(),
    ]);
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    match idempotency::claim(&mut tx, REPLACE, key, &hash)
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
    queue_replace(tx, key, asset, &intent, &quarantine_key).await
}

/// `replace`'s committed half (D-136): the reverify job inside the idempotency claim, the
/// audit entry and the recorded response. The asset itself reports why a state can't take a
/// replacement.
async fn queue_replace(
    mut tx: ProjectTransaction,
    key: &str,
    asset: Uuid,
    intent: &Intent,
    quarantine_key: &str,
) -> Result<Response, ApiError> {
    let job = Uuid::now_v7();
    let deadline = time::OffsetDateTime::now_utc()
        + time::Duration::seconds(colophon_domain::job::LIFETIME_SECS);
    match assets::request_replace(
        &mut tx,
        asset,
        &assets::Replace {
            quarantine_key,
            declared_type: &intent.declared_type,
            size_bytes: i64::try_from(intent.size).unwrap_or(i64::MAX),
            job,
            idempotency_key: key,
            deadline_at: deadline,
        },
    )
    .await
    .map_err(ApiError::from_db)?
    {
        assets::ReplaceAsked::Unknown => Err(ApiError::NotFound),
        assets::ReplaceAsked::Quarantined => Err(ApiError::conflict(
            "asset_quarantined",
            "this asset is still being verified; wait for it to settle, then replace it",
        )),
        assets::ReplaceAsked::Rejected => Err(ApiError::conflict(
            "asset_rejected",
            "a rejected upload has no version to replace — upload a new asset instead",
        )),
        assets::ReplaceAsked::Gone => Err(ApiError::conflict(
            "already_deleted",
            "this asset is already deleted or on its way out",
        )),
        assets::ReplaceAsked::Queued { asset: found } => {
            let detail = json!({
                "file_name": found.file_name,
                "declared_type": intent.declared_type,
                "size_bytes": intent.size,
            })
            .to_string();
            colophon_db::audit::record_in(
                &mut tx.transaction,
                tx.project,
                tx.identity,
                "asset.replaced",
                &asset.to_string(),
                &detail,
            )
            .await
            .map_err(ApiError::from_db)?;
            let body = serde_json::to_string(&json!({"data": view(&found)}))
                .map_err(ApiError::internal)?;
            idempotency::complete(&mut tx, REPLACE, key, StatusCode::ACCEPTED.as_u16(), &body)
                .await
                .map_err(ApiError::from_db)?;
            tx.commit().await.map_err(ApiError::from_db)?;
            Ok(json(StatusCode::ACCEPTED, body, None, false))
        }
    }
}

/// A library list's parameters (D-172).
#[derive(Deserialize)]
pub(crate) struct ListQuery {
    limit: Option<i64>,
    cursor: Option<String>,
    q: Option<String>,
    state: Option<String>,
    tag: Option<String>,
}

/// The longest search text, in characters.
const MAX_QUERY_CHARS: usize = 100;
/// The states a library list can narrow to.
const STATES: [&str; 5] = ["quarantined", "usable", "rejected", "deleting", "deleted"];

/// `GET {P}/assets?limit=&cursor=&q=&state=&tag=`: a page of the environment's assets, newest
/// first, with the cursor for the next (D-172). Keyset paging on `(created_at, id)`: an upload
/// landing between two reads never shifts what the next page holds.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ListPath>,
    Query(query): Query<ListQuery>,
) -> Result<Response, ApiError> {
    let limit = query.limit.unwrap_or(assets::MAX_PAGE);
    if !(1..=assets::MAX_PAGE).contains(&limit) {
        return Err(ApiError::InvalidField {
            reason: "invalid_limit",
            pointer: "/limit",
            message: format!("limit is 1 to {}", assets::MAX_PAGE),
        });
    }
    let q = query.q.as_deref().map(str::trim).filter(|q| !q.is_empty());
    if q.is_some_and(|q| q.chars().count() > MAX_QUERY_CHARS || q.chars().any(char::is_control)) {
        return Err(ApiError::InvalidField {
            reason: "invalid_query",
            pointer: "/q",
            message: format!("a search is at most {MAX_QUERY_CHARS} characters of plain text"),
        });
    }
    let wanted_state = query.state.as_deref().filter(|state| !state.is_empty());
    if wanted_state.is_some_and(|state| !STATES.contains(&state)) {
        return Err(ApiError::InvalidField {
            reason: "invalid_state",
            pointer: "/state",
            message: format!("state is one of {}", STATES.join(", ")),
        });
    }
    let tag = query.tag.as_deref().filter(|tag| !tag.is_empty());
    if tag.is_some_and(|tag| !is_tag(tag)) {
        return Err(ApiError::InvalidField {
            reason: "invalid_tag",
            pointer: "/tag",
            message: TAG_RULE.to_owned(),
        });
    }
    let after = query.cursor.as_deref().map(read_cursor).transpose()?;
    let filter = assets::Filter {
        q,
        state: wanted_state,
        tag,
        after,
    };
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_read(&tx)?;
    let mut items = assets::page(&mut tx, &filter, limit + 1)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let more = items.len() > usize::try_from(limit).unwrap_or(usize::MAX);
    items.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    let next_cursor = more
        .then(|| {
            items
                .last()
                .map(|last| write_cursor(last.created_at, last.id))
        })
        .flatten();
    Ok(json(
        StatusCode::OK,
        serde_json::to_string(&json!({
            "data": items.iter().map(view).collect::<Vec<_>>(),
            "next_cursor": next_cursor,
        }))
        .map_err(ApiError::internal)?,
        None,
        false,
    ))
}

/// A library page's cursor: the last row's `(created_at, id)`, opaque to clients. It is a
/// position, not a permission — the list it continues is read under the caller's own scope.
fn write_cursor(created_at: time::OffsetDateTime, id: Uuid) -> String {
    URL_SAFE_NO_PAD.encode(format!("a1:{}:{id}", created_at.unix_timestamp_nanos()))
}

fn read_cursor(text: &str) -> Result<(time::OffsetDateTime, Uuid), ApiError> {
    let bad = || ApiError::InvalidField {
        reason: "invalid_cursor",
        pointer: "/cursor",
        message: "that cursor isn't one this list gave out; start from the first page".to_owned(),
    };
    if text.len() > 200 {
        return Err(bad());
    }
    let decoded = URL_SAFE_NO_PAD.decode(text).map_err(|_| bad())?;
    let decoded = std::str::from_utf8(&decoded).map_err(|_| bad())?;
    let mut parts = decoded.splitn(3, ':');
    let (Some("a1"), Some(nanos), Some(id)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(bad());
    };
    let nanos: i128 = nanos.parse().map_err(|_| bad())?;
    let at = time::OffsetDateTime::from_unix_timestamp_nanos(nanos).map_err(|_| bad())?;
    Ok((at, Uuid::try_parse(id).map_err(|_| bad())?))
}

/// The tag rule (D-172), worded for a refusal.
const TAG_RULE: &str =
    "a tag is 1 to 40 lowercase letters, digits and hyphens, starting with a letter or digit";
/// The most tags one asset carries.
const MAX_TAGS: usize = 10;

/// `^[a-z0-9][a-z0-9-]{0,39}$`
fn is_tag(tag: &str) -> bool {
    let bytes = tag.as_bytes();
    bytes.len() <= 40
        && bytes
            .first()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// Metadata changes are claimed under this operation (D-172).
const UPDATE: &str = "asset.update";

/// `PATCH {P}/assets/{asset}`: the library's own words for an upload — alt text, title, credit,
/// tags (D-172). `If-Match` names the `metadata_version` the editor saw (`"v3"`, or `"3"`), so
/// two people editing one upload can't silently overwrite each other: the second gets
/// `409 stale_version` with the current `ETag`. A field left out stays; `null` clears it.
pub(crate) async fn update(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Response, ApiError> {
    let asset = Uuid::try_parse(&path.asset).map_err(|_| ApiError::NotFound)?;
    let expected = metadata_if_match(&headers)?;
    let key = idempotency_key(&headers)?;
    let Json(value) = body?;
    let change = read_metadata(&value)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    crate::members::require(
        &tx.policy,
        colophon_domain::policy::Action::ContentUpdate,
        None,
        "your role can't change this project's assets",
    )?;
    let hash = request_hash(&[
        UPDATE,
        &asset.to_string(),
        &expected.to_string(),
        &value.to_string(),
    ]);
    match idempotency::claim(&mut tx, UPDATE, key, &hash)
        .await
        .map_err(ApiError::from_db)?
    {
        idempotency::Claim::Fresh => {}
        idempotency::Claim::Mismatch => return Err(ApiError::IdempotencyKeyReused),
        idempotency::Claim::Replay { status, response } => {
            let version = serde_json::from_str::<Value>(&response)
                .ok()
                .and_then(|body| i32::try_from(body["data"]["metadata_version"].as_i64()?).ok())
                .unwrap_or(1);
            return Ok(crate::content_types::respond(
                StatusCode::from_u16(status).map_err(ApiError::internal)?,
                response,
                version,
                true,
            ));
        }
    }
    let found = match assets::update_metadata(&mut tx, asset, expected, &change)
        .await
        .map_err(ApiError::from_db)?
    {
        assets::Updated::Unknown => return Err(ApiError::NotFound),
        assets::Updated::Stale(current) => return Err(ApiError::StaleVersion { current }),
        assets::Updated::Done(found) => found,
    };
    // The audit names which fields changed, never their text.
    let fields: Vec<&str> = [
        change.alt.is_some().then_some("alt"),
        change.title.is_some().then_some("title"),
        change.credit.is_some().then_some("credit"),
        change.tags.is_some().then_some("tags"),
    ]
    .into_iter()
    .flatten()
    .collect();
    colophon_db::audit::record_in(
        &mut tx.transaction,
        tx.project,
        tx.identity,
        "asset.updated",
        &asset.to_string(),
        &json!({"fields": fields, "metadata_version": found.metadata_version}).to_string(),
    )
    .await
    .map_err(ApiError::from_db)?;
    let body = serde_json::to_string(&json!({"data": view(&found)})).map_err(ApiError::internal)?;
    idempotency::complete(&mut tx, UPDATE, key, StatusCode::OK.as_u16(), &body)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(crate::content_types::respond(
        StatusCode::OK,
        body,
        found.metadata_version,
        false,
    ))
}

/// The `metadata_version` an `If-Match` names: `"v3"` (the `ETag` this API answers with) or
/// `"3"`.
fn metadata_if_match(headers: &HeaderMap) -> Result<i32, ApiError> {
    let value = headers
        .get(header::IF_MATCH)
        .ok_or(ApiError::PreconditionRequired(
            "changing an asset's details needs an If-Match header with the ETag you read",
        ))?;
    value
        .to_str()
        .ok()
        .and_then(|text| {
            let inner = text.trim().strip_prefix('"')?.strip_suffix('"')?;
            inner.strip_prefix('v').unwrap_or(inner).parse().ok()
        })
        .filter(|version: &i32| *version >= 1)
        .ok_or(ApiError::InvalidHeader(
            "If-Match must be one asset ETag, such as \"v3\"",
        ))
}

/// A PATCH body checked into a [`assets::Metadata`]: an object of `alt` (≤ 300 characters),
/// `title` and `credit` (≤ 200 each) — a string or `null` — and `tags`, at most ten distinct
/// tags. Text is trimmed; blank text clears the field like `null`.
fn read_metadata(value: &Value) -> Result<assets::Metadata, ApiError> {
    let refuse = |code: &'static str, pointer: String, message: String| {
        ApiError::Invalid(colophon_domain::schema::Refusal {
            code,
            pointer,
            message,
        })
    };
    let Value::Object(object) = value else {
        return Err(refuse(
            "wrong_type",
            String::new(),
            "the body is an object of alt, title, credit and tags".to_owned(),
        ));
    };
    if let Some(unknown) = object
        .keys()
        .find(|key| !matches!(key.as_str(), "alt" | "title" | "credit" | "tags"))
    {
        return Err(refuse(
            "unknown_field",
            format!("/{unknown}"),
            "an asset's details are alt, title, credit and tags".to_owned(),
        ));
    }
    let text = |name: &str, max: usize| -> Result<Option<Option<String>>, ApiError> {
        match object.get(name) {
            None => Ok(None),
            Some(Value::Null) => Ok(Some(None)),
            Some(Value::String(text)) => {
                let trimmed = text.trim();
                if trimmed.chars().count() > max || trimmed.chars().any(char::is_control) {
                    return Err(refuse(
                        "invalid_value",
                        format!("/{name}"),
                        format!("{name} is at most {max} characters, on one line"),
                    ));
                }
                Ok(Some((!trimmed.is_empty()).then(|| trimmed.to_owned())))
            }
            Some(_) => Err(refuse(
                "wrong_type",
                format!("/{name}"),
                format!("{name} is text or null"),
            )),
        }
    };
    let tags = match object.get("tags") {
        None => None,
        Some(Value::Array(items)) => {
            if items.len() > MAX_TAGS {
                return Err(refuse(
                    "too_many_tags",
                    "/tags".to_owned(),
                    format!("an asset has at most {MAX_TAGS} tags"),
                ));
            }
            let mut tags: Vec<String> = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                let tag = item.as_str().filter(|tag| is_tag(tag)).ok_or_else(|| {
                    refuse("invalid_tag", format!("/tags/{index}"), TAG_RULE.to_owned())
                })?;
                if tags.iter().any(|seen| seen == tag) {
                    return Err(refuse(
                        "duplicate_tag",
                        format!("/tags/{index}"),
                        "each tag appears once".to_owned(),
                    ));
                }
                tags.push(tag.to_owned());
            }
            Some(tags)
        }
        Some(_) => {
            return Err(refuse(
                "wrong_type",
                "/tags".to_owned(),
                "tags is a list of tags".to_owned(),
            ));
        }
    };
    Ok(assets::Metadata {
        alt: text("alt", colophon_domain::rich_text::MAX_ALT_CHARS)?,
        title: text("title", 200)?,
        credit: text("credit", 200)?,
        tags,
    })
}

/// `GET /media/v1/{project}/{environment}/{asset}`: the asset origin — the current version's
/// verified bytes, publicly (media M5, D-136). `media_object` answers which key the row
/// serves: a usable asset's `public_key`, nothing for anything else — quarantined, rejected,
/// deleting and deleted assets are 404s even while their objects still sit in the store, and
/// so is everything that never was. The stored `Content-Type` (written at promote) is served
/// back. The address follows the current version, which a replace moves (D-136) and a delete
/// ends, so it revalidates after a minute rather than caching forever (D-157).
pub(crate) async fn serve(
    State(state): State<AppState>,
    Path(path): Path<OnePath>,
    request: HeaderMap,
) -> Result<Response, ApiError> {
    stream(
        &state,
        &request,
        path.project,
        path.environment,
        path.asset,
        None,
        None,
    )
    .await
}

/// `GET /media/v1/{project}/{environment}/{asset}/{name}`: a rendition under the name the
/// worker wrote (`w320`, `w1280`) — or, when the segment is a version's content sha, that
/// pinned version's original (D-136). What a published release references resolves to the
/// bytes it named, so rolling back serves the media the old release saw.
pub(crate) async fn serve_rendition(
    State(state): State<AppState>,
    Path(path): Path<RenditionPath>,
    request: HeaderMap,
) -> Result<Response, ApiError> {
    let (sha, name) = if colophon_media::is_version_sha(&path.name) {
        (Some(path.name), None)
    } else {
        (None, Some(path.name))
    };
    stream(
        &state,
        &request,
        path.project,
        path.environment,
        path.asset,
        sha,
        name,
    )
    .await
}

/// `GET /media/v1/{project}/{environment}/{asset}/{sha}/{name}`: a rendition of a pinned
/// version — the worker wrote it under the version's key, and this serves it by the same
/// address (D-136). `media_object` is the only check the sha needs: a value that isn't a
/// version's digest matches no row and is a 404 exactly like an unknown asset.
pub(crate) async fn serve_versioned(
    State(state): State<AppState>,
    Path(path): Path<VersionedPath>,
    request: HeaderMap,
) -> Result<Response, ApiError> {
    stream(
        &state,
        &request,
        path.project,
        path.environment,
        path.asset,
        Some(path.sha),
        Some(path.name),
    )
    .await
}

/// How long a cache may reuse what an unversioned media address answered before asking again
/// (D-157): the address follows the asset's current version, which a replace moves and a
/// delete ends, so it is never `immutable` — a minute, then a conditional request that the
/// version's `ETag` usually answers `304`.
const UNVERSIONED_CACHE: &str = "public, max-age=60, must-revalidate";

/// What a versioned address — one that names a version's content sha — answers: those bytes
/// never change, so caches keep them for a year.
const VERSIONED_CACHE: &str = "public, max-age=31536000, immutable";

/// Whether `If-None-Match` names `etag`: a list, `*`, or weak tags all count (RFC 9110's weak
/// comparison, which is what a `GET`'s revalidation uses).
fn none_match(request: &HeaderMap, etag: &str) -> bool {
    let bare = |tag: &str| tag.trim().trim_start_matches("W/").to_owned();
    let want = bare(etag);
    request
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|tag| tag.trim() == "*" || bare(tag) == want)
}

/// The origin's one read: `media_object` resolves the environment slug and picks the object
/// key — the usable asset's current version, or the version `sha` pins — then the object
/// streams back with its stored type. A pinned version caches forever; the current-version
/// address revalidates (D-157), and a matching `If-None-Match` answers `304`.
async fn stream(
    state: &AppState,
    request: &HeaderMap,
    project: String,
    environment: String,
    asset: String,
    sha: Option<String>,
    name: Option<String>,
) -> Result<Response, ApiError> {
    let pinned = sha.is_some();
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
    let resolved = colophon_db::delivery::media_object(
        &services.pool,
        project,
        &environment,
        asset,
        sha.as_deref(),
    )
    .await
    .map_err(ApiError::from_db)?
    .ok_or(ApiError::NotFound)?;
    let mut key = resolved.public_key.ok_or(ApiError::NotFound)?;
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
    let cache = if pinned {
        VERSIONED_CACHE
    } else {
        UNVERSIONED_CACHE
    };
    headers.insert(
        header::CACHE_CONTROL,
        cache.parse().map_err(ApiError::internal)?,
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        "nosniff".parse().map_err(ApiError::internal)?,
    );
    if let Some(etag) = object.meta.e_tag.as_deref() {
        headers.insert(header::ETAG, etag.parse().map_err(ApiError::internal)?);
        // The cache already holds these bytes: say so and send none (the stream is dropped
        // unread).
        if none_match(request, etag) {
            headers.remove(header::CONTENT_LENGTH);
            headers.remove(header::CONTENT_TYPE);
            let mut response = Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .body(axum::body::Body::empty())
                .map_err(ApiError::internal)?;
            response.headers_mut().extend(headers);
            return Ok(response);
        }
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
    let versions = assets::versions(&mut tx, asset.id)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    // The ETag names the metadata version a PATCH's If-Match quotes back (D-172).
    Ok(crate::content_types::respond(
        StatusCode::OK,
        serde_json::to_string(&json!({
            "data": view(&asset),
            "versions": versions.iter().map(version_view).collect::<Vec<_>>(),
        }))
        .map_err(ApiError::internal)?,
        asset.metadata_version,
        false,
    ))
}

/// `GET {P}/assets/{asset}/references`: the entries still naming it — through a live draft, or
/// through any publication of them, which pins it forever (media M3, D-098). What would have
/// to let go before `DELETE` accepts it.
pub(crate) async fn references(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
) -> Result<Response, ApiError> {
    let asset = Uuid::try_parse(&path.asset).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_read(&tx)?;
    assets::get(&mut tx, asset)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let referrers = assets::references(&mut tx, asset)
        .await
        .map_err(ApiError::from_db)?
        .into_iter()
        // Only referrers in collections the role reads, as a refusal's `referenced_by` (D-065).
        .filter(|referrer| {
            tx.policy
                .fields(Action::ContentRead, &referrer.collection, false)
                .is_some()
        })
        .map(|referrer| {
            json!({
                "entry_id": referrer.entry_id,
                "collection": referrer.collection,
                "field": referrer.field,
                "in_draft": referrer.in_draft,
                "published": referrer.published,
            })
        })
        .collect::<Vec<_>>();
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(json(
        StatusCode::OK,
        serde_json::to_string(&json!({"data": referrers})).map_err(ApiError::internal)?,
        None,
        false,
    ))
}

/// `DELETE {P}/assets/{asset}`: asks for the asset to go, `Idempotency-Key` required like
/// every write. `202` means requested, not gone (truthful states): the transaction commits
/// only the `delete` job and the audit event, and the worker removes the objects and lands
/// `deleted` — a live draft or any publication that still names it refuses the request
/// (`still_referenced`, the entry deletion's shape), and the worker rechecks that same rule
/// under the row lock before a byte is removed (media M3, D-098).
pub(crate) async fn delete(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let key = idempotency_key(&headers)?;
    let asset = Uuid::try_parse(&path.asset).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    // Assets belong to the project, not one collection: the whole-project form of the grant,
    // mirroring upload's ContentCreate.
    crate::members::require(
        &tx.policy,
        Action::ContentDelete,
        None,
        "your role can't delete assets in this project",
    )?;
    // Claimed before the checks: a retry after a lost answer replays the recorded 202 rather
    // than meeting the now-`deleted` asset's `already_deleted`.
    let request = request_hash(&[DELETE, &asset.to_string()]);
    match idempotency::claim(&mut tx, DELETE, key, &request)
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
    match assets::request_delete(&mut tx, asset, job, key, deadline)
        .await
        .map_err(ApiError::from_db)?
    {
        assets::DeleteAsked::Unknown => Err(ApiError::NotFound),
        assets::DeleteAsked::Quarantined => Err(ApiError::conflict(
            "asset_quarantined",
            "this asset is still being verified; wait for it to settle, then delete it",
        )),
        assets::DeleteAsked::Gone => Err(ApiError::conflict(
            "already_deleted",
            "this asset is already deleted or on its way out",
        )),
        assets::DeleteAsked::Referenced(referrers) => Err(still_referenced(&tx.policy, referrers)),
        assets::DeleteAsked::Queued { asset: found } => {
            let detail = json!({
                "file_name": found.file_name,
                "declared_type": found.declared_type,
                "size_bytes": found.size_bytes,
            })
            .to_string();
            colophon_db::audit::record_in(
                &mut tx.transaction,
                tx.project,
                tx.identity,
                "asset.deleted",
                &asset.to_string(),
                &detail,
            )
            .await
            .map_err(ApiError::from_db)?;
            let body = serde_json::to_string(&json!({"data": view(&found)}))
                .map_err(ApiError::internal)?;
            idempotency::complete(&mut tx, DELETE, key, StatusCode::ACCEPTED.as_u16(), &body)
                .await
                .map_err(ApiError::from_db)?;
            tx.commit().await.map_err(ApiError::from_db)?;
            Ok(json(StatusCode::ACCEPTED, body, None, false))
        }
    }
}

/// A refused deletion, as the entry deletion's answer shapes it (D-080): who still names the
/// asset, in collections the caller reads.
fn still_referenced(policy: &Policy, referrers: Vec<assets::AssetReferrer>) -> ApiError {
    let referenced_by = referrers
        .into_iter()
        .filter(|referrer| {
            policy
                .fields(Action::ContentRead, &referrer.collection, false)
                .is_some()
        })
        .map(|referrer| crate::error::Referrer {
            entry_id: referrer.entry_id,
            collection: referrer.collection,
            field: referrer.field,
            in_draft: referrer.in_draft,
            published: referrer.published,
        })
        .collect();
    ApiError::StillNeeded(Box::new(StillNeeded {
        reason: "still_referenced",
        message: "entries still name this asset: remove it from them first, then delete it"
            .to_owned(),
        referenced_by,
        changesets: Vec::new(),
    }))
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

/// The public shape of an asset row. `sha256` is the current version's digest — the hex the
/// `…/{asset}/{sha}` pinned URL takes (D-136).
fn view(asset: &assets::Asset) -> serde_json::Value {
    json!({
        "id": asset.id,
        "state": asset.state,
        "file_name": asset.file_name,
        "declared_type": asset.declared_type,
        "detected_type": asset.detected_type,
        "sha256": asset.sha256.as_deref().map(colophon_media::sha_hex),
        "size_bytes": asset.size_bytes,
        "width": asset.width,
        "height": asset.height,
        "rejected_reason": asset.rejected_reason,
        "derivatives": asset.derivatives,
        "created_at": asset
            .created_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
        "alt": asset.alt,
        "title": asset.title,
        "credit": asset.credit,
        "tags": asset.tags,
        "metadata_version": asset.metadata_version,
    })
}

/// The public shape of one version row — the asset's byte history (D-136).
fn version_view(version: &assets::Version) -> serde_json::Value {
    json!({
        "seq": version.seq,
        "declared_type": version.declared_type,
        "size_bytes": version.size_bytes,
        "detected_type": version.detected_type,
        "sha256": colophon_media::sha_hex(&version.sha256),
        "width": version.width,
        "height": version.height,
        "derivatives": version.derivatives,
        "created_at": version
            .created_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, header};
    use serde_json::json;

    use super::{is_tag, metadata_if_match, read_cursor, read_metadata, write_cursor};
    use crate::ApiError;

    fn refused(value: &serde_json::Value) -> (&'static str, String) {
        match read_metadata(value) {
            Err(ApiError::Invalid(refusal)) => (refusal.code, refusal.pointer),
            other => panic!("not a refusal: {other:?}"),
        }
    }

    #[test]
    fn metadata_leaves_absent_fields_and_clears_null_or_blank_ones() {
        let change = read_metadata(&json!({"alt": "  A press  ", "title": null, "credit": " "}))
            .expect("valid");
        assert_eq!(change.alt, Some(Some("A press".to_owned())));
        assert_eq!(change.title, Some(None));
        assert_eq!(change.credit, Some(None), "blank clears");
        assert_eq!(change.tags, None, "absent stays");
        let tags = read_metadata(&json!({"tags": ["press", "1920s"]})).expect("valid");
        assert_eq!(
            tags.tags,
            Some(vec!["press".to_owned(), "1920s".to_owned()])
        );
    }

    #[test]
    fn metadata_refusals_point_at_the_value() {
        assert_eq!(refused(&json!([])), ("wrong_type", String::new()));
        assert_eq!(
            refused(&json!({"url": "x"})),
            ("unknown_field", "/url".into())
        );
        assert_eq!(
            refused(&json!({"alt": "x".repeat(301)})),
            ("invalid_value", "/alt".into())
        );
        assert_eq!(
            refused(&json!({"title": "a\nb"})),
            ("invalid_value", "/title".into())
        );
        assert_eq!(
            refused(&json!({"credit": 3})),
            ("wrong_type", "/credit".into())
        );
        assert_eq!(
            refused(&json!({"tags": ["ok", "Not OK"]})),
            ("invalid_tag", "/tags/1".into())
        );
        assert_eq!(
            refused(&json!({"tags": ["a", "b", "a"]})),
            ("duplicate_tag", "/tags/2".into())
        );
        let eleven: Vec<String> = (0..11).map(|n| format!("t{n}")).collect();
        assert_eq!(
            refused(&json!({"tags": eleven})),
            ("too_many_tags", "/tags".into())
        );
        assert_eq!(
            refused(&json!({"tags": "a"})),
            ("wrong_type", "/tags".into())
        );
    }

    #[test]
    fn tags_are_slug_shaped() {
        assert!(is_tag("a") && is_tag("press-2") && is_tag(&"a".repeat(40)));
        for bad in ["", "-a", "A", "a b", "a_b", "é", &"a".repeat(41)] {
            assert!(!is_tag(bad), "{bad:?}");
        }
    }

    #[test]
    fn if_match_takes_a_version_etag_with_or_without_its_v() {
        let with = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::IF_MATCH,
                HeaderValue::from_str(value).expect("header"),
            );
            metadata_if_match(&headers).ok()
        };
        assert_eq!(with("\"v3\""), Some(3));
        assert_eq!(with("\"3\""), Some(3));
        assert_eq!(with("3"), None, "unquoted");
        assert_eq!(with("\"v0\""), None);
        assert_eq!(with("\"vx\""), None);
        assert!(metadata_if_match(&HeaderMap::new()).is_err());
    }

    #[test]
    fn cursors_round_trip_and_refuse_anything_else() {
        let at = time::OffsetDateTime::from_unix_timestamp_nanos(1_763_400_000_123_456_789)
            .expect("a time");
        let id = uuid::Uuid::now_v7();
        assert_eq!(read_cursor(&write_cursor(at, id)).ok(), Some((at, id)));
        for bad in ["", "x", "YTE6MTox", &"a".repeat(201)] {
            assert!(read_cursor(bad).is_err(), "{bad:?}");
        }
    }
}
