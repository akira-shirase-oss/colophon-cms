//! Content schema migrations: plans and their per-entry items (docs/modules/02-db.md;
//! migration 0040, D-100).
//!
//! A plan is computed under the collection's [`content_types::lock_for_change`], so it sees
//! exactly the drafts and open publications there are, and it names them all: the dry run the
//! spec asks for (§12), never a reservation (§11). Its states:
//!
//! - `proposed`: computed, awaiting approval;
//! - `approved`: approved on `plan_hash` — the definition, base version and operations the
//!   approver looked at;
//! - `running`: apply started; a worker drives the items (step M.5b). While a plan runs, a
//!   change to the collection's type is refused (`migration_in_progress`), and the unique index
//!   keeps a second plan from running;
//! - `done`, `blocked`: the run's ends (M.5b);
//! - `superseded`: the collection's definition moved first, or a newer plan replaced it;
//! - `cancelled`: a person cancelled it before it ran.
//!
//! Everything runs inside a [`ProjectTransaction`]. The API parses and classifies definitions,
//! computes `plan_hash`, checks the policy and writes the audit log; the predicted outcomes are
//! computed here from `ContentType::accepts`, off the async workers like the update path's own
//! check.

use std::{collections::BTreeMap, sync::Arc};

use colophon_domain::schema::{ContentType, Successor};
use sqlx::{Acquire, PgConnection, PgPool, Postgres, Transaction, types::time::OffsetDateTime};
use uuid::Uuid;

use crate::{DbError, content_types, scope::ProjectTransaction};

/// The most plans a list returns, newest first.
pub const MAX_PAGE: i64 = 100;
/// The most items one page of a plan's items returns.
pub const MAX_ITEMS_PAGE: i64 = 100;
/// Stored revisions checked per blocking task while computing a plan, like the update path's
/// [`crate::content_types`] check.
const CHECK_PAGE: i64 = 200;

/// A migration plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Its id.
    pub id: Uuid,
    /// The collection it migrates.
    pub collection: String,
    /// The definition version it was computed on.
    pub base_version: i32,
    /// The version the apply wrote, once it did.
    pub target_version: Option<i32>,
    /// The canonical target definition.
    pub target_definition: String,
    /// Its SHA-256.
    pub target_definition_hash: Vec<u8>,
    /// The declared plan-level operations, as JSON text; `None` when there are none.
    pub operations: Option<String>,
    /// `proposed`, `approved`, `running`, `done`, `blocked`, `superseded` or `cancelled`.
    pub state: String,
    /// What approvals and applies are bound to (D-100).
    pub plan_hash: Vec<u8>,
    /// Who made the plan.
    pub created_by: Uuid,
    /// Who approved it, if anyone has.
    pub approved_by: Option<Uuid>,
    /// When.
    pub approved_at: Option<OffsetDateTime>,
    /// How many items the plan names.
    pub expected_items: i64,
    /// How many have been applied.
    pub done_items: i64,
    /// How many still block the run: `needs_input` or `blocked`.
    pub blocked_items: i64,
    /// When the run started.
    pub started_at: Option<OffsetDateTime>,
    /// When it ended, either way, or the plan was superseded or cancelled.
    pub finished_at: Option<OffsetDateTime>,
    /// Why a blocked plan stopped.
    pub error: Option<String>,
    /// When the plan was made.
    pub created_at: OffsetDateTime,
    /// When it last changed.
    pub updated_at: OffsetDateTime,
}

/// One entry a plan names, with the outcome the dry run predicted (or a person or the apply
/// gave it since).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The entry.
    pub entry_id: Uuid,
    /// Its draft at plan time: what the apply rewrites.
    pub expected_revision_id: Uuid,
    /// `ok`, `migrates`, `needs_input`, `blocked`, `resolved`, `applied` or `skipped`.
    pub outcome: String,
    /// Why it can't proceed, as a refusal reports it: `{"reason", "pointer", "message"}`.
    pub problem: Option<String>,
    /// The replacement document a person gave it, in canonical JSON.
    pub resolution: Option<String>,
    /// The revision the apply wrote, once it did.
    pub new_revision_id: Option<Uuid>,
    /// When.
    pub applied_at: Option<OffsetDateTime>,
    /// The entry's draft title, for lists; `None` once the entry is deleted (the item stays:
    /// the plan named it).
    pub title: Option<String>,
    /// Whether an open publication of the entry still pins a revision written before the
    /// run's target version: it does, once the item is applied, until someone publishes the
    /// migrated draft. Answered live, so a publish between the plan and now reads right.
    pub republish_required: bool,
}

/// A plan to store; the API supplies what it computed (the hash and the parsed target).
#[derive(Debug)]
pub struct NewPlan<'a> {
    /// The plan's id.
    pub id: Uuid,
    /// The collection.
    pub collection: &'a str,
    /// The parsed target definition.
    pub next: &'a ContentType,
    /// Its canonical form.
    pub canonical: &'a str,
    /// Its SHA-256.
    pub definition_hash: &'a [u8; 32],
    /// The declared plan-level operations as JSON text, if any.
    pub operations: Option<&'a str>,
    /// The hash over collection, base version, target definition hash and operations.
    pub plan_hash: &'a [u8; 32],
}

/// What [`create_plan`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Planned {
    /// The plan and every item are stored.
    Done(Box<Plan>),
    /// The definition already in force accepts the target: a plain update does it, so there is
    /// nothing to migrate.
    NotNeeded,
    /// A plan is already running on this collection: plan again when it finishes.
    Running,
    /// No such collection here.
    NotFound,
}

/// What an item's dry run predicted, as it's stored.
struct DraftItem {
    /// The entry's draft revision at plan time.
    expected_revision_id: Uuid,
    /// `ok` or `needs_input`.
    outcome: &'static str,
    /// The refusal its content would get, as a refusal reports it.
    problem: Option<serde_json::Value>,
}

/// Computes and stores a plan for moving `new.collection`'s type to `new.next`, under the
/// collection's change lock so it names exactly the entries there are — the dry run the spec's
/// plan step asks for, never a reservation. Open plans of the collection are superseded first:
/// this one replaces them.
pub async fn create_plan(
    tx: &mut ProjectTransaction,
    new: &NewPlan<'_>,
) -> Result<Planned, DbError> {
    // Durable work, so it admits first — before the collection's change lock, the one lock
    // another admission (a publish) also holds and could wait on this one for (D-131). The
    // count is pre-`supersede_open`: at the bound a fresh plan can be refused although its
    // own supersede would have freed a slot — a retryable refusal, never an over-admission.
    crate::work::admit(tx, 1).await?;
    // Writes in the collection wait from here until this commits, and this waited for the ones
    // already under way — the same lock a type update takes (D-055).
    content_types::lock_for_change(tx, new.collection).await?;
    let Some(current) = content_types::current(tx, new.collection).await? else {
        return Ok(Planned::NotFound);
    };
    let stored: serde_json::Value = serde_json::from_str(&current.definition)
        .map_err(|_| DbError::Corrupt("a stored definition is not JSON"))?;
    let previous =
        ContentType::parse(&stored).map_err(|_| DbError::Corrupt("a stored definition"))?;
    // Reclassified under the lock: the check the API ran before it could have raced an update.
    if matches!(previous.successor(new.next), Successor::Compatible) {
        return Ok(Planned::NotNeeded);
    }
    // A plan running on the collection will write a version that supersedes any base a new plan
    // could take; refuse up front rather than have it superseded on landing.
    if running_for_collection(tx, new.collection).await? {
        return Ok(Planned::Running);
    }
    let items = checked_items(tx, new.collection, Arc::new(new.next.clone())).await?;
    // A newer plan replaces the collection's open ones; running ones it leaves alone (the API
    // refuses to plan while one runs, and the apply's unique index would refuse a second).
    supersede_open(tx, new.collection).await?;
    let expected = i64::try_from(items.len()).unwrap_or(i64::MAX);
    let blocked = i64::try_from(items.values().filter(|item| item.outcome != "ok").count())
        .unwrap_or(i64::MAX);
    sqlx::query!(
        "INSERT INTO migration_plans
           (project_id, environment_id, id, collection, base_version, target_definition,
            target_definition_hash, operations, plan_hash, created_by, expected_items,
            blocked_items)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8::text::jsonb, $9, $10, $11, $12)",
        tx.project,
        tx.environment,
        new.id,
        new.collection,
        current.version,
        new.canonical,
        &new.definition_hash[..],
        new.operations,
        &new.plan_hash[..],
        tx.identity,
        expected,
        blocked
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    let rows: Vec<serde_json::Value> = items
        .iter()
        .map(|(entry, item)| {
            let problem = item.problem.clone().unwrap_or(serde_json::Value::Null);
            serde_json::json!({
                "entry": entry,
                "revision": item.expected_revision_id,
                "outcome": item.outcome,
                "problem": problem,
            })
        })
        .collect();
    sqlx::query!(
        "INSERT INTO migration_items
           (project_id, environment_id, plan_id, entry_id, expected_revision_id, outcome, problem)
         SELECT $1, $2, $3, (item->>'entry')::uuid, (item->>'revision')::uuid,
                item->>'outcome', item->'problem'
         FROM jsonb_array_elements($4::jsonb) AS item",
        tx.project,
        tx.environment,
        new.id,
        serde_json::Value::Array(rows)
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    let plan = get(tx, new.id)
        .await?
        .ok_or(DbError::Corrupt("a migration plan vanished as it was made"))?;
    Ok(Planned::Done(Box::new(plan)))
}

/// The plan's dry run: every draft and open publication of `collection` checked against `next`,
/// off the async workers in pages of [`CHECK_PAGE`]. An entry the target accepts is `ok`; one
/// it refuses is `needs_input` with the refusal — the transform machinery that could turn a
/// refusal into `migrates` lands with the worker (M.5b). The caller holds the collection's
/// change lock, so the set it checks is the set there is.
async fn checked_items(
    tx: &mut ProjectTransaction,
    collection: &str,
    next: Arc<ContentType>,
) -> Result<BTreeMap<Uuid, DraftItem>, DbError> {
    let mut items: BTreeMap<Uuid, DraftItem> = BTreeMap::new();
    for published in [false, true] {
        let mut after = Uuid::nil();
        loop {
            let page =
                content_types::stored_page(tx, collection, published, after, CHECK_PAGE).await?;
            let Some(last) = page.last() else { break };
            after = last.entry_id;
            let more = i64::try_from(page.len()).is_ok_and(|len| len == CHECK_PAGE);
            // Validation is CPU work: up to a page of documents, off the async workers.
            let next = Arc::clone(&next);
            let checked = tokio::task::spawn_blocking(move || {
                page.into_iter()
                    .map(|stored| {
                        let problem = next.accepts(&stored.content).err();
                        (stored, problem)
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .map_err(|_| DbError::Corrupt("a plan's check task failed"))?;
            for (stored, problem) in checked {
                let problem = problem.map(|refusal| {
                    serde_json::json!({
                        "reason": refusal.code,
                        "pointer": refusal.pointer,
                        "message": refusal.message,
                    })
                });
                let outcome: &'static str = if problem.is_some() {
                    "needs_input"
                } else {
                    "ok"
                };
                let item = items.entry(stored.entry_id).or_insert(DraftItem {
                    expected_revision_id: stored.revision_id,
                    outcome,
                    problem: None,
                });
                // A publication distinct from the draft that no longer fits also blocks the
                // entry; the draft's own revision stays the one apply rewrites. A draft's
                // refusal wins the item's problem, being what an editor fixes first.
                if outcome == "needs_input" && item.outcome == "ok" {
                    item.outcome = outcome;
                    item.problem = problem;
                } else if item.problem.is_none() {
                    item.problem = problem;
                }
            }
            if !more {
                break;
            }
        }
    }
    Ok(items)
}

/// The plan, read without a lock.
pub async fn get(tx: &mut ProjectTransaction, id: Uuid) -> Result<Option<Plan>, DbError> {
    sqlx::query_as!(
        Plan,
        r#"SELECT id, collection, base_version, target_version, target_definition,
                  target_definition_hash, operations::text AS operations, state, plan_hash,
                  created_by, approved_by, approved_at, expected_items, done_items,
                  blocked_items, started_at, finished_at, error, created_at, updated_at
           FROM migration_plans
           WHERE project_id = $1 AND environment_id = $2 AND id = $3"#,
        tx.project,
        tx.environment,
        id
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// The plan, locked until the transaction ends: approvals, applies, cancellations and
/// resolutions all queue on it, so each sees the state the one before committed.
pub async fn lock(tx: &mut ProjectTransaction, id: Uuid) -> Result<Option<Plan>, DbError> {
    sqlx::query_as!(
        Plan,
        r#"SELECT id, collection, base_version, target_version, target_definition,
                  target_definition_hash, operations::text AS operations, state, plan_hash,
                  created_by, approved_by, approved_at, expected_items, done_items,
                  blocked_items, started_at, finished_at, error, created_at, updated_at
           FROM migration_plans
           WHERE project_id = $1 AND environment_id = $2 AND id = $3
           FOR UPDATE"#,
        tx.project,
        tx.environment,
        id
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// The environment's plans, newest first, at most [`MAX_PAGE`]; of `collection`, or only in
/// `state`, when given.
pub async fn list(
    tx: &mut ProjectTransaction,
    collection: Option<&str>,
    state: Option<&str>,
) -> Result<Vec<Plan>, DbError> {
    sqlx::query_as!(
        Plan,
        r#"SELECT id, collection, base_version, target_version, target_definition,
                  target_definition_hash, operations::text AS operations, state, plan_hash,
                  created_by, approved_by, approved_at, expected_items, done_items,
                  blocked_items, started_at, finished_at, error, created_at, updated_at
           FROM migration_plans
           WHERE project_id = $1 AND environment_id = $2
             AND ($3::text IS NULL OR collection = $3)
             AND ($4::text IS NULL OR state = $4)
           ORDER BY id DESC
           LIMIT $5"#,
        tx.project,
        tx.environment,
        collection,
        state,
        MAX_PAGE
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// A plan's items in entry order after `after`, at most `limit`; only those with `outcome`
/// when given. Each joins its entry's draft title; a deleted entry's title is `None` (the
/// application role can't see tombstones), which the API renders as `null` like a changeset's
/// published item (D-080).
pub async fn items(
    tx: &mut ProjectTransaction,
    plan: Uuid,
    outcome: Option<&str>,
    after: Option<Uuid>,
    limit: i64,
) -> Result<Vec<Item>, DbError> {
    sqlx::query_as!(
        Item,
        r#"SELECT i.entry_id, i.expected_revision_id, i.outcome, i.problem::text AS problem,
                  i.resolution::text AS resolution, i.new_revision_id, i.applied_at,
                  e.draft_title AS "title?",
                  (i.outcome = 'applied' AND EXISTS (
                      SELECT 1 FROM publications p
                      JOIN revisions r ON r.project_id = p.project_id
                                      AND r.environment_id = p.environment_id
                                      AND r.entry_id = p.entry_id AND r.id = p.revision_id
                      JOIN migration_plans mp ON mp.project_id = i.project_id
                                             AND mp.environment_id = i.environment_id
                                             AND mp.id = i.plan_id
                      WHERE p.project_id = i.project_id AND p.environment_id = i.environment_id
                        AND p.entry_id = i.entry_id AND p.valid_to IS NULL
                        AND r.schema_version < mp.target_version
                  )) AS "republish_required!"
           FROM migration_items i
           LEFT JOIN entries e ON e.project_id = i.project_id AND e.environment_id = i.environment_id
                              AND e.id = i.entry_id
           WHERE i.project_id = $1 AND i.environment_id = $2 AND i.plan_id = $3
             AND ($4::text IS NULL OR i.outcome = $4)
             AND ($5::uuid IS NULL OR i.entry_id > $5)
           ORDER BY i.entry_id
           LIMIT $6"#,
        tx.project,
        tx.environment,
        plan,
        outcome,
        after,
        limit
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// One of a plan's items.
pub async fn item(
    tx: &mut ProjectTransaction,
    plan: Uuid,
    entry: Uuid,
) -> Result<Option<Item>, DbError> {
    sqlx::query_as!(
        Item,
        r#"SELECT i.entry_id, i.expected_revision_id, i.outcome, i.problem::text AS problem,
                  i.resolution::text AS resolution, i.new_revision_id, i.applied_at,
                  e.draft_title AS "title?",
                  (i.outcome = 'applied' AND EXISTS (
                      SELECT 1 FROM publications p
                      JOIN revisions r ON r.project_id = p.project_id
                                      AND r.environment_id = p.environment_id
                                      AND r.entry_id = p.entry_id AND r.id = p.revision_id
                      JOIN migration_plans mp ON mp.project_id = i.project_id
                                             AND mp.environment_id = i.environment_id
                                             AND mp.id = i.plan_id
                      WHERE p.project_id = i.project_id AND p.environment_id = i.environment_id
                        AND p.entry_id = i.entry_id AND p.valid_to IS NULL
                        AND r.schema_version < mp.target_version
                  )) AS "republish_required!"
           FROM migration_items i
           LEFT JOIN entries e ON e.project_id = i.project_id AND e.environment_id = i.environment_id
                              AND e.id = i.entry_id
           WHERE i.project_id = $1 AND i.environment_id = $2 AND i.plan_id = $3 AND i.entry_id = $4"#,
        tx.project,
        tx.environment,
        plan,
        entry
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Marks the plan approved by the transaction's identity. The caller holds [`lock`], has
/// checked the hash the approver saw and that the state is still `proposed`.
pub async fn approve(tx: &mut ProjectTransaction, id: Uuid) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE migration_plans
         SET state = 'approved', approved_by = $4, approved_at = now(), updated_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND state = 'proposed'",
        tx.project,
        tx.environment,
        id,
        tx.identity
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(())
}

/// Moves the plan from `approved` to `running`, atomically, stamping `started_at`: the apply's
/// first step. `false` when it isn't `approved` — under [`lock`] that can't happen, but a
/// worker (M.5b) calls this without the lock and relies on the state guard itself.
pub async fn begin_apply(tx: &mut ProjectTransaction, id: Uuid) -> Result<bool, DbError> {
    let started = sqlx::query!(
        "UPDATE migration_plans SET state = 'running', started_at = now(), updated_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND state = 'approved'",
        tx.project,
        tx.environment,
        id
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(started.rows_affected() == 1)
}

/// Marks every `proposed` or `approved` plan of `collection` `superseded`: a definition that
/// landed first, or a newer plan, made its base stale. How many it superseded.
pub async fn supersede_open(tx: &mut ProjectTransaction, collection: &str) -> Result<u64, DbError> {
    let superseded = sqlx::query!(
        "UPDATE migration_plans
         SET state = 'superseded', finished_at = now(), updated_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND collection = $3
           AND state IN ('proposed', 'approved')",
        tx.project,
        tx.environment,
        collection
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(superseded.rows_affected())
}

/// Cancels a `proposed` or `approved` plan. The caller holds [`lock`] and checked the state:
/// `false` when it isn't either — a running plan is refused (v1: cancel before it runs), as is
/// a finished one.
pub async fn cancel(tx: &mut ProjectTransaction, id: Uuid) -> Result<bool, DbError> {
    let cancelled = sqlx::query!(
        "UPDATE migration_plans SET state = 'cancelled', finished_at = now(), updated_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND id = $3
           AND state IN ('proposed', 'approved')",
        tx.project,
        tx.environment,
        id
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(cancelled.rows_affected() == 1)
}

/// What [`resolve_item`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// The resolution is stored and the item is `resolved`; the plan's blocked count dropped.
    Done(Item),
    /// No such item on the plan.
    NotFound,
    /// The item isn't `needs_input`: its outcome now.
    WrongState(String),
}

/// Stores `resolution` (canonical JSON that fits the target) on a `needs_input` item and moves
/// it to `resolved`, unblocking the plan by one. The caller holds the plan's [`lock`] and has
/// checked its state (`proposed` or `approved`); the item is locked and checked itself.
pub async fn resolve_item(
    tx: &mut ProjectTransaction,
    plan: Uuid,
    entry: Uuid,
    resolution: &str,
) -> Result<Resolved, DbError> {
    let outcome = sqlx::query_scalar!(
        "SELECT outcome FROM migration_items
         WHERE project_id = $1 AND environment_id = $2 AND plan_id = $3 AND entry_id = $4
         FOR UPDATE",
        tx.project,
        tx.environment,
        plan,
        entry
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    match outcome.as_deref() {
        None => return Ok(Resolved::NotFound),
        Some("needs_input") => {}
        Some(other) => return Ok(Resolved::WrongState(other.to_owned())),
    }
    sqlx::query!(
        "UPDATE migration_items SET outcome = 'resolved', resolution = $5::text::jsonb
         WHERE project_id = $1 AND environment_id = $2 AND plan_id = $3 AND entry_id = $4",
        tx.project,
        tx.environment,
        plan,
        entry,
        resolution
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    sqlx::query!(
        "UPDATE migration_plans SET blocked_items = blocked_items - 1, updated_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND id = $3",
        tx.project,
        tx.environment,
        plan
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    let item = item(tx, plan, entry).await?.ok_or(DbError::Corrupt(
        "a migration item vanished as it was resolved",
    ))?;
    Ok(Resolved::Done(item))
}

/// Whether any applied item of `plan` still has an open publication pinned to a revision
/// written before the run's target version — the plan-level form of each item's
/// `republish_required`, answered live so a publish between the run and now reads right.
/// `false` while `target_version` is unset (nothing applied yet).
pub async fn republish_required(tx: &mut ProjectTransaction, plan: Uuid) -> Result<bool, DbError> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM migration_items i
               JOIN publications p ON p.project_id = i.project_id
                                  AND p.environment_id = i.environment_id
                                  AND p.entry_id = i.entry_id AND p.valid_to IS NULL
               JOIN revisions r ON r.project_id = p.project_id
                               AND r.environment_id = p.environment_id
                               AND r.entry_id = p.entry_id AND r.id = p.revision_id
               JOIN migration_plans mp ON mp.project_id = i.project_id
                                      AND mp.environment_id = i.environment_id
                                      AND mp.id = i.plan_id
               WHERE i.project_id = $1 AND i.environment_id = $2 AND i.plan_id = $3
                 AND i.outcome = 'applied' AND r.schema_version < mp.target_version
           ) AS "required!""#,
        tx.project,
        tx.environment,
        plan
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Whether `collection` has any plan, in any state: plans name its versions forever, so it
/// can't be deleted — the foreign key is the backstop; this gives the honest refusal.
pub async fn has_plans(tx: &mut ProjectTransaction, collection: &str) -> Result<bool, DbError> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM migration_plans
               WHERE project_id = $1 AND environment_id = $2 AND collection = $3
           ) AS "any!""#,
        tx.project,
        tx.environment,
        collection
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// Whether a plan is running for `collection`: while one is, a change to the collection's
/// type is refused `migration_in_progress` (D-100) — the conflict answer for a schema edit
/// racing a migration.
pub async fn running_for_collection(
    tx: &mut ProjectTransaction,
    collection: &str,
) -> Result<bool, DbError> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM migration_plans
               WHERE project_id = $1 AND environment_id = $2 AND collection = $3
                 AND state = 'running'
           ) AS "running!""#,
        tx.project,
        tx.environment,
        collection
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

// -- the run (M.5b) ------------------------------------------------------------
//
// A worker drives a `running` plan to `done` or `blocked` (docs/modules/05-worker.md). It
// connects as `colophon_worker`, finds running plans across every project with [`running`],
// and claims one with a session advisory lock on its own connection ([`claim_plan`]): at most
// one worker drives a plan, and a crash frees it for the next scan. Every transaction of the
// drive comes from [`worker_on`] on that same connection, so the claim is held throughout.
//
// What a run writes and the worker's own grants can't reach — the plan's content-type version
// and the per-entry revision write — is migration 0041's pair of SECURITY DEFINER functions,
// owned by `colophon_migrator`. They run in the caller's transaction, so every check they make
// (the collection's change lock, the entry's draft pointer) is under the locks the same
// transaction holds and can't drift between checking and writing.

/// The most running plans one scan returns.
pub const MAX_RUNNING: i64 = 64;
/// The most items one batch of a run applies.
pub const RUN_BATCH: i64 = 16;

/// A running plan, as the worker's scan sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningPlan {
    /// The project.
    pub project: Uuid,
    /// The environment.
    pub environment: Uuid,
    /// The plan.
    pub id: Uuid,
    /// Its collection.
    pub collection: String,
    /// The canonical target definition, for the parse that validates every rewritten document.
    pub target_definition: String,
    /// Its declared operations, as JSON text; `None` when there are none.
    pub operations: Option<String>,
}

/// A plan's state and counters as a checkpoint leaves them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunState {
    /// `running`, or `done`/`blocked` once the run ended (a stale claim can read any terminal
    /// state — `superseded`, `cancelled` — and the driver should stop).
    pub state: String,
    /// Items applied or skipped.
    pub done: i64,
    /// Items still blocking: `needs_input` or `blocked`.
    pub blocked: i64,
    /// Why a blocked plan stopped; `None` while it runs.
    pub error: Option<String>,
}

/// An item a run may apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunItem {
    /// The entry.
    pub entry: Uuid,
    /// Its draft at plan time: [`apply_item`] rewrites it or refuses `draft_moved`.
    pub expected_revision: Uuid,
    /// `ok`, `migrates`, `needs_input` or `resolved` — the rest are terminal.
    pub outcome: String,
    /// The replacement document a person stored, for `resolved`.
    pub resolution: Option<String>,
}

/// What [`begin`] reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Begun {
    /// `begun`, `already` (a resumed drive's version is stamped), `moved` (the collection's
    /// version left the plan's base), `not_running`, or `missing`.
    pub outcome: String,
    /// The run's target version once it exists.
    pub target_version: Option<i32>,
}

/// A transformed document ready to write, as [`apply_item`] passes it to migration 0041's
/// function: the canonical JSON text plus the metadata the caller computed from it, as
/// `entries::NewRevision` carries them for a save.
#[derive(Debug)]
pub struct Applied<'a> {
    /// The new revision's id.
    pub revision: Uuid,
    /// Its canonical JSON.
    pub content: &'a str,
    /// The content's SHA-256.
    pub content_hash: &'a [u8; 32],
    /// Its title.
    pub title: &'a str,
    /// Its slug, when the type has a slug field.
    pub slug: Option<&'a str>,
    /// Its reference rows as a JSON array of `{"field","position","collection","entry"}`.
    pub references: serde_json::Value,
    /// Its asset rows as a JSON array of `{"field","position","asset"}`.
    pub assets: serde_json::Value,
    /// The target definition's unique fields: which draft claims the write replaces.
    pub unique: &'a [String],
}

/// The running plans across every project, at most [`MAX_RUNNING`] in arbitrary order — the
/// worker's scan, as it finds due schedules. As `colophon_worker`: the login role's own
/// policies show a plan only inside its project.
pub async fn running(pool: &PgPool) -> Result<Vec<RunningPlan>, DbError> {
    let mut tx = pool.begin().await.map_err(DbError::from_query)?;
    sqlx::query("SET LOCAL ROLE colophon_worker")
        .execute(&mut *tx)
        .await
        .map_err(DbError::from_query)?;
    let plans = sqlx::query_as!(
        RunningPlan,
        r#"SELECT project_id AS project, environment_id AS environment, id, collection,
                  target_definition, operations::text AS operations
           FROM migration_plans WHERE state = 'running' ORDER BY id LIMIT $1"#,
        MAX_RUNNING
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(DbError::from_query)?;
    drop(tx);
    Ok(plans)
}

/// Claims `plan` for the session on `conn`: a `pg_try_advisory_lock` on a key unique to the
/// plan, held until [`release_plan`] or the connection drops — so one worker drives it, and a
/// crash frees it. The lock is a session lock, not a transaction's: it outlives each batch's
/// commit. `false` when another worker holds it.
pub async fn claim_plan(
    conn: &mut PgConnection,
    project: Uuid,
    environment: Uuid,
    plan: Uuid,
) -> Result<bool, DbError> {
    sqlx::query_scalar!(
        r#"SELECT pg_try_advisory_lock(hashtext($1), hashtext($2)) AS "claimed!""#,
        format!("{project}/{environment}"),
        format!("migration {plan}")
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(DbError::from_query)
}

/// Releases the claim [`claim_plan`] made; a dropped connection does it anyway.
pub async fn release_plan(
    conn: &mut PgConnection,
    project: Uuid,
    environment: Uuid,
    plan: Uuid,
) -> Result<(), DbError> {
    sqlx::query_scalar!(
        r#"SELECT pg_advisory_unlock(hashtext($1), hashtext($2)) AS "released!""#,
        format!("{project}/{environment}"),
        format!("migration {plan}")
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(DbError::from_query)?;
    Ok(())
}

/// A transaction for a run, on the connection that holds the claim — as `colophon_worker`
/// for its duration, so the run's reads and writes get the worker's grants and policies, not
/// the login role's project-scoped ones.
pub async fn worker_on(conn: &mut PgConnection) -> Result<Transaction<'_, Postgres>, DbError> {
    let mut tx = Acquire::begin(&mut *conn)
        .await
        .map_err(DbError::from_query)?;
    sqlx::query("SET LOCAL ROLE colophon_worker")
        .execute(&mut *tx)
        .await
        .map_err(DbError::from_query)?;
    Ok(tx)
}

/// The drive's first step, migration 0041's `colophon_migration_begin`: under the plan's lock
/// and the collection's change lock it writes the run's target version (the approver's
/// definition at base+1, as a type update writes it) and points the plan at it. `already`
/// when a previous drive stamped it — the resume after a crash — `moved` when the collection's
/// version left the plan's base, `not_running` or `missing` otherwise.
pub async fn begin(
    tx: &mut Transaction<'_, Postgres>,
    project: Uuid,
    environment: Uuid,
    plan: Uuid,
) -> Result<Begun, DbError> {
    sqlx::query_as!(
        Begun,
        r#"SELECT outcome AS "outcome!", target_version
         FROM colophon_migration_begin($1, $2, $3)"#,
        project,
        environment,
        plan
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(DbError::from_query)
}

/// The items still to apply (`ok`, `migrates`, `needs_input` or `resolved`), in entry order
/// after `after`, at most `limit` — `needs_input` is attempted too: the declared operations
/// may supply what the dry run found missing, and an apply that still can't reaches `blocked`
/// itself. A fresh read each batch, so a resolution that landed mid-run or an item blocked
/// since is seen.
pub async fn pending_items(
    tx: &mut Transaction<'_, Postgres>,
    project: Uuid,
    environment: Uuid,
    plan: Uuid,
    after: Uuid,
    limit: i64,
) -> Result<Vec<RunItem>, DbError> {
    sqlx::query_as!(
        RunItem,
        r#"SELECT entry_id AS entry, expected_revision_id AS expected_revision, outcome,
                  resolution::text AS resolution
           FROM migration_items
           WHERE project_id = $1 AND environment_id = $2 AND plan_id = $3
             AND outcome IN ('ok', 'migrates', 'needs_input', 'resolved') AND entry_id > $4
           ORDER BY entry_id
           LIMIT $5"#,
        project,
        environment,
        plan,
        after,
        limit
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(DbError::from_query)
}

/// One item's state, to confirm a blocked write is what it says.
pub async fn item_for_run(
    tx: &mut Transaction<'_, Postgres>,
    project: Uuid,
    environment: Uuid,
    plan: Uuid,
    entry: Uuid,
) -> Result<Option<RunItem>, DbError> {
    sqlx::query_as!(
        RunItem,
        r#"SELECT entry_id AS entry, expected_revision_id AS expected_revision, outcome,
                  resolution::text AS resolution
           FROM migration_items
           WHERE project_id = $1 AND environment_id = $2 AND plan_id = $3 AND entry_id = $4"#,
        project,
        environment,
        plan,
        entry
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(DbError::from_query)
}

/// The expected revision's document, the one the transform rewrites; `None` when the revision
/// isn't there (the entry was deleted — [`apply_item`] answers `skipped` for it).
pub async fn expected_content(
    tx: &mut Transaction<'_, Postgres>,
    project: Uuid,
    environment: Uuid,
    entry: Uuid,
    revision: Uuid,
) -> Result<Option<String>, DbError> {
    sqlx::query_scalar!(
        "SELECT content FROM revisions
         WHERE project_id = $1 AND environment_id = $2 AND entry_id = $3 AND id = $4",
        project,
        environment,
        entry,
        revision
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(DbError::from_query)
}

/// One item's write, migration 0041's `colophon_migration_apply_item`: locks plan, item and
/// entry in that order, refuses `draft_moved` when a save won the entry first, `skipped` when
/// it was deleted, names a reference or asset that isn't there, and otherwise writes the
/// revision — references, assets, unique claims and the draft pointer — and marks the item
/// `applied` with it, all in the caller's transaction. The returned outcome is also written
/// on the item when it isn't `applied`.
pub async fn apply_item(
    tx: &mut Transaction<'_, Postgres>,
    project: Uuid,
    environment: Uuid,
    plan: Uuid,
    entry: Uuid,
    expected: Uuid,
    applied: &Applied<'_>,
) -> Result<String, DbError> {
    sqlx::query_scalar!(
        r#"SELECT outcome AS "outcome!" FROM colophon_migration_apply_item(
             $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::jsonb, $12::jsonb, $13)"#,
        project,
        environment,
        plan,
        entry,
        expected,
        applied.revision,
        applied.content,
        &applied.content_hash[..],
        applied.title,
        applied.slug,
        applied.references,
        applied.assets,
        applied.unique as &[String]
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(DbError::from_write)
}

/// Marks one item's outcome and problem, for the endings only the worker can write —
/// `blocked` with a refusal-shaped `problem` (`{"reason","pointer","message"}`) when the
/// stored document can't reach the target even with its operations.
pub async fn mark_item(
    tx: &mut Transaction<'_, Postgres>,
    project: Uuid,
    environment: Uuid,
    plan: Uuid,
    entry: Uuid,
    outcome: &str,
    problem: Option<serde_json::Value>,
) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE migration_items SET outcome = $5, problem = $6
         WHERE project_id = $1 AND environment_id = $2 AND plan_id = $3 AND entry_id = $4",
        project,
        environment,
        plan,
        entry,
        outcome,
        problem
    )
    .execute(&mut **tx)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// Ends the run `blocked` with `error`, for what no item can be blamed on: declared
/// operations that don't parse, or [`begin`] reporting `moved` — the collection's version
/// left the plan's base, so the run can't write what was approved. `None` when the plan
/// already left `running` (the row a person sees is whoever ended it first).
pub async fn fail_run(
    tx: &mut Transaction<'_, Postgres>,
    project: Uuid,
    environment: Uuid,
    plan: Uuid,
    error: &str,
) -> Result<Option<RunState>, DbError> {
    sqlx::query_as!(
        RunState,
        r#"UPDATE migration_plans p
              SET state = 'blocked', error = $4, finished_at = now(), updated_at = now()
            WHERE p.project_id = $1 AND p.environment_id = $2 AND p.id = $3
              AND p.state = 'running'
            RETURNING p.state, p.done_items AS done, p.blocked_items AS blocked, p.error"#,
        project,
        environment,
        plan,
        error
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(DbError::from_query)
}

/// The run's checkpoint, after every batch: recomputes the plan's counters and, when nothing
/// is left to apply, ends it — `done` when every named entry reached a terminal outcome, or
/// `blocked` once an apply refused (with the first blocker's reason in `error`). A
/// `needs_input` item keeps the run open: it is still pending, attempted like the rest.
/// `None` when the plan is gone or already left `running`.
pub async fn checkpoint(
    tx: &mut Transaction<'_, Postgres>,
    project: Uuid,
    environment: Uuid,
    plan: Uuid,
) -> Result<Option<RunState>, DbError> {
    sqlx::query_as!(
        RunState,
        r#"UPDATE migration_plans p
              SET done_items = (SELECT count(*) FROM migration_items i
                                WHERE i.project_id = p.project_id
                                  AND i.environment_id = p.environment_id
                                  AND i.plan_id = p.id
                                  AND i.outcome IN ('applied', 'skipped')),
                  blocked_items = (SELECT count(*) FROM migration_items i
                                   WHERE i.project_id = p.project_id
                                     AND i.environment_id = p.environment_id
                                     AND i.plan_id = p.id
                                     AND i.outcome IN ('needs_input', 'blocked')),
                  state = CASE
                      WHEN EXISTS (SELECT 1 FROM migration_items i
                                   WHERE i.project_id = p.project_id
                                     AND i.environment_id = p.environment_id
                                     AND i.plan_id = p.id
                                     AND i.outcome = 'blocked')
                          THEN 'blocked'
                      WHEN NOT EXISTS (SELECT 1 FROM migration_items i
                                       WHERE i.project_id = p.project_id
                                         AND i.environment_id = p.environment_id
                                         AND i.plan_id = p.id
                                         AND i.outcome IN ('ok', 'migrates', 'needs_input', 'resolved'))
                          THEN 'done'
                      ELSE p.state END,
                  finished_at = CASE
                      WHEN EXISTS (SELECT 1 FROM migration_items i
                                   WHERE i.project_id = p.project_id
                                     AND i.environment_id = p.environment_id
                                     AND i.plan_id = p.id
                                     AND i.outcome = 'blocked')
                        OR NOT EXISTS (SELECT 1 FROM migration_items i
                                       WHERE i.project_id = p.project_id
                                         AND i.environment_id = p.environment_id
                                         AND i.plan_id = p.id
                                         AND i.outcome IN ('ok', 'migrates', 'needs_input', 'resolved'))
                          THEN now()
                      ELSE p.finished_at END,
                  error = CASE
                      WHEN EXISTS (SELECT 1 FROM migration_items i
                                   WHERE i.project_id = p.project_id
                                     AND i.environment_id = p.environment_id
                                     AND i.plan_id = p.id
                                     AND i.outcome = 'blocked')
                          THEN (SELECT coalesce(i.problem->>'reason', 'blocked')
                                FROM migration_items i
                                WHERE i.project_id = p.project_id
                                  AND i.environment_id = p.environment_id
                                  AND i.plan_id = p.id
                                  AND i.outcome = 'blocked'
                                ORDER BY i.entry_id LIMIT 1)
                      ELSE p.error END,
                  updated_at = now()
            WHERE p.project_id = $1 AND p.environment_id = $2 AND p.id = $3
              AND p.state = 'running'
            RETURNING p.state, p.done_items AS done, p.blocked_items AS blocked, p.error"#,
        project,
        environment,
        plan
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(DbError::from_query)
}
