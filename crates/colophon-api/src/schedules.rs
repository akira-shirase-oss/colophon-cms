//! Scheduled publishing (D-060; spec "Scheduled publishing"):
//!
//! - `POST   {P}/changesets/{changeset}/schedule`: `{"local_time", "time_zone", "choice"?}`, with
//!   `If-Match` (the changeset's hash, as approved);
//! - `GET    {P}/schedules` (`?changeset=` for one changeset's): any member;
//! - `DELETE {P}/schedules/{schedule}`: cancels a pending one.
//!
//! Scheduling needs what publishing needs, now: the changeset approved on the hash the caller saw,
//! and a role that may publish in every collection it touches. The local time is resolved in its
//! IANA time zone; one the clocks show twice or skip is refused with both candidate instants until
//! the request chooses. The time must be in the future, within a year and a day, and before the
//! approval expires. When it falls due, a worker publishes it as the caller, checking everything
//! again (`colophon_db::schedules::run`).

use axum::{
    Json,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
};
use colophon_db::{
    changesets, publish,
    schedules::{self, Cancelled, Created, NewSchedule, Schedule},
    scope::ProjectTransaction,
};
use colophon_domain::{
    changeset::{APPROVAL_LIFETIME_SECS, Status},
    policy::Action,
    schedule::{self, Choice, ScheduleError},
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    changesets::{hash_bytes, if_match, publishable},
    entries::{content_hash, open, rfc3339},
    session::SignedIn,
    shape::{Data, Page},
};

#[derive(Serialize)]
pub(crate) struct ScheduleView {
    id: Uuid,
    changeset_id: Uuid,
    title: String,
    changeset_hash: String,
    local_time: String,
    time_zone: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    choice: Option<String>,
    run_at: String,
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    finished_at: Option<String>,
    /// Started more than a minute after its time: labelled, never passed off as on time.
    delayed: bool,
    scheduled_by: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    scheduled_by_name: Option<String>,
    created_at: String,
}

impl ScheduleView {
    fn new(schedule: Schedule) -> Result<Self, ApiError> {
        Ok(Self {
            delayed: schedule.delayed(),
            id: schedule.id,
            changeset_id: schedule.changeset_id,
            title: schedule.title,
            changeset_hash: content_hash(&schedule.changeset_hash),
            local_time: schedule.local_time,
            time_zone: schedule.time_zone,
            choice: schedule.choice,
            run_at: rfc3339(schedule.run_at)?,
            state: schedule.state,
            reason: schedule.reason,
            generation: schedule.generation,
            started_at: schedule.started_at.map(rfc3339).transpose()?,
            finished_at: schedule.finished_at.map(rfc3339).transpose()?,
            scheduled_by: schedule.scheduled_by,
            scheduled_by_name: schedule.scheduled_by_name,
            created_at: rfc3339(schedule.created_at)?,
        })
    }
}

#[derive(Deserialize)]
pub(crate) struct OnePath {
    project: String,
    environment: String,
    changeset: String,
}

#[derive(Deserialize)]
pub(crate) struct ListPath {
    project: String,
    environment: String,
}

#[derive(Deserialize)]
pub(crate) struct SchedulePath {
    project: String,
    environment: String,
    schedule: String,
}

#[derive(Deserialize)]
pub(crate) struct ListQuery {
    changeset: Option<String>,
}

/// `{"local_time": "2026-11-03T09:00", "time_zone": "Europe/London", "choice": "earlier"}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewScheduleBody {
    local_time: String,
    time_zone: String,
    #[serde(default)]
    choice: Option<String>,
}

fn seconds(at: i64) -> Result<OffsetDateTime, ApiError> {
    OffsetDateTime::from_unix_timestamp(at).map_err(ApiError::internal)
}

/// What the API answers when a schedule's time is refused.
fn refused(error: ScheduleError) -> Result<ApiError, ApiError> {
    let invalid = |pointer: &'static str| ApiError::InvalidField {
        reason: error.code(),
        pointer,
        message: error.to_string(),
    };
    Ok(match error {
        ScheduleError::Ambiguous { earlier, later } | ScheduleError::Skipped { earlier, later } => {
            ApiError::NeedsChoice {
                reason: error.code(),
                message: error.to_string(),
                candidates: [rfc3339(seconds(earlier)?)?, rfc3339(seconds(later)?)?],
            }
        }
        ScheduleError::UnknownZone => invalid("/time_zone"),
        ScheduleError::InvalidLocalTime
        | ScheduleError::InThePast
        | ScheduleError::TooFar
        | ScheduleError::AfterApproval { .. } => invalid("/local_time"),
    })
}

/// Refuses unless the transaction's role may publish in every collection `changeset` touches.
async fn may_publish(tx: &mut ProjectTransaction, changeset: Uuid) -> Result<(), ApiError> {
    let collections = changesets::collections(tx, changeset)
        .await
        .map_err(ApiError::from_db)?;
    if collections
        .iter()
        .all(|collection| tx.policy.allows(Action::ContentPublish, Some(collection)))
    {
        Ok(())
    } else {
        Err(ApiError::Forbidden(
            "your role can't publish this content, so it can't schedule it either",
        ))
    }
}

/// `POST {P}/changesets/{changeset}/schedule`.
pub(crate) async fn create(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
    headers: HeaderMap,
    body: Result<Json<NewScheduleBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Data<ScheduleView>>), ApiError> {
    let expected = if_match(&headers)?;
    let Json(body) = body?;
    let choice = match body.choice.as_deref() {
        None => None,
        Some(name) => Some(Choice::parse(name).ok_or_else(|| ApiError::InvalidField {
            reason: "invalid_choice",
            pointer: "/choice",
            message: "a choice is earlier or later".to_owned(),
        })?),
    };
    let resolved = schedule::resolve(&body.local_time, &body.time_zone, choice)
        .map_err(|error| refused(error).unwrap_or_else(|internal| internal))?;
    let id = Uuid::try_parse(&path.changeset).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let head = changesets::lock(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    if head.hash != hash_bytes(&expected) {
        return Err(ApiError::StaleChangeset {
            current: content_hash(&head.hash),
        });
    }
    may_publish(&mut tx, id).await?;
    let now = OffsetDateTime::now_utc();
    let items = changesets::items(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?;
    let reviews = changesets::reviews(&mut tx, id, Some(&head.hash))
        .await
        .map_err(ApiError::from_db)?;
    let edited = items.iter().filter(|item| item.moved()).count();
    let status = publish::status(&head, edited, &reviews, now);
    if !matches!(status, Status::Approved { .. }) {
        publishable(&status)?;
    }
    let approval = reviews
        .first()
        .ok_or_else(|| ApiError::internal(std::io::Error::other("approved without a review")))?;
    let expires = approval.created_at.unix_timestamp() + APPROVAL_LIFETIME_SECS;
    schedule::check_window(resolved.at, now.unix_timestamp(), expires)
        .map_err(|error| refused(error).unwrap_or_else(|internal| internal))?;
    let created = schedules::create(
        &mut tx,
        &NewSchedule {
            id: Uuid::now_v7(),
            changeset: id,
            hash: &head.hash,
            approval: approval.id,
            local_time: &resolved.local,
            time_zone: &resolved.zone,
            choice: resolved.choice.map(Choice::as_str),
            run_at: seconds(resolved.at)?,
        },
    )
    .await
    .map_err(ApiError::from_db)?;
    let Created::Done(made) = created else {
        return Err(ApiError::Conflict(
            "this publish request is already scheduled; cancel that schedule to choose another time",
        ));
    };
    tx.commit().await.map_err(ApiError::from_db)?;
    tracing::info!(schedule = %made.id, changeset = %id, "publishing scheduled");
    Ok((
        StatusCode::CREATED,
        Json(Data {
            data: ScheduleView::new(*made)?,
        }),
    ))
}

/// `GET {P}/schedules`.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ListPath>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Page<ScheduleView>>, ApiError> {
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    // Any member may see what is scheduled: being in the transaction is the check.
    let rows = match query.changeset.as_deref() {
        None => schedules::list(&mut tx).await,
        Some(changeset) => {
            let changeset = Uuid::try_parse(changeset).map_err(|_| ApiError::InvalidField {
                reason: "invalid_changeset",
                pointer: "",
                message: "changeset is a changeset id".to_owned(),
            })?;
            schedules::of_changeset(&mut tx, changeset).await
        }
    }
    .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let data = rows
        .into_iter()
        .map(ScheduleView::new)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(Page {
        data,
        next_cursor: None,
    }))
}

/// `DELETE {P}/schedules/{schedule}`: by its author, or anyone who may publish what it publishes.
pub(crate) async fn cancel(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<SchedulePath>,
) -> Result<Json<Data<ScheduleView>>, ApiError> {
    let id = Uuid::try_parse(&path.schedule).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let found = schedules::get(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    if found.scheduled_by != tx.identity {
        may_publish(&mut tx, found.changeset_id).await?;
    }
    match schedules::cancel(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
    {
        Cancelled::Done(cancelled) => {
            tx.commit().await.map_err(ApiError::from_db)?;
            Ok(Json(Data {
                data: ScheduleView::new(*cancelled)?,
            }))
        }
        Cancelled::NotFound => Err(ApiError::NotFound),
        Cancelled::Finished(state) => Err(ApiError::Conflict(match state.as_str() {
            "published" => "this schedule has published already; cancelling never undoes a release",
            "blocked" => "this schedule was blocked when its time came; there is nothing to cancel",
            _ => "this schedule was cancelled already",
        })),
    }
}
