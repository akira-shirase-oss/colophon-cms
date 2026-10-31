//! Entries and their drafts (docs/modules/04-api.md, "Route families"; D-034, D-035). Under
//! `{P}` = `/api/v1/projects/{project}/environments/{environment}`:
//!
//! - `GET  {P}/collections/{collection}/entries`: newest first, with a cursor; each row also
//!   carries the draft's values of up to three short fields (a number, a choice, yes or no, a
//!   date and time, in the definition's order) under `fields`, for list columns;
//! - `POST {P}/collections/{collection}/entries`: create (`Idempotency-Key` required);
//! - `GET  {P}/collections/{collection}/entries/{entry}`: the draft, with its `ETag`;
//! - `PUT  {P}/collections/{collection}/entries/{entry}`: save (`If-Match` and
//!   `Idempotency-Key` required);
//! - `GET  {P}/collections/{collection}/entries/{entry}/revisions`: history, newest first.
//!
//! A project the caller isn't in, an environment, collection or entry that doesn't exist: all
//! answer 404 alike. A draft's `ETag` names its revision; a save whose `If-Match` names an older
//! one is refused with 409 and the current `ETag`, and writes nothing.

use aws_lc_rs::digest;
use axum::{
    Json,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use colophon_db::{
    content_types,
    entries::{self, Draft, NewRevision, Removed, Revision, Saved, Summary},
    idempotency::{self, Claim},
    scope::{self, ProjectTransaction},
};
use colophon_domain::schema::{Content, ContentType, FieldKind, Refusal};
use serde::{Deserialize, Serialize};
use serde_json::{Value, value::RawValue};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::{
    ApiError, AppState, fault,
    session::{SignedIn, services},
    shape::{Data, Page},
};

/// The header that carries an idempotency key.
pub(crate) const IDEMPOTENCY_KEY: &str = "idempotency-key";
/// Present (`true`) on a response that replays the recorded answer to an earlier request.
pub(crate) const IDEMPOTENT_REPLAYED: &str = "idempotent-replayed";
/// Entries per page unless `limit` says otherwise.
const DEFAULT_PAGE: i64 = 20;

const CREATE: &str = "entry.create";
const SAVE: &str = "entry.save";
const IMPORT: &str = "entry.import";
/// An import is at most this many lines, so one upload stays a bounded transaction.
const IMPORT_MAX_LINES: usize = 512;

/// The path of the collection routes.
#[derive(Deserialize)]
pub(crate) struct CollectionPath {
    project: String,
    environment: String,
    collection: String,
}

/// The path of the single-entry routes.
#[derive(Deserialize)]
pub(crate) struct EntryPath {
    project: String,
    environment: String,
    collection: String,
    entry: String,
}

#[derive(Serialize)]
struct RevisionView {
    id: Uuid,
    base_revision_id: Option<Uuid>,
    author_id: Uuid,
    created_at: String,
    content_hash: String,
    title: String,
}

impl RevisionView {
    fn new(revision: Revision) -> Result<Self, ApiError> {
        Ok(Self {
            id: revision.id,
            base_revision_id: revision.base_revision_id,
            author_id: revision.author_id,
            created_at: rfc3339(revision.created_at)?,
            content_hash: content_hash(&revision.content_hash),
            title: revision.title,
        })
    }
}

#[derive(Serialize)]
struct DraftView {
    id: Uuid,
    collection: String,
    created_at: String,
    updated_at: String,
    revision: RevisionView,
    /// The stored canonical JSON, embedded as is: re-serializing could reorder its keys.
    content: Box<RawValue>,
    /// The entry's locale tag, or `null` for the default locale.
    locale: Option<String>,
}

impl DraftView {
    fn new(draft: Draft) -> Result<Self, ApiError> {
        Ok(Self {
            id: draft.entry_id,
            collection: draft.collection,
            created_at: rfc3339(draft.created_at)?,
            updated_at: rfc3339(draft.updated_at)?,
            revision: RevisionView::new(draft.revision)?,
            content: RawValue::from_string(draft.content).map_err(ApiError::internal)?,
            locale: draft.locale,
        })
    }
}

#[derive(Serialize)]
struct SummaryView {
    id: Uuid,
    updated_at: String,
    revision: RevisionView,
    /// What readers get now, if the entry is published: so a list can say draft, live or
    /// changed since it went live.
    published: Option<LiveView>,
    /// The draft's values of the collection's column fields, embedded as stored: a field the
    /// content doesn't have is `null`.
    fields: Box<RawValue>,
    /// The entry's locale tag, or `null` for the default locale.
    locale: Option<String>,
}

#[derive(Serialize)]
struct LiveView {
    revision_id: Uuid,
    /// The release it went out in.
    generation: i64,
}

/// `?limit=&cursor=&q=` for lists; `q` searches titles.
#[derive(Deserialize)]
pub(crate) struct ListQuery {
    limit: Option<i64>,
    cursor: Option<String>,
    q: Option<String>,
    locale: Option<String>,
}

/// The `?locale=` a `create` or `save` may carry.
#[derive(Deserialize)]
pub(crate) struct LocaleQuery {
    locale: Option<String>,
}

/// A locale tag is a short BCP-47-ish label — `en`, `en-US`, `pt-BR` — checked by shape,
/// not by a table of known tags.
fn check_locale(locale: &Option<String>) -> Result<(), ApiError> {
    let Some(locale) = locale else { return Ok(()) };
    let fine = locale.len() <= 35
        && !locale.is_empty()
        && locale
            .split('-')
            .all(|part| part.len() <= 8 && part.chars().all(|c| c.is_ascii_alphanumeric()))
        && locale
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic());
    if !fine {
        return Err(ApiError::Invalid(Refusal {
            code: "invalid_locale",
            pointer: "/locale".to_owned(),
            message: "a locale looks like `en` or `en-US`".to_owned(),
        }));
    }
    Ok(())
}

#[derive(Deserialize)]
pub(crate) struct RevisionPath {
    project: String,
    environment: String,
    collection: String,
    entry: String,
    revision: String,
}

#[derive(Serialize)]
struct RevisionContentView {
    revision: RevisionView,
    content: Box<serde_json::value::RawValue>,
}

pub(crate) fn rfc3339(at: OffsetDateTime) -> Result<String, ApiError> {
    at.format(&Rfc3339).map_err(ApiError::internal)
}

/// `sha256:<64 hex digits>` (D-029).
pub(crate) fn content_hash(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(7 + bytes.len() * 2);
    text.push_str("sha256:");
    for byte in bytes {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0xf)]));
    }
    text
}

pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = digest::digest(&digest::SHA256, bytes);
    let mut out = [0; 32];
    out.copy_from_slice(digest.as_ref());
    out
}

/// The hash an idempotency record compares: every part, length-prefixed so no two different
/// requests can produce the same input.
pub(crate) fn request_hash(parts: &[&str]) -> [u8; 32] {
    let mut context = digest::Context::new(&digest::SHA256);
    for part in parts {
        context.update(part.len().to_string().as_bytes());
        context.update(b":");
        context.update(part.as_bytes());
    }
    let mut out = [0; 32];
    out.copy_from_slice(context.finish().as_ref());
    out
}

pub(crate) fn idempotency_key(headers: &HeaderMap) -> Result<&str, ApiError> {
    let value = headers
        .get(IDEMPOTENCY_KEY)
        .ok_or(ApiError::PreconditionRequired(
            "this request needs an Idempotency-Key header",
        ))?;
    value
        .to_str()
        .ok()
        .filter(|key| {
            (16..=128).contains(&key.len())
                && key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        })
        .ok_or(ApiError::InvalidHeader(
            "an Idempotency-Key is 16 to 128 letters, digits, '-' or '_'",
        ))
}

/// The revision an `If-Match` names: exactly one strong `ETag`, `"<revision id>"`.
fn if_match(headers: &HeaderMap) -> Result<Uuid, ApiError> {
    let value = headers
        .get(header::IF_MATCH)
        .ok_or(ApiError::PreconditionRequired(
            "saving needs an If-Match header with the ETag of the revision you edited",
        ))?;
    value
        .to_str()
        .ok()
        .and_then(|text| text.trim().strip_prefix('"')?.strip_suffix('"'))
        .and_then(|inner| Uuid::try_parse(inner).ok())
        .ok_or(ApiError::InvalidHeader(
            "If-Match must be one revision ETag, exactly as the draft's ETag header gave it",
        ))
}

fn etag(revision: Uuid) -> HeaderValue {
    // A UUID in quotes is always a valid header value.
    HeaderValue::from_str(&format!("\"{revision}\"")).unwrap_or(HeaderValue::from_static("\"\""))
}

pub(crate) fn json(
    status: StatusCode,
    body: String,
    revision: Option<Uuid>,
    replayed: bool,
) -> Response {
    let mut response = (status, body).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    if let Some(revision) = revision {
        headers.insert(header::ETAG, etag(revision));
    }
    if replayed {
        headers.insert(IDEMPOTENT_REPLAYED, HeaderValue::from_static("true"));
    }
    response
}

/// Opens a transaction in the caller's project and environment, or 404.
pub(crate) async fn open(
    state: &AppState,
    caller: SignedIn,
    project: &str,
    environment: &str,
) -> Result<ProjectTransaction, ApiError> {
    let services = services(state)?;
    let project = Uuid::try_parse(project).map_err(|_| ApiError::NotFound)?;
    scope::begin_in_project(&services.pool, caller.identity, project, environment)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)
}

/// The collection's current content type and its version, or 404.
pub(crate) async fn content_type(
    tx: &mut ProjectTransaction,
    collection: &str,
) -> Result<(ContentType, i32), ApiError> {
    let stored = content_types::current(tx, collection)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    // Stored definitions were validated when written; failing to parse one is a bug.
    let value: Value = serde_json::from_str(&stored.definition).map_err(ApiError::internal)?;
    let parsed = ContentType::parse(&value).map_err(ApiError::internal)?;
    Ok((parsed, stored.version))
}

/// Claims the key, or answers with the recorded response.
async fn claim(
    tx: &mut ProjectTransaction,
    operation: &str,
    key: &str,
    hash: &[u8; 32],
) -> Result<Option<Response>, ApiError> {
    match idempotency::claim(tx, operation, key, hash)
        .await
        .map_err(ApiError::from_db)?
    {
        Claim::Fresh => Ok(None),
        Claim::Mismatch => Err(ApiError::IdempotencyKeyReused),
        Claim::Replay { status, response } => {
            let status = StatusCode::from_u16(status).map_err(ApiError::internal)?;
            let revision = serde_json::from_str::<Value>(&response)
                .ok()
                .and_then(|body| Uuid::try_parse(body["data"]["revision"]["id"].as_str()?).ok());
            Ok(Some(json(status, response, revision, true)))
        }
    }
}

/// Validates content against its content type: the canonical content and its hash.
fn validate(value: &Value, content_type: &ContentType) -> Result<(Content, [u8; 32]), ApiError> {
    let content = content_type.validate(value).map_err(ApiError::Invalid)?;
    let hash = sha256(content.canonical_json().as_bytes());
    Ok((content, hash))
}

/// Every `reference` value must name an entry that exists in this environment: a dangling id
/// is refused rather than stored. (An entry deleted later still dangles — that's documented
/// in `FieldKind::Reference`, and delivery carries the id either way.)
async fn check_dangling(
    tx: &mut ProjectTransaction,
    content_type: &ContentType,
    content: &Content,
) -> Result<(), ApiError> {
    for field in content_type.fields() {
        if let FieldKind::Reference { to, many } = &field.kind
            && let Some(raw) = content.value(&field.id)
        {
            let ids: Vec<String> = if *many {
                serde_json::from_str(raw).unwrap_or_default()
            } else {
                serde_json::from_str::<String>(raw).map_or(vec![], |id| vec![id])
            };
            for id in ids {
                if let Ok(uuid) = Uuid::try_parse(&id)
                    && entries::get(tx, to, uuid)
                        .await
                        .map_err(ApiError::from_db)?
                        .is_none()
                {
                    return Err(ApiError::Invalid(Refusal {
                        code: "dangling_reference",
                        pointer: format!("/{}", field.id),
                        message: format!("`{}` names an entry that isn't in `{to}`", field.label),
                    }));
                }
            }
        }
    }
    Ok(())
}

/// Each `unique` field's id with the hash of its canonical value — what `unique_values` claims.
fn unique_pairs(content_type: &ContentType, content: &Content) -> Vec<(String, [u8; 32])> {
    content_type
        .fields()
        .iter()
        .filter(|field| field.unique)
        .filter_map(|field| {
            content
                .value(&field.id)
                .map(|spelling| (field.id.clone(), sha256(spelling.as_bytes())))
        })
        .collect()
}

/// Records the answer, commits, and only then responds: an acknowledged write is durable.
async fn finish(
    mut tx: ProjectTransaction,
    operation: &str,
    key: &str,
    status: StatusCode,
    draft: Draft,
) -> Result<Response, ApiError> {
    let revision = draft.revision.id;
    let body = serde_json::to_string(&Data {
        data: DraftView::new(draft)?,
    })
    .map_err(ApiError::internal)?;
    idempotency::complete(&mut tx, operation, key, status.as_u16(), &body)
        .await
        .map_err(ApiError::from_db)?;
    fault::point(&format!("{operation}:before-commit"));
    tx.commit().await.map_err(ApiError::from_db)?;
    fault::point(&format!("{operation}:after-commit"));
    Ok(json(status, body, Some(revision), false))
}

/// `POST {P}/collections/{collection}/entries`.
pub(crate) async fn create(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<CollectionPath>,
    Query(query): Query<LocaleQuery>,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Response, ApiError> {
    check_locale(&query.locale)?;
    let key = idempotency_key(&headers)?;
    let Json(value) = body?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    crate::members::require(
        &tx.role,
        colophon_domain::policy::Action::WriteContent,
        "your role can't write content in this project",
    )?;
    let (content_type, version) = content_type(&mut tx, &path.collection).await?;
    let (content, hash) = validate(&value, &content_type)?;
    let request = request_hash(&[CREATE, &path.collection, content.canonical_json()]);
    if let Some(replay) = claim(&mut tx, CREATE, key, &request).await? {
        return Ok(replay);
    }
    if content_type.is_single()
        && !entries::list(&mut tx, &path.collection, None, 1, None, &[], None)
            .await
            .map_err(ApiError::from_db)?
            .is_empty()
    {
        return Err(ApiError::Conflict("a single collection holds one entry"));
    }
    let uniques = unique_pairs(&content_type, &content);
    check_dangling(&mut tx, &content_type, &content).await?;
    let revision = NewRevision {
        id: Uuid::now_v7(),
        content: content.canonical_json(),
        content_hash: &hash,
        title: content.title(),
        schema_version: version,
    };
    let draft = entries::create(
        &mut tx,
        Uuid::now_v7(),
        &path.collection,
        revision,
        &uniques,
        query.locale.as_deref(),
        None,
    )
    .await
    .map_err(ApiError::from_db)?;
    finish(tx, CREATE, key, StatusCode::CREATED, draft).await
}

/// `GET {P}/collections/{collection}/export`: every draft as a JSONL line of its canonical
/// content — what `import` takes back. The newest 10,000 entries.
pub(crate) async fn export(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<CollectionPath>,
) -> Result<Response, ApiError> {
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    // An unknown collection is 404, not an empty export.
    content_type(&mut tx, &path.collection).await?;
    let lines = entries::export(&mut tx, &path.collection)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let mut response = format!("{}\n", lines.join("\n")).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/jsonlines"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"{}.jsonl\"",
            path.collection
        ))
        .map_err(ApiError::internal)?,
    );
    Ok(response)
}

/// `POST {P}/collections/{collection}/import`: a JSONL body, each non-empty line one
/// entry's content, all validated against the type first. The file lands in one
/// transaction or nothing does (`Idempotency-Key` required; at most `IMPORT_MAX_LINES`
/// lines). References resolve against entries that already exist, including earlier lines.
pub(crate) async fn import(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<CollectionPath>,
    headers: HeaderMap,
    body: String,
) -> Result<Response, ApiError> {
    let key = idempotency_key(&headers)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    crate::members::require(
        &tx.role,
        colophon_domain::policy::Action::WriteContent,
        "your role can't write content in this project",
    )?;
    let (content_type, version) = content_type(&mut tx, &path.collection).await?;
    let request = request_hash(&[IMPORT, &path.collection, &body]);
    if let Some(replay) = claim(&mut tx, IMPORT, key, &request).await? {
        return Ok(replay);
    }
    // One JSON per line, or one JSON array of the same objects: `[{...},{...}]`.
    let trimmed = body.trim();
    let (items, at_index): (Vec<Value>, fn(usize) -> String) = if trimmed.starts_with('[') {
        (
            serde_json::from_str::<Vec<Value>>(trimmed).map_err(|_| {
                ApiError::Invalid(Refusal {
                    code: "invalid_json",
                    pointer: "/".to_owned(),
                    message: "the body isn't a JSON array".to_owned(),
                })
            })?,
            |index| format!("/{index}"),
        )
    } else {
        (
            trimmed
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .enumerate()
                .map(|(index, line)| {
                    serde_json::from_str::<Value>(line).map_err(|_| {
                        ApiError::Invalid(Refusal {
                            code: "invalid_json",
                            pointer: format!("/line/{}", index + 1),
                            message: format!("line {} isn't JSON", index + 1),
                        })
                    })
                })
                .collect::<Result<_, ApiError>>()?,
            |index| format!("/line/{}", index + 1),
        )
    };
    if items.len() > IMPORT_MAX_LINES {
        return Err(ApiError::Invalid(Refusal {
            code: "too_many_lines",
            pointer: "/".to_owned(),
            message: format!("an import is at most {IMPORT_MAX_LINES} items"),
        }));
    }
    if content_type.is_single()
        && (items.len() > 1
            || !entries::list(&mut tx, &path.collection, None, 1, None, &[], None)
                .await
                .map_err(ApiError::from_db)?
                .is_empty())
    {
        return Err(ApiError::Conflict("a single collection holds one entry"));
    }
    for (index, value) in items.iter().enumerate() {
        let (content, hash) =
            validate(value, &content_type).map_err(|error| on_index(error, &at_index(index)))?;
        let uniques = unique_pairs(&content_type, &content);
        check_dangling(&mut tx, &content_type, &content)
            .await
            .map_err(|error| on_index(error, &at_index(index)))?;
        let revision = NewRevision {
            id: Uuid::now_v7(),
            content: content.canonical_json(),
            content_hash: &hash,
            title: content.title(),
            schema_version: version,
        };
        entries::create(
            &mut tx,
            Uuid::now_v7(),
            &path.collection,
            revision,
            &uniques,
            None,
        )
        .await
        .map_err(ApiError::from_db)?;
    }
    let body = serde_json::to_string(&Data {
        data: serde_json::json!({ "imported": items.len() }),
    })
    .map_err(ApiError::internal)?;
    idempotency::complete(&mut tx, IMPORT, key, StatusCode::CREATED.as_u16(), &body)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(json(StatusCode::CREATED, body, None, false))
}

/// Prefixes a refusal's pointer with its import item — `/line/3` for JSONL, `/2` for a
/// JSON array — so an error names the item it came from.
fn on_index(error: ApiError, prefix: &str) -> ApiError {
    match error {
        ApiError::Invalid(mut refusal) => {
            refusal.pointer = format!("{}{}", prefix, refusal.pointer);
            ApiError::Invalid(refusal)
        }
        other => other,
    }
}

/// `PUT {P}/collections/{collection}/entries/{entry}`.
pub(crate) async fn save(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<EntryPath>,
    Query(query): Query<LocaleQuery>,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Response, ApiError> {
    check_locale(&query.locale)?;
    let expected = if_match(&headers)?;
    let key = idempotency_key(&headers)?;
    let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
    let Json(value) = body?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    crate::members::require(
        &tx.role,
        colophon_domain::policy::Action::WriteContent,
        "your role can't write content in this project",
    )?;
    let (content_type, version) = content_type(&mut tx, &path.collection).await?;
    let (content, hash) = validate(&value, &content_type)?;
    let request = request_hash(&[
        SAVE,
        &path.collection,
        &entry.to_string(),
        &expected.to_string(),
        content.canonical_json(),
    ]);
    if let Some(replay) = claim(&mut tx, SAVE, key, &request).await? {
        return Ok(replay);
    }
    let revision = NewRevision {
        id: Uuid::now_v7(),
        content: content.canonical_json(),
        content_hash: &hash,
        title: content.title(),
        schema_version: version,
    };
    let uniques = unique_pairs(&content_type, &content);
    check_dangling(&mut tx, &content_type, &content).await?;
    // A refusal returns early, dropping the transaction: the claim and anything else roll back.
    match entries::save(&mut tx, entry, expected, revision, &uniques)
        .await
        .map_err(ApiError::from_db)?
    {
        Saved::Saved(draft) => {
            if let Some(tag) = query.locale.as_deref() {
                entries::set_locale(&mut tx, &path.collection, entry, Some(tag))
                    .await
                    .map_err(ApiError::from_db)?;
            }
            finish(tx, SAVE, key, StatusCode::OK, *draft).await
        }
        Saved::Stale { current } => Err(ApiError::StaleRevision { current }),
        Saved::NotFound => Err(ApiError::NotFound),
    }
}

/// `GET {P}/collections/{collection}/entries/{entry}`.
pub(crate) async fn get(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<EntryPath>,
) -> Result<Response, ApiError> {
    let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let draft = entries::get(&mut tx, &path.collection, entry)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let revision = draft.revision.id;
    let body = serde_json::to_string(&Data {
        data: DraftView::new(draft)?,
    })
    .map_err(ApiError::internal)?;
    Ok(json(StatusCode::OK, body, Some(revision), false))
}

/// `DELETE {P}/collections/{collection}/entries/{entry}`: gone for good, but only while
/// nothing references it — a changeset item or a published copy answers `conflict`.
pub(crate) async fn remove(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<EntryPath>,
) -> Result<Response, ApiError> {
    let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let removed = entries::remove(&mut tx, &path.collection, entry)
        .await
        .map_err(ApiError::from_db)?;
    match removed {
        Removed::Removed => {
            tx.commit().await.map_err(ApiError::from_db)?;
            Ok(json(
                StatusCode::OK,
                r#"{"data":{"removed":true}}"#.to_string(),
                None,
                false,
            ))
        }
        Removed::NotFound => Err(ApiError::NotFound),
        Removed::Referenced => Err(ApiError::Conflict(
            "the entry is still referenced: it is in a changeset or was published",
        )),
    }
}

/// A cursor is bound to the project, environment and collection it came from (module 04).
fn cursor(path: &CollectionPath, project: Uuid, last: Uuid) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "v1:{project}:{}:{}:{last}",
        path.environment, path.collection
    ))
}

fn after_cursor(path: &CollectionPath, project: Uuid, cursor: &str) -> Result<Uuid, ApiError> {
    let prefix = format!("v1:{project}:{}:{}:", path.environment, path.collection);
    URL_SAFE_NO_PAD
        .decode(cursor)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| Uuid::try_parse(text.strip_prefix(&prefix)?).ok())
        .ok_or(ApiError::InvalidHeader(
            "this cursor is not valid for this list",
        ))
}

/// `GET {P}/collections/{collection}/entries?limit=&cursor=`.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<CollectionPath>,
    Query(query): Query<ListQuery>,
) -> Result<Response, ApiError> {
    check_locale(&query.locale)?;
    let limit = query.limit.unwrap_or(DEFAULT_PAGE);
    if !(1..=entries::MAX_PAGE).contains(&limit) {
        return Err(ApiError::InvalidHeader("limit must be between 1 and 100"));
    }
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    // An unknown collection is 404, not an empty list.
    let (definition, _) = content_type(&mut tx, &path.collection).await?;
    // List columns: up to three short fields — a number, a choice, yes or no, a date, a
    // time — in the definition's order, so a list can show the fields that say what an
    // entry is beyond its title.
    let columns: Vec<String> = definition
        .fields()
        .iter()
        .filter(|field| {
            matches!(
                field.kind,
                FieldKind::Number { .. }
                    | FieldKind::Boolean
                    | FieldKind::Enum { .. }
                    | FieldKind::DateTime
                    | FieldKind::Date
                    | FieldKind::Time
            )
        })
        .take(3)
        .map(|field| field.id.clone())
        .collect();
    let project = tx.project;
    let before = query
        .cursor
        .as_deref()
        .map(|cursor| after_cursor(&path, project, cursor))
        .transpose()?;
    // One more than the page, to know whether another page follows.
    let search = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty());
    if search.is_some_and(|text| text.chars().count() > entries::MAX_SEARCH) {
        return Err(ApiError::InvalidHeader(
            "a search is at most 100 characters",
        ));
    }
    let mut rows: Vec<Summary> = entries::list(
        &mut tx,
        &path.collection,
        before,
        limit + 1,
        search,
        &columns,
        query.locale.as_deref(),
    )
    .await
    .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let more = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
    rows.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    let next_cursor = more
        .then(|| rows.last().map(|row| cursor(&path, project, row.entry_id)))
        .flatten();
    let data = rows
        .into_iter()
        .map(|row| {
            Ok(SummaryView {
                id: row.entry_id,
                updated_at: rfc3339(row.updated_at)?,
                revision: RevisionView::new(row.revision)?,
                published: row.published.map(|(revision_id, generation)| LiveView {
                    revision_id,
                    generation,
                }),
                fields: RawValue::from_string(row.fields).map_err(ApiError::internal)?,
                locale: row.locale,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let body = serde_json::to_string(&Page { data, next_cursor }).map_err(ApiError::internal)?;
    Ok(json(StatusCode::OK, body, None, false))
}

/// `GET {P}/collections/{collection}/entries/{entry}/revisions/{revision}`: one revision with its
/// content, for comparing versions.
pub(crate) async fn revision(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<RevisionPath>,
) -> Result<Response, ApiError> {
    let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
    let revision = Uuid::try_parse(&path.revision).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let found = entries::revision(&mut tx, &path.collection, entry, revision)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let (revision, content) = found;
    let body = serde_json::to_string(&crate::shape::Data {
        data: RevisionContentView {
            revision: RevisionView::new(revision)?,
            content: serde_json::value::RawValue::from_string(content)
                .map_err(ApiError::internal)?,
        },
    })
    .map_err(ApiError::internal)?;
    Ok(json(StatusCode::OK, body, None, false))
}

/// `GET {P}/collections/{collection}/entries/{entry}/revisions`: the newest 100.
pub(crate) async fn history(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<EntryPath>,
) -> Result<Response, ApiError> {
    let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let revisions = entries::history(&mut tx, &path.collection, entry, entries::MAX_PAGE)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    // Every entry has at least one revision, so none means no such entry.
    if revisions.is_empty() {
        return Err(ApiError::NotFound);
    }
    let data = revisions
        .into_iter()
        .map(RevisionView::new)
        .collect::<Result<Vec<_>, ApiError>>()?;
    let body = serde_json::to_string(&Data { data }).map_err(ApiError::internal)?;
    Ok(json(StatusCode::OK, body, None, false))
}

/// `GET {P}/collections/{collection}/entries/{entry}/referenced-by`: the newest drafts
/// that point at this entry through a `reference` field — the other side of a link. The
/// `LIKE` scan screens candidates; each row here is verified against a real reference
/// field, so a uuid sitting in some other field doesn't name itself.
pub(crate) async fn referenced_by(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<EntryPath>,
) -> Result<Response, ApiError> {
    let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    entries::get(&mut tx, &path.collection, entry)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let needle = entry.to_string();
    let mut rows = Vec::new();
    for version in content_types::list(&mut tx)
        .await
        .map_err(ApiError::from_db)?
    {
        let definition: Value =
            serde_json::from_str(&version.definition).map_err(ApiError::internal)?;
        let parsed = ContentType::parse(&definition).map_err(ApiError::internal)?;
        let pointing: Vec<_> = parsed
            .fields()
            .iter()
            .filter(|field| {
                matches!(&field.kind, FieldKind::Reference { to, .. } if to == &path.collection)
            })
            .collect();
        if pointing.is_empty() {
            continue;
        }
        for candidate in entries::referencing(&mut tx, &version.collection, entry)
            .await
            .map_err(ApiError::from_db)?
        {
            let Ok(content) = serde_json::from_str::<Value>(&candidate.content) else {
                continue;
            };
            for field in &pointing {
                let held = match content.get(&field.id) {
                    Some(Value::String(held)) => held == &needle,
                    Some(Value::Array(held)) => held
                        .iter()
                        .any(|item| item.as_str() == Some(needle.as_str())),
                    _ => false,
                };
                if held {
                    rows.push(serde_json::json!({
                        "collection": version.collection,
                        "collection_name": parsed.name(),
                        "field": field.id,
                        "field_label": field.label,
                        "entry": candidate.id,
                        "title": candidate.title,
                    }));
                }
            }
        }
    }
    rows.truncate(50);
    tx.commit().await.map_err(ApiError::from_db)?;
    let body = serde_json::to_string(&Data {
        data: Value::Array(rows),
    })
    .map_err(ApiError::internal)?;
    Ok(json(StatusCode::OK, body, None, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, HeaderValue::from_str(value).expect("ASCII"));
        }
        map
    }

    #[test]
    fn if_match_takes_exactly_one_quoted_revision() {
        let id = Uuid::now_v7();
        assert_eq!(
            if_match(&headers(&[("if-match", &format!("\"{id}\""))])).ok(),
            Some(id)
        );
        assert!(matches!(
            if_match(&HeaderMap::new()),
            Err(ApiError::PreconditionRequired(_))
        ));
        for bad in [
            "*",
            &id.to_string(),
            &format!("W/\"{id}\""),
            &format!("\"{id}\", \"{id}\""),
        ] {
            assert!(
                matches!(
                    if_match(&headers(&[("if-match", bad)])),
                    Err(ApiError::InvalidHeader(_))
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn idempotency_keys_are_bounded_and_plain() {
        assert!(idempotency_key(&headers(&[(IDEMPOTENCY_KEY, "abcdefgh-1234_XYZ")])).is_ok());
        assert!(matches!(
            idempotency_key(&HeaderMap::new()),
            Err(ApiError::PreconditionRequired(_))
        ));
        for bad in [
            "short",
            "has space in it here",
            &"k".repeat(129),
            "semi;colon-0123456789",
        ] {
            assert!(
                matches!(
                    idempotency_key(&headers(&[(IDEMPOTENCY_KEY, bad)])),
                    Err(ApiError::InvalidHeader(_))
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn request_hashes_differ_when_any_part_does() {
        let base = request_hash(&["entry.save", "a", "bc"]);
        assert_ne!(
            base,
            request_hash(&["entry.save", "ab", "c"]),
            "parts are length-prefixed"
        );
        assert_ne!(base, request_hash(&["entry.create", "a", "bc"]));
        assert_eq!(base, request_hash(&["entry.save", "a", "bc"]));
    }

    #[test]
    fn content_hashes_are_prefixed_lowercase_hex() {
        let hash = content_hash(&sha256(b""));
        assert_eq!(
            hash, "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "SHA-256 of nothing, from FIPS 180-2"
        );
    }

    #[test]
    fn cursors_only_work_where_they_came_from() {
        let project = Uuid::now_v7();
        let last = Uuid::now_v7();
        let here = CollectionPath {
            project: project.to_string(),
            environment: "production".to_owned(),
            collection: "articles".to_owned(),
        };
        let token = cursor(&here, project, last);
        assert_eq!(after_cursor(&here, project, &token).ok(), Some(last));
        let elsewhere = CollectionPath {
            environment: "staging".to_owned(),
            ..here
        };
        assert!(after_cursor(&elsewhere, project, &token).is_err());
        let other_project = Uuid::now_v7();
        let here_again = CollectionPath {
            project: other_project.to_string(),
            environment: "production".to_owned(),
            collection: "articles".to_owned(),
        };
        assert!(after_cursor(&here_again, other_project, &token).is_err());
        assert!(after_cursor(&here_again, other_project, "not base64 !").is_err());
    }
}
