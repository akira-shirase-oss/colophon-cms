//! Entries and their drafts (docs/modules/04-api.md, "Route families"; D-034, D-035). Under
//! `{P}` = `/api/v1/projects/{project}/environments/{environment}`:
//!
//! - `GET  {P}/collections/{collection}/entries`: newest first, with a cursor;
//! - `POST {P}/collections/{collection}/entries`: create (`Idempotency-Key` required);
//! - `GET  {P}/collections/{collection}/entries/{entry}`: the draft, with its `ETag`;
//! - `PUT  {P}/collections/{collection}/entries/{entry}`: save (`If-Match` and
//!   `Idempotency-Key` required);
//! - `DELETE {P}/collections/{collection}/entries/{entry}`: delete, leaving a tombstone
//!   (`If-Match` and `Idempotency-Key` required; D-080);
//! - `GET  {P}/collections/{collection}/entries/{entry}/revisions`: history, newest first;
//! - `GET  {P}/collections/{collection}/export`: every draft as a JSONL line (D-086);
//! - `POST {P}/collections/{collection}/import`: JSONL or a JSON array of entries back, in one
//!   transaction (`Idempotency-Key` required; D-086).
//!
//! A project the caller isn't in, an environment, collection or entry that doesn't exist: all
//! answer 404 alike. A draft's `ETag` names its revision; a save whose `If-Match` names an older
//! one is refused with 409 and the current `ETag`, and writes nothing.

use std::sync::Arc;

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
    entries::{self, Deleted, Draft, NewRevision, Revision, Saved, Summary},
    idempotency::{self, Claim},
    scope::{self, ProjectTransaction},
};
use colophon_domain::{
    policy::{Action, Fields, Policy},
    schema::{Content, ContentType, FieldKind, Refusal},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, value::RawValue};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    error::{EntryInTheWay, PendingChangeset, StillNeeded},
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
const DELETE: &str = "entry.delete";
const IMPORT: &str = "entry.import";
const LOCALIZE: &str = "entry.localize";
/// An import is at most this many items, so one upload stays a bounded transaction (D-086).
const IMPORT_MAX_ITEMS: usize = 512;

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
    /// On a read, the entries whose draft or published revision names this one (D-065), in
    /// collections the caller's role reads; left out of create and save answers.
    #[serde(skip_serializing_if = "Option::is_none")]
    referenced_by: Option<Vec<ReferrerView>>,
    /// The entry's locale tag, or `null` for the project's default (D-087).
    locale: Option<String>,
    /// The entries of its localization group, including itself (D-087): `[]` when it is
    /// unlocalized. Filled by the answer's writer, which looks them up in the transaction.
    localizations: Vec<LocalizationView>,
}

/// A sibling in a localization group, as a draft's `localizations` lists it (D-087).
#[derive(Serialize)]
struct LocalizationView {
    id: Uuid,
    locale: Option<String>,
}

#[derive(Serialize)]
struct ReferrerView {
    entry_id: Uuid,
    collection: String,
    field: String,
    /// Its draft names this entry.
    in_draft: bool,
    /// What readers get of it names this entry.
    published: bool,
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
            referenced_by: None,
            locale: draft.locale,
            localizations: Vec::new(),
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
    /// The entry's locale tag, or `null` for the project's default (D-087).
    locale: Option<String>,
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

/// `?limit=&cursor=&q=&created_by=` for lists; `q` searches titles, `created_by` keeps one
/// person's (or token's) entries.
#[derive(Deserialize)]
pub(crate) struct ListQuery {
    limit: Option<i64>,
    cursor: Option<String>,
    q: Option<String>,
    created_by: Option<String>,
    locale: Option<String>,
}

/// The `?locale=` a create, save or localize takes (D-087).
#[derive(Deserialize)]
pub(crate) struct LocaleQuery {
    locale: Option<String>,
}

/// A locale label is a BCP-47 tag: `en`, `en-US`, `pt-BR` — a language subtag of two or three
/// letters, then subtags of one to eight letters or digits, at most 35 characters over all
/// (BCP 47's own bound). The API shape-checks it; there is no locale table to join against.
fn check_locale(locale: &str) -> Result<(), ApiError> {
    let shape = locale.len() <= 35
        && locale.split('-').enumerate().all(|(index, tag)| {
            if index == 0 {
                (2..=3).contains(&tag.len()) && tag.bytes().all(|byte| byte.is_ascii_alphabetic())
            } else {
                (1..=8).contains(&tag.len()) && tag.bytes().all(|byte| byte.is_ascii_alphanumeric())
            }
        });
    if shape {
        Ok(())
    } else {
        Err(ApiError::Invalid(Refusal {
            code: "invalid_locale",
            pointer: "/locale".to_owned(),
            message: format!("\"{locale}\" isn't a BCP-47 locale tag (like \"en\" or \"pt-BR\")"),
        }))
    }
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
    if_match_or(
        headers,
        "saving needs an If-Match header with the ETag of the revision you edited",
    )
}

/// [`if_match`], saying `missing` when there is none.
fn if_match_or(headers: &HeaderMap, missing: &'static str) -> Result<Uuid, ApiError> {
    let value = headers
        .get(header::IF_MATCH)
        .ok_or(ApiError::PreconditionRequired(missing))?;
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
    let opened = match caller.project {
        // An API token: only in its own project, with its own permissions (D-063).
        Some(own) if own == project => {
            scope::begin_in_project_as_token(&services.pool, caller.identity, project, environment)
                .await
        }
        Some(_) => return Err(ApiError::NotFound),
        None => {
            scope::begin_in_project(&services.pool, caller.identity, project, environment).await
        }
    };
    opened.map_err(ApiError::from_db)?.ok_or(ApiError::NotFound)
}

/// The collection's current content type and its version, or 404. The definition is read from
/// Postgres every time; only its parsing is cached (D-078).
pub(crate) async fn content_type(
    state: &AppState,
    tx: &mut ProjectTransaction,
    collection: &str,
) -> Result<(Arc<ContentType>, i32), ApiError> {
    let stored = content_types::current(tx, collection)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let parsed = services(state)?.content_types.parse(&stored.definition)?;
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
    let named = may_name(tx, collection, content_type, holder.created_by);
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
        named.then_some(EntryInTheWay {
            entry: holder.entry_id,
            title: holder.title,
            state,
            problem: None,
        }),
    ))
}

/// Refuses content holding a value of a unique field that another entry of the collection
/// holds, as a draft or published (D-070), naming that entry when the caller may read its title.
/// Only the wording: `unique_claims_value` is the rule, and a write that loses a race to it is
/// refused without a name ([`ApiError::from_db`]).
async fn unique_free(
    tx: &mut ProjectTransaction,
    collection: &str,
    content_type: &ContentType,
    unique: &[String],
    content: &Content,
    entry: Option<Uuid>,
) -> Result<(), ApiError> {
    let Some(holder) =
        entries::unique_holder(tx, collection, unique, content.canonical_json(), entry)
            .await
            .map_err(ApiError::from_db)?
    else {
        return Ok(());
    };
    let named = may_name(tx, collection, content_type, holder.created_by);
    let label = content_type
        .fields()
        .iter()
        .find(|field| field.id == holder.field)
        .map_or(holder.field.as_str(), |field| field.label.as_str());
    let state = if holder.published {
        "published"
    } else {
        "draft"
    };
    Err(ApiError::value_taken(
        format!("/{}", holder.field),
        format!("another entry already has this {label} ({state}), and it must be unique"),
        named.then_some(EntryInTheWay {
            entry: holder.entry_id,
            title: holder.title,
            state,
            problem: None,
        }),
    ))
}

/// The `?locale=` a save writes once its revision lands (D-087): `None` clears to the project's
/// default. A locale a sibling of the entry's group already holds is refused `locale_taken`,
/// and clearing a grouped entry `locale_required` — the group needs each member named.
async fn apply_locale(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry: Uuid,
    locale: Option<&str>,
) -> Result<(), ApiError> {
    match entries::set_locale(tx, collection, entry, locale)
        .await
        .map_err(ApiError::from_db)?
    {
        entries::SetLocale::Set => Ok(()),
        entries::SetLocale::Occupied(_) => Err(ApiError::conflict(
            "locale_taken",
            "a version in that locale already exists",
        )),
        entries::SetLocale::Grouped => Err(ApiError::conflict(
            "locale_required",
            "an entry in a localization group keeps a locale",
        )),
        entries::SetLocale::Missing => Err(ApiError::NotFound),
    }
}

/// Whether a refusal may name an entry in the way, created by `created_by`: only when the
/// caller's role reads its title (step R.3), so a refusal never shows a title the role can't
/// read. The refusal itself still says the slug or value is taken, as any unique rule must.
fn may_name(
    tx: &ProjectTransaction,
    collection: &str,
    content_type: &ContentType,
    created_by: Uuid,
) -> bool {
    tx.policy
        .fields(Action::ContentRead, collection, created_by == tx.identity)
        .is_some_and(|fields| fields.includes(content_type.title_field()))
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
    state: &AppState,
    tx: &mut ProjectTransaction,
    collection: &str,
    entry: Uuid,
) -> Result<(Fields, Arc<ContentType>), ApiError> {
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
    let (content_type, _) = content_type(state, tx, collection).await?;
    Ok((read, content_type))
}

/// Validates content against its content type: the canonical content and its hash.
fn validate(value: &Value, content_type: &ContentType) -> Result<(Content, [u8; 32]), ApiError> {
    let content = content_type.validate(value).map_err(ApiError::Invalid)?;
    let hash = sha256(content.canonical_json().as_bytes());
    Ok((content, hash))
}

/// A revision of `content` at `version`, for a create, a save or a mirror write.
fn new_revision<'a>(
    content: &'a Content,
    hash: &'a [u8; 32],
    version: i32,
    unique: &'a [String],
) -> NewRevision<'a> {
    NewRevision {
        id: Uuid::now_v7(),
        content: content.canonical_json(),
        content_hash: hash,
        title: content.title(),
        slug: content.slug(),
        schema_version: version,
        references: content.references(),
        unique,
    }
}

/// The ids a top-level reference field holds in a stored document: one string, or a list of
/// them. Anything else (or a document that isn't one) answers empty.
fn reference_ids(canonical: Option<&str>, field: &str) -> Vec<String> {
    let Some(canonical) = canonical else {
        return Vec::new();
    };
    let Ok(document) = serde_json::from_str::<Value>(canonical) else {
        return Vec::new();
    };
    match document.get(field) {
        Some(Value::String(one)) => vec![one.clone()],
        Some(Value::Array(list)) => list
            .iter()
            .filter_map(|id| id.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// A declared inverse: `to`'s `back` field, which must be a reference pointing at `mine` — the
/// collection whose field declared it.
#[derive(Clone, Copy)]
struct Mirror<'a> {
    to: &'a str,
    back: &'a str,
    mine: &'a str,
}

/// Adds `entry` to (or removes it from) the `back` field of `other` — one new revision on the
/// target, inside this transaction (D-085). A `back` that can't take the write — a target that
/// went away, a field that stopped pointing back, a value the target's type refuses, a save
/// that lost a race — is skipped rather than sinking the entry write it came from: the forward
/// reference is the truth; the mirror is kept where it fits. Mirror writes don't recurse.
async fn write_back(
    state: &AppState,
    tx: &mut ProjectTransaction,
    mirror: Mirror<'_>,
    other: Uuid,
    entry: Uuid,
    add: bool,
) -> Result<(), ApiError> {
    let Mirror { to, back, mine } = mirror;
    let Some(draft) = entries::get(tx, to, other)
        .await
        .map_err(ApiError::from_db)?
    else {
        return Ok(());
    };
    let (b_type, version) = content_type(state, tx, to).await?;
    let Some(back_field) = b_type.fields().iter().find(|field| field.id == back) else {
        return Ok(());
    };
    let FieldKind::Reference {
        to: mirror_to,
        many,
        ..
    } = &back_field.kind
    else {
        return Ok(());
    };
    if mirror_to != mine {
        return Ok(());
    }
    let mut document: Value = serde_json::from_str(&draft.content).map_err(ApiError::internal)?;
    let id = entry.to_string();
    if *many {
        let mut list: Vec<String> = document
            .get(back)
            .and_then(Value::as_array)
            .map(|ids| {
                ids.iter()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        if add {
            if list.contains(&id) {
                return Ok(());
            }
            list.push(id);
        } else {
            if list.iter().all(|seen| *seen != id) {
                return Ok(());
            }
            list.retain(|seen| *seen != id);
        }
        // A `many` field takes one id or more, never `[]`: emptied mirrors drop the key instead.
        if list.is_empty() {
            if let Some(object) = document.as_object_mut() {
                object.remove(back);
            }
        } else {
            document[back] = serde_json::json!(list);
        }
    } else if add {
        if document.get(back).and_then(Value::as_str) == Some(id.as_str()) {
            return Ok(());
        }
        document[back] = Value::String(id);
    } else {
        if document.get(back).and_then(Value::as_str) != Some(id.as_str()) {
            return Ok(());
        }
        if let Some(object) = document.as_object_mut() {
            object.remove(back);
        }
    }
    // Revalidated against the target's own current type: a value it won't take is skipped.
    let Ok(content) = b_type.validate(&document) else {
        return Ok(());
    };
    let hash = sha256(content.canonical_json().as_bytes());
    let unique = b_type.unique_fields();
    let revision = new_revision(&content, &hash, version, &unique);
    let _ = entries::save(tx, other, draft.revision.id, revision)
        .await
        .map_err(ApiError::from_db)?;
    Ok(())
}

/// Mirrors `entry`'s `inverse`-declaring reference fields onto their targets (D-085): targets
/// it gained get `entry` written into the back-field, targets it lost have it removed. `before`
/// is the outgoing canonical document (`None` on create), `after` the new one (`None` on
/// delete). Same transaction, so a link and its mirror commit or roll back together.
async fn sync_inverses(
    state: &AppState,
    tx: &mut ProjectTransaction,
    content_type: &ContentType,
    collection: &str,
    entry: Uuid,
    before: Option<&str>,
    after: Option<&str>,
) -> Result<(), ApiError> {
    for field in content_type.fields() {
        let FieldKind::Reference {
            to,
            inverse: Some(back),
            ..
        } = &field.kind
        else {
            continue;
        };
        let old_ids = reference_ids(before, &field.id);
        let new_ids = reference_ids(after, &field.id);
        let mirror = Mirror {
            to,
            back,
            mine: collection,
        };
        for other in new_ids.iter().filter(|id| !old_ids.contains(id)) {
            if let Ok(other) = Uuid::try_parse(other) {
                write_back(state, tx, mirror, other, entry, true).await?;
            }
        }
        for other in old_ids.iter().filter(|id| !new_ids.contains(id)) {
            if let Ok(other) = Uuid::try_parse(other) {
                write_back(state, tx, mirror, other, entry, false).await?;
            }
        }
    }
    Ok(())
}

/// Records the answer, commits, and only then responds: an acknowledged write is durable.
async fn finish(
    mut tx: ProjectTransaction,
    operation: &str,
    key: &str,
    status: StatusCode,
    mut view: DraftView,
) -> Result<Response, ApiError> {
    let revision = view.revision.id;
    // Every draft answer carries its localization group (D-087); unlocalized entries answer `[]`.
    view.localizations = entries::localizations(&mut tx, &view.collection, view.id)
        .await
        .map_err(ApiError::from_db)?
        .into_iter()
        .map(|sibling| LocalizationView {
            id: sibling.id,
            locale: sibling.locale,
        })
        .collect();
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
    Query(query): Query<LocaleQuery>,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Response, ApiError> {
    let key = idempotency_key(&headers)?;
    let Json(value) = body?;
    // `?locale=` names the entry's locale; an empty value leaves it unlocalized, like omitting
    // the parameter.
    let locale = query.locale.as_deref().filter(|locale| !locale.is_empty());
    if let Some(locale) = locale {
        check_locale(locale)?;
    }
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
    let (content_type, version) = content_type(&state, &mut tx, &path.collection).await?;
    // A new entry gets the defaults of the fields it leaves out (D-071). They are the type's,
    // not the caller's, so a role's field list applies to what it sent.
    let (value, defaulted) = content_type.with_defaults(&value);
    let (content, hash) = validate(&value, &content_type)?;
    visibility::only_permitted_fields(content.canonical_json(), &create, &defaulted)?;
    let request = request_hash(&[
        CREATE,
        &path.collection,
        locale.unwrap_or(""),
        content.canonical_json(),
    ]);
    if let Some(replay) = claim(&mut tx, CREATE, key, &request).await? {
        return Ok(replay);
    }
    slug_free(&mut tx, &path.collection, &content_type, &content, None).await?;
    let unique = content_type.unique_fields();
    unique_free(
        &mut tx,
        &path.collection,
        &content_type,
        &unique,
        &content,
        None,
    )
    .await?;
    let revision = new_revision(&content, &hash, version, &unique);
    let draft = entries::create(&mut tx, Uuid::now_v7(), &path.collection, revision, locale)
        .await
        .map_err(ApiError::from_db)?;
    sync_inverses(
        &state,
        &mut tx,
        &content_type,
        &path.collection,
        draft.entry_id,
        None,
        Some(&draft.content),
    )
    .await?;
    let access = Access::of(&tx.policy, tx.identity, &path.collection, draft.created_by);
    let view = DraftView::shown(draft, Publication::NotLookedUp, &access, &content_type)?;
    finish(tx, CREATE, key, StatusCode::CREATED, view).await
}

/// `GET {P}/collections/{collection}/export`: every draft's canonical content as a JSONL
/// line — what `import` takes back (D-086). The file has to hold every field to import back,
/// so the role must read them all, of everyone's entries or of its own: a partial read would
/// write a file that can't come back whole, so it is refused rather than clipped.
pub(crate) async fn export(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<CollectionPath>,
) -> Result<Response, ApiError> {
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    // An unknown collection is 404, not an empty export.
    content_type(&state, &mut tx, &path.collection).await?;
    let by = if tx
        .policy
        .fields(Action::ContentRead, &path.collection, false)
        == Some(Fields::All)
    {
        None
    } else if tx
        .policy
        .fields(Action::ContentRead, &path.collection, true)
        == Some(Fields::All)
    {
        Some(tx.identity)
    } else {
        return Err(ApiError::forbidden(
            "fields_not_readable",
            "an export holds every field, so it needs a role that reads them all",
        ));
    };
    let lines = entries::export(&mut tx, &path.collection, by)
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

/// `POST {P}/collections/{collection}/import`: one JSON object per line, or one JSON array
/// of the same objects, each an entry's content — and a Strapi export entity
/// (`{"type":"api::…","data":{…}}`) or a `{"version":…,"data":{…}}` marker hands validation
/// its `data` (D-086). The file lands in one transaction or nothing does (`Idempotency-Key`
/// required; at most `IMPORT_MAX_ITEMS` items). References resolve against entries that
/// already exist, including earlier items, and mirrors update as on any create (D-085).
pub(crate) async fn import(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<CollectionPath>,
    headers: HeaderMap,
    body: String,
) -> Result<Response, ApiError> {
    let key = idempotency_key(&headers)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    // Imported entries are their importer's own, like a create's are its creator's.
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
    let (content_type, version) = content_type(&state, &mut tx, &path.collection).await?;
    let request = request_hash(&[IMPORT, &path.collection, &body]);
    if let Some(replay) = claim(&mut tx, IMPORT, key, &request).await? {
        return Ok(replay);
    }
    let ImportItems {
        items,
        at: at_index,
    } = import_items(&body)?;
    if items.len() > IMPORT_MAX_ITEMS {
        return Err(ApiError::Invalid(Refusal {
            code: "too_many_items",
            pointer: "/".to_owned(),
            message: format!("an import is at most {IMPORT_MAX_ITEMS} items"),
        }));
    }
    // A single type holds one entry: refuse the file before writing any of it.
    if content_type.single()
        && (items.len() > 1
            || !entries::list(&mut tx, &path.collection, None, 1, None, None, None)
                .await
                .map_err(ApiError::from_db)?
                .is_empty())
    {
        return Err(ApiError::conflict(
            "single_exists",
            "this collection is a single type and already has its entry; edit that one",
        ));
    }
    for (index, value) in items.iter().enumerate() {
        let at = at_index(index);
        // Left-out fields get the type's defaults, like a create (D-071).
        let (value, defaulted) = content_type.with_defaults(entity(value));
        let (content, hash) =
            validate(&value, &content_type).map_err(|error| on_index(error, &at))?;
        visibility::only_permitted_fields(content.canonical_json(), &create, &defaulted)
            .map_err(|error| on_index(error, &at))?;
        slug_free(&mut tx, &path.collection, &content_type, &content, None)
            .await
            .map_err(|error| on_index(error, &at))?;
        let unique = content_type.unique_fields();
        unique_free(
            &mut tx,
            &path.collection,
            &content_type,
            &unique,
            &content,
            None,
        )
        .await
        .map_err(|error| on_index(error, &at))?;
        let draft = entries::create(
            &mut tx,
            Uuid::now_v7(),
            &path.collection,
            new_revision(&content, &hash, version, &unique),
            // An imported entry lands unlocalized; its locale is set on it afterwards.
            None,
        )
        .await
        .map_err(|error| on_index(ApiError::from_db(error), &at))?;
        sync_inverses(
            &state,
            &mut tx,
            &content_type,
            &path.collection,
            draft.entry_id,
            None,
            Some(&draft.content),
        )
        .await?;
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

/// `POST {P}/collections/{collection}/entries/{entry}/localizations?locale=` (D-087): a
/// sibling entry holding a copy of the source's draft, in one group with it. Creating it is a
/// create — `content.create` gates it and the copied document goes through the same
/// validation, field lists, slug and uniqueness checks as any new entry — plus its own rules:
/// one entry per locale per group, and a group member's locale can't be taken by another.
pub(crate) async fn localize(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<EntryPath>,
    Query(query): Query<LocaleQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let key = idempotency_key(&headers)?;
    let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
    let locale = query.locale.as_deref().ok_or(ApiError::InvalidField {
        reason: "missing_locale",
        pointer: "/locale",
        message: "name the locale to add, like ?locale=fr".to_owned(),
    })?;
    check_locale(locale)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
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
    let (content_type, version) = content_type(&state, &mut tx, &path.collection).await?;
    let request = request_hash(&[LOCALIZE, &path.collection, &entry.to_string(), locale]);
    if let Some(replay) = claim(&mut tx, LOCALIZE, key, &request).await? {
        return Ok(replay);
    }
    let source = entries::source_for_localization(&mut tx, &path.collection, entry)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    // A group member keeps a locale (the table's CHECK); an untagged source can't join one, so
    // its first version is asked to name its own locale first — the editor's locale field does.
    if source.locale.is_none() {
        return Err(ApiError::conflict(
            "locale_required",
            "give this entry a locale before adding versions",
        ));
    }
    // The sibling starts as the source's draft, with a slug of its own where the type has one:
    // the copied one would fail `entries_draft_slug` the moment it was stored.
    let mut value: Value = serde_json::from_str(&source.content).map_err(ApiError::internal)?;
    derive_slug(&mut tx, &path.collection, &content_type, &mut value, locale).await?;
    let (value, defaulted) = content_type.with_defaults(&value);
    let (content, hash) = validate(&value, &content_type)?;
    visibility::only_permitted_fields(content.canonical_json(), &create, &defaulted)?;
    let unique = content_type.unique_fields();
    unique_free(
        &mut tx,
        &path.collection,
        &content_type,
        &unique,
        &content,
        None,
    )
    .await?;
    let revision = new_revision(&content, &hash, version, &unique);
    match entries::create_localization(
        &mut tx,
        Uuid::now_v7(),
        entry,
        &path.collection,
        locale,
        revision,
    )
    .await
    .map_err(ApiError::from_db)?
    {
        entries::Localized::Made(draft) => {
            // The copy's references count like a create's: their mirrors point back at the
            // sibling (D-085).
            sync_inverses(
                &state,
                &mut tx,
                &content_type,
                &path.collection,
                draft.entry_id,
                None,
                Some(&draft.content),
            )
            .await?;
            let access = Access::of(&tx.policy, tx.identity, &path.collection, draft.created_by);
            let view = DraftView::shown(*draft, Publication::NotLookedUp, &access, &content_type)?;
            finish(tx, LOCALIZE, key, StatusCode::CREATED, view).await
        }
        entries::Localized::Occupied(_) => Err(ApiError::conflict(
            "locale_taken",
            "a version in that locale already exists",
        )),
        entries::Localized::Missing => Err(ApiError::NotFound),
    }
}

/// Gives a copied document a slug of its own: `{slug}-{locale}`, numbered past it when that is
/// taken — at most ten tries before the write is refused. A document whose type has no slug
/// field, or whose slug is empty, copies verbatim.
async fn derive_slug(
    tx: &mut ProjectTransaction,
    collection: &str,
    content_type: &ContentType,
    value: &mut Value,
    locale: &str,
) -> Result<(), ApiError> {
    let Some(field) = content_type
        .fields()
        .iter()
        .find(|field| matches!(field.kind, FieldKind::Slug { .. }))
    else {
        return Ok(());
    };
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    let Some(base) = object.get(&field.id).and_then(Value::as_str) else {
        return Ok(());
    };
    if base.is_empty() {
        return Ok(());
    }
    for suffix in 1..=10 {
        let candidate = if suffix == 1 {
            format!("{base}-{locale}")
        } else {
            format!("{base}-{locale}-{suffix}")
        };
        if entries::slug_holder(tx, collection, &candidate, None)
            .await
            .map_err(ApiError::from_db)?
            .is_none()
        {
            object.insert(field.id.clone(), Value::String(candidate));
            return Ok(());
        }
    }
    Err(ApiError::conflict(
        "slug_taken",
        "no free slug for a version in that locale; rename a sibling's slug first",
    ))
}

/// An import body as items, with `at` mapping an index to its pointer prefix — `/line/3` for
/// JSONL, `/2` for a JSON array — so a refusal names the item it came from.
struct ImportItems {
    items: Vec<Value>,
    at: fn(usize) -> String,
}

/// One JSON object per line, or one JSON array of the same objects (`[{...},{...}]`).
fn import_items(body: &str) -> Result<ImportItems, ApiError> {
    let trimmed = body.trim();
    if trimmed.starts_with('[') {
        let items = serde_json::from_str::<Vec<Value>>(trimmed).map_err(|_| {
            ApiError::Invalid(Refusal {
                code: "invalid_json",
                pointer: "/".to_owned(),
                message: "the body isn't a JSON array".to_owned(),
            })
        })?;
        return Ok(ImportItems {
            items,
            at: |index| format!("/{index}"),
        });
    }
    let items = trimmed
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
        .collect::<Result<_, ApiError>>()?;
    Ok(ImportItems {
        items,
        at: |index| format!("/line/{}", index + 1),
    })
}

/// A Strapi export entity — `{"type":"api::x.x","data":{…}}` — or a `{"version":2,"data":{…}}`
/// marker: the payload sits under `data`, so unwrap it. Content that merely has fields named
/// `type` or `data` is left alone: only a `::`-marked type or an explicit `version` counts.
fn entity(value: &Value) -> &Value {
    let Some(object) = value.as_object() else {
        return value;
    };
    let Some(data) = object.get("data") else {
        return value;
    };
    let marked = object
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind.contains("::"))
        || object.contains_key("version");
    if marked { data } else { value }
}

/// Prefixes a refusal's pointer with its import item — `/line/3` for JSONL, `/2` for a JSON
/// array — so an error names the item it came from.
fn on_index(error: ApiError, prefix: &str) -> ApiError {
    match error {
        ApiError::Invalid(mut refusal) => {
            refusal.pointer = format!("{prefix}{}", refusal.pointer);
            ApiError::Invalid(refusal)
        }
        ApiError::Taken(mut in_the_way) => {
            in_the_way.pointer = format!("{prefix}{}", in_the_way.pointer);
            ApiError::Taken(in_the_way)
        }
        other => other,
    }
}

/// What a save's preamble established (D-058, step R.3).
struct PreparedSave {
    /// What the caller may do with this entry, by whose it is.
    access: Access,
    /// The collection's current content type and its version.
    content_type: Arc<ContentType>,
    version: i32,
    /// The validated new document and its hash.
    content: Content,
    hash: [u8; 32],
    /// The document `If-Match` named — the mirror diff's "before" and the base hidden fields
    /// are restored from.
    stored: Option<String>,
}

/// The checks a save runs before its revision is built: the role may change this entry at all,
/// the type is locked and loaded, the stored revision the editor started from is restored into
/// the hidden fields, and the document validates under the caller's update grant.
async fn prepare_save(
    state: &AppState,
    tx: &mut ProjectTransaction,
    path: &EntryPath,
    entry: Uuid,
    expected: Uuid,
    value: Value,
) -> Result<PreparedSave, ApiError> {
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
    content_types::lock_for_write(tx, &path.collection)
        .await
        .map_err(ApiError::from_db)?;
    let (content_type, version) = content_type(state, tx, &path.collection).await?;
    // What this caller may do with this entry, by whose it is (D-058, step R.3).
    let created_by = entries::creators(tx, &[entry])
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
    let stored = entries::revision(tx, &path.collection, entry, expected)
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
    Ok(PreparedSave {
        access,
        content_type,
        version,
        content,
        hash,
        stored,
    })
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
    let expected = if_match(&headers)?;
    let key = idempotency_key(&headers)?;
    let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
    let Json(value) = body?;
    // `?locale=` rides the same write: a tag names the entry's locale, an empty value clears it
    // to the project's default (D-087). An absent parameter leaves the locale alone.
    let set = query
        .locale
        .as_deref()
        .map(|locale| (!locale.is_empty()).then_some(locale));
    if let Some(Some(locale)) = set {
        check_locale(locale)?;
    }
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let prepared = prepare_save(&state, &mut tx, &path, entry, expected, value).await?;
    let request = request_hash(&[
        SAVE,
        &path.collection,
        &entry.to_string(),
        &expected.to_string(),
        // Absent and clear must hash differently: they are different requests.
        set.map_or("", |locale| locale.unwrap_or("\u{0}")),
        prepared.content.canonical_json(),
    ]);
    if let Some(replay) = claim(&mut tx, SAVE, key, &request).await? {
        return Ok(replay);
    }
    slug_free(
        &mut tx,
        &path.collection,
        &prepared.content_type,
        &prepared.content,
        Some(entry),
    )
    .await?;
    let unique = prepared.content_type.unique_fields();
    unique_free(
        &mut tx,
        &path.collection,
        &prepared.content_type,
        &unique,
        &prepared.content,
        Some(entry),
    )
    .await?;
    let revision = new_revision(&prepared.content, &prepared.hash, prepared.version, &unique);
    // A refusal returns early, dropping the transaction: the claim and anything else roll back.
    match entries::save(&mut tx, entry, expected, revision)
        .await
        .map_err(ApiError::from_db)?
    {
        Saved::Saved(draft) => {
            // The mirrors move with the diff: `stored` is the outgoing revision's document,
            // which `expected` named and the save just confirmed was current.
            sync_inverses(
                &state,
                &mut tx,
                &prepared.content_type,
                &path.collection,
                draft.entry_id,
                prepared.stored.as_deref(),
                Some(&draft.content),
            )
            .await?;
            let mut draft = *draft;
            // The save and the locale change commit or fail together (D-087).
            if let Some(locale) = set {
                apply_locale(&mut tx, &path.collection, entry, locale).await?;
                draft.locale = locale.map(str::to_owned);
            }
            let view = DraftView::shown(
                draft,
                Publication::NotLookedUp,
                &prepared.access,
                &prepared.content_type,
            )?;
            finish(tx, SAVE, key, StatusCode::OK, view).await
        }
        Saved::Stale { current } => Err(ApiError::StaleRevision { current }),
        Saved::NotFound => Err(ApiError::NotFound),
    }
}

/// What a deletion answers (D-080).
#[derive(Serialize)]
struct DeletedView {
    id: Uuid,
    collection: String,
    /// The draft it had: the revision the caller confirmed with `If-Match`.
    revision_id: Uuid,
    deleted_at: String,
    /// Who deleted it: the caller, a person or an API token's own identity (D-063).
    deleted_by: Uuid,
}

/// `DELETE {P}/collections/{collection}/entries/{entry}` (D-080): the entry becomes a tombstone
/// that no read, list, history or save finds again; its revisions and releases stay. Needs
/// `content.delete` on this entry (anyone's, or the caller's own), the draft's `ETag` in
/// `If-Match`, and an `Idempotency-Key`. Refused with 409 while it is published
/// (`entry_published`), while other live entries name it (`still_referenced`), while a changeset
/// that hasn't been published pins it (`entry_in_changeset`), or when the draft moved on
/// (`stale_revision`).
pub(crate) async fn delete(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<EntryPath>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let expected = if_match_or(
        &headers,
        "deleting needs an If-Match header with the draft's ETag, so you delete what you looked at",
    )?;
    let key = idempotency_key(&headers)?;
    let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    if [false, true].into_iter().all(|own| {
        tx.policy
            .fields(Action::ContentDelete, &path.collection, own)
            .is_none()
    }) {
        return Err(ApiError::missing(
            Action::ContentDelete,
            "your role can't delete entries here",
        ));
    }
    // Claimed before the entry is looked up: a retry after a lost answer finds it deleted, and
    // must get the recorded answer, not a 404.
    let request = request_hash(&[
        DELETE,
        &path.collection,
        &entry.to_string(),
        &expected.to_string(),
    ]);
    if let Some(replay) = claim(&mut tx, DELETE, key, &request).await? {
        return Ok(replay);
    }
    // What this caller may do with this entry, by whose it is (D-058, step R.3). One it can't
    // read is not found.
    let created_by = entries::creators(&mut tx, &[entry])
        .await
        .map_err(ApiError::from_db)?
        .into_iter()
        .find(|(_, collection, _)| *collection == path.collection)
        .map(|(_, _, created_by)| created_by)
        .ok_or(ApiError::NotFound)?;
    let own = created_by == tx.identity;
    if tx
        .policy
        .fields(Action::ContentRead, &path.collection, own)
        .is_none()
    {
        return Err(ApiError::NotFound);
    }
    if tx
        .policy
        .fields(Action::ContentDelete, &path.collection, own)
        .is_none()
    {
        return Err(ApiError::missing(
            Action::ContentDelete,
            "your role can't delete this entry",
        ));
    }
    // Its mirrors lose it first: the back-fields drop this entry before the tombstone's
    // reference check runs, in the same transaction (D-085).
    if let Some(draft) = entries::get(&mut tx, &path.collection, entry)
        .await
        .map_err(ApiError::from_db)?
    {
        let (definition, _) = content_type(&state, &mut tx, &path.collection).await?;
        sync_inverses(
            &state,
            &mut tx,
            &definition,
            &path.collection,
            entry,
            Some(&draft.content),
            None,
        )
        .await?;
    }
    // A refusal returns early, dropping the transaction: the claim and the lock roll back.
    match entries::delete(&mut tx, &path.collection, entry, expected)
        .await
        .map_err(ApiError::from_db)?
    {
        Deleted::Deleted { revision, at } => {
            let view = DeletedView {
                id: entry,
                collection: path.collection,
                revision_id: revision,
                deleted_at: rfc3339(at)?,
                deleted_by: tx.identity,
            };
            let body = serde_json::to_string(&Data { data: view }).map_err(ApiError::internal)?;
            idempotency::complete(&mut tx, DELETE, key, StatusCode::OK.as_u16(), &body)
                .await
                .map_err(ApiError::from_db)?;
            fault::point("entry.delete:before-commit");
            tx.commit().await.map_err(ApiError::from_db)?;
            fault::point("entry.delete:after-commit");
            Ok(json(StatusCode::OK, body, None, false))
        }
        refused => Err(deletion_refused(&tx.policy, refused)),
    }
}

/// Why a deletion was refused, as the API answers it (D-080).
fn deletion_refused(policy: &Policy, refused: Deleted) -> ApiError {
    match refused {
        Deleted::Deleted { .. } => ApiError::internal(std::io::Error::other(
            "a deletion that succeeded was handled as a refusal",
        )),
        Deleted::NotFound => ApiError::NotFound,
        Deleted::Stale { current } => ApiError::StaleRevision { current },
        Deleted::Published => ApiError::conflict(
            "entry_published",
            "this entry is published, so readers still get it: take it down first (unpublish it in a changeset), then delete it",
        ),
        Deleted::Referenced(referrers) => {
            // Only referrers in collections the role reads, as a read's `referenced_by` (D-065).
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
                message: "other entries still link to this one: remove those links first, then delete it".to_owned(),
                referenced_by,
                changesets: Vec::new(),
            }))
        }
        Deleted::InChangeset(pending) => ApiError::StillNeeded(Box::new(StillNeeded {
            reason: "entry_in_changeset",
            message: "a changeset that hasn't been published includes this entry, and publishing it would bring the entry back: it can be deleted once that changeset is published".to_owned(),
            referenced_by: Vec::new(),
            changesets: pending
                .into_iter()
                .map(|pending| PendingChangeset {
                    id: pending.id,
                    title: pending.title,
                    scheduled: pending.scheduled,
                })
                .collect(),
        })),
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
    let (content_type, _) = content_type(&state, &mut tx, &path.collection).await?;
    let access = Access::of(&tx.policy, tx.identity, &path.collection, draft.created_by);
    // Only referrers in collections the role reads, so the list shows nothing it couldn't.
    let referenced_by = entries::referrers(&mut tx, entry)
        .await
        .map_err(ApiError::from_db)?
        .into_iter()
        .filter(|referrer| {
            tx.policy
                .fields(Action::ContentRead, &referrer.collection, false)
                .is_some()
        })
        .map(|referrer| ReferrerView {
            entry_id: referrer.entry_id,
            collection: referrer.collection,
            field: referrer.field,
            in_draft: referrer.in_draft,
            published: referrer.published,
        })
        .collect();
    // Its localization group, the entry itself included (D-087).
    let localizations = entries::localizations(&mut tx, &path.collection, entry)
        .await
        .map_err(ApiError::from_db)?
        .into_iter()
        .map(|sibling| LocalizationView {
            id: sibling.id,
            locale: sibling.locale,
        })
        .collect();
    tx.commit().await.map_err(ApiError::from_db)?;
    let revision = draft.revision.id;
    let mut view = DraftView::shown(draft, published.into(), &access, &content_type)?;
    view.referenced_by = Some(referenced_by);
    view.localizations = localizations;
    let body = serde_json::to_string(&Data { data: view }).map_err(ApiError::internal)?;
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

/// A list's filters, checked.
struct ListArgs<'a> {
    /// Page size.
    limit: i64,
    /// Title search.
    search: Option<&'a str>,
    /// Whose entries were asked for.
    asked: Option<Uuid>,
    /// The locale to filter on.
    locale: Option<&'a str>,
}

/// A list's page size, title search, creator and locale, checked.
fn list_query(query: &ListQuery) -> Result<ListArgs<'_>, ApiError> {
    let limit = query.limit.unwrap_or(DEFAULT_PAGE);
    if !(1..=entries::MAX_PAGE).contains(&limit) {
        return Err(ApiError::InvalidHeader("limit must be between 1 and 100"));
    }
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
    let asked = query
        .created_by
        .as_deref()
        .map(|text| {
            Uuid::try_parse(text)
                .map_err(|_| ApiError::InvalidHeader("created_by must be an identity id"))
        })
        .transpose()?;
    let locale = query
        .locale
        .as_deref()
        .map(|locale| check_locale(locale).map(|()| locale))
        .transpose()?;
    Ok(ListArgs {
        limit,
        search,
        asked,
        locale,
    })
}

/// Whose entries a list may hold.
enum Whose {
    /// Everyone's the role reads.
    Anyone,
    /// Only those this identity created.
    Only(Uuid),
    /// None: the answer is an empty page.
    Nobody,
}

/// Whose entries a list holds, from what the role reads (D-058): only the caller's own when that
/// is all it reads (step R.3); when it searches titles, only where it reads them, or else which
/// entries match would confirm a guess at a title the answer leaves blank (refused outright when
/// it reads no title at all); and, when one creator is asked for, theirs within that.
fn whose(
    policy: &Policy,
    identity: Uuid,
    collection: &str,
    title_field: Option<&str>,
    searching: bool,
    asked: Option<Uuid>,
) -> Result<Whose, ApiError> {
    let reads_title = |own: bool| {
        title_field.is_none_or(|title| {
            policy
                .fields(Action::ContentRead, collection, own)
                .is_some_and(|fields| fields.includes(title))
        })
    };
    let mut only = policy
        .fields(Action::ContentRead, collection, false)
        .is_none()
        .then_some(identity);
    if searching && !reads_title(false) {
        if reads_title(true) {
            only = Some(identity);
        } else {
            return Err(ApiError::forbidden(
                "titles_not_readable",
                "your role can't read titles here, so it can't search them",
            ));
        }
    }
    Ok(match (only, asked) {
        (Some(mine), Some(asked)) if mine != asked => Whose::Nobody,
        (Some(mine), _) => Whose::Only(mine),
        (None, Some(asked)) => Whose::Only(asked),
        (None, None) => Whose::Anyone,
    })
}

/// `GET {P}/collections/{collection}/entries?limit=&cursor=&q=&created_by=`.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<CollectionPath>,
    Query(query): Query<ListQuery>,
) -> Result<Response, ApiError> {
    let ListArgs {
        limit,
        search,
        asked,
        locale,
    } = list_query(&query)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_read(&tx, &path.collection)?;
    // An unknown collection is 404, not an empty list.
    let (content_type, _) = content_type(&state, &mut tx, &path.collection).await?;
    let project = tx.project;
    let before = query
        .cursor
        .as_deref()
        .map(|cursor| after_cursor(&path, project, cursor))
        .transpose()?;
    // Titles only where the role may read the title field (D-058, step R.3).
    let anyone = tx
        .policy
        .fields(Action::ContentRead, &path.collection, false);
    let title_field = match anyone {
        Some(Fields::All) => None,
        _ => Some(content_type.title_field()),
    };
    let listed = whose(
        &tx.policy,
        tx.identity,
        &path.collection,
        title_field,
        search.is_some(),
        asked,
    )?;
    let by = match listed {
        Whose::Anyone => None,
        Whose::Only(creator) => Some(creator),
        Whose::Nobody => {
            tx.commit().await.map_err(ApiError::from_db)?;
            let empty = Page::<SummaryView> {
                data: Vec::new(),
                next_cursor: None,
            };
            let body = serde_json::to_string(&empty).map_err(ApiError::internal)?;
            return Ok(json(StatusCode::OK, body, None, false));
        }
    };
    let mut rows: Vec<Summary> = entries::list(
        &mut tx,
        &path.collection,
        before,
        limit + 1,
        search,
        by,
        locale,
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
    may_read(&tx, &path.collection)?;
    let (read, content_type) = readable(&state, &mut tx, &path.collection, entry).await?;
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
    let (read, content_type) = readable(&state, &mut tx, &path.collection, entry).await?;
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

    #[test]
    fn a_strapi_entity_unwraps_to_its_data_and_plain_content_passes_through() {
        let bare = serde_json::json!({"title": "Hi", "data": "a field literally called data"});
        assert_eq!(entity(&bare), &bare);
        let strapi = serde_json::json!({"type": "api::article.article", "data": {"title": "Hi"}});
        assert_eq!(entity(&strapi), &serde_json::json!({"title": "Hi"}));
        let marker = serde_json::json!({"version": 2, "data": {"title": "Hi"}});
        assert_eq!(entity(&marker), &serde_json::json!({"title": "Hi"}));
        // A `type` that isn't a Strapi namespace doesn't mark anything.
        let unmarked = serde_json::json!({"type": "note", "data": {"title": "Hi"}});
        assert_eq!(entity(&unmarked), &unmarked);
    }
}
