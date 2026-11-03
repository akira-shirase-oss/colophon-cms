//! Comments and editorial tasks (docs/modules/02-db.md; migration 0042, D-102).
//!
//! A [`Comment`] is bound to one immutable revision of one entry — never the entry alone — so
//! what it says about is exactly what its author saw, and it stays in history forever: the API
//! role may insert comments and flip their resolution pair, but can never rewrite or delete one.
//! Each row copies its entry's `created_by`, so a deleted entry's comments keep their visibility
//! rule after the application's role can no longer read the entry (0030).
//!
//! A [`Task`] is editorial work on an entry — optionally one revision of it, or one changeset —
//! with a title, an assignee and a status. Its mutable columns are its lifecycle's only; marking
//! it `done` records who did it and when as a pair.
//!
//! Everything runs inside a [`ProjectTransaction`]: the same membership check and row-level
//! security context that scope entries scope these, so a comment or task can never cross a
//! project (A6). Which member may do what is decided in the API with the one policy evaluator
//! (D-058); here the shapes and the storage invariants hold.

use sqlx::types::time::OffsetDateTime;
use uuid::Uuid;

use crate::{DbError, scope::ProjectTransaction};

/// The most comments or tasks one page returns, newest first.
pub const MAX_PAGE: i64 = 100;

/// A comment on one revision of one entry (D-102).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    /// Its id (time-ordered).
    pub id: Uuid,
    /// The entry it belongs to.
    pub entry_id: Uuid,
    /// The entry's collection.
    pub collection: String,
    /// The revision it was written about.
    pub revision_id: Uuid,
    /// Where in the content it points, as revision references record it; `None` when it is
    /// about the revision as a whole.
    pub field: Option<String>,
    /// What the author wrote, verbatim.
    pub body: String,
    /// Who wrote it.
    pub author_id: Uuid,
    /// Who created the entry, as it was when the comment was written.
    pub entry_created_by: Uuid,
    /// Who resolved it, if anyone has.
    pub resolved_by: Option<Uuid>,
    /// When.
    pub resolved_at: Option<OffsetDateTime>,
    /// When it was written.
    pub created_at: OffsetDateTime,
}

/// A comment to write; the API has checked the entry, the revision and the field anchor.
#[derive(Debug)]
pub struct NewComment<'a> {
    /// The comment's id.
    pub id: Uuid,
    /// The entry.
    pub entry_id: Uuid,
    /// The entry's collection.
    pub collection: &'a str,
    /// The revision it is about.
    pub revision_id: Uuid,
    /// Where in the content it points, if anywhere.
    pub field: Option<&'a str>,
    /// What it says.
    pub body: &'a str,
    /// Who created the entry.
    pub entry_created_by: Uuid,
}

/// A task's status (migration 0042).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Work waiting.
    Open,
    /// Finished; `done_by` and `done_at` say by whom and when.
    Done,
    /// Given up on.
    Cancelled,
}

impl TaskStatus {
    /// The stored name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }

    /// The status with this stored name.
    pub fn parse(name: &str) -> Option<Self> {
        [Self::Open, Self::Done, Self::Cancelled]
            .into_iter()
            .find(|status| status.as_str() == name)
    }
}

/// A task on an entry (D-102).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// Its id (time-ordered).
    pub id: Uuid,
    /// The entry it is about.
    pub entry_id: Uuid,
    /// The entry's collection.
    pub collection: String,
    /// The revision it is about, when it is about one.
    pub revision_id: Option<Uuid>,
    /// The changeset it is about, when it is about one.
    pub changeset_id: Option<Uuid>,
    /// What has to be done.
    pub title: String,
    /// Who it's waiting on, if anyone.
    pub assignee_id: Option<Uuid>,
    /// `open`, `done` or `cancelled`.
    pub status: String,
    /// Who created it.
    pub created_by: Uuid,
    /// Who created the entry, as it was when the task was written.
    pub entry_created_by: Uuid,
    /// Who marked it done, when it is.
    pub done_by: Option<Uuid>,
    /// When.
    pub done_at: Option<OffsetDateTime>,
    /// When it was created.
    pub created_at: OffsetDateTime,
    /// When it last changed.
    pub updated_at: OffsetDateTime,
}

/// A task to write; the API has checked the entry, the revision and the assignee's membership.
#[derive(Debug)]
pub struct NewTask<'a> {
    /// The task's id.
    pub id: Uuid,
    /// The entry.
    pub entry_id: Uuid,
    /// The entry's collection.
    pub collection: &'a str,
    /// The revision it is about, if one.
    pub revision_id: Option<Uuid>,
    /// The changeset it is about, if one.
    pub changeset_id: Option<Uuid>,
    /// What has to be done.
    pub title: &'a str,
    /// Who it's waiting on, if anyone.
    pub assignee_id: Option<Uuid>,
    /// Who created the entry.
    pub entry_created_by: Uuid,
}

/// Who a task waits on, as a write or a filter names it: `Clear` is nobody.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assignee {
    /// Unassign the task, or filter to the unassigned.
    Clear,
    /// Assign to, or filter to, this member.
    To(Uuid),
}

/// What a task update changes: `None` leaves a column.
#[derive(Debug, Default)]
pub struct TaskUpdate<'a> {
    /// A new title.
    pub title: Option<&'a str>,
    /// A new assignee.
    pub assignee: Option<Assignee>,
    /// A new status.
    pub status: Option<TaskStatus>,
}

/// How task rows read.
#[derive(Debug, Default)]
pub struct TaskFilter<'a> {
    /// One entry's only.
    pub entry_id: Option<Uuid>,
    /// One collection's only.
    pub collection: Option<&'a str>,
    /// One assignee's only.
    pub assignee: Option<Assignee>,
    /// One status's only.
    pub status: Option<TaskStatus>,
    /// Strictly before this id (the page cursor).
    pub before: Option<Uuid>,
}

/// The entries whose tasks a role may see, for the environment's task list: `all` sees every
/// task; otherwise `collections` are where it reads anyone's entries, `own_anywhere` reads its
/// own entries' tasks project-wide, and `own_collections` its own entries' tasks in those
/// collections. All empty or false answers nothing, like a list a role can't read (D-058).
#[derive(Debug, Default)]
pub struct ReadScope {
    /// It reads every collection's entries.
    pub all: bool,
    /// Collections where it reads anyone's entries.
    pub collections: Vec<String>,
    /// Collections where it reads only its own entries.
    pub own_collections: Vec<String>,
    /// It reads its own entries in every collection.
    pub own_anywhere: bool,
}

/// Writes `comment` as the transaction's identity; the row it became.
pub async fn create_comment(
    tx: &mut ProjectTransaction,
    comment: &NewComment<'_>,
) -> Result<Comment, DbError> {
    sqlx::query_as!(
        Comment,
        "INSERT INTO comments
            (project_id, environment_id, id, entry_id, collection, revision_id, field, body,
             author_id, entry_created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING id, entry_id, collection, revision_id, field, body, author_id,
                   entry_created_by, resolved_by, resolved_at, created_at",
        tx.project,
        tx.environment,
        comment.id,
        comment.entry_id,
        comment.collection,
        comment.revision_id,
        comment.field,
        comment.body,
        tx.identity,
        comment.entry_created_by,
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)
}

/// One of `entry`'s comments, if it's there; the caller scopes by collection too, so a comment
/// under another collection's entry can't surface.
pub async fn comment(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry_id: Uuid,
    comment_id: Uuid,
) -> Result<Option<Comment>, DbError> {
    sqlx::query_as!(
        Comment,
        "SELECT id, entry_id, collection, revision_id, field, body, author_id, entry_created_by,
                resolved_by, resolved_at, created_at
         FROM comments
         WHERE project_id = $1 AND environment_id = $2 AND collection = $3 AND entry_id = $4
           AND id = $5",
        tx.project,
        tx.environment,
        collection,
        entry_id,
        comment_id,
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// [`comment`], locking the row for a resolution change: a resolve or reopen that commits
/// decides the race with another one doing the same.
pub async fn lock_comment(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry_id: Uuid,
    comment_id: Uuid,
) -> Result<Option<Comment>, DbError> {
    sqlx::query_as!(
        Comment,
        "SELECT id, entry_id, collection, revision_id, field, body, author_id, entry_created_by,
                resolved_by, resolved_at, created_at
         FROM comments
         WHERE project_id = $1 AND environment_id = $2 AND collection = $3 AND entry_id = $4
           AND id = $5
         FOR UPDATE",
        tx.project,
        tx.environment,
        collection,
        entry_id,
        comment_id,
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// `entry`'s comments, newest first, at most `limit` (capped at one more than [`MAX_PAGE`] so a
/// caller can learn whether another page follows): only `revision`'s when given; `resolved` keeps
/// only the resolved (`Some(true)`) or only the open (`Some(false)`) ones.
pub async fn comments(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry_id: Uuid,
    revision_id: Option<Uuid>,
    resolved: Option<bool>,
    before: Option<Uuid>,
    limit: i64,
) -> Result<Vec<Comment>, DbError> {
    sqlx::query_as!(
        Comment,
        "SELECT id, entry_id, collection, revision_id, field, body, author_id, entry_created_by,
                resolved_by, resolved_at, created_at
         FROM comments
         WHERE project_id = $1 AND environment_id = $2 AND collection = $3 AND entry_id = $4
           AND ($5::uuid IS NULL OR revision_id = $5)
           AND ($6::bool IS NULL OR (resolved_at IS NOT NULL) = $6)
           AND ($7::uuid IS NULL OR id < $7)
         ORDER BY id DESC
         LIMIT $8",
        tx.project,
        tx.environment,
        collection,
        entry_id,
        revision_id,
        resolved,
        before,
        limit.clamp(1, MAX_PAGE + 1),
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Who created the entry an environment's comments name: the visibility of a deleted entry's
/// comments, when the application's role can no longer read the entry itself (0030).
pub async fn entry_creator(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry_id: Uuid,
) -> Result<Option<Uuid>, DbError> {
    sqlx::query_scalar!(
        "SELECT entry_created_by FROM comments
         WHERE project_id = $1 AND environment_id = $2 AND collection = $3 AND entry_id = $4
         LIMIT 1",
        tx.project,
        tx.environment,
        collection,
        entry_id,
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Flips `comment`'s resolution pair: `actor` and now when `resolved`, neither when not. The
/// row afterwards, or `None` if it isn't there.
pub async fn set_resolved(
    tx: &mut ProjectTransaction,
    comment_id: Uuid,
    resolved: bool,
    actor: Uuid,
) -> Result<Option<Comment>, DbError> {
    sqlx::query_as!(
        Comment,
        "UPDATE comments
         SET resolved_by = CASE WHEN $4 THEN $5::uuid ELSE NULL END,
             resolved_at = CASE WHEN $4 THEN now() ELSE NULL END
         WHERE project_id = $1 AND environment_id = $2 AND id = $3
         RETURNING id, entry_id, collection, revision_id, field, body, author_id,
                   entry_created_by, resolved_by, resolved_at, created_at",
        tx.project,
        tx.environment,
        comment_id,
        resolved,
        actor,
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)
}

/// Whether `revision_id` is one of `entry`'s revisions in `collection` — the anchor a comment
/// or a task may name. Deleted entries keep their revisions (0030); whether the entry itself is
/// live is a separate check ([`create_comment`] fails it as a foreign-key violation).
pub async fn revision_exists(
    tx: &mut ProjectTransaction,
    collection: &str,
    entry_id: Uuid,
    revision_id: Uuid,
) -> Result<bool, DbError> {
    sqlx::query_scalar!(
        "SELECT EXISTS (
             SELECT FROM revisions
             WHERE project_id = $1 AND environment_id = $2 AND collection = $3
               AND entry_id = $4 AND id = $5
         ) AS \"exists!\"",
        tx.project,
        tx.environment,
        collection,
        entry_id,
        revision_id,
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Whether `identity` belongs to this project — the rule for who a task may be assigned to.
/// `None` means it isn't, or was never, a member.
pub async fn member_role(
    tx: &mut ProjectTransaction,
    identity: Uuid,
) -> Result<Option<String>, DbError> {
    sqlx::query_scalar!(
        "SELECT role FROM memberships WHERE project_id = $1 AND identity_id = $2",
        tx.project,
        identity,
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Whether `changeset_id` is a changeset of this environment.
pub async fn changeset_exists(
    tx: &mut ProjectTransaction,
    changeset_id: Uuid,
) -> Result<bool, DbError> {
    sqlx::query_scalar!(
        "SELECT EXISTS (
             SELECT FROM changesets
             WHERE project_id = $1 AND environment_id = $2 AND id = $3
         ) AS \"exists!\"",
        tx.project,
        tx.environment,
        changeset_id,
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Writes `task` as the transaction's identity; the row it became.
pub async fn create_task(tx: &mut ProjectTransaction, task: &NewTask<'_>) -> Result<Task, DbError> {
    sqlx::query_as!(
        Task,
        "INSERT INTO tasks
            (project_id, environment_id, id, entry_id, collection, revision_id, changeset_id,
             title, assignee_id, created_by, entry_created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         RETURNING id, entry_id, collection, revision_id, changeset_id, title, assignee_id,
                   status, created_by, entry_created_by, done_by, done_at, created_at, updated_at",
        tx.project,
        tx.environment,
        task.id,
        task.entry_id,
        task.collection,
        task.revision_id,
        task.changeset_id,
        task.title,
        task.assignee_id,
        tx.identity,
        task.entry_created_by,
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)
}

/// One task of this environment, if it's there.
pub async fn task(tx: &mut ProjectTransaction, task_id: Uuid) -> Result<Option<Task>, DbError> {
    sqlx::query_as!(
        Task,
        "SELECT id, entry_id, collection, revision_id, changeset_id, title, assignee_id, status,
                created_by, entry_created_by, done_by, done_at, created_at, updated_at
         FROM tasks
         WHERE project_id = $1 AND environment_id = $2 AND id = $3",
        tx.project,
        tx.environment,
        task_id,
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// [`task`], locking the row for an update.
pub async fn lock_task(
    tx: &mut ProjectTransaction,
    task_id: Uuid,
) -> Result<Option<Task>, DbError> {
    sqlx::query_as!(
        Task,
        "SELECT id, entry_id, collection, revision_id, changeset_id, title, assignee_id, status,
                created_by, entry_created_by, done_by, done_at, created_at, updated_at
         FROM tasks
         WHERE project_id = $1 AND environment_id = $2 AND id = $3
         FOR UPDATE",
        tx.project,
        tx.environment,
        task_id,
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// The environment's tasks matching `filter`, newest first, at most `limit` (capped at one more
/// than [`MAX_PAGE`], as [`comments`]), restricted to the entries `read` sees: tasks on entries
/// the role may not read are never returned.
pub async fn tasks(
    tx: &mut ProjectTransaction,
    filter: &TaskFilter<'_>,
    read: &ReadScope,
    limit: i64,
) -> Result<Vec<Task>, DbError> {
    sqlx::query_as!(
        Task,
        "SELECT id, entry_id, collection, revision_id, changeset_id, title, assignee_id, status,
                created_by, entry_created_by, done_by, done_at, created_at, updated_at
         FROM tasks
         WHERE project_id = $1 AND environment_id = $2
           AND ($3::uuid IS NULL OR entry_id = $3)
           AND ($4::text IS NULL OR collection = $4)
           AND ($5::bool IS FALSE OR assignee_id IS NOT DISTINCT FROM $6)
           AND ($7::text IS NULL OR status = $7)
           AND ($8::uuid IS NULL OR id < $8)
           AND ($9::bool
                OR collection = ANY($10)
                OR (entry_created_by = $11 AND ($12::bool OR collection = ANY($13))))
         ORDER BY id DESC
         LIMIT $14",
        tx.project,
        tx.environment,
        filter.entry_id,
        filter.collection,
        filter.assignee.is_some(),
        filter.assignee.and_then(|assignee| match assignee {
            Assignee::Clear => None,
            Assignee::To(id) => Some(id),
        }),
        filter.status.map(TaskStatus::as_str),
        filter.before,
        read.all,
        &read.collections[..],
        tx.identity,
        read.own_anywhere,
        &read.own_collections[..],
        limit.clamp(1, MAX_PAGE + 1),
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Applies `update` to `task`, as `actor`: a status becoming `done` records `actor` and now, a
/// status leaving `done` clears the pair, and one staying `done` keeps it. The row afterwards,
/// or `None` if it isn't there.
pub async fn update_task(
    tx: &mut ProjectTransaction,
    task_id: Uuid,
    update: &TaskUpdate<'_>,
    actor: Uuid,
) -> Result<Option<Task>, DbError> {
    sqlx::query_as!(
        Task,
        "UPDATE tasks
         SET title = coalesce($4, title),
             assignee_id = CASE WHEN $5 THEN $6 ELSE assignee_id END,
             status = coalesce($7, status),
             done_by = CASE
                 WHEN coalesce($7, status) <> 'done' THEN NULL
                 WHEN status = 'done' THEN done_by
                 ELSE $8
             END,
             done_at = CASE
                 WHEN coalesce($7, status) <> 'done' THEN NULL
                 WHEN status = 'done' THEN done_at
                 ELSE now()
             END,
             updated_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND id = $3
         RETURNING id, entry_id, collection, revision_id, changeset_id, title, assignee_id,
                   status, created_by, entry_created_by, done_by, done_at, created_at, updated_at",
        tx.project,
        tx.environment,
        task_id,
        update.title,
        update.assignee.is_some(),
        update.assignee.and_then(|assignee| match assignee {
            Assignee::Clear => None,
            Assignee::To(id) => Some(id),
        }),
        update.status.map(TaskStatus::as_str),
        actor,
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)
}
