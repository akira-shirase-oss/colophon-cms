//! Scheduled publishing (D-060; spec "Scheduled publishing", §17, case X07; migration 0019).
//!
//! The API creates and cancels schedules in a member's transaction, after checking they may
//! publish the changeset. A worker finds what is due ([`due`]) and runs each one ([`run`]) as the
//! person who scheduled it, so their role as it is then decides: [`crate::publish::publish`]
//! makes every check a publish makes, and the release and the schedule's `published` state commit
//! together. Whatever refuses it blocks the schedule with a reason, and nothing is published.

use std::time::Duration;

use colophon_domain::{changeset::Status, schedule};
use sqlx::{PgPool, Postgres, Transaction, types::time::OffsetDateTime};
use uuid::Uuid;

use crate::{
    DbError,
    publish::{self, Base, Refusal},
    scope::{self, ProjectTransaction},
};

/// A schedule, as its project's members see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    /// Its id.
    pub id: Uuid,
    /// The changeset it publishes.
    pub changeset_id: Uuid,
    /// That changeset's title.
    pub title: String,
    /// The changeset's hash when it was scheduled: what it publishes.
    pub changeset_hash: Vec<u8>,
    /// The local date and time its author chose.
    pub local_time: String,
    /// In this IANA time zone.
    pub time_zone: String,
    /// The choice an ambiguous or skipped local time needed.
    pub choice: Option<String>,
    /// The instant that resolves to.
    pub run_at: OffsetDateTime,
    /// `scheduled`, `published`, `blocked` or `cancelled`.
    pub state: String,
    /// Why it was blocked.
    pub reason: Option<String>,
    /// The release it made.
    pub generation: Option<i64>,
    /// When a worker started on it.
    pub started_at: Option<OffsetDateTime>,
    /// When it finished, either way, or was cancelled.
    pub finished_at: Option<OffsetDateTime>,
    /// Who scheduled it.
    pub scheduled_by: Uuid,
    /// Their name, if known.
    pub scheduled_by_name: Option<String>,
    /// When it was scheduled.
    pub created_at: OffsetDateTime,
}

impl Schedule {
    /// Whether it started later than its time allows ([`schedule::delayed`]).
    pub fn delayed(&self) -> bool {
        self.started_at.is_some_and(|started| {
            schedule::delayed(self.run_at.unix_timestamp(), started.unix_timestamp())
        })
    }
}

/// A schedule to store.
#[derive(Debug, Clone, Copy)]
pub struct NewSchedule<'a> {
    /// Its id.
    pub id: Uuid,
    /// The changeset.
    pub changeset: Uuid,
    /// Its hash, as approved.
    pub hash: &'a [u8],
    /// The approval it publishes under.
    pub approval: Uuid,
    /// What its author chose.
    pub local_time: &'a str,
    /// In this zone.
    pub time_zone: &'a str,
    /// With this choice, if one was needed.
    pub choice: Option<&'a str>,
    /// Resolved.
    pub run_at: OffsetDateTime,
}

/// The most pending schedules a list returns, in time order.
pub const PENDING_LIMIT: i64 = 100;

/// The most finished schedules a list returns, newest first.
pub const FINISHED_LIMIT: i64 = 20;

/// Pending schedules in time order, then the most recently finished; of one changeset, or of the
/// whole environment. Each half reads its own partial index and is bounded, however many
/// schedules the environment ever had.
async fn read(
    tx: &mut ProjectTransaction,
    changeset: Option<Uuid>,
) -> Result<Vec<Schedule>, DbError> {
    sqlx::query_as!(
        Schedule,
        r#"(SELECT s.id AS "id!", s.changeset_id AS "changeset_id!", c.title AS "title!",
                   s.changeset_hash AS "changeset_hash!", s.local_time AS "local_time!",
                   s.time_zone AS "time_zone!", s.choice, s.run_at AS "run_at!", s.state AS "state!",
                   s.reason, s.generation, s.started_at, s.finished_at,
                   s.scheduled_by AS "scheduled_by!", i.display_name AS scheduled_by_name,
                   s.created_at AS "created_at!"
            FROM schedules s
            JOIN changesets c ON c.project_id = s.project_id AND c.environment_id = s.environment_id
                             AND c.id = s.changeset_id
            JOIN identities i ON i.id = s.scheduled_by
            WHERE s.project_id = $1 AND s.environment_id = $2 AND s.state = 'scheduled'
              AND ($3::uuid IS NULL OR s.changeset_id = $3)
            ORDER BY s.run_at, s.id
            LIMIT $4)
           UNION ALL
           (SELECT s.id, s.changeset_id, c.title, s.changeset_hash, s.local_time, s.time_zone,
                   s.choice, s.run_at, s.state, s.reason, s.generation, s.started_at,
                   s.finished_at, s.scheduled_by, i.display_name, s.created_at
            FROM schedules s
            JOIN changesets c ON c.project_id = s.project_id AND c.environment_id = s.environment_id
                             AND c.id = s.changeset_id
            JOIN identities i ON i.id = s.scheduled_by
            WHERE s.project_id = $1 AND s.environment_id = $2 AND s.state <> 'scheduled'
              AND ($3::uuid IS NULL OR s.changeset_id = $3)
            ORDER BY s.finished_at DESC, s.id
            LIMIT $5)"#,
        tx.project,
        tx.environment,
        changeset,
        PENDING_LIMIT,
        FINISHED_LIMIT
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// The environment's pending schedules in time order, then its most recently finished ones.
pub async fn list(tx: &mut ProjectTransaction) -> Result<Vec<Schedule>, DbError> {
    read(tx, None).await
}

/// One schedule.
pub async fn get(tx: &mut ProjectTransaction, id: Uuid) -> Result<Option<Schedule>, DbError> {
    sqlx::query_as!(
        Schedule,
        r#"SELECT s.id, s.changeset_id, c.title, s.changeset_hash, s.local_time, s.time_zone,
                  s.choice, s.run_at, s.state, s.reason, s.generation, s.started_at, s.finished_at,
                  s.scheduled_by, i.display_name AS scheduled_by_name, s.created_at
           FROM schedules s
           JOIN changesets c ON c.project_id = s.project_id AND c.environment_id = s.environment_id
                            AND c.id = s.changeset_id
           JOIN identities i ON i.id = s.scheduled_by
           WHERE s.project_id = $1 AND s.environment_id = $2 AND s.id = $3"#,
        tx.project,
        tx.environment,
        id
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)
}

/// A changeset's pending schedule, if it has one, then its most recently finished ones.
pub async fn of_changeset(
    tx: &mut ProjectTransaction,
    changeset: Uuid,
) -> Result<Vec<Schedule>, DbError> {
    read(tx, Some(changeset)).await
}

/// What [`create`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Created {
    /// Stored.
    Done(Box<Schedule>),
    /// The changeset already has a pending schedule.
    AlreadyScheduled,
}

/// Stores a schedule, made by the transaction's identity. The caller has locked the changeset and
/// checked that it is approved on `new.hash` and that the caller may publish it.
pub async fn create(
    tx: &mut ProjectTransaction,
    new: &NewSchedule<'_>,
) -> Result<Created, DbError> {
    let made = sqlx::query!(
        "INSERT INTO schedules (project_id, environment_id, id, changeset_id, changeset_hash,
                                approval_id, local_time, time_zone, choice, run_at, scheduled_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        tx.project,
        tx.environment,
        new.id,
        new.changeset,
        new.hash,
        new.approval,
        new.local_time,
        new.time_zone,
        new.choice,
        new.run_at,
        tx.identity
    )
    .execute(&mut *tx.transaction)
    .await;
    match made {
        Ok(_) => {}
        Err(sqlx::Error::Database(error))
            if error.is_unique_violation()
                && error.constraint() == Some("schedules_one_pending") =>
        {
            return Ok(Created::AlreadyScheduled);
        }
        Err(error) => return Err(DbError::from_query(error)),
    }
    let stored = get(tx, new.id)
        .await?
        .ok_or(DbError::Corrupt("a schedule vanished as it was made"))?;
    Ok(Created::Done(Box::new(stored)))
}

/// What [`cancel`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cancelled {
    /// Cancelled: it won't run.
    Done(Box<Schedule>),
    /// No such schedule.
    NotFound,
    /// It already ran, or was cancelled: its state now.
    Finished(String),
}

/// Cancels a pending schedule. If a worker is running it this moment, this waits for that to
/// finish and then reports what happened: cancelling never undoes a release.
pub async fn cancel(tx: &mut ProjectTransaction, id: Uuid) -> Result<Cancelled, DbError> {
    let state = sqlx::query_scalar!(
        "SELECT state FROM schedules
         WHERE project_id = $1 AND environment_id = $2 AND id = $3
         FOR UPDATE",
        tx.project,
        tx.environment,
        id
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    match state.as_deref() {
        None => return Ok(Cancelled::NotFound),
        Some("scheduled") => {}
        Some(other) => return Ok(Cancelled::Finished(other.to_owned())),
    }
    sqlx::query!(
        "UPDATE schedules SET state = 'cancelled', cancelled_by = $4, finished_at = now()
         WHERE project_id = $1 AND environment_id = $2 AND id = $3",
        tx.project,
        tx.environment,
        id,
        tx.identity
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    let cancelled = get(tx, id)
        .await?
        .ok_or(DbError::Corrupt("a schedule vanished as it was cancelled"))?;
    Ok(Cancelled::Done(Box::new(cancelled)))
}

/// A schedule that has fallen due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Due {
    /// Its project.
    pub project_id: Uuid,
    /// Its environment.
    pub environment_id: Uuid,
    /// Its id.
    pub id: Uuid,
    /// Who scheduled it: it runs as them.
    pub scheduled_by: Uuid,
    /// Its time.
    pub run_at: OffsetDateTime,
}

/// A worker's transaction: as `colophon_worker`, for its duration.
async fn worker(pool: &PgPool) -> Result<Transaction<'static, Postgres>, DbError> {
    let mut transaction = pool.begin().await.map_err(DbError::from_query)?;
    sqlx::query("SET LOCAL ROLE colophon_worker")
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    Ok(transaction)
}

/// Pending schedules whose time is at or before `at`, oldest first, in every project.
pub async fn due(pool: &PgPool, at: OffsetDateTime, limit: i64) -> Result<Vec<Due>, DbError> {
    let mut transaction = worker(pool).await?;
    let found = sqlx::query_as!(
        Due,
        "SELECT project_id, environment_id, id, scheduled_by, run_at FROM schedules
         WHERE state = 'scheduled' AND run_at <= $1
         ORDER BY run_at, id
         LIMIT $2",
        at,
        limit
    )
    .fetch_all(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(found)
}

/// What [`run`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ran {
    /// Published as the release with this generation.
    Published {
        /// The release.
        generation: i64,
        /// Whether it started too late to count as on time.
        delayed: bool,
    },
    /// Refused, for this reason; nothing was published.
    Blocked {
        /// Why, for people.
        reason: String,
    },
    /// Another worker has it, or it was cancelled or ran already.
    Gone,
}

/// Why a refusal blocks a schedule, in the Studio's words (D-059).
fn reason(refusal: &Refusal) -> String {
    match refusal {
        Refusal::NotFound => "the publish request no longer exists".to_owned(),
        Refusal::HashMoved { .. } => {
            "the publish request changed after it was scheduled; review it and schedule it again"
                .to_owned()
        }
        Refusal::NotPermitted => {
            "the person who scheduled it may no longer publish this content".to_owned()
        }
        Refusal::NotPublishable(status) => match status {
            Status::Published { generation } => {
                format!("it was already published, as release {generation}")
            }
            Status::Stale { .. } => {
                "an entry in it was edited after it was approved; review it and schedule it again"
                    .to_owned()
            }
            Status::ApprovalExpired => {
                "its approval expired before the scheduled time; approve it again".to_owned()
            }
            Status::Open
            | Status::AwaitingReview
            | Status::ChangesRequested { .. }
            | Status::Approved { .. } => "it is no longer approved".to_owned(),
        },
        Refusal::GenerationMoved { .. } => {
            "other releases kept publishing first; schedule it again".to_owned()
        }
        Refusal::SlugTaken => "a URL name in it is still published by another entry".to_owned(),
    }
}

/// Blocks `due` with `reason`, as a worker, if it is still pending.
async fn block(
    pool: &PgPool,
    due: &Due,
    now: OffsetDateTime,
    reason: &str,
) -> Result<Ran, DbError> {
    let mut transaction = worker(pool).await?;
    let blocked = sqlx::query!(
        "UPDATE schedules SET state = 'blocked', reason = $4, started_at = $5, finished_at = $5
         WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND state = 'scheduled'",
        due.project_id,
        due.environment_id,
        due.id,
        reason,
        now
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(if blocked.rows_affected() == 1 {
        Ran::Blocked {
            reason: reason.to_owned(),
        }
    } else {
        Ran::Gone
    })
}

/// Runs a due schedule at `now`, as the person who scheduled it: publishes it and marks it
/// published in one transaction, or blocks it with the reason publishing refused.
pub async fn run(
    pool: &PgPool,
    due: &Due,
    now: OffsetDateTime,
    job_lifetime: Duration,
) -> Result<Ran, DbError> {
    let Some(mut tx) =
        scope::begin_in_environment(pool, due.scheduled_by, due.project_id, due.environment_id)
            .await?
    else {
        return block(
            pool,
            due,
            now,
            "the person who scheduled it is no longer a member of the project",
        )
        .await;
    };
    // Another worker running it holds the row: leave it to them.
    let pending = sqlx::query!(
        "SELECT changeset_id, changeset_hash, run_at FROM schedules
         WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND state = 'scheduled'
         FOR UPDATE SKIP LOCKED",
        tx.project,
        tx.environment,
        due.id
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    let Some(pending) = pending else {
        return Ok(Ran::Gone);
    };
    let outcome = publish::publish(
        &mut tx,
        pending.changeset_id,
        Some(&pending.changeset_hash),
        Base::Latest,
        now,
        job_lifetime,
    )
    .await?;
    match outcome {
        Ok(published) => {
            sqlx::query!(
                "UPDATE schedules SET state = 'published', generation = $4, started_at = $5,
                                      finished_at = $5
                 WHERE project_id = $1 AND environment_id = $2 AND id = $3",
                tx.project,
                tx.environment,
                due.id,
                published.generation,
                now
            )
            .execute(&mut *tx.transaction)
            .await
            .map_err(DbError::from_query)?;
            tx.commit().await?;
            Ok(Ran::Published {
                generation: published.generation,
                delayed: schedule::delayed(pending.run_at.unix_timestamp(), now.unix_timestamp()),
            })
        }
        Err(refusal) => {
            // The publish's transaction may be aborted (a slug taken): drop it, then block as a
            // worker, if no one cancelled it in between.
            drop(tx);
            block(pool, due, now, &reason(&refusal)).await
        }
    }
}
