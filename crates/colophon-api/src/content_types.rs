//! Content types (docs/modules/04-api.md, "Route families"; D-040). Under
//! `{P}` = `/api/v1/projects/{project}/environments/{environment}`:
//!
//! - `GET  {P}/content-types`: the current version of each;
//! - `POST {P}/content-types`: `{"slug", "definition"}` (`Idempotency-Key` required);
//! - `GET  {P}/content-types/{slug}`: the current version, with `ETag: "v<version>"`;
//! - `PUT  {P}/content-types/{slug}`: a new definition (`If-Match` and `Idempotency-Key`
//!   required), allowed only if every stored entry stays valid; anything else is refused with
//!   `needs_migration`;
//! - `DELETE {P}/content-types/{slug}`: the collection and its versions, gone for good
//!   (`If-Match` and `Idempotency-Key` required), refused while another type's field links to
//!   it, while it still has entries, and while deleted entries' revisions still pin it (D-083).
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
    components,
    content_types::{self, Claimed, MadeSingle, Removed, Updated, Version},
    idempotency::{self, Claim},
    migrations,
    scope::ProjectTransaction,
};
use colophon_domain::{
    project::Slug,
    schema::{self, Component, ContentType, FieldKind, Refusal, Successor, Unlisted},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, value::RawValue};
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    entries::{content_hash, idempotency_key, json, open, request_hash, rfc3339, sha256},
    error::{EntryInTheWay, InTheWay, MAX_ENTRIES_IN_THE_WAY, Problem, StillNeeded},
    session::SignedIn,
    shape::{Data, Page},
};

/// Studio pages that sit beside a project's collections, at `/studio/projects/{project}/{slug}`.
const RESERVED_SLUGS: [&str; 4] = ["members", "settings", "changesets", "releases"];

const CREATE: &str = "content_type.create";
const UPDATE: &str = "content_type.update";
const DELETE: &str = "content_type.delete";

/// The path of the collection routes.
#[derive(Deserialize)]
pub(crate) struct TypesPath {
    pub(crate) project: String,
    pub(crate) environment: String,
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

pub(crate) fn etag(version: i32) -> HeaderValue {
    HeaderValue::from_str(&format!("\"v{version}\"")).unwrap_or(HeaderValue::from_static("\"\""))
}

/// The version an `If-Match` names: exactly `"v<version>"`.
pub(crate) fn if_match(headers: &HeaderMap) -> Result<i32, ApiError> {
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

/// `definition` with each component it uses holding that component's current fields, the
/// components held still until the transaction ends (D-073): taken before the content type's own
/// lock, as every component lock is.
pub(crate) async fn expand(
    tx: &mut ProjectTransaction,
    definition: &Value,
) -> Result<Value, ApiError> {
    let used = schema::components_used(definition);
    if used.is_empty() {
        return Ok(definition.clone());
    }
    components::lock_for_use(tx, &used)
        .await
        .map_err(ApiError::from_db)?;
    let mut fields = std::collections::BTreeMap::new();
    for version in components::current_of(tx, &used)
        .await
        .map_err(ApiError::from_db)?
    {
        let stored: Value =
            serde_json::from_str(&version.definition).map_err(ApiError::internal)?;
        let component = Component::parse(&stored).map_err(ApiError::internal)?;
        fields.insert(version.component, component.fields_json());
    }
    schema::expand_components(definition, &fields, Unlisted::Refuse).map_err(ApiError::Invalid)
}

fn may_change(tx: &ProjectTransaction, collection: &str) -> Result<(), ApiError> {
    crate::members::require(
        &tx.policy,
        colophon_domain::policy::Action::SchemaChange,
        Some(collection),
        "your role can't change this content type",
    )
}

pub(crate) fn respond(status: StatusCode, body: String, version: i32, replayed: bool) -> Response {
    let mut response = json(status, body, None, replayed);
    response.headers_mut().insert(header::ETAG, etag(version));
    response
}

/// Claims the key, or answers with the recorded response (its `ETag` read back from it).
pub(crate) async fn claim(
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
pub(crate) async fn check_stored(
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
        content_type: None,
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
    let within = |refusal: Refusal| {
        ApiError::Invalid(Refusal {
            pointer: format!("/definition{}", refusal.pointer),
            ..refusal
        })
    };
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_change(&tx, slug.as_str())?;
    let expanded = match expand(&mut tx, &body.definition).await {
        Err(ApiError::Invalid(refusal)) => return Err(within(refusal)),
        other => other?,
    };
    let definition = ContentType::parse(&expanded).map_err(within)?;
    let canonical = definition.canonical_json();
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
    // Inside the write's transaction, so a collection may mirror its own fields.
    match check_inverses(&mut tx, slug.as_str(), &definition).await {
        Err(ApiError::Invalid(refusal)) => return Err(within(refusal)),
        other => other?,
    }
    finish(tx, CREATE, key, StatusCode::CREATED, version).await
}

/// A declared `inverse` must name a top-level reference field on the target collection whose
/// `to` points back at this one (D-085) — the mirror every entry write keeps in sync. A plain
/// `to` still tolerates a missing collection (D-065); an inverse can't be verified against
/// one, so it refuses. The target's definition is read expanded, so the mirror may come from a
/// component.
async fn check_inverses(
    tx: &mut ProjectTransaction,
    slug: &str,
    next: &ContentType,
) -> Result<(), ApiError> {
    for (index, field) in next.fields().iter().enumerate() {
        let FieldKind::Reference {
            to,
            inverse: Some(back),
            ..
        } = &field.kind
        else {
            continue;
        };
        let pointer = format!("/fields/{index}/inverse");
        let Some(target) = content_types::current(tx, to)
            .await
            .map_err(ApiError::from_db)?
        else {
            return Err(ApiError::Invalid(Refusal {
                code: "invalid_inverse",
                pointer,
                message: format!(
                    "`inverse` mirrors onto the `{to}` collection, which doesn't exist"
                ),
            }));
        };
        let stored: Value = serde_json::from_str(&target.definition).map_err(ApiError::internal)?;
        let expanded = expand(tx, &stored).await?;
        let target = ContentType::parse(&expanded).map_err(ApiError::internal)?;
        let mirrors = target.fields().iter().any(|mirror| {
            mirror.id == *back
                && matches!(
                    &mirror.kind,
                    FieldKind::Reference { to: mirror_to, .. } if mirror_to == slug
                )
        });
        if !mirrors {
            return Err(ApiError::Invalid(Refusal {
                code: "invalid_inverse",
                pointer,
                message: format!("`{to}` has no reference field `{back}` back to `{slug}`"),
            }));
        }
    }
    Ok(())
}

/// Brings the collection's unique claims in line with a change from unique fields `before` to
/// `after` (D-070), refusing it, like any change stored content is in the way of (D-055), when
/// two drafts or two publications already share a value of a field it makes unique. `ids` are
/// the new definition's field ids, in order, for the refusal's pointer.
async fn claim_unique(
    tx: &mut ProjectTransaction,
    collection: &str,
    ids: &[String],
    before: &[String],
    after: &[String],
) -> Result<(), ApiError> {
    let added: Vec<String> = after
        .iter()
        .filter(|field| !before.contains(field))
        .cloned()
        .collect();
    if !added.is_empty() {
        let count = content_types::stored_count(tx, collection)
            .await
            .map_err(ApiError::from_db)?;
        if count > content_types::MAX_CHECKED {
            return Err(ApiError::Invalid(Refusal {
                code: "too_many_to_check",
                pointer: "/fields".to_owned(),
                message: format!(
                    "making a field unique means checking {count} drafts and publications, which needs a migration (the limit is {})",
                    content_types::MAX_CHECKED
                ),
            }));
        }
    }
    let limit = i64::try_from(MAX_ENTRIES_IN_THE_WAY).unwrap_or(i64::MAX);
    let Claimed::Shared { holders, total } =
        content_types::claim_unique(tx, collection, after, &added, limit)
            .await
            .map_err(ApiError::from_db)?
    else {
        return Ok(());
    };
    let first = holders
        .first()
        .map_or_else(String::new, |holder| holder.field.clone());
    let pointer = ids.iter().position(|id| *id == first).map_or_else(
        || "/fields".to_owned(),
        |index| format!("/fields/{index}/unique"),
    );
    let entries = holders
        .into_iter()
        .map(|holder| {
            let state = if holder.published {
                "published"
            } else {
                "draft"
            };
            EntryInTheWay {
                entry: holder.entry_id,
                title: holder.title,
                state,
                problem: Some(Problem {
                    reason: "value_taken",
                    pointer: format!("/{}", holder.field),
                    message: format!("another entry's {state} has the same `{}`", holder.field),
                }),
            }
        })
        .collect();
    let total = u64::try_from(total).unwrap_or(0);
    let noun = if total == 1 {
        "entry is"
    } else {
        "entries are"
    };
    Err(ApiError::NeedsMigration(Box::new(InTheWay {
        reason: "needs_migration",
        pointer,
        message: format!(
            "making `{first}` unique: {total} {noun} in the way, sharing a value; change them first, or keep the old definition"
        ),
        entries,
        total: Some(total),
        content_type: None,
    })))
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
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_change(&tx, &path.slug)?;
    let value = expand(&mut tx, &value).await?;
    let next = ContentType::parse(&value).map_err(ApiError::Invalid)?;
    let canonical = next.canonical_json();
    let request = request_hash(&[UPDATE, &path.slug, &expected.to_string(), &canonical]);
    if let Some(replay) = claim(&mut tx, UPDATE, key, &request).await? {
        return Ok(replay);
    }
    // Before reading anything: writes in the collection wait from here until this commits, and
    // this waits for the ones already under way (D-055).
    content_types::lock_for_change(&mut tx, &path.slug)
        .await
        .map_err(ApiError::from_db)?;
    // A running migration owns the next version (D-100): it rewrites entries against it, so a
    // different definition must not land meanwhile.
    if migrations::running_for_collection(&mut tx, &path.slug)
        .await
        .map_err(ApiError::from_db)?
    {
        return Err(ApiError::conflict(
            "migration_in_progress",
            "a migration is running for this collection; update it when the run finishes",
        ));
    }
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
    let unique = next.unique_fields();
    let ids: Vec<String> = next.fields().iter().map(|field| field.id.clone()).collect();
    match previous.successor(&next) {
        Successor::Compatible => {}
        Successor::CheckContent(refusal) => {
            check_stored(&mut tx, &path.slug, Arc::new(next.clone()), refusal).await?;
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
    // No two entries share a unique field's value (D-070): claim the values of fields made unique.
    let before = previous.unique_fields();
    if before != unique {
        claim_unique(&mut tx, &path.slug, &ids, &before, &unique).await?;
    }
    check_inverses(&mut tx, &path.slug, &next).await?;
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

/// A deleted collection's answer.
#[derive(Serialize)]
struct RemovedView {
    slug: String,
    removed: bool,
}

/// Every (type, field path) whose reference fields name `collection`, over every other type's
/// current definition, its components expanded (D-073). Runs before the collection's own change
/// lock, as every component lock must.
async fn schema_referrers(
    tx: &mut ProjectTransaction,
    collection: &str,
) -> Result<Vec<(String, String)>, ApiError> {
    let mut out = Vec::new();
    for version in content_types::list(tx).await.map_err(ApiError::from_db)? {
        if version.collection == collection {
            continue;
        }
        let stored: Value =
            serde_json::from_str(&version.definition).map_err(ApiError::internal)?;
        let expanded = expand(tx, &stored).await?;
        let parsed = ContentType::parse(&expanded).map_err(ApiError::internal)?;
        out.extend(
            parsed
                .reference_targets()
                .into_iter()
                .filter(|(_, to)| to == collection)
                .map(|(path, _)| (version.collection.clone(), path)),
        );
    }
    Ok(out)
}

/// `DELETE {P}/content-types/{slug}`: the collection and every version of its definition, gone
/// for good (D-083). Refused while another type's field links to entries here
/// (`still_referenced`, naming them), while it still has entries (`collection_not_empty`), and
/// while deleted entries' revisions still pin its versions (`collection_has_history`).
pub(crate) async fn delete(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<TypePath>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let expected = if_match(&headers)?;
    let key = idempotency_key(&headers)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_change(&tx, &path.slug)?;
    let request = request_hash(&[DELETE, &path.slug, &expected.to_string()]);
    // Claimed before the collection is looked up: a retry after a lost answer finds it deleted,
    // and must get the recorded answer, not a 404.
    match idempotency::claim(&mut tx, DELETE, key, &request)
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
    // Schema-level references first: a field that links to entries here keeps meaning only
    // while the collection exists.
    let referrers = schema_referrers(&mut tx, &path.slug).await?;
    if let Some((first_type, first_field)) = referrers.first() {
        let others = if referrers.len() > 1 {
            format!(" (and {} more)", referrers.len() - 1)
        } else {
            String::new()
        };
        let message = format!(
            "the `{first_type}` type links to entries here through `{first_field}`{others}: retarget or remove the field first, then delete the collection"
        );
        return Err(ApiError::StillNeeded(Box::new(StillNeeded {
            reason: "still_referenced",
            message,
            referenced_by: Vec::new(),
            changesets: Vec::new(),
        })));
    }
    // From here, writes in the collection wait until this commits (D-055).
    content_types::lock_for_change(&mut tx, &path.slug)
        .await
        .map_err(ApiError::from_db)?;
    if migrations::running_for_collection(&mut tx, &path.slug)
        .await
        .map_err(ApiError::from_db)?
    {
        return Err(ApiError::conflict(
            "migration_in_progress",
            "a migration is running for this collection; delete it when the run finishes",
        ));
    }
    if migrations::has_plans(&mut tx, &path.slug)
        .await
        .map_err(ApiError::from_db)?
    {
        return Err(ApiError::conflict(
            "collection_has_plans",
            "its migration plans name its versions forever, so the collection stays: make a new collection instead",
        ));
    }
    let current = content_types::current(&mut tx, &path.slug)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    if current.version != expected {
        return Err(ApiError::StaleVersion {
            current: current.version,
        });
    }
    match content_types::remove(&mut tx, &path.slug)
        .await
        .map_err(ApiError::from_db)?
    {
        Removed::Removed => {
            let body = serde_json::to_string(&Data {
                data: RemovedView {
                    slug: path.slug.clone(),
                    removed: true,
                },
            })
            .map_err(ApiError::internal)?;
            idempotency::complete(&mut tx, DELETE, key, StatusCode::OK.as_u16(), &body)
                .await
                .map_err(ApiError::from_db)?;
            tx.commit().await.map_err(ApiError::from_db)?;
            Ok(json(StatusCode::OK, body, None, false))
        }
        Removed::NotFound => Err(ApiError::NotFound),
        Removed::NotEmpty(live) => {
            let noun = if live == 1 { "entry" } else { "entries" };
            Err(ApiError::StillNeeded(Box::new(StillNeeded {
                reason: "collection_not_empty",
                message: format!(
                    "the collection still has {live} {noun}: delete them first, then delete it"
                ),
                referenced_by: Vec::new(),
                changesets: Vec::new(),
            })))
        }
        Removed::HasHistory => Err(ApiError::conflict(
            "collection_has_history",
            "its deleted entries keep their history here, so the collection stays: make a new collection instead",
        )),
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
