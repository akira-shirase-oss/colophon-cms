//! Entries and their drafts (docs/modules/04-api.md, "Route families"; D-034, D-035). Under
//! `{P}` = `/api/v1/projects/{project}/environments/{environment}`:
//!
//! - `GET  {P}/collections/{collection}/entries`: newest first, with a cursor;
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
    entries::{self, Draft, NewRevision, Revision, Saved, Summary},
    idempotency::{self, Claim},
    scope::{self, ProjectTransaction},
};
use colophon_domain::{
    policy::{Action, Fields},
    schema::{Content, ContentType, FieldKind},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, value::RawValue};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    error::EntryInTheWay,
    fault,
    session::{SignedIn, services},
    shape::{Data, Page},
    visibility::{self, Access},
};

/// The header that carries an idempotency key.
pub(crate) const IDEMPOTENCY_KEY: &str = "idempotency-key";
/// Present (`true`) on a response that replays the recorded answer to an earlier request.
pub(crate) const IDEMPOTENT_REPLAYED: &str = "idempotent-replayed";
/// Entries per page unless `limit` says otherwise.
const DEFAULT_PAGE: i64 = 20;

const CREATE: &str = "entry.create";
const SAVE: &str = "entry.save";

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
    created_by: Uuid,
    updated_at: String,
    revision: RevisionView,
    /// The stored canonical JSON, embedded as is: re-serializing could reorder its keys. Only the
    /// fields the caller's role may read (D-058, step R.3).
    content: Box<RawValue>,
    /// The content type's fields left out because the caller's role can't read them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    hidden_fields: Vec<String>,
    /// Fields the caller's role can read but not change: an editor shows them locked.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    locked_fields: Vec<String>,
    /// On a read, what readers get now: the live version, or `null` when the entry isn't
    /// published (as on a list's summaries). Left out of create and save answers, which don't
    /// look it up; a save never changes what is live.
    #[serde(skip_serializing_if = "Publication::not_looked_up")]
    published: Publication,
}

impl DraftView {
    /// The draft as the caller may see it: `None` if their role can't read it.
    fn shown(
        draft: Draft,
        published: Publication,
        access: &Access,
        content_type: &ContentType,
    ) -> Result<Self, ApiError> {
        let read = access.read.as_ref().ok_or(ApiError::NotFound)?;
        let fields = field_ids(content_type);
        let (content, hidden_fields) = visibility::project(draft.content, read, &fields)?;
        let mut revision = draft.revision;
        if !read.includes(content_type.title_field()) {
            revision.title = String::new();
        }
        Ok(Self {
            id: draft.entry_id,
            collection: draft.collection,
            created_at: rfc3339(draft.created_at)?,
            created_by: draft.created_by,
            updated_at: rfc3339(draft.updated_at)?,
            revision: RevisionView::new(revision)?,
            content: RawValue::from_string(content).map_err(ApiError::internal)?,
            locked_fields: visibility::locked(access, &fields),
            hidden_fields,
            published,
        })
    }
}

fn field_ids(content_type: &ContentType) -> Vec<String> {
    content_type
        .fields()
        .iter()
        .map(|field| field.id.clone())
        .collect()
}

#[derive(Serialize)]
struct SummaryView {
    id: Uuid,
    /// Who created it: the member whose own entry it is (D-058).
    created_by: Uuid,
    updated_at: String,
    revision: RevisionView,
    /// What readers get now, if the entry is published: so a list can say draft, live or
    /// changed since it went live.
    published: Option<LiveView>,
}

/// Whether an entry is live, as an answer about it says.
enum Publication {
    /// The answer didn't look it up: the field is left out.
    NotLookedUp,
    /// Not published: `null`.
    Unpublished,
    /// Published: the live version and its release.
    Live(LiveView),
}

impl Publication {
    fn not_looked_up(&self) -> bool {
        matches!(self, Self::NotLookedUp)
    }
}

impl From<Option<(Uuid, i64)>> for Publication {
    fn from(live: Option<(Uuid, i64)>) -> Self {
        live.map_or(Self::Unpublished, |(revision_id, generation)| {
            Self::Live(LiveView {
                revision_id,
                generation,
            })
        })
    }
}

impl Serialize for Publication {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Live(live) => live.serialize(serializer),
            Self::NotLookedUp | Self::Unpublished => serializer.serialize_none(),
        }
    }
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

/// Refuses content whose slug another entry of the collection has, as a draft or published,
/// naming that entry. Only the wording: the unique indexes are the rule, and a write that loses a
/// race to them is refused without a name ([`ApiError::from_db`]).
async fn slug_free(
    tx: &mut ProjectTransaction,
    collection: &str,
    content_type: &ContentType,
    content: &Content,
    entry: Option<Uuid>,
) -> Result<(), ApiError> {
    let Some(slug) = content.slug() else {
        return Ok(());
    };
    let Some(holder) = entries::slug_holder(tx, collection, slug, entry)
        .await
        .map_err(ApiError::from_db)?
    else {
        return Ok(());
    };
    let field = content_type
        .fields()
        .iter()
        .find(|field| matches!(field.kind, FieldKind::Slug { .. }))
        .map_or("", |field| field.id.as_str());
    let state = if holder.published {
        "published"
    } else {
        "draft"
    };
    Err(ApiError::slug_taken(
        format!("/{field}"),
        format!("\"{slug}\" is already the slug of another entry ({state})"),
        Some(EntryInTheWay {
            entry: holder.entry_id,
            title: holder.title,
            state,
            problem: None,
        }),
    ))
}

/// Refuses unless the caller's role may read entries of `collection` (D-058).
/// Refuses unless the caller's role may read some entries of `collection`: anyone's, or at least
/// their own.
fn may_read(tx: &ProjectTransaction, collection: &str) -> Result<(), ApiError> {
    let some = [false, true].into_iter().any(|own| {
        tx.policy
            .fields(Action::ContentRead, collection, own)
            .is_some()
    });
    if some {
        Ok(())
    } else {
        Err(ApiError::missing(
            Action::ContentRead,
            "your role can't read entries here",
        ))
    }
}

/// The fields of `entry` the caller's role may read, and its content type; an entry it may not
/// read at all is not found, as if it didn't exist.
async fn readable(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry: Uuid,
) -> Result<(Fields, ContentType), ApiError> {
    let created_by = entries::creators(tx, &[entry])
        .await
        .map_err(ApiError::from_db)?
        .into_iter()
        .find(|(_, found, _)| found == collection)
        .map(|(_, _, created_by)| created_by)
        .ok_or(ApiError::NotFound)?;
    let read = Access::of(&tx.policy, tx.identity, collection, created_by)
        .read
        .ok_or(ApiError::NotFound)?;
    let (content_type, _) = content_type(tx, collection).await?;
    Ok((read, content_type))
}

/// Validates content against its content type: the canonical content and its hash.
fn validate(value: &Value, content_type: &ContentType) -> Result<(Content, [u8; 32]), ApiError> {
    let content = content_type.validate(value).map_err(ApiError::Invalid)?;
    let hash = sha256(content.canonical_json().as_bytes());
    Ok((content, hash))
}

/// Records the answer, commits, and only then responds: an acknowledged write is durable.
async fn finish(
    mut tx: ProjectTransaction,
    operation: &str,
    key: &str,
    status: StatusCode,
    view: DraftView,
) -> Result<Response, ApiError> {
    let revision = view.revision.id;
    let body = serde_json::to_string(&Data { data: view }).map_err(ApiError::internal)?;
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
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Response, ApiError> {
    let key = idempotency_key(&headers)?;
    let Json(value) = body?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    // A new entry is the creator's own.
    let create = tx
        .policy
        .fields(Action::ContentCreate, &path.collection, true)
        .ok_or(ApiError::missing(
            Action::ContentCreate,
            "your role can't create entries here",
        ))?;
    content_types::lock_for_write(&mut tx, &path.collection)
        .await
        .map_err(ApiError::from_db)?;
    let (content_type, version) = content_type(&mut tx, &path.collection).await?;
    let (content, hash) = validate(&value, &content_type)?;
    visibility::only_permitted_fields(content.canonical_json(), &create)?;
    let request = request_hash(&[CREATE, &path.collection, content.canonical_json()]);
    if let Some(replay) = claim(&mut tx, CREATE, key, &request).await? {
        return Ok(replay);
    }
    slug_free(&mut tx, &path.collection, &content_type, &content, None).await?;
    let revision = NewRevision {
        id: Uuid::now_v7(),
        content: content.canonical_json(),
        content_hash: &hash,
        title: content.title(),
        slug: content.slug(),
        schema_version: version,
    };
    let draft = entries::create(&mut tx, Uuid::now_v7(), &path.collection, revision)
        .await
        .map_err(ApiError::from_db)?;
    let access = Access::of(&tx.policy, tx.identity, &path.collection, draft.created_by);
    let view = DraftView::shown(draft, Publication::NotLookedUp, &access, &content_type)?;
    finish(tx, CREATE, key, StatusCode::CREATED, view).await
}

/// `PUT {P}/collections/{collection}/entries/{entry}`.
pub(crate) async fn save(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<EntryPath>,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Response, ApiError> {
    let expected = if_match(&headers)?;
    let key = idempotency_key(&headers)?;
    let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
    let Json(value) = body?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    if [false, true].into_iter().all(|own| {
        tx.policy
            .fields(Action::ContentUpdate, &path.collection, own)
            .is_none()
    }) {
        return Err(ApiError::missing(
            Action::ContentUpdate,
            "your role can't change entries here",
        ));
    }
    content_types::lock_for_write(&mut tx, &path.collection)
        .await
        .map_err(ApiError::from_db)?;
    let (content_type, version) = content_type(&mut tx, &path.collection).await?;
    // What this caller may do with this entry, by whose it is (D-058, step R.3).
    let created_by = entries::creators(&mut tx, &[entry])
        .await
        .map_err(ApiError::from_db)?
        .into_iter()
        .find(|(_, collection, _)| *collection == path.collection)
        .map(|(_, _, created_by)| created_by)
        .ok_or(ApiError::NotFound)?;
    let access = Access::of(&tx.policy, tx.identity, &path.collection, created_by);
    let read = access.read.clone().ok_or(ApiError::NotFound)?;
    let update = access.update.clone().ok_or(ApiError::missing(
        Action::ContentUpdate,
        "your role can't change this entry",
    ))?;
    // The version the editor started from; revisions never change. A stale one is reported by
    // the save itself.
    let stored = entries::revision(&mut tx, &path.collection, entry, expected)
        .await
        .map_err(ApiError::from_db)?
        .map(|(_, content)| content);
    let value = match &stored {
        Some(stored) => visibility::restore_hidden(value, stored, &read)?,
        None => value,
    };
    let (content, hash) = validate(&value, &content_type)?;
    if let Some(stored) = &stored {
        visibility::only_permitted_changes(content.canonical_json(), stored, &update)?;
    }
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
    slug_free(
        &mut tx,
        &path.collection,
        &content_type,
        &content,
        Some(entry),
    )
    .await?;
    let revision = NewRevision {
        id: Uuid::now_v7(),
        content: content.canonical_json(),
        content_hash: &hash,
        title: content.title(),
        slug: content.slug(),
        schema_version: version,
    };
    // A refusal returns early, dropping the transaction: the claim and anything else roll back.
    match entries::save(&mut tx, entry, expected, revision)
        .await
        .map_err(ApiError::from_db)?
    {
        Saved::Saved(draft) => {
            let view = DraftView::shown(*draft, Publication::NotLookedUp, &access, &content_type)?;
            finish(tx, SAVE, key, StatusCode::OK, view).await
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
    may_read(&tx, &path.collection)?;
    let (draft, published) = entries::get_with_live(&mut tx, &path.collection, entry)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let (content_type, _) = content_type(&mut tx, &path.collection).await?;
    let access = Access::of(&tx.policy, tx.identity, &path.collection, draft.created_by);
    tx.commit().await.map_err(ApiError::from_db)?;
    let revision = draft.revision.id;
    let body = serde_json::to_string(&Data {
        data: DraftView::shown(draft, published.into(), &access, &content_type)?,
    })
    .map_err(ApiError::internal)?;
    Ok(json(StatusCode::OK, body, Some(revision), false))
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
    let limit = query.limit.unwrap_or(DEFAULT_PAGE);
    if !(1..=entries::MAX_PAGE).contains(&limit) {
        return Err(ApiError::InvalidHeader("limit must be between 1 and 100"));
    }
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_read(&tx, &path.collection)?;
    // An unknown collection is 404, not an empty list.
    content_type(&mut tx, &path.collection).await?;
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
    // Only their own entries, when that is all the role may read (D-058, step R.3); and titles
    // only where the role may read the title field.
    let anyone = tx
        .policy
        .fields(Action::ContentRead, &path.collection, false);
    let mut only_by = anyone.is_none().then_some(tx.identity);
    let title_field = match anyone {
        Some(Fields::All) => None,
        _ => Some(
            content_type(&mut tx, &path.collection)
                .await?
                .0
                .title_field()
                .to_owned(),
        ),
    };
    // A search matches titles, so it looks only where the role reads them: otherwise which
    // entries match would confirm a guess at a title the answer leaves blank.
    if let (Some(_), Some(title_field)) = (search, &title_field) {
        let reads_title = |own: bool| {
            tx.policy
                .fields(Action::ContentRead, &path.collection, own)
                .is_some_and(|fields| fields.includes(title_field))
        };
        if !reads_title(false) {
            if reads_title(true) {
                only_by = Some(tx.identity);
            } else {
                return Err(ApiError::forbidden(
                    "titles_not_readable",
                    "your role can't read titles here, so it can't search them",
                ));
            }
        }
    }
    let mut rows: Vec<Summary> = entries::list(
        &mut tx,
        &path.collection,
        before,
        limit + 1,
        search,
        only_by,
    )
    .await
    .map_err(ApiError::from_db)?;
    let (policy, identity) = (tx.policy.clone(), tx.identity);
    tx.commit().await.map_err(ApiError::from_db)?;
    let more = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
    rows.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    let next_cursor = more
        .then(|| rows.last().map(|row| cursor(&path, project, row.entry_id)))
        .flatten();
    let data = rows
        .into_iter()
        .map(|mut row| {
            if let Some(title_field) = &title_field {
                let read = Access::of(&policy, identity, &path.collection, row.created_by).read;
                if !read.is_some_and(|read| read.includes(title_field)) {
                    row.revision.title = String::new();
                }
            }
            Ok(SummaryView {
                id: row.entry_id,
                created_by: row.created_by,
                updated_at: rfc3339(row.updated_at)?,
                revision: RevisionView::new(row.revision)?,
                published: row.published.map(|(revision_id, generation)| LiveView {
                    revision_id,
                    generation,
                }),
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
    may_read(&tx, &path.collection)?;
    let (read, content_type) = readable(&mut tx, &path.collection, entry).await?;
    let found = entries::revision(&mut tx, &path.collection, entry, revision)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let (mut revision, content) = found;
    let (content, _) = visibility::project(content, &read, &field_ids(&content_type))?;
    if !read.includes(content_type.title_field()) {
        revision.title = String::new();
    }
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
    may_read(&tx, &path.collection)?;
    let (read, content_type) = readable(&mut tx, &path.collection, entry).await?;
    let revisions = entries::history(&mut tx, &path.collection, entry, entries::MAX_PAGE)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    // Every entry has at least one revision, so none means no such entry.
    if revisions.is_empty() {
        return Err(ApiError::NotFound);
    }
    let titles = read.includes(content_type.title_field());
    let data = revisions
        .into_iter()
        .map(|mut revision| {
            if !titles {
                revision.title = String::new();
            }
            RevisionView::new(revision)
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let body = serde_json::to_string(&Data { data }).map_err(ApiError::internal)?;
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
