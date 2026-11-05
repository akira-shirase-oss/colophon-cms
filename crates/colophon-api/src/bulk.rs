//! Bounded bulk work on drafts (docs/modules/04-api.md; P5, D-116). Under
//! `{P}` = `/api/v1/projects/{project}/environments/{environment}` — `/bulk`, not `/jobs`,
//! which is already the delivery family's name:
//!
//! - `POST {P}/bulk`: `{"collection", "items": [{key, entry, expected_revision, op}], "seal"?}`
//!   (`Idempotency-Key` required): accepts a manifest of exact entries and base revisions —
//!   at most 100 items and 1 MiB a chunk (§17) — and stores it `accepting`, or `running`
//!   when `seal` closes it. Every item's operation is parsed now; every named entry must
//!   exist in the collection, and the caller must hold the permission each operation asks
//!   for: `content.update` for `update`/`set`/`move`, `content.delete` for `delete`. A
//!   caller with a limits row reserves a `jobs` slot and one `documents` unit per item in
//!   the same transaction (D-113, D-116). 202 `{id, state}` — accepted is not done — with
//!   `ETag: "sha256:<manifest hash>"`.
//! - `POST {P}/bulk/{job}/items`: `{"items": [...], "seal"?}` (`If-Match`, `Idempotency-Key`):
//!   appends the next chunk while the job is `accepting` (`job_moved`, `job_not_accepting`).
//!   An item whose `key` the manifest already has lands once — appends are per-item
//!   idempotent, the resumable half of the design.
//! - `GET {P}/bulk`: the newest jobs, `?state=` filtered, cursored.
//! - `GET {P}/bulk/{job}`: one job, `ETag`'d with its manifest hash.
//! - `GET {P}/bulk/{job}/items`: its items in key order — the resumable cursor —
//!   `?outcome=` filtered, with per-item `problem` details.
//! - `POST {P}/bulk/{job}/cancel` (`Idempotency-Key`): the submitter or a caller with
//!   `content.update` on the collection cancels an `accepting` or `running` job; pending
//!   items become `skipped`, held reservations release in the same commit, and the audit
//!   log records `bulk.cancelled`.
//!
//! Nothing here pretends a multi-chunk manifest is globally atomic: per-item outcomes are
//! the contract, and the worker drives each item in its own transaction as the submitter.

use axum::{
    Json,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use colophon_db::{
    budgets, bulk, content_types,
    idempotency::{self, Claim},
    scope::ProjectTransaction,
};
use colophon_domain::{
    bulk::{CHUNK_BYTES, CHUNK_ITEMS, MAX_ITEMS},
    policy::Action,
    project::Slug,
    schema::Refusal,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, value::RawValue};
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    entries::{content_hash, idempotency_key, json, open, request_hash, rfc3339, sha256},
    members::require,
    session::SignedIn,
    shape::{Data, Page},
};

const SUBMIT: &str = "bulk.submit";
const ITEMS: &str = "bulk.items";
const CANCEL: &str = "bulk.cancel";

/// A list's page size when none is asked for.
const DEFAULT_PAGE: usize = 50;

/// The item outcomes, for validating `?outcome=`.
const OUTCOMES: [&str; 5] = ["pending", "applied", "conflict", "refused", "skipped"];

/// The job states, for validating `?state=`.
const STATES: [&str; 5] = ["accepting", "running", "done", "blocked", "cancelled"];

#[derive(Deserialize)]
pub(crate) struct ListPath {
    project: String,
    environment: String,
}

#[derive(Deserialize)]
pub(crate) struct OnePath {
    project: String,
    environment: String,
    job: String,
}

/// `{"collection": "articles", "items": [...], "seal": true}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewJobBody {
    collection: String,
    items: Vec<ItemBody>,
    #[serde(default)]
    seal: bool,
}

/// `{"items": [...], "seal": true}` for an append.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AppendBody {
    items: Vec<ItemBody>,
    #[serde(default)]
    seal: bool,
}

/// One manifest item: the client key it resumes under, the exact entry and base revision,
/// and the operation document `{"op": "update"|"set"|"move"|"delete", …}`.
#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct ItemBody {
    key: String,
    entry: Uuid,
    expected_revision: Uuid,
    op: Value,
}

#[derive(Deserialize)]
pub(crate) struct ListQuery {
    state: Option<String>,
    cursor: Option<String>,
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

/// A job as the API shows it.
#[derive(Serialize)]
pub(crate) struct JobView {
    id: Uuid,
    kind: String,
    /// Its operation document, embedded as stored (`{"collection": "…"}`).
    operation: Box<RawValue>,
    state: String,
    /// Its manifest hash, as `sha256:<hex>`: what `If-Match` binds.
    hash: String,
    items: ItemsCount,
    created_by: Uuid,
    principal: String,
    started_at: Option<String>,
    finished_at: Option<String>,
    error: Option<String>,
    created_at: String,
    updated_at: String,
}

/// What an append answered with: the job after the chunk, what landed and which keys were
/// already the manifest's — the per-item idempotency, reported.
#[derive(Serialize)]
pub(crate) struct AppendView {
    job: JobView,
    appended: i64,
    /// Client keys the manifest already had: the append landed them zero times.
    duplicates: Vec<String>,
    sealed: bool,
}

/// One item, the way its outcome reads: what it named, what happened, why it didn't when
/// it didn't.
#[derive(Serialize)]
pub(crate) struct ItemView {
    key: String,
    entry_id: Uuid,
    expected_revision_id: Uuid,
    /// The operation document it was accepted with.
    input: Box<RawValue>,
    outcome: String,
    /// Why it didn't apply, as a refusal reports it.
    problem: Option<Box<RawValue>>,
    new_revision_id: Option<Uuid>,
    applied_at: Option<String>,
}

fn counts(job: &bulk::Job) -> ItemsCount {
    ItemsCount {
        expected: job.expected_items,
        done: job.done_items,
        blocked: job.blocked_items,
    }
}

fn view(job: bulk::Job) -> Result<JobView, ApiError> {
    let items = counts(&job);
    Ok(JobView {
        id: job.id,
        kind: job.kind,
        operation: RawValue::from_string(job.operation.to_string()).map_err(ApiError::internal)?,
        state: job.state,
        hash: content_hash(&job.manifest_hash),
        items,
        created_by: job.created_by,
        principal: job.principal,
        started_at: job.started_at.map(rfc3339).transpose()?,
        finished_at: job.finished_at.map(rfc3339).transpose()?,
        error: job.error,
        created_at: rfc3339(job.created_at)?,
        updated_at: rfc3339(job.updated_at)?,
    })
}

fn item_view(item: bulk::Item) -> Result<ItemView, ApiError> {
    Ok(ItemView {
        key: item.key,
        entry_id: item.entry_id,
        expected_revision_id: item.expected_revision_id,
        input: RawValue::from_string(item.input.to_string()).map_err(ApiError::internal)?,
        outcome: item.outcome,
        problem: item
            .problem
            .map(|problem| RawValue::from_string(problem.to_string()))
            .transpose()
            .map_err(ApiError::internal)?,
        new_revision_id: item.new_revision_id,
        applied_at: item.applied_at.map(rfc3339).transpose()?,
    })
}

/// `ETag: "<the manifest hash as `sha256:<hex>`>"`.
fn respond(status: StatusCode, body: String, hash: &str, replayed: bool) -> Response {
    let mut response = json(status, body, None, replayed);
    if let Ok(etag) = HeaderValue::from_str(&format!("\"{hash}\"")) {
        response.headers_mut().insert(header::ETAG, etag);
    }
    response
}

/// The `sha256:<hex>` an `If-Match` names, like a plan's.
fn if_match(headers: &HeaderMap) -> Result<String, ApiError> {
    let value = headers
        .get(header::IF_MATCH)
        .ok_or(ApiError::PreconditionRequired(
            "this needs an If-Match header with the job's ETag, so you act on what you looked at",
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
            "If-Match must be one job ETag, such as \"sha256:…\"",
        ))
}

/// The `v1:` item cursor: the manifest-order key, base64'd, like the migrations cursor.
fn cursor(key: &str) -> String {
    URL_SAFE_NO_PAD.encode(format!("v1:{key}"))
}

fn parse_cursor(after: &str) -> Result<String, ApiError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(after)
        .map_err(|_| ApiError::InvalidHeader("a bulk items cursor is the one a page answered"))?;
    String::from_utf8(bytes)
        .ok()
        .and_then(|text| text.strip_prefix("v1:").map(str::to_owned))
        .ok_or(ApiError::InvalidHeader(
            "a bulk items cursor is the one a page answered",
        ))
}

/// Claims the idempotency key, or answers with the recorded response (its `ETag` read back
/// from `data.hash` or `data.job.hash`, both of which the views here carry).
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
                .and_then(|body| {
                    body["data"]["hash"]
                        .as_str()
                        .or_else(|| body["data"]["job"]["hash"].as_str())
                        .map(str::to_owned)
                })
                .unwrap_or_default();
            Ok(Some(respond(status, response, &hash, true)))
        }
    }
}

/// Records the answer and commits before responding, with the manifest hash as `ETag`.
async fn finish(
    mut tx: ProjectTransaction,
    operation: &str,
    key: &str,
    status: StatusCode,
    job: &bulk::Job,
    body: String,
) -> Result<Response, ApiError> {
    idempotency::complete(&mut tx, operation, key, status.as_u16(), &body)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(respond(
        status,
        body,
        &content_hash(&job.manifest_hash),
        false,
    ))
}

/// A chunk's items, validated the way the worker validates them on apply: keys unique and
/// bounded, entries real, operations parseable — each refusal pointing at its item.
fn check_chunk(items: &[ItemBody]) -> Result<Vec<bulk::NewItem<'_>>, ApiError> {
    let refuse = |code: &'static str, pointer: &str, message: String| {
        ApiError::Invalid(Refusal {
            code,
            pointer: pointer.to_owned(),
            message,
        })
    };
    if items.is_empty() {
        return Err(refuse(
            "manifest_empty",
            "/items",
            "a chunk carries at least one item".to_owned(),
        ));
    }
    if items.len() > CHUNK_ITEMS {
        return Err(refuse(
            "chunk_too_large",
            "/items",
            format!("a chunk carries at most {CHUNK_ITEMS} items"),
        ));
    }
    let size = serde_json::to_vec(items).map_err(ApiError::internal)?.len();
    if size > CHUNK_BYTES {
        return Err(refuse(
            "chunk_too_large",
            "/items",
            "a chunk's items are at most 1 MiB".to_owned(),
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut checked = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = format!("/items/{index}");
        if item.key.is_empty() || item.key.chars().count() > 256 {
            return Err(refuse(
                "invalid_key",
                &format!("{at}/key"),
                "an item's key is 1 to 256 characters".to_owned(),
            ));
        }
        if !seen.insert(item.key.clone()) {
            return Err(refuse(
                "duplicate_key",
                &format!("{at}/key"),
                "item keys are unique within a chunk".to_owned(),
            ));
        }
        colophon_domain::bulk::parse(&item.op).map_err(|refusal| {
            ApiError::Invalid(Refusal {
                pointer: format!("{at}/op{}", refusal.pointer),
                ..refusal
            })
        })?;
        checked.push(bulk::NewItem {
            key: &item.key,
            entry_id: item.entry,
            expected_revision_id: item.expected_revision,
            input: item.op.clone(),
        });
    }
    Ok(checked)
}

/// Every item's entry must exist in the job's collection, and the caller must hold the
/// permission each operation asks — `content.delete` for a delete, `content.update` for
/// the rest — evaluated per item's `own`, like a save (D-116).
async fn check_items(
    tx: &mut ProjectTransaction,
    collection: &str,
    items: &[ItemBody],
) -> Result<(), ApiError> {
    let ids: Vec<Uuid> = items.iter().map(|item| item.entry).collect();
    let creators = colophon_db::entries::creators(tx, &ids)
        .await
        .map_err(ApiError::from_db)?;
    let found: std::collections::HashMap<Uuid, Uuid> = creators
        .into_iter()
        .filter(|(_, in_collection, _)| in_collection == collection)
        .map(|(id, _, created_by)| (id, created_by))
        .collect();
    for (index, item) in items.iter().enumerate() {
        let Some(created_by) = found.get(&item.entry) else {
            return Err(ApiError::Invalid(Refusal {
                code: "unknown_entry",
                pointer: format!("/items/{index}/entry"),
                message: "the entry isn't a live entry of this collection".to_owned(),
            }));
        };
        let delete = matches!(item.op.get("op").and_then(Value::as_str), Some("delete"));
        let action = if delete {
            Action::ContentDelete
        } else {
            Action::ContentUpdate
        };
        if tx
            .policy
            .fields(action, collection, *created_by == tx.identity)
            .is_none()
        {
            return Err(ApiError::missing(
                action,
                "your role can't run this operation on these entries",
            ));
        }
    }
    Ok(())
}

/// The hash a chunk rolls into the manifest: the job head at create, then every landed
/// item's canonical form appended in chunk order.
fn roll_hash(previous: &[u8; 32], items: &[ItemBody]) -> [u8; 32] {
    let canonical = serde_json::to_vec(items).unwrap_or_default();
    let mut bytes = Vec::with_capacity(32 + canonical.len());
    bytes.extend_from_slice(previous);
    bytes.extend_from_slice(&canonical);
    sha256(&bytes)
}

/// `POST {P}/bulk`.
pub(crate) async fn create(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ListPath>,
    headers: HeaderMap,
    body: Result<Json<NewJobBody>, JsonRejection>,
) -> Result<Response, ApiError> {
    let key = idempotency_key(&headers)?;
    let Json(new) = body?;
    let collection = Slug::parse(&new.collection).map_err(|error| ApiError::InvalidField {
        reason: error.code(),
        pointer: "/collection",
        message: error.to_string(),
    })?;
    let items = check_chunk(&new.items)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    // The collection must be there to validate anything (NotFound when it isn't — the same
    // answer as the collection's own routes).
    content_types::current(&mut tx, collection.as_str())
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    check_items(&mut tx, collection.as_str(), &new.items).await?;
    let canonical_items = serde_json::to_string(&new.items).map_err(ApiError::internal)?;
    let request = request_hash(&[SUBMIT, collection.as_str(), &canonical_items]);
    if let Some(replay) = claim(&mut tx, SUBMIT, key, &request).await? {
        return Ok(replay);
    }
    let id = Uuid::now_v7();
    let head = sha256(format!("entries/{}", collection.as_str()).as_bytes());
    let manifest_hash = roll_hash(&head, &new.items);
    let principal = if caller.project.is_some() {
        "token"
    } else {
        "member"
    };
    // The job's `create` admits it against the project's work bound (D-131), and it comes
    // before the reservations for two reasons that point the same way: `create` is the
    // INSERT `budget_reservations.bulk_job_id` references, and the admission lock must
    // precede the token-limits row a reservation locks.
    let job = bulk::create(
        &mut tx,
        &bulk::NewJob {
            id,
            kind: "entries",
            operation: serde_json::json!({"collection": collection.as_str()}),
            manifest_hash,
            principal,
            expected_items: i64::try_from(items.len()).unwrap_or(0),
            seal: new.seal,
        },
    )
    .await
    .map_err(ApiError::from_db)?;
    // The job's reservations land in its accepting transaction, like a release's dispatch
    // (D-113): the jobs slot, then a documents unit per item — refused, and the whole job
    // rolls back with them.
    budgets::reserve_bulk_job(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?;
    budgets::reserve_bulk_documents(&mut tx, id, i64::try_from(items.len()).unwrap_or(0))
        .await
        .map_err(ApiError::from_db)?;
    bulk::append(&mut tx, id, &items)
        .await
        .map_err(ApiError::from_db)?;
    let detail = serde_json::json!({
        "collection": collection.as_str(),
        "hash": content_hash(&job.manifest_hash),
        "expected_items": job.expected_items,
        "sealed": new.seal,
    })
    .to_string();
    colophon_db::audit::record_in(
        &mut tx.transaction,
        tx.project,
        tx.identity,
        "bulk.submitted",
        &id.to_string(),
        &detail,
    )
    .await
    .map_err(ApiError::from_db)?;
    let body = serde_json::to_string(&Data {
        data: view(job.clone())?,
    })
    .map_err(ApiError::internal)?;
    finish(tx, SUBMIT, key, StatusCode::ACCEPTED, &job, body).await
}

/// `POST {P}/bulk/{job}/items`: the next chunk, while the job accepts — `If-Match` binds
/// the manifest hash it looked at (`job_moved`).
pub(crate) async fn append(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
    headers: HeaderMap,
    body: Result<Json<AppendBody>, JsonRejection>,
) -> Result<Response, ApiError> {
    let key = idempotency_key(&headers)?;
    let expected = if_match(&headers)?;
    let Json(new) = body?;
    let checked = check_chunk(&new.items)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let id = Uuid::try_parse(&path.job).map_err(|_| ApiError::NotFound)?;
    let job = bulk::lock(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    if content_hash(&job.manifest_hash) != expected {
        return Err(ApiError::conflict(
            "job_moved",
            "the job moved; the ETag names it now",
        ));
    }
    if job.state != "accepting" {
        return Err(ApiError::conflict(
            "job_not_accepting",
            "this job's manifest is closed to new items",
        ));
    }
    if job.expected_items + i64::try_from(new.items.len()).unwrap_or(i64::MAX) > MAX_ITEMS {
        return Err(ApiError::Invalid(Refusal {
            code: "manifest_too_large",
            pointer: "/items".to_owned(),
            message: format!("a manifest carries at most {MAX_ITEMS} items"),
        }));
    }
    let collection = job.collection().unwrap_or_default().to_owned();
    check_items(&mut tx, &collection, &new.items).await?;
    let canonical_items = serde_json::to_string(&new.items).map_err(ApiError::internal)?;
    let request = request_hash(&[ITEMS, &path.job, &canonical_items]);
    if let Some(replay) = claim(&mut tx, ITEMS, key, &request).await? {
        return Ok(replay);
    }
    let (landed, duplicates) = bulk::append(&mut tx, id, &checked)
        .await
        .map_err(ApiError::from_db)?;
    if landed > 0 {
        budgets::reserve_bulk_documents(&mut tx, id, landed)
            .await
            .map_err(ApiError::from_db)?;
        let previous: [u8; 32] = job.manifest_hash.as_slice().try_into().unwrap_or([0; 32]);
        let next = roll_hash(&previous, &new.items);
        bulk::set_hash(&mut tx, id, &next)
            .await
            .map_err(ApiError::from_db)?;
    }
    if new.seal {
        bulk::seal(&mut tx, id).await.map_err(ApiError::from_db)?;
    }
    let detail = serde_json::json!({
        "appended": landed, "duplicates": duplicates.len(), "sealed": new.seal,
    })
    .to_string();
    colophon_db::audit::record_in(
        &mut tx.transaction,
        tx.project,
        tx.identity,
        "bulk.appended",
        &id.to_string(),
        &detail,
    )
    .await
    .map_err(ApiError::from_db)?;
    let job = bulk::get(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let body = serde_json::to_string(&Data {
        data: AppendView {
            job: view(job.clone())?,
            appended: landed,
            duplicates,
            sealed: new.seal,
        },
    })
    .map_err(ApiError::internal)?;
    finish(tx, ITEMS, key, StatusCode::ACCEPTED, &job, body).await
}

/// `GET {P}/bulk`, `?state=` filtered, cursored by job id.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ListPath>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Page<JobView>>, ApiError> {
    if let Some(state) = &query.state
        && !STATES.contains(&state.as_str())
    {
        return Err(ApiError::Invalid(Refusal {
            code: "invalid_state",
            pointer: "state".to_owned(),
            message: format!("a job's state is one of {}", STATES.join(", ")),
        }));
    }
    let after = query
        .cursor
        .as_deref()
        .map(parse_cursor)
        .transpose()?
        .and_then(|text| Uuid::try_parse(&text).ok());
    if query.cursor.is_some() && after.is_none() {
        return Err(ApiError::InvalidHeader(
            "a bulk jobs cursor is the one a page answered",
        ));
    }
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    // The state filter rides the SQL cheaply: a filtered list is still bounded.
    let jobs = match &query.state {
        Some(state) => {
            let mut all = Vec::new();
            let mut after = after;
            loop {
                let page = bulk::list(&mut tx, after, i64::try_from(DEFAULT_PAGE).unwrap_or(50))
                    .await
                    .map_err(ApiError::from_db)?;
                let Some(last) = page.last().map(|job| job.id) else {
                    break;
                };
                all.extend(page.into_iter().filter(|job| job.state == *state));
                after = Some(last);
                if all.len() >= DEFAULT_PAGE {
                    break;
                }
            }
            all.truncate(DEFAULT_PAGE);
            all
        }
        None => bulk::list(
            &mut tx,
            after,
            i64::try_from(DEFAULT_PAGE).unwrap_or(50) + 1,
        )
        .await
        .map_err(ApiError::from_db)?,
    };
    let next_cursor = match (query.state.is_none(), jobs.len() > DEFAULT_PAGE) {
        (true, true) => jobs.last().map(|job| cursor(&job.id.to_string())),
        _ => None,
    };
    let data = jobs
        .into_iter()
        .take(DEFAULT_PAGE)
        .map(view)
        .collect::<Result<Vec<_>, _>>()?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(Json(Page { data, next_cursor }))
}

/// `GET {P}/bulk/{job}`: one job, `ETag`'d with its manifest hash.
pub(crate) async fn get(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
) -> Result<Response, ApiError> {
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let id = Uuid::try_parse(&path.job).map_err(|_| ApiError::NotFound)?;
    let job = bulk::get(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let body = serde_json::to_string(&Data {
        data: view(job.clone())?,
    })
    .map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(respond(
        StatusCode::OK,
        body,
        &content_hash(&job.manifest_hash),
        false,
    ))
}

/// `GET {P}/bulk/{job}/items`: the resumable cursor over the manifest's items.
pub(crate) async fn items(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
    Query(query): Query<ItemsQuery>,
) -> Result<Json<Page<ItemView>>, ApiError> {
    if let Some(outcome) = &query.outcome
        && !OUTCOMES.contains(&outcome.as_str())
    {
        return Err(ApiError::Invalid(Refusal {
            code: "invalid_outcome",
            pointer: "outcome".to_owned(),
            message: format!("an item's outcome is one of {}", OUTCOMES.join(", ")),
        }));
    }
    let after = query.after.as_deref().map(parse_cursor).transpose()?;
    let limit = usize::try_from(query.limit.unwrap_or(50).clamp(1, 100)).unwrap_or(50);
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let id = Uuid::try_parse(&path.job).map_err(|_| ApiError::NotFound)?;
    let job = bulk::get(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let items = bulk::items(
        &mut tx,
        job.id,
        query.outcome.as_deref(),
        after.as_deref(),
        i64::try_from(limit).unwrap_or(50) + 1,
    )
    .await
    .map_err(ApiError::from_db)?;
    let next_cursor = items
        .len()
        .gt(&limit)
        .then(|| items.get(limit - 1).map(|item| cursor(&item.key)))
        .flatten();
    let data = items
        .into_iter()
        .take(limit)
        .map(item_view)
        .collect::<Result<Vec<_>, _>>()?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(Json(Page { data, next_cursor }))
}

/// `POST {P}/bulk/{job}/cancel`: the submitter, or a caller with `content.update` on the
/// collection — `bulk.cancelled` audited, held reservations released in the commit
/// (D-116).
pub(crate) async fn cancel(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let key = idempotency_key(&headers)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let id = Uuid::try_parse(&path.job).map_err(|_| ApiError::NotFound)?;
    let job = bulk::lock(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let collection = job.collection().unwrap_or_default().to_owned();
    if job.created_by != tx.identity {
        require(
            &tx.policy,
            Action::ContentUpdate,
            Some(&collection),
            "your role can't cancel this job",
        )?;
    }
    let request = request_hash(&[CANCEL, &path.job]);
    if let Some(replay) = claim(&mut tx, CANCEL, key, &request).await? {
        return Ok(replay);
    }
    let ran = job.started_at.is_some();
    if !bulk::cancel(&mut tx, id).await.map_err(ApiError::from_db)? {
        return Err(ApiError::conflict(
            "job_finished",
            "this job is already finished",
        ));
    }
    // What the job still held closes with its terminal state (D-113): the jobs slot
    // settles if it ran, releases if it never started; unspent documents release.
    budgets::close_bulk(&mut tx.transaction, tx.project, tx.environment, id, ran)
        .await
        .map_err(ApiError::from_db)?;
    let detail = serde_json::json!({
        "collection": collection,
        "hash": content_hash(&job.manifest_hash),
    })
    .to_string();
    colophon_db::audit::record_in(
        &mut tx.transaction,
        tx.project,
        tx.identity,
        "bulk.cancelled",
        &id.to_string(),
        &detail,
    )
    .await
    .map_err(ApiError::from_db)?;
    let job = bulk::get(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let body = serde_json::to_string(&Data {
        data: view(job.clone())?,
    })
    .map_err(ApiError::internal)?;
    finish(tx, CANCEL, key, StatusCode::OK, &job, body).await
}
