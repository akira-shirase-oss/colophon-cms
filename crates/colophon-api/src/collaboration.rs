//! Comments and editorial tasks (docs/modules/04-api.md; migration 0042, D-102). Under `{P}` =
//! `/api/v1/projects/{project}/environments/{environment}`:
//!
//! - `GET  {P}/collections/{collection}/entries/{entry}/comments`: the entry's comments, newest
//!   first, `?revision=` and `?state=open|resolved` filtered;
//! - `POST {P}/collections/{collection}/entries/{entry}/comments`: write one on a revision
//!   (`Idempotency-Key` required);
//! - `GET  {P}/collections/{collection}/entries/{entry}/comments/{comment}`: one, with its
//!   `ETag`;
//! - `POST {P}/collections/{collection}/entries/{entry}/comments/{comment}/resolve` and
//!   `…/reopen`: flip its resolution (`If-Match` and `Idempotency-Key` required);
//! - `GET  {P}/tasks`: the environment's tasks, `?entry=`, `?collection=`, `?assignee=` and
//!   `?status=` filtered; a task on an entry the role can't read is never listed;
//! - `POST {P}/tasks`: create one on an entry (`Idempotency-Key` required);
//! - `GET  {P}/tasks/{task}`: one, with its `ETag`;
//! - `PATCH {P}/tasks/{task}`: change its title, assignee or status (`If-Match` and
//!   `Idempotency-Key` required).
//!
//! A comment names a revision, not just an entry: what it says about is immutable, so it stays
//! true as drafts move on, and it is kept verbatim — never edited, never deleted, and sent as
//! text for the client to render escaped. Its `ETag` is a hash of what resolution state it was
//! in, like a changeset's hash names what was looked at (D-044); resolving or reopening against
//! anything else is refused `comment_moved`, and a task's `task_moved` works the same way.
//!
//! Reading needs the role's `content.read` on the entry — its own entries or anyone's, as the
//! role's `own` condition says (D-058); writing needs `content.update`. A comment or task on a
//! deleted entry stays answerable: every row copied `entry_created_by` when it was written, so
//! the same read rule applies after the entry itself can no longer be seen (0030, D-102).

use axum::{
    Json,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use colophon_db::{
    collaboration::{
        self, Comment, NewComment, NewTask, ReadScope, Task, TaskFilter, TaskStatus, TaskUpdate,
    },
    entries,
    idempotency::{self, Claim},
    scope::ProjectTransaction,
};
use colophon_domain::policy::{Action, Condition, Policy, Subject};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    entries::{
        content_hash, idempotency_key, json as respond_with, open, request_hash, rfc3339, sha256,
    },
    session::SignedIn,
    shape::{Data, Page},
    visibility::Access,
};

/// Comments and tasks per page unless `limit` says otherwise.
const DEFAULT_PAGE: i64 = 20;

const CREATE_COMMENT: &str = "comment.create";
const RESOLVE_COMMENT: &str = "comment.resolve";
const CREATE_TASK: &str = "task.create";
const UPDATE_TASK: &str = "task.update";

/// The path of the comment list routes: one entry's.
#[derive(Deserialize)]
pub(crate) struct CommentListPath {
    project: String,
    environment: String,
    collection: String,
    entry: String,
}

/// The path of the single-comment routes.
#[derive(Deserialize)]
pub(crate) struct CommentPath {
    project: String,
    environment: String,
    collection: String,
    entry: String,
    comment: String,
}

/// The path of the task list routes: the environment's.
#[derive(Deserialize)]
pub(crate) struct TaskListPath {
    project: String,
    environment: String,
}

/// The path of the single-task routes.
#[derive(Deserialize)]
pub(crate) struct TaskPath {
    project: String,
    environment: String,
    task: String,
}

/// `?limit=&cursor=&revision=&state=` for a comment list.
#[derive(Deserialize)]
pub(crate) struct CommentsQuery {
    limit: Option<i64>,
    cursor: Option<String>,
    revision: Option<String>,
    state: Option<String>,
}

/// `?limit=&cursor=&entry=&collection=&assignee=&status=` for the task list.
#[derive(Deserialize)]
pub(crate) struct TasksQuery {
    limit: Option<i64>,
    cursor: Option<String>,
    entry: Option<String>,
    collection: Option<String>,
    assignee: Option<String>,
    status: Option<String>,
}

/// `POST …/comments`: what a new comment says and where it points.
#[derive(Deserialize)]
pub(crate) struct NewCommentBody {
    /// The revision it is about.
    revision: String,
    /// Where in the content, if anywhere.
    field: Option<String>,
    /// What it says.
    body: String,
}

/// `POST {P}/tasks`: what has to be done and on what.
#[derive(Deserialize)]
pub(crate) struct NewTaskBody {
    /// The entry it is about.
    entry: String,
    /// What has to be done.
    title: String,
    /// The revision it is about, if one.
    revision: Option<String>,
    /// The changeset it is about, if one.
    changeset: Option<String>,
    /// Who it's waiting on, if anyone.
    assignee: Option<String>,
}

/// `PATCH {P}/tasks/{task}`: fields present change, absent stay; `assignee: null` unassigns.
#[derive(Deserialize)]
pub(crate) struct TaskUpdateBody {
    /// A new title.
    title: Option<String>,
    /// A new assignee, `null` for none, absent to leave it.
    #[serde(default)]
    assignee: AssigneeField,
    /// `open`, `done` or `cancelled`.
    status: Option<String>,
}

/// An `assignee` field's three states in a `PATCH` body (serde reads a bare `Option`'s `null`
/// as absent, so the states get their own type).
#[derive(Default)]
enum AssigneeField {
    /// Absent: leave it.
    #[default]
    Keep,
    /// `null`: nobody.
    Clear,
    /// An identity id.
    Set(String),
}

impl<'de> Deserialize<'de> for AssigneeField {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Option::<String>::deserialize(deserializer)?.map_or(Self::Clear, Self::Set))
    }
}

#[derive(Serialize)]
struct CommentView {
    id: Uuid,
    entry_id: Uuid,
    collection: String,
    revision_id: Uuid,
    /// Where in the content it points; `null` for the revision as a whole.
    field: Option<String>,
    /// What the author wrote, as text — the client renders it escaped, never as markup.
    body: String,
    author_id: Uuid,
    resolved_by: Option<Uuid>,
    resolved_at: Option<String>,
    created_at: String,
    /// What `If-Match` binds: the hash of what may change (D-102).
    etag: String,
}

impl CommentView {
    fn new(comment: &Comment) -> Result<Self, ApiError> {
        Ok(Self {
            id: comment.id,
            entry_id: comment.entry_id,
            collection: comment.collection.clone(),
            revision_id: comment.revision_id,
            field: comment.field.clone(),
            body: comment.body.clone(),
            author_id: comment.author_id,
            resolved_by: comment.resolved_by,
            resolved_at: comment.resolved_at.map(rfc3339).transpose()?,
            created_at: rfc3339(comment.created_at)?,
            etag: comment_etag(comment),
        })
    }
}

#[derive(Serialize)]
struct TaskView {
    id: Uuid,
    entry_id: Uuid,
    collection: String,
    revision_id: Option<Uuid>,
    changeset_id: Option<Uuid>,
    title: String,
    assignee_id: Option<Uuid>,
    status: String,
    created_by: Uuid,
    done_by: Option<Uuid>,
    done_at: Option<String>,
    created_at: String,
    updated_at: String,
    /// What `If-Match` binds.
    etag: String,
}

impl TaskView {
    fn new(task: &Task) -> Result<Self, ApiError> {
        Ok(Self {
            id: task.id,
            entry_id: task.entry_id,
            collection: task.collection.clone(),
            revision_id: task.revision_id,
            changeset_id: task.changeset_id,
            title: task.title.clone(),
            assignee_id: task.assignee_id,
            status: task.status.clone(),
            created_by: task.created_by,
            done_by: task.done_by,
            done_at: task.done_at.map(rfc3339).transpose()?,
            created_at: rfc3339(task.created_at)?,
            updated_at: rfc3339(task.updated_at)?,
            etag: task_etag(task),
        })
    }
}

/// The hash a comment's `ETag` carries: over its id and resolution pair, so any change to what
/// a resolve or reopen reads is a different tag.
fn comment_etag(comment: &Comment) -> String {
    let resolved_by = comment
        .resolved_by
        .map_or_else(|| "-".to_owned(), |id| id.to_string());
    let resolved_at = comment.resolved_at.map_or_else(
        || "-".to_owned(),
        |at| at.unix_timestamp_nanos().to_string(),
    );
    content_hash(&sha256(
        format!("v1:{}:{resolved_by}:{resolved_at}", comment.id).as_bytes(),
    ))
}

/// The hash a task's `ETag` carries: over everything an update may change, including the update
/// time so two identical edits still produce different tags.
fn task_etag(task: &Task) -> String {
    let id = |uuid: Option<Uuid>| uuid.map_or_else(|| "-".to_owned(), |id| id.to_string());
    let at = |at: Option<time::OffsetDateTime>| {
        at.map_or_else(
            || "-".to_owned(),
            |at| at.unix_timestamp_nanos().to_string(),
        )
    };
    content_hash(&sha256(
        format!(
            "v1:{}:{}:{}:{}:{}:{}:{}:{}",
            task.id,
            task.title,
            id(task.assignee_id),
            task.status,
            id(task.done_by),
            at(task.done_at),
            task.updated_at.unix_timestamp_nanos(),
            task.entry_created_by,
        )
        .as_bytes(),
    ))
}

/// `ETag: "<state hash>"`; an empty one when a replayed body lacked it, like the entry routes'.
fn respond(status: StatusCode, body: String, etag: &str, replayed: bool) -> Response {
    let mut response = respond_with(status, body, None, replayed);
    if let Ok(etag) = HeaderValue::from_str(&format!("\"{etag}\"")) {
        response.headers_mut().insert(header::ETAG, etag);
    }
    response
}

/// The `sha256:<hex>` an `If-Match` names, like a plan's.
fn if_match(headers: &HeaderMap, what: &'static str) -> Result<String, ApiError> {
    let value = headers
        .get(header::IF_MATCH)
        .ok_or(ApiError::PreconditionRequired(what))?;
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
            "If-Match must be one ETag exactly as a response's ETag header gave it, such as \"sha256:…\"",
        ))
}

/// Claims the idempotency key, or answers with the recorded response (its `ETag` read back from
/// `data.etag`, which every response here carries).
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
            let etag = serde_json::from_str::<Value>(&response)
                .ok()
                .and_then(|body| body["data"]["etag"].as_str().map(str::to_owned))
                .unwrap_or_default();
            Ok(Some(respond(status, response, &etag, true)))
        }
    }
}

/// Records the answer and commits before responding, with the resource's `ETag`.
async fn finish(
    mut tx: ProjectTransaction,
    operation: &str,
    key: &str,
    status: StatusCode,
    etag: &str,
    body: String,
) -> Result<Response, ApiError> {
    idempotency::complete(&mut tx, operation, key, status.as_u16(), &body)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(respond(status, body, etag, false))
}

/// Refuses unless the role may change entries of `collection` — anyone's or at least its own,
/// before the entry is looked at (the way a save checks first).
fn may_update(tx: &ProjectTransaction, collection: &str) -> Result<(), ApiError> {
    if [false, true].into_iter().all(|own| {
        tx.policy
            .fields(Action::ContentUpdate, collection, own)
            .is_none()
    }) {
        return Err(ApiError::missing(
            Action::ContentUpdate,
            "your role can't change anything here",
        ));
    }
    Ok(())
}

/// Whether the role may read the entry this comment or task is on, by whose entry it is: an
/// entry it may not read is not found, like the entry itself (D-058).
fn may_read(tx: &ProjectTransaction, collection: &str, entry_created_by: Uuid) -> bool {
    Access::of(&tx.policy, tx.identity, collection, entry_created_by)
        .read
        .is_some()
}

/// [`may_read`], for a write.
fn may_write(tx: &ProjectTransaction, collection: &str, entry_created_by: Uuid) -> bool {
    Access::of(&tx.policy, tx.identity, collection, entry_created_by)
        .update
        .is_some()
}

/// A field anchor's shape: the same path grammar `revision_references.field` holds (0025) —
/// a field id, then up to eight nested field ids or one- to two-digit positions.
fn check_field(field: &str) -> Result<&str, ApiError> {
    let field_id = |text: &str| {
        (1..=64).contains(&text.len())
            && text.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    };
    let position =
        |text: &str| (1..=2).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit());
    let segments: Vec<&str> = field.split('/').collect();
    let shape = field.len() <= 400
        && segments.len() <= 9
        && field_id(segments.first().unwrap_or(&""))
        && segments[1..]
            .iter()
            .all(|segment| field_id(segment) || position(segment));
    if shape {
        Ok(field)
    } else {
        Err(ApiError::InvalidField {
            reason: "invalid_field_path",
            pointer: "/field",
            message: "a field anchor looks like \"author\", \"sections/2/byline\" or \"people/0\""
                .to_owned(),
        })
    }
}

/// A comment's body: something written, at most what the schema holds.
fn check_body(body: &str) -> Result<(), ApiError> {
    if body.trim().is_empty() {
        return Err(ApiError::InvalidField {
            reason: "empty_body",
            pointer: "/body",
            message: "a comment needs something written".to_owned(),
        });
    }
    if body.chars().count() > 8000 {
        return Err(ApiError::InvalidField {
            reason: "body_too_long",
            pointer: "/body",
            message: "a comment is at most 8,000 characters".to_owned(),
        });
    }
    Ok(())
}

/// An id held in a string field, or a 422 naming it.
fn uuid_field(
    value: Option<&String>,
    pointer: &'static str,
    reason: &'static str,
) -> Result<Option<Uuid>, ApiError> {
    value
        .map(|text| {
            Uuid::try_parse(text).map_err(|_| ApiError::InvalidField {
                reason,
                pointer,
                message: "this must be an id".to_owned(),
            })
        })
        .transpose()
}

/// Whether `assignee` is a member of this project, when one is named.
async fn check_assignee(
    tx: &mut ProjectTransaction,
    assignee: Option<Uuid>,
) -> Result<(), ApiError> {
    if let Some(assignee) = assignee
        && collaboration::member_role(tx, assignee)
            .await
            .map_err(ApiError::from_db)?
            .is_none()
    {
        return Err(ApiError::InvalidField {
            reason: "assignee_not_member",
            pointer: "/assignee",
            message: "a task's assignee must be a member of this project".to_owned(),
        });
    }
    Ok(())
}

/// A task's audit detail: what it is and where it stands.
fn task_detail(task: &Task) -> String {
    json!({
        "entry": task.entry_id,
        "collection": task.collection,
        "revision": task.revision_id,
        "changeset": task.changeset_id,
        "title": task.title,
        "assignee": task.assignee_id,
        "status": task.status,
    })
    .to_string()
}

/// Audits `action` on `task` and answers with it recorded, `ETag` and all: the shared tail of
/// creating and updating one.
async fn finish_task(
    mut tx: ProjectTransaction,
    operation: &str,
    key: &str,
    status: StatusCode,
    task: &Task,
    action: &'static str,
) -> Result<Response, ApiError> {
    colophon_db::audit::record_in(
        &mut tx.transaction,
        tx.project,
        tx.identity,
        action,
        &task.id.to_string(),
        &task_detail(task),
    )
    .await
    .map_err(ApiError::from_db)?;
    let etag = task_etag(task);
    let body = serde_json::to_string(&Data {
        data: TaskView::new(task)?,
    })
    .map_err(ApiError::internal)?;
    finish(tx, operation, key, status, &etag, body).await
}

/// A task's title: something to do, at most what the schema holds.
fn check_title(title: &str) -> Result<(), ApiError> {
    if title.trim().is_empty() {
        return Err(ApiError::InvalidField {
            reason: "empty_title",
            pointer: "/title",
            message: "a task needs a title".to_owned(),
        });
    }
    if title.chars().count() > 200 {
        return Err(ApiError::InvalidField {
            reason: "title_too_long",
            pointer: "/title",
            message: "a task title is at most 200 characters".to_owned(),
        });
    }
    Ok(())
}

/// A page size, checked the same way as the entries list's.
fn check_limit(limit: Option<i64>) -> Result<i64, ApiError> {
    let limit = limit.unwrap_or(DEFAULT_PAGE);
    if !(1..=collaboration::MAX_PAGE).contains(&limit) {
        return Err(ApiError::InvalidHeader("limit must be between 1 and 100"));
    }
    Ok(limit)
}

/// A cursor is bound to the project, environment and list it came from (`scope` names the
/// list: `collection/entry` for comments, `tasks` for the task list), as the entries cursor is.
fn cursor(project: Uuid, environment: &str, scope: &str, last: Uuid) -> String {
    URL_SAFE_NO_PAD.encode(format!("v1:{project}:{environment}:{scope}:{last}"))
}

fn after_cursor(
    project: Uuid,
    environment: &str,
    scope: &str,
    cursor: &str,
) -> Result<Uuid, ApiError> {
    let prefix = format!("v1:{project}:{environment}:{scope}:");
    URL_SAFE_NO_PAD
        .decode(cursor)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| Uuid::try_parse(text.strip_prefix(&prefix)?).ok())
        .ok_or(ApiError::InvalidHeader(
            "this cursor is not valid for this list",
        ))
}

/// Who created `entry` in `collection`, live or deleted: the live row when there is one, else
/// what a comment of the entry remembered (a deleted entry stays answerable, D-102). `None`
/// when there is no such entry — or none the caller may see — with comments to tell.
async fn entry_creator(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry: Uuid,
) -> Result<Option<Uuid>, ApiError> {
    let live = entries::creators(tx, &[entry])
        .await
        .map_err(ApiError::from_db)?
        .into_iter()
        .find(|(_, found, _)| found == collection)
        .map(|(_, _, created_by)| created_by);
    match live {
        Some(created_by) => Ok(Some(created_by)),
        None => collaboration::entry_creator(tx, collection, entry)
            .await
            .map_err(ApiError::from_db),
    }
}

/// `GET {P}/collections/{collection}/entries/{entry}/comments`.
pub(crate) async fn comments(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<CommentListPath>,
    Query(query): Query<CommentsQuery>,
) -> Result<Response, ApiError> {
    let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
    let limit = check_limit(query.limit)?;
    let revision = query
        .revision
        .as_deref()
        .map(|revision| {
            Uuid::try_parse(revision)
                .map_err(|_| ApiError::InvalidHeader("revision must be a revision id"))
        })
        .transpose()?;
    let resolved = query
        .state
        .as_deref()
        .map(|state| match state {
            "open" => Ok(false),
            "resolved" => Ok(true),
            _ => Err(ApiError::InvalidHeader("state must be open or resolved")),
        })
        .transpose()?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let created_by = entry_creator(&mut tx, &path.collection, entry)
        .await?
        .ok_or(ApiError::NotFound)?;
    if !may_read(&tx, &path.collection, created_by) {
        return Err(ApiError::NotFound);
    }
    let scope = format!("{}:{entry}", path.collection);
    let before = query
        .cursor
        .as_deref()
        .map(|cursor| after_cursor(tx.project, &path.environment, &scope, cursor))
        .transpose()?;
    let mut rows = collaboration::comments(
        &mut tx,
        &path.collection,
        entry,
        revision,
        resolved,
        before,
        limit + 1,
    )
    .await
    .map_err(ApiError::from_db)?;
    let project = tx.project;
    tx.commit().await.map_err(ApiError::from_db)?;
    let more = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
    rows.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    let next_cursor = more
        .then(|| {
            rows.last()
                .map(|row| cursor(project, &path.environment, &scope, row.id))
        })
        .flatten();
    let data = rows
        .iter()
        .map(CommentView::new)
        .collect::<Result<Vec<_>, ApiError>>()?;
    let body = serde_json::to_string(&Page { data, next_cursor }).map_err(ApiError::internal)?;
    Ok(respond_with(StatusCode::OK, body, None, false))
}

/// `POST {P}/collections/{collection}/entries/{entry}/comments`.
pub(crate) async fn create_comment(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<CommentListPath>,
    headers: HeaderMap,
    body: Result<Json<NewCommentBody>, JsonRejection>,
) -> Result<Response, ApiError> {
    let key = idempotency_key(&headers)?;
    let Json(new) = body?;
    let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
    let revision = Uuid::try_parse(&new.revision).map_err(|_| ApiError::InvalidField {
        reason: "invalid_revision",
        pointer: "/revision",
        message: "a comment names a revision id".to_owned(),
    })?;
    let field = new.field.as_deref().map(check_field).transpose()?;
    check_body(&new.body)?;
    let request = request_hash(&[
        CREATE_COMMENT,
        &path.collection,
        &path.entry,
        &new.revision,
        field.unwrap_or(""),
        &new.body,
    ]);
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_update(&tx, &path.collection)?;
    if let Some(replay) = claim(&mut tx, CREATE_COMMENT, key, &request).await? {
        return Ok(replay);
    }
    // Only a live entry takes new comments — a deleted one's stay in history, readable.
    let created_by = entries::creators(&mut tx, &[entry])
        .await
        .map_err(ApiError::from_db)?
        .into_iter()
        .find(|(_, found, _)| *found == path.collection)
        .map(|(_, _, created_by)| created_by)
        .ok_or(ApiError::NotFound)?;
    if !may_write(&tx, &path.collection, created_by) {
        return Err(ApiError::missing(
            Action::ContentUpdate,
            "your role can't write comments here",
        ));
    }
    if !collaboration::revision_exists(&mut tx, &path.collection, entry, revision)
        .await
        .map_err(ApiError::from_db)?
    {
        return Err(ApiError::InvalidField {
            reason: "revision_not_found",
            pointer: "/revision",
            message: "this entry has no such revision".to_owned(),
        });
    }
    let comment = collaboration::create_comment(
        &mut tx,
        &NewComment {
            id: Uuid::now_v7(),
            entry_id: entry,
            collection: &path.collection,
            revision_id: revision,
            field,
            body: &new.body,
            entry_created_by: created_by,
        },
    )
    .await
    .map_err(ApiError::from_db)?;
    let detail = json!({
        "entry": comment.entry_id,
        "collection": comment.collection,
        "revision": comment.revision_id,
        "field": comment.field,
    })
    .to_string();
    colophon_db::audit::record_in(
        &mut tx.transaction,
        tx.project,
        tx.identity,
        "comment.created",
        &comment.id.to_string(),
        &detail,
    )
    .await
    .map_err(ApiError::from_db)?;
    let etag = comment_etag(&comment);
    let body = serde_json::to_string(&Data {
        data: CommentView::new(&comment)?,
    })
    .map_err(ApiError::internal)?;
    finish(tx, CREATE_COMMENT, key, StatusCode::CREATED, &etag, body).await
}

/// `GET {P}/collections/{collection}/entries/{entry}/comments/{comment}`.
pub(crate) async fn comment(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<CommentPath>,
) -> Result<Response, ApiError> {
    let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
    let comment_id = Uuid::try_parse(&path.comment).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let comment = collaboration::comment(&mut tx, &path.collection, entry, comment_id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    if !may_read(&tx, &comment.collection, comment.entry_created_by) {
        return Err(ApiError::NotFound);
    }
    let etag = comment_etag(&comment);
    let body = serde_json::to_string(&Data {
        data: CommentView::new(&comment)?,
    })
    .map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(respond(StatusCode::OK, body, &etag, false))
}

/// `POST …/comments/{comment}/resolve` and `…/reopen`: flip its resolution, against the state
/// `If-Match` names.
async fn set_resolution(
    state: AppState,
    caller: SignedIn,
    path: CommentPath,
    headers: HeaderMap,
    resolved: bool,
) -> Result<Response, ApiError> {
    let expected = if_match(
        &headers,
        "this needs an If-Match header with the comment's ETag, so you act on what you looked at",
    )?;
    let key = idempotency_key(&headers)?;
    let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
    let comment_id = Uuid::try_parse(&path.comment).map_err(|_| ApiError::NotFound)?;
    let action = if resolved { "resolve" } else { "reopen" };
    let request = request_hash(&[
        RESOLVE_COMMENT,
        &path.collection,
        &path.entry,
        &path.comment,
        action,
        &expected,
    ]);
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_update(&tx, &path.collection)?;
    if let Some(replay) = claim(&mut tx, RESOLVE_COMMENT, key, &request).await? {
        return Ok(replay);
    }
    let comment = collaboration::lock_comment(&mut tx, &path.collection, entry, comment_id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    if !may_write(&tx, &comment.collection, comment.entry_created_by) {
        return Err(ApiError::missing(
            Action::ContentUpdate,
            "your role can't change comments here",
        ));
    }
    let current = comment_etag(&comment);
    if current != expected {
        return Err(ApiError::conflict(
            "comment_moved",
            "the comment has changed since you loaded it; load it again and check what changed",
        ));
    }
    if resolved == comment.resolved_at.is_some() {
        return Err(if resolved {
            ApiError::conflict("already_resolved", "this comment is already resolved")
        } else {
            ApiError::conflict("already_open", "this comment is not resolved")
        });
    }
    let actor = tx.identity;
    let comment = collaboration::set_resolved(&mut tx, comment_id, resolved, actor)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let detail = json!({
        "entry": comment.entry_id,
        "collection": comment.collection,
        "revision": comment.revision_id,
    })
    .to_string();
    colophon_db::audit::record_in(
        &mut tx.transaction,
        tx.project,
        tx.identity,
        if resolved {
            "comment.resolved"
        } else {
            "comment.reopened"
        },
        &comment.id.to_string(),
        &detail,
    )
    .await
    .map_err(ApiError::from_db)?;
    let etag = comment_etag(&comment);
    let body = serde_json::to_string(&Data {
        data: CommentView::new(&comment)?,
    })
    .map_err(ApiError::internal)?;
    finish(tx, RESOLVE_COMMENT, key, StatusCode::OK, &etag, body).await
}

/// `POST {P}/collections/{collection}/entries/{entry}/comments/{comment}/resolve`.
pub(crate) async fn resolve(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<CommentPath>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    set_resolution(state, caller, path, headers, true).await
}

/// `POST {P}/collections/{collection}/entries/{entry}/comments/{comment}/reopen`.
pub(crate) async fn reopen(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<CommentPath>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    set_resolution(state, caller, path, headers, false).await
}

/// What of the environment's tasks `policy` may see (D-058): every collection's where it reads
/// anyone's entries, or its own entries' tasks where the `own` condition holds.
fn read_scope(policy: &Policy) -> ReadScope {
    let mut scope = ReadScope::default();
    for permission in policy.permissions() {
        if permission.action != Action::ContentRead {
            continue;
        }
        match (&permission.subject, permission.condition) {
            (Subject::All, None) => {
                scope.all = true;
                break;
            }
            (Subject::All, Some(Condition::Own)) => scope.own_anywhere = true,
            (Subject::Collection(collection), None) => {
                scope.collections.push(collection.clone());
            }
            (Subject::Collection(collection), Some(Condition::Own)) => {
                scope.own_collections.push(collection.clone());
            }
        }
    }
    scope
}

/// `GET {P}/tasks`.
pub(crate) async fn tasks(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<TaskListPath>,
    Query(query): Query<TasksQuery>,
) -> Result<Response, ApiError> {
    let limit = check_limit(query.limit)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let scope = read_scope(&tx.policy);
    let before = query
        .cursor
        .as_deref()
        .map(|cursor| after_cursor(tx.project, &path.environment, "tasks", cursor))
        .transpose()?;
    let filter = TaskFilter {
        entry_id: query
            .entry
            .as_deref()
            .map(|entry| {
                Uuid::try_parse(entry)
                    .map_err(|_| ApiError::InvalidHeader("entry must be an entry id"))
            })
            .transpose()?,
        collection: query.collection.as_deref(),
        assignee: query
            .assignee
            .as_deref()
            .map(|assignee| match assignee {
                "me" => Ok(collaboration::Assignee::To(tx.identity)),
                "none" => Ok(collaboration::Assignee::Clear),
                id => Uuid::try_parse(id)
                    .map(collaboration::Assignee::To)
                    .map_err(|_| ApiError::InvalidHeader("assignee must be me, none or an id")),
            })
            .transpose()?,
        status: query
            .status
            .as_deref()
            .map(|status| {
                TaskStatus::parse(status).ok_or(ApiError::InvalidHeader(
                    "status must be open, done or cancelled",
                ))
            })
            .transpose()?,
        before,
    };
    let mut rows = collaboration::tasks(&mut tx, &filter, &scope, limit + 1)
        .await
        .map_err(ApiError::from_db)?;
    let project = tx.project;
    tx.commit().await.map_err(ApiError::from_db)?;
    let more = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
    rows.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    let next_cursor = more
        .then(|| {
            rows.last()
                .map(|row| cursor(project, &path.environment, "tasks", row.id))
        })
        .flatten();
    let data = rows
        .iter()
        .map(TaskView::new)
        .collect::<Result<Vec<_>, ApiError>>()?;
    let body = serde_json::to_string(&Page { data, next_cursor }).map_err(ApiError::internal)?;
    Ok(respond_with(StatusCode::OK, body, None, false))
}

/// `POST {P}/tasks`.
pub(crate) async fn create_task(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<TaskListPath>,
    headers: HeaderMap,
    body: Result<Json<NewTaskBody>, JsonRejection>,
) -> Result<Response, ApiError> {
    let key = idempotency_key(&headers)?;
    let Json(new) = body?;
    check_title(&new.title)?;
    let entry = Uuid::try_parse(&new.entry).map_err(|_| ApiError::InvalidField {
        reason: "invalid_entry",
        pointer: "/entry",
        message: "a task names an entry id".to_owned(),
    })?;
    let revision = uuid_field(new.revision.as_ref(), "/revision", "invalid_revision")?;
    let changeset = uuid_field(new.changeset.as_ref(), "/changeset", "invalid_changeset")?;
    let assignee = uuid_field(new.assignee.as_ref(), "/assignee", "invalid_assignee")?;
    let request = request_hash(&[
        CREATE_TASK,
        &new.entry,
        &new.title,
        new.revision.as_deref().unwrap_or(""),
        new.changeset.as_deref().unwrap_or(""),
        new.assignee.as_deref().unwrap_or(""),
    ]);
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    if let Some(replay) = claim(&mut tx, CREATE_TASK, key, &request).await? {
        return Ok(replay);
    }
    let (collection, created_by) = entries::creators(&mut tx, &[entry])
        .await
        .map_err(ApiError::from_db)?
        .into_iter()
        .map(|(_, collection, created_by)| (collection, created_by))
        .next()
        .ok_or(ApiError::NotFound)?;
    may_update(&tx, &collection)?;
    if !may_write(&tx, &collection, created_by) {
        return Err(ApiError::missing(
            Action::ContentUpdate,
            "your role can't change this entry",
        ));
    }
    if let Some(revision) = revision
        && !collaboration::revision_exists(&mut tx, &collection, entry, revision)
            .await
            .map_err(ApiError::from_db)?
    {
        return Err(ApiError::InvalidField {
            reason: "revision_not_found",
            pointer: "/revision",
            message: "this entry has no such revision".to_owned(),
        });
    }
    if let Some(changeset) = changeset
        && !collaboration::changeset_exists(&mut tx, changeset)
            .await
            .map_err(ApiError::from_db)?
    {
        return Err(ApiError::InvalidField {
            reason: "changeset_not_found",
            pointer: "/changeset",
            message: "no such changeset in this environment".to_owned(),
        });
    }
    check_assignee(&mut tx, assignee).await?;
    let task = collaboration::create_task(
        &mut tx,
        &NewTask {
            id: Uuid::now_v7(),
            entry_id: entry,
            collection: &collection,
            revision_id: revision,
            changeset_id: changeset,
            title: &new.title,
            assignee_id: assignee,
            entry_created_by: created_by,
        },
    )
    .await
    .map_err(ApiError::from_db)?;
    finish_task(
        tx,
        CREATE_TASK,
        key,
        StatusCode::CREATED,
        &task,
        "task.created",
    )
    .await
}

/// `GET {P}/tasks/{task}`.
pub(crate) async fn task(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<TaskPath>,
) -> Result<Response, ApiError> {
    let task_id = Uuid::try_parse(&path.task).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let task = collaboration::task(&mut tx, task_id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    if !may_read(&tx, &task.collection, task.entry_created_by) {
        return Err(ApiError::NotFound);
    }
    let etag = task_etag(&task);
    let body = serde_json::to_string(&Data {
        data: TaskView::new(&task)?,
    })
    .map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(respond(StatusCode::OK, body, &etag, false))
}

/// What a `PATCH` body parsed to: each `None` leaves its column.
struct TaskChange {
    /// A new title.
    title: Option<String>,
    /// A new assignee.
    assignee: Option<collaboration::Assignee>,
    /// A new status.
    status: Option<TaskStatus>,
}

/// `update`'s fields checked and parsed, or a 422; `nothing_to_change` when there's nothing.
fn task_change(update: &TaskUpdateBody) -> Result<TaskChange, ApiError> {
    if let Some(title) = update.title.as_deref() {
        check_title(title)?;
    }
    let status = update
        .status
        .as_deref()
        .map(|status| {
            TaskStatus::parse(status).ok_or(ApiError::InvalidField {
                reason: "invalid_status",
                pointer: "/status",
                message: "status must be open, done or cancelled".to_owned(),
            })
        })
        .transpose()?;
    let assignee = match &update.assignee {
        AssigneeField::Keep => None,
        AssigneeField::Clear => Some(collaboration::Assignee::Clear),
        AssigneeField::Set(id) => Some(collaboration::Assignee::To(Uuid::try_parse(id).map_err(
            |_| ApiError::InvalidField {
                reason: "invalid_assignee",
                pointer: "/assignee",
                message: "an assignee must be an identity id".to_owned(),
            },
        )?)),
    };
    if update.title.is_none() && matches!(update.assignee, AssigneeField::Keep) && status.is_none()
    {
        return Err(ApiError::InvalidField {
            reason: "nothing_to_change",
            pointer: "/",
            message: "a change needs a title, an assignee or a status".to_owned(),
        });
    }
    Ok(TaskChange {
        title: update.title.clone(),
        assignee,
        status,
    })
}

/// `PATCH {P}/tasks/{task}`.
pub(crate) async fn update_task(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<TaskPath>,
    headers: HeaderMap,
    body: Result<Json<TaskUpdateBody>, JsonRejection>,
) -> Result<Response, ApiError> {
    let expected = if_match(
        &headers,
        "this needs an If-Match header with the task's ETag, so you change what you looked at",
    )?;
    let key = idempotency_key(&headers)?;
    let Json(update) = body?;
    let change = task_change(&update)?;
    let task_id = Uuid::try_parse(&path.task).map_err(|_| ApiError::NotFound)?;
    let request = request_hash(&[
        UPDATE_TASK,
        &path.task,
        &expected,
        update.title.as_deref().unwrap_or(""),
        match &update.assignee {
            AssigneeField::Keep => "-",
            AssigneeField::Clear => "\u{0}",
            AssigneeField::Set(id) => id.as_str(),
        },
        update.status.as_deref().unwrap_or(""),
    ]);
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    if let Some(replay) = claim(&mut tx, UPDATE_TASK, key, &request).await? {
        return Ok(replay);
    }
    let task = collaboration::lock_task(&mut tx, task_id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    may_update(&tx, &task.collection)?;
    if !may_write(&tx, &task.collection, task.entry_created_by) {
        return Err(ApiError::missing(
            Action::ContentUpdate,
            "your role can't change tasks here",
        ));
    }
    if task_etag(&task) != expected {
        return Err(ApiError::conflict(
            "task_moved",
            "the task has changed since you loaded it; load it again and check what changed",
        ));
    }
    check_assignee(
        &mut tx,
        match change.assignee {
            Some(collaboration::Assignee::To(id)) => Some(id),
            _ => None,
        },
    )
    .await?;
    let actor = tx.identity;
    let task = collaboration::update_task(
        &mut tx,
        task_id,
        &TaskUpdate {
            title: change.title.as_deref(),
            assignee: change.assignee,
            status: change.status,
        },
        actor,
    )
    .await
    .map_err(ApiError::from_db)?
    .ok_or(ApiError::NotFound)?;
    finish_task(tx, UPDATE_TASK, key, StatusCode::OK, &task, "task.updated").await
}
