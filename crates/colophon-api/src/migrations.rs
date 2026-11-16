//! Content schema migrations (docs/modules/04-api.md, "Route families"; D-100). Under
//! `{P}` = `/api/v1/projects/{project}/environments/{environment}`:
//!
//! - `POST {P}/migrations`: `{"collection", "definition", "operations"?}`
//!   (`Idempotency-Key` required): computes a plan for the change a `PUT` on the collection's
//!   content type refuses with `needs_migration`, and stores its dry run — every draft and open
//!   publication checked against the target, each item marked `ok` or `needs_input`. A change a
//!   plain update could apply is refused `no_migration_needed`; so is planning while a plan runs
//!   on the collection (`migration_in_progress`). 201, with `ETag: "sha256:<plan hash>"`;
//! - `GET  {P}/migrations`: the newest 100 plans, `?collection=` and `?state=` filtered;
//! - `GET  {P}/migrations/{migration}`: one plan, with the plan hash as `ETag`;
//! - `GET  {P}/migrations/{migration}/items`: its items in entry order, `?outcome=` filtered,
//!   cursored like the entries list;
//! - `POST {P}/migrations/{migration}/approve` (`If-Match`, `Idempotency-Key`): binds the
//!   approval to the plan hash the approver looked at (`plan_moved` when it isn't), like a
//!   review binds a changeset's (D-044);
//! - `POST {P}/migrations/{migration}/apply` (`If-Match`, `Idempotency-Key`): starts the run —
//!   202 `{state: "running"}`. A worker drives the items (step M.5b); until one does, the plan
//!   stays running, and a running plan blocks schema writes on its collection
//!   (`migration_in_progress`);
//! - `PUT  {P}/migrations/{migration}/items/{entry}`: `{"resolution": <document>}`
//!   (`If-Match`, `Idempotency-Key`): stores a replacement document that the target accepts,
//!   for an item that needs input;
//! - `POST {P}/migrations/{migration}/cancel` (`Idempotency-Key`): cancels a plan that hasn't
//!   run yet; a running plan can't be cancelled in v1 (`cannot_cancel_running`).
//!
//! Everything a plan changes needs `schema.change` on the collection (D-058); reads are any
//! member's. A plan's hash is computed from the collection, its base version, the target
//! definition hash and the operations, so approval and apply act on exactly what was looked
//! at — a plan's predicted items are a dry run, never a reservation.

use axum::{
    Json,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use colophon_db::{
    content_types,
    idempotency::{self, Claim},
    migrations::{self, Item, NewPlan, Plan, Planned, Resolved},
    scope::ProjectTransaction,
};
use colophon_domain::{
    migrate,
    project::Slug,
    schema::{ContentType, Refusal, Successor},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, value::RawValue};
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    content_types::expand,
    entries::{content_hash, idempotency_key, json, open, request_hash, rfc3339, sha256},
    members::require,
    session::SignedIn,
    shape::{Data, Page},
};

const PLAN: &str = "migration.plan";
const APPROVE: &str = "migration.approve";
const APPLY: &str = "migration.apply";
const CANCEL: &str = "migration.cancel";
const RESOLVE: &str = "migration.resolve";

/// A list's page size when none is asked for.
const DEFAULT_PAGE: i64 = 50;

/// The plan states, for validating `?state=`.
const STATES: [&str; 7] = [
    "proposed",
    "approved",
    "running",
    "done",
    "blocked",
    "superseded",
    "cancelled",
];

/// The item outcomes, for validating `?outcome=`.
const OUTCOMES: [&str; 7] = [
    "ok",
    "migrates",
    "needs_input",
    "blocked",
    "resolved",
    "applied",
    "skipped",
];

#[derive(Deserialize)]
pub(crate) struct ListPath {
    project: String,
    environment: String,
}

#[derive(Deserialize)]
pub(crate) struct OnePath {
    project: String,
    environment: String,
    migration: String,
}

#[derive(Deserialize)]
pub(crate) struct ItemPath {
    project: String,
    environment: String,
    migration: String,
    entry: String,
}

/// `{"collection": "articles", "definition": {...}, "operations": {...}}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewPlanBody {
    collection: String,
    definition: Value,
    #[serde(default)]
    operations: Option<Value>,
}

/// `{"resolution": <document>}`: the replacement content for an item that needs input.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewResolution {
    resolution: Value,
}

#[derive(Deserialize)]
pub(crate) struct ListQuery {
    collection: Option<String>,
    state: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct ItemsQuery {
    outcome: Option<String>,
    after: Option<String>,
    limit: Option<i64>,
}

#[derive(Serialize)]
pub(crate) struct ItemsCount {
    expected: i64,
    done: i64,
    blocked: i64,
}

/// A plan as a list shows it: everything but the target definition itself.
#[derive(Serialize)]
pub(crate) struct SummaryView {
    id: Uuid,
    collection: String,
    base_version: i32,
    target_version: Option<i32>,
    state: String,
    /// Its plan hash, as `sha256:<hex>`: what `If-Match` binds (D-100).
    hash: String,
    items: ItemsCount,
    created_by: Uuid,
    approved_by: Option<Uuid>,
    approved_at: Option<String>,
    started_at: Option<String>,
    finished_at: Option<String>,
    error: Option<String>,
    created_at: String,
    updated_at: String,
}

/// The full plan, with the target definition embedded.
#[derive(Serialize)]
pub(crate) struct PlanView {
    id: Uuid,
    collection: String,
    base_version: i32,
    target_version: Option<i32>,
    /// The canonical target definition, embedded as stored.
    target_definition: Box<RawValue>,
    target_definition_hash: String,
    operations: Option<Box<RawValue>>,
    state: String,
    hash: String,
    items: ItemsCount,
    /// Whether any applied item still serves an open publication pinned before the run's
    /// target version — those entries need a publish before their public copy is current.
    republish_required: bool,
    created_by: Uuid,
    approved_by: Option<Uuid>,
    approved_at: Option<String>,
    started_at: Option<String>,
    finished_at: Option<String>,
    error: Option<String>,
    created_at: String,
    updated_at: String,
}

/// One item, the way the "entries in the way" elsewhere name theirs: the entry, its title,
/// what it is to the plan and why.
#[derive(Serialize)]
pub(crate) struct ItemView {
    entry_id: Uuid,
    /// The draft at plan time: what the apply rewrites.
    expected_revision_id: Uuid,
    title: Option<String>,
    outcome: String,
    /// Why it can't proceed, as a refusal reports it.
    problem: Option<Box<RawValue>>,
    /// The replacement document a person gave it, if any.
    resolution: Option<Box<RawValue>>,
    new_revision_id: Option<Uuid>,
    applied_at: Option<String>,
    /// Whether an open publication of this entry still pins a revision from before the run's
    /// target version — `true` once applied until someone publishes the migrated draft.
    republish_required: bool,
}

/// A resolved item, with the plan's hash the resolution was bound to.
#[derive(Serialize)]
pub(crate) struct ResolvedView {
    hash: String,
    item: ItemView,
}

fn counts(plan: &Plan) -> ItemsCount {
    ItemsCount {
        expected: plan.expected_items,
        done: plan.done_items,
        blocked: plan.blocked_items,
    }
}

fn summary(plan: Plan) -> Result<SummaryView, ApiError> {
    let items = counts(&plan);
    Ok(SummaryView {
        id: plan.id,
        collection: plan.collection,
        base_version: plan.base_version,
        target_version: plan.target_version,
        state: plan.state,
        hash: content_hash(&plan.plan_hash),
        items,
        created_by: plan.created_by,
        approved_by: plan.approved_by,
        approved_at: plan.approved_at.map(rfc3339).transpose()?,
        started_at: plan.started_at.map(rfc3339).transpose()?,
        finished_at: plan.finished_at.map(rfc3339).transpose()?,
        error: plan.error,
        created_at: rfc3339(plan.created_at)?,
        updated_at: rfc3339(plan.updated_at)?,
    })
}

fn view(plan: Plan, republish_required: bool) -> Result<PlanView, ApiError> {
    let items = counts(&plan);
    Ok(PlanView {
        id: plan.id,
        collection: plan.collection,
        base_version: plan.base_version,
        target_version: plan.target_version,
        target_definition: RawValue::from_string(plan.target_definition)
            .map_err(ApiError::internal)?,
        target_definition_hash: content_hash(&plan.target_definition_hash),
        operations: plan
            .operations
            .map(RawValue::from_string)
            .transpose()
            .map_err(ApiError::internal)?,
        state: plan.state,
        hash: content_hash(&plan.plan_hash),
        items,
        republish_required,
        created_by: plan.created_by,
        approved_by: plan.approved_by,
        approved_at: plan.approved_at.map(rfc3339).transpose()?,
        started_at: plan.started_at.map(rfc3339).transpose()?,
        finished_at: plan.finished_at.map(rfc3339).transpose()?,
        error: plan.error,
        created_at: rfc3339(plan.created_at)?,
        updated_at: rfc3339(plan.updated_at)?,
    })
}

fn item_view(item: Item) -> Result<ItemView, ApiError> {
    Ok(ItemView {
        entry_id: item.entry_id,
        expected_revision_id: item.expected_revision_id,
        title: item.title,
        outcome: item.outcome,
        problem: item
            .problem
            .map(RawValue::from_string)
            .transpose()
            .map_err(ApiError::internal)?,
        resolution: item
            .resolution
            .map(RawValue::from_string)
            .transpose()
            .map_err(ApiError::internal)?,
        new_revision_id: item.new_revision_id,
        applied_at: item.applied_at.map(rfc3339).transpose()?,
        republish_required: item.republish_required,
    })
}

/// `ETag: "<the hash as `sha256:<hex>`>"`; an empty one when a replayed body lacked it.
fn respond(status: StatusCode, body: String, hash: &str, replayed: bool) -> Response {
    let mut response = json(status, body, None, replayed);
    if let Ok(etag) = HeaderValue::from_str(&format!("\"{hash}\"")) {
        response.headers_mut().insert(header::ETAG, etag);
    }
    response
}

/// The `sha256:<hex>` an `If-Match` names, like a changeset's.
fn if_match(headers: &HeaderMap) -> Result<String, ApiError> {
    let value = headers
        .get(header::IF_MATCH)
        .ok_or(ApiError::PreconditionRequired(
            "this needs an If-Match header with the plan's ETag, so you act on what you looked at",
        ))?;
    value
        .to_str()
        .ok()
        .and_then(|text| text.trim().strip_prefix('"')?.strip_suffix('"'))
        .filter(|hash| {
            hash.strip_prefix("sha256:").is_some_and(|hex| {
                hex.len() == 64
                    && hex
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        })
        .map(str::to_owned)
        .ok_or(ApiError::InvalidHeader(
            "If-Match must be one plan ETag, such as \"sha256:…\"",
        ))
}

/// Refuses unless the caller's role allows `schema.change` on `collection` (D-058): the
/// migration permission is that one (D-100).
fn may_change(tx: &ProjectTransaction, collection: &str) -> Result<(), ApiError> {
    require(
        &tx.policy,
        colophon_domain::policy::Action::SchemaChange,
        Some(collection),
        "your role can't change this collection's schema",
    )
}

/// Claims the idempotency key, or answers with the recorded response (its `ETag` read back
/// from `data.hash`, which every response here carries).
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
            let hash = serde_json::from_str::<Value>(&response)
                .ok()
                .and_then(|body| body["data"]["hash"].as_str().map(str::to_owned))
                .unwrap_or_default();
            Ok(Some(respond(status, response, &hash, true)))
        }
    }
}

/// Records the answer and commits before responding, with the plan hash as `ETag`.
async fn finish(
    mut tx: ProjectTransaction,
    operation: &str,
    key: &str,
    status: StatusCode,
    plan: &Plan,
    body: String,
) -> Result<Response, ApiError> {
    idempotency::complete(&mut tx, operation, key, status.as_u16(), &body)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(respond(status, body, &content_hash(&plan.plan_hash), false))
}

/// Locks the plan and checks it still has the hash the caller looked at; `plan_moved` names
/// the current one when it doesn't (D-100).
async fn locked(tx: &mut ProjectTransaction, id: &str, expected: &str) -> Result<Plan, ApiError> {
    let id = Uuid::try_parse(id).map_err(|_| ApiError::NotFound)?;
    let plan = migrations::lock(tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let current = content_hash(&plan.plan_hash);
    if current != expected {
        return Err(ApiError::StalePlan { current });
    }
    Ok(plan)
}

/// A refusal inside `definition` gets its pointer moved under `/definition`.
fn within(refusal: Refusal) -> ApiError {
    ApiError::Invalid(Refusal {
        pointer: format!("/definition{}", refusal.pointer),
        ..refusal
    })
}

/// 422: the change a plain update applies; there is nothing to migrate.
fn no_migration_needed() -> ApiError {
    ApiError::InvalidField {
        reason: "no_migration_needed",
        pointer: "/definition",
        message: "the current definition already accepts this change: update the content type"
            .to_owned(),
    }
}

/// `operations` as the worker will read it: what the plan declares must be operations it can
/// run — parsed the way the worker parses them, so nothing gets approved that can only block
/// later (D-101).
fn checked_operations(body: &NewPlanBody) -> Result<Option<String>, ApiError> {
    let operations = body
        .operations
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(ApiError::internal)?;
    if operations.as_deref().is_some_and(|text| text.len() > 65536) {
        return Err(ApiError::InvalidField {
            reason: "operations_too_large",
            pointer: "/operations",
            message: "the plan's operations are at most 65,536 bytes".to_owned(),
        });
    }
    if let Some(ops) = body.operations.as_ref()
        && let Err(refusal) = migrate::parse(ops)
    {
        return Err(ApiError::Invalid(Refusal {
            pointer: format!("/operations{}", refusal.pointer),
            ..refusal
        }));
    }
    Ok(operations)
}

/// `POST {P}/migrations`.
pub(crate) async fn create(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ListPath>,
    headers: HeaderMap,
    body: Result<Json<NewPlanBody>, JsonRejection>,
) -> Result<Response, ApiError> {
    let key = idempotency_key(&headers)?;
    let Json(new) = body?;
    let collection = Slug::parse(&new.collection).map_err(|error| ApiError::InvalidField {
        reason: error.code(),
        pointer: "/collection",
        message: error.to_string(),
    })?;
    let operations = checked_operations(&new)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_change(&tx, collection.as_str())?;
    let expanded = expand(&mut tx, &new.definition).await.map_err(|error| {
        if let ApiError::Invalid(refusal) = error {
            within(refusal)
        } else {
            error
        }
    })?;
    let next = ContentType::parse(&expanded).map_err(within)?;
    let canonical = next.canonical_json();
    let definition_hash = sha256(canonical.as_bytes());
    let declared = operations.as_deref().unwrap_or_default();
    let request = request_hash(&[PLAN, collection.as_str(), &canonical, declared]);
    if let Some(replay) = claim(&mut tx, PLAN, key, &request).await? {
        return Ok(replay);
    }
    // The check `create_plan` repeats under the collection's change lock; this early answer
    // only saves it the work.
    let current = content_types::current(&mut tx, collection.as_str())
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let stored: Value = serde_json::from_str(&current.definition).map_err(ApiError::internal)?;
    let previous = ContentType::parse(&stored).map_err(ApiError::internal)?;
    if matches!(previous.successor(&next), Successor::Compatible) {
        return Err(no_migration_needed());
    }
    let plan_hash = request_hash(&[
        collection.as_str(),
        &current.version.to_string(),
        &content_hash(&definition_hash),
        declared,
    ]);
    let new_plan = NewPlan {
        id: Uuid::now_v7(),
        collection: collection.as_str(),
        next: &next,
        canonical: &canonical,
        definition_hash: &definition_hash,
        operations: operations.as_deref(),
        plan_hash: &plan_hash,
    };
    let plan = match migrations::create_plan(&mut tx, &new_plan).await {
        Ok(Planned::Done(plan)) => *plan,
        Ok(Planned::NotNeeded) => return Err(no_migration_needed()),
        Ok(Planned::NotFound) => return Err(ApiError::NotFound),
        Ok(Planned::Running) => {
            return Err(ApiError::conflict(
                "migration_in_progress",
                "a migration is already running for this collection; let it finish first",
            ));
        }
        Err(error) => return Err(ApiError::from_db(error)),
    };
    let detail = serde_json::json!({
        "collection": plan.collection,
        "base_version": plan.base_version,
        "hash": content_hash(&plan.plan_hash),
        "expected_items": plan.expected_items,
    })
    .to_string();
    colophon_db::audit::record_in(
        &mut tx.transaction,
        tx.project,
        tx.identity,
        "migration.planned",
        &plan.id.to_string(),
        &detail,
    )
    .await
    .map_err(ApiError::from_db)?;
    let republish = migrations::republish_required(&mut tx, plan.id)
        .await
        .map_err(ApiError::from_db)?;
    let body = serde_json::to_string(&Data {
        data: view(plan.clone(), republish)?,
    })
    .map_err(ApiError::internal)?;
    finish(tx, PLAN, key, StatusCode::CREATED, &plan, body).await
}

/// `GET {P}/migrations`, `?collection=` and `?state=` filtered.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ListPath>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Page<SummaryView>>, ApiError> {
    let collection = query
        .collection
        .as_deref()
        .map(Slug::parse)
        .transpose()
        .map_err(|error| ApiError::InvalidField {
            reason: error.code(),
            pointer: "/collection",
            message: error.to_string(),
        })?;
    let wanted = match query.state.as_deref() {
        None | Some("") => None,
        Some(state) if STATES.contains(&state) => Some(state),
        Some(_) => {
            return Err(ApiError::InvalidField {
                reason: "invalid_state",
                pointer: "/state",
                message: format!("a plan's state is one of {}", STATES.join(", ")),
            });
        }
    };
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let plans = migrations::list(&mut tx, collection.as_ref().map(Slug::as_str), wanted)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let data = plans
        .into_iter()
        .map(summary)
        .collect::<Result<Vec<_>, ApiError>>()?;
    Ok(Json(Page {
        data,
        next_cursor: None,
    }))
}

/// `GET {P}/migrations/{migration}`.
pub(crate) async fn get(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
) -> Result<Response, ApiError> {
    let id = Uuid::try_parse(&path.migration).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let plan = migrations::get(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let republish = migrations::republish_required(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?;
    let hash = content_hash(&plan.plan_hash);
    let body = serde_json::to_string(&Data {
        data: view(plan, republish)?,
    })
    .map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(respond(StatusCode::OK, body, &hash, false))
}

/// `GET {P}/migrations/{migration}/items?outcome=&after=&limit=`.
pub(crate) async fn items(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
    Query(query): Query<ItemsQuery>,
) -> Result<Response, ApiError> {
    let id = Uuid::try_parse(&path.migration).map_err(|_| ApiError::NotFound)?;
    let outcome = match query.outcome.as_deref() {
        None | Some("") => None,
        Some(outcome) if OUTCOMES.contains(&outcome) => Some(outcome),
        Some(_) => {
            return Err(ApiError::InvalidField {
                reason: "invalid_outcome",
                pointer: "/outcome",
                message: format!("an item's outcome is one of {}", OUTCOMES.join(", ")),
            });
        }
    };
    let limit = query.limit.unwrap_or(DEFAULT_PAGE);
    if !(1..=migrations::MAX_ITEMS_PAGE).contains(&limit) {
        return Err(ApiError::InvalidField {
            reason: "invalid_limit",
            pointer: "/limit",
            message: format!("limit is between 1 and {}", migrations::MAX_ITEMS_PAGE),
        });
    }
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let plan = migrations::get(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let after = query
        .after
        .as_deref()
        .map(|cursor| after_cursor(&plan, cursor))
        .transpose()?;
    let mut rows = migrations::items(&mut tx, id, outcome, after, limit + 1)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let more = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
    rows.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    let next_cursor = more
        .then(|| rows.last().map(|row| cursor(&plan, row.entry_id)))
        .flatten();
    let data = rows
        .into_iter()
        .map(item_view)
        .collect::<Result<Vec<_>, ApiError>>()?;
    let body = serde_json::to_string(&Page { data, next_cursor }).map_err(ApiError::internal)?;
    Ok(json(StatusCode::OK, body, None, false))
}

/// A cursor is bound to the plan it came from.
fn cursor(plan: &Plan, last: Uuid) -> String {
    URL_SAFE_NO_PAD.encode(format!("v1:{}:{}:{last}", plan.id, plan.collection))
}

fn after_cursor(plan: &Plan, cursor: &str) -> Result<Uuid, ApiError> {
    let prefix = format!("v1:{}:{}:", plan.id, plan.collection);
    URL_SAFE_NO_PAD
        .decode(cursor)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| Uuid::try_parse(text.strip_prefix(&prefix)?).ok())
        .ok_or(ApiError::InvalidHeader(
            "this cursor is not valid for this list",
        ))
}

/// `POST {P}/migrations/{migration}/approve`.
pub(crate) async fn approve(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let expected = if_match(&headers)?;
    let key = idempotency_key(&headers)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let plan = locked(&mut tx, &path.migration, &expected).await?;
    may_change(&tx, &plan.collection)?;
    let request = request_hash(&[APPROVE, &path.migration, &expected]);
    if let Some(replay) = claim(&mut tx, APPROVE, key, &request).await? {
        return Ok(replay);
    }
    if plan.state != "proposed" {
        return Err(ApiError::conflict(
            "plan_not_proposed",
            "only a proposed plan can be approved",
        ));
    }
    migrations::approve(&mut tx, plan.id)
        .await
        .map_err(ApiError::from_db)?;
    record(&mut tx, "migration.approved", &plan).await?;
    let plan = migrations::get(&mut tx, plan.id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let republish = migrations::republish_required(&mut tx, plan.id)
        .await
        .map_err(ApiError::from_db)?;
    let body = serde_json::to_string(&Data {
        data: view(plan.clone(), republish)?,
    })
    .map_err(ApiError::internal)?;
    finish(tx, APPROVE, key, StatusCode::OK, &plan, body).await
}

/// `POST {P}/migrations/{migration}/apply`: starts the run; a worker drives it (M.5b).
pub(crate) async fn apply(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let expected = if_match(&headers)?;
    let key = idempotency_key(&headers)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let plan = locked(&mut tx, &path.migration, &expected).await?;
    may_change(&tx, &plan.collection)?;
    let request = request_hash(&[APPLY, &path.migration, &expected]);
    if let Some(replay) = claim(&mut tx, APPLY, key, &request).await? {
        return Ok(replay);
    }
    if plan.state != "approved" {
        return Err(ApiError::conflict(
            "plan_not_approved",
            "only an approved plan can be applied",
        ));
    }
    // approved → running, atomically, stamping started_at; under the plan lock this can't lose,
    // and the unique index would refuse a second running plan on the collection either way.
    let started = migrations::begin_apply(&mut tx, plan.id)
        .await
        .map_err(ApiError::from_db)?;
    if !started {
        return Err(ApiError::internal(std::io::Error::other(
            "a plan approved under its lock refused to start",
        )));
    }
    let plan = migrations::get(&mut tx, plan.id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let body = serde_json::to_string(&Data {
        data: serde_json::json!({
            "id": plan.id,
            "state": plan.state,
            "hash": content_hash(&plan.plan_hash),
            "started_at": plan.started_at.map(rfc3339).transpose()?,
        }),
    })
    .map_err(ApiError::internal)?;
    finish(tx, APPLY, key, StatusCode::ACCEPTED, &plan, body).await
}

/// `PUT {P}/migrations/{migration}/items/{entry}`: `{"resolution": <document>}`, validated
/// against the target definition as a save would store it.
pub(crate) async fn resolve(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ItemPath>,
    headers: HeaderMap,
    body: Result<Json<NewResolution>, JsonRejection>,
) -> Result<Response, ApiError> {
    let expected = if_match(&headers)?;
    let key = idempotency_key(&headers)?;
    let Json(new) = body?;
    let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let plan = locked(&mut tx, &path.migration, &expected).await?;
    may_change(&tx, &plan.collection)?;
    let resolution_text = serde_json::to_string(&new.resolution).map_err(ApiError::internal)?;
    let request = request_hash(&[RESOLVE, &path.migration, &path.entry, &resolution_text]);
    if let Some(replay) = claim(&mut tx, RESOLVE, key, &request).await? {
        return Ok(replay);
    }
    if !matches!(plan.state.as_str(), "proposed" | "approved") {
        return Err(ApiError::conflict(
            "plan_not_open",
            "only a proposed or approved plan takes resolutions",
        ));
    }
    // The replacement must be a document the target accepts.
    let target: Value =
        serde_json::from_str(&plan.target_definition).map_err(ApiError::internal)?;
    let target = ContentType::parse(&target).map_err(ApiError::internal)?;
    let canonical = target
        .validate(&new.resolution)
        .map_err(ApiError::Invalid)?
        .canonical_json()
        .to_owned();
    match migrations::resolve_item(&mut tx, plan.id, entry, &canonical).await {
        Ok(Resolved::Done(item)) => {
            let body = serde_json::to_string(&Data {
                data: ResolvedView {
                    hash: content_hash(&plan.plan_hash),
                    item: item_view(item)?,
                },
            })
            .map_err(ApiError::internal)?;
            finish(tx, RESOLVE, key, StatusCode::OK, &plan, body).await
        }
        Ok(Resolved::NotFound) => Err(ApiError::NotFound),
        Ok(Resolved::WrongState(outcome)) => Err(ApiError::conflict(
            "item_not_needs_input",
            outcome_message(&outcome),
        )),
        Err(error) => Err(ApiError::from_db(error)),
    }
}

/// Why a resolution can't land, for an item in `outcome`.
fn outcome_message(outcome: &str) -> &'static str {
    match outcome {
        "resolved" => "this item is resolved already",
        "applied" => "this item is applied already",
        "skipped" => "this item is skipped already",
        _ => "only an item that needs input takes a resolution",
    }
}

/// `POST {P}/migrations/{migration}/cancel`: a plan that hasn't run yet. A running plan is
/// refused: v1's answer to a run to stop is to let it finish (M.5b).
pub(crate) async fn cancel(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let key = idempotency_key(&headers)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let id = Uuid::try_parse(&path.migration).map_err(|_| ApiError::NotFound)?;
    let plan = migrations::lock(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    may_change(&tx, &plan.collection)?;
    let request = request_hash(&[CANCEL, &path.migration]);
    if let Some(replay) = claim(&mut tx, CANCEL, key, &request).await? {
        return Ok(replay);
    }
    match plan.state.as_str() {
        "proposed" | "approved" => {}
        "running" => {
            return Err(ApiError::conflict(
                "cannot_cancel_running",
                "a running migration can't be cancelled; let it finish",
            ));
        }
        _ => {
            return Err(ApiError::conflict(
                "plan_finished",
                "this migration plan is already finished",
            ));
        }
    }
    let cancelled = migrations::cancel(&mut tx, plan.id)
        .await
        .map_err(ApiError::from_db)?;
    if !cancelled {
        return Err(ApiError::internal(std::io::Error::other(
            "a plan open under its lock refused to cancel",
        )));
    }
    record(&mut tx, "migration.cancelled", &plan).await?;
    let plan = migrations::get(&mut tx, plan.id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let republish = migrations::republish_required(&mut tx, plan.id)
        .await
        .map_err(ApiError::from_db)?;
    let body = serde_json::to_string(&Data {
        data: view(plan.clone(), republish)?,
    })
    .map_err(ApiError::internal)?;
    finish(tx, CANCEL, key, StatusCode::OK, &plan, body).await
}

/// Records a plan event in the audit log, in the transaction that made it.
async fn record(tx: &mut ProjectTransaction, action: &str, plan: &Plan) -> Result<(), ApiError> {
    let detail = serde_json::json!({
        "collection": plan.collection,
        "base_version": plan.base_version,
        "hash": content_hash(&plan.plan_hash),
    })
    .to_string();
    colophon_db::audit::record_in(
        &mut tx.transaction,
        tx.project,
        tx.identity,
        action,
        &plan.id.to_string(),
        &detail,
    )
    .await
    .map_err(ApiError::from_db)
}
