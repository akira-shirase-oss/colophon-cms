//! Content types (docs/modules/04-api.md, "Route families"; D-040). Under
//! `{P}` = `/api/v1/projects/{project}/environments/{environment}`:
//!
//! - `GET  {P}/content-types`: the current version of each;
//! - `POST {P}/content-types`: `{"slug", "definition"}` (`Idempotency-Key` required);
//! - `GET  {P}/content-types/{slug}`: the current version, with `ETag: "v<version>"`;
//! - `PUT  {P}/content-types/{slug}`: a new definition (`If-Match` and `Idempotency-Key`
//!   required), allowed only if every stored entry stays valid; anything else is refused with
//!   `needs_migration`.
//!
//! Any member may read them; a role with `schema.change` may change them (the built-in owners
//! and developers, and any role an owner gives it, D-058). The slugs in [`RESERVED_SLUGS`] name
//! Studio pages beside a project's collections, so no content type may take them (D-042).

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
};
use colophon_db::{
    content_types::{self, MadeSingle, Updated, Version},
    idempotency::{self, Claim},
    scope::ProjectTransaction,
};
use colophon_domain::{
    project::Slug,
    schema::{ContentType, Refusal, Successor},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, value::RawValue};
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    entries::{content_hash, idempotency_key, json, open, request_hash, rfc3339, sha256},
    error::{EntryInTheWay, InTheWay, MAX_ENTRIES_IN_THE_WAY, Problem},
    session::SignedIn,
    shape::{Data, Page},
};

/// Studio pages that sit beside a project's collections, at `/studio/projects/{project}/{slug}`.
const RESERVED_SLUGS: [&str; 4] = ["members", "settings", "changesets", "releases"];

const CREATE: &str = "content_type.create";
const UPDATE: &str = "content_type.update";

/// The path of the collection routes.
#[derive(Deserialize)]
pub(crate) struct TypesPath {
    project: String,
    environment: String,
}

/// The path of the single-type routes.
#[derive(Deserialize)]
pub(crate) struct TypePath {
    project: String,
    environment: String,
    slug: String,
}

#[derive(Serialize)]
struct VersionView {
    slug: String,
    version: i32,
    /// The canonical definition, embedded as stored.
    definition: Box<RawValue>,
    definition_hash: String,
    created_at: String,
}

impl VersionView {
    fn new(version: Version) -> Result<Self, ApiError> {
        Ok(Self {
            slug: version.collection,
            version: version.version,
            definition: RawValue::from_string(version.definition).map_err(ApiError::internal)?,
            definition_hash: content_hash(&version.definition_hash),
            created_at: rfc3339(version.created_at)?,
        })
    }
}

/// `{"slug": ..., "definition": {...}}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewType {
    slug: String,
    definition: Value,
}

fn etag(version: i32) -> HeaderValue {
    HeaderValue::from_str(&format!("\"v{version}\"")).unwrap_or(HeaderValue::from_static("\"\""))
}

/// The version an `If-Match` names: exactly `"v<version>"`.
fn if_match(headers: &HeaderMap) -> Result<i32, ApiError> {
    let value = headers.get(header::IF_MATCH).ok_or(ApiError::PreconditionRequired(
        "changing a content type needs an If-Match header with the ETag of the version you edited",
    ))?;
    value
        .to_str()
        .ok()
        .and_then(|text| {
            text.trim()
                .strip_prefix("\"v")?
                .strip_suffix('"')?
                .parse()
                .ok()
        })
        .filter(|version| *version >= 1)
        .ok_or(ApiError::InvalidHeader(
            "If-Match must be one content type ETag, such as \"v3\"",
        ))
}

fn may_change(tx: &ProjectTransaction, collection: &str) -> Result<(), ApiError> {
    crate::members::require(
        &tx.policy,
        colophon_domain::policy::Action::SchemaChange,
        Some(collection),
        "your role can't change this content type",
    )
}

fn respond(status: StatusCode, body: String, version: i32, replayed: bool) -> Response {
    let mut response = json(status, body, None, replayed);
    response.headers_mut().insert(header::ETAG, etag(version));
    response
}

/// Claims the key, or answers with the recorded response (its `ETag` read back from it).
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
            let version = serde_json::from_str::<Value>(&response)
                .ok()
                .and_then(|body| i32::try_from(body["data"]["version"].as_i64()?).ok())
                .unwrap_or(1);
            Ok(Some(respond(status, response, version, true)))
        }
    }
}

/// Records the answer and commits before responding.
async fn finish(
    mut tx: ProjectTransaction,
    operation: &str,
    key: &str,
    status: StatusCode,
    version: Version,
) -> Result<Response, ApiError> {
    let number = version.version;
    let body = serde_json::to_string(&Data {
        data: VersionView::new(version)?,
    })
    .map_err(ApiError::internal)?;
    idempotency::complete(&mut tx, operation, key, status.as_u16(), &body)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(respond(status, body, number, false))
}

/// Revisions validated per blocking task while checking a change.
const CHECK_PAGE: i64 = 200;

/// Checks that `next` accepts every draft and published revision of `collection`, under the
/// change lock. Refuses with the first [`MAX_ENTRIES_IN_THE_WAY`] that don't fit and how many
/// don't, or, past [`content_types::MAX_CHECKED`], without looking (D-055).
async fn check_stored(
    tx: &mut ProjectTransaction,
    collection: &str,
    next: Arc<ContentType>,
    refusal: Refusal,
) -> Result<(), ApiError> {
    let count = content_types::stored_count(tx, collection)
        .await
        .map_err(ApiError::from_db)?;
    if count > content_types::MAX_CHECKED {
        return Err(ApiError::Invalid(Refusal {
            code: "too_many_to_check",
            message: format!(
                "{}: checking it against {count} drafts and publications needs a migration (the limit is {})",
                refusal.message,
                content_types::MAX_CHECKED
            ),
            ..refusal
        }));
    }
    let mut entries = Vec::new();
    let mut total = 0_u64;
    for published in [false, true] {
        let mut after = Uuid::nil();
        loop {
            let page = content_types::stored_page(tx, collection, published, after, CHECK_PAGE)
                .await
                .map_err(ApiError::from_db)?;
            let Some(last) = page.last() else { break };
            after = last.entry_id;
            let more = i64::try_from(page.len()).is_ok_and(|len| len == CHECK_PAGE);
            // Validation is CPU work: up to a page of documents, off the async workers.
            let next = Arc::clone(&next);
            let misfits = tokio::task::spawn_blocking(move || {
                page.into_iter()
                    .filter_map(|stored| {
                        let problem = next.accepts(&stored.content).err()?;
                        Some((stored, problem))
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .map_err(ApiError::internal)?;
            for (stored, problem) in misfits {
                total += 1;
                if entries.len() < MAX_ENTRIES_IN_THE_WAY {
                    entries.push(EntryInTheWay {
                        entry: stored.entry_id,
                        title: stored.title,
                        state: if stored.published {
                            "published"
                        } else {
                            "draft"
                        },
                        problem: Some(Problem {
                            reason: problem.code,
                            pointer: problem.pointer,
                            message: problem.message,
                        }),
                    });
                }
            }
            if !more {
                break;
            }
        }
    }
    if total == 0 {
        return Ok(());
    }
    let noun = if total == 1 {
        "entry is"
    } else {
        "entries are"
    };
    Err(ApiError::NeedsMigration(Box::new(InTheWay {
        reason: refusal.code,
        message: format!(
            "{}: {total} {noun} in the way; change them first, or keep the old definition",
            refusal.message
        ),
        pointer: refusal.pointer,
        entries,
        total: Some(total),
    })))
}

/// `GET {P}/content-types`.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<TypesPath>,
) -> Result<Response, ApiError> {
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let versions = content_types::list(&mut tx)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let data = versions
        .into_iter()
        .map(VersionView::new)
        .collect::<Result<Vec<_>, ApiError>>()?;
    let body = serde_json::to_string(&Page {
        data,
        next_cursor: None,
    })
    .map_err(ApiError::internal)?;
    Ok(json(StatusCode::OK, body, None, false))
}

/// `GET {P}/content-types/{slug}`.
pub(crate) async fn get(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<TypePath>,
) -> Result<Response, ApiError> {
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let version = content_types::current(&mut tx, &path.slug)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let number = version.version;
    let body = serde_json::to_string(&Data {
        data: VersionView::new(version)?,
    })
    .map_err(ApiError::internal)?;
    Ok(respond(StatusCode::OK, body, number, false))
}

/// `POST {P}/content-types`.
pub(crate) async fn create(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<TypesPath>,
    headers: HeaderMap,
    body: Result<Json<NewType>, JsonRejection>,
) -> Result<Response, ApiError> {
    let key = idempotency_key(&headers)?;
    let Json(body) = body?;
    let slug = Slug::parse(&body.slug).map_err(|error| ApiError::InvalidField {
        reason: error.code(),
        pointer: "/slug",
        message: error.to_string(),
    })?;
    if RESERVED_SLUGS.contains(&slug.as_str()) {
        return Err(ApiError::InvalidField {
            reason: "reserved_slug",
            pointer: "/slug",
            message: format!("\"{slug}\" is the name of a Studio page; choose another slug"),
        });
    }
    let definition = ContentType::parse(&body.definition).map_err(|refusal| {
        ApiError::Invalid(Refusal {
            pointer: format!("/definition{}", refusal.pointer),
            ..refusal
        })
    })?;
    let canonical = definition.canonical_json();
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_change(&tx, slug.as_str())?;
    let request = request_hash(&[CREATE, slug.as_str(), &canonical]);
    if let Some(replay) = claim(&mut tx, CREATE, key, &request).await? {
        return Ok(replay);
    }
    let version = content_types::create(
        &mut tx,
        slug.as_str(),
        &canonical,
        &sha256(canonical.as_bytes()),
    )
    .await
    .map_err(|error| match error {
        colophon_db::DbError::Conflict => ApiError::conflict(
            "content_type_exists",
            "a content type with this slug already exists",
        ),
        other => ApiError::from_db(other),
    })?;
    finish(tx, CREATE, key, StatusCode::CREATED, version).await
}

/// `PUT {P}/content-types/{slug}`.
pub(crate) async fn update(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<TypePath>,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Response, ApiError> {
    let expected = if_match(&headers)?;
    let key = idempotency_key(&headers)?;
    let Json(value) = body?;
    let next = ContentType::parse(&value).map_err(ApiError::Invalid)?;
    let canonical = next.canonical_json();
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_change(&tx, &path.slug)?;
    let request = request_hash(&[UPDATE, &path.slug, &expected.to_string(), &canonical]);
    if let Some(replay) = claim(&mut tx, UPDATE, key, &request).await? {
        return Ok(replay);
    }
    // Before reading anything: writes in the collection wait from here until this commits, and
    // this waits for the ones already under way (D-055).
    content_types::lock_for_change(&mut tx, &path.slug)
        .await
        .map_err(ApiError::from_db)?;
    let current = content_types::current(&mut tx, &path.slug)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    if current.version != expected {
        return Err(ApiError::StaleVersion {
            current: current.version,
        });
    }
    let stored: Value = serde_json::from_str(&current.definition).map_err(ApiError::internal)?;
    let previous = ContentType::parse(&stored).map_err(ApiError::internal)?;
    let single = next.single();
    match previous.successor(&next) {
        Successor::Compatible => {}
        Successor::CheckContent(refusal) => {
            check_stored(&mut tx, &path.slug, Arc::new(next), refusal).await?;
        }
        Successor::Refused(refusal) => return Err(ApiError::Invalid(refusal)),
    }
    // A single type holds at most one entry (D-067): its entries carry the flag the index checks.
    if previous.single() != single
        && let MadeSingle::TooMany(count) = content_types::set_single(&mut tx, &path.slug, single)
            .await
            .map_err(ApiError::from_db)?
    {
        return Err(ApiError::Invalid(Refusal {
            code: "too_many_entries",
            pointer: "/single".to_owned(),
            message: format!("a single type holds one entry, and this collection has {count}"),
        }));
    }
    match content_types::update(
        &mut tx,
        &path.slug,
        expected,
        &canonical,
        &sha256(canonical.as_bytes()),
    )
    .await
    .map_err(ApiError::from_db)?
    {
        Updated::Updated(version) => finish(tx, UPDATE, key, StatusCode::OK, version).await,
        Updated::Stale { current } => Err(ApiError::StaleVersion { current }),
        Updated::NotFound => Err(ApiError::NotFound),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn if_match_names_one_version() {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_MATCH, HeaderValue::from_static("\"v3\""));
        assert_eq!(if_match(&headers).ok(), Some(3));
        for bad in ["v3", "\"3\"", "\"v0\"", "\"v-1\"", "*", "\"vx\""] {
            headers.insert(header::IF_MATCH, HeaderValue::from_static(bad));
            assert!(
                matches!(if_match(&headers), Err(ApiError::InvalidHeader(_))),
                "{bad}"
            );
        }
        assert!(matches!(
            if_match(&HeaderMap::new()),
            Err(ApiError::PreconditionRequired(_))
        ));
    }
}
