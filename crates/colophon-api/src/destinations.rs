//! Destinations and delivery jobs (docs/modules/04-api.md, "Route families"; D-046). Under `{P}`:
//!
//! - `GET {P}/destinations` and `POST {P}/destinations` (`{"kind", "name", "url"}`, `kind` being
//!   `webhook`, the default, or `cdn`, which is sent purges by tag instead; D-050): owners and
//!   developers only, since an address can carry a secret;
//! - `PUT {P}/destinations/{id}` (`{"enabled"}`): turns announcements on or off;
//! - `GET {P}/jobs?generation=`: delivery jobs, newest first, for any member.
//!
//! An address must be https and public (`colophon_domain::destination::check_url`); a
//! development server on loopback may also use http and private addresses.

use axum::{
    Json,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::StatusCode,
};
use colophon_db::{destinations, jobs};
use colophon_domain::{destination::check_url, policy::Action};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    entries::{open, rfc3339},
    members::require,
    session::{SignedIn, services},
    shape::{Data, Page},
};

#[derive(Deserialize)]
pub(crate) struct ListPath {
    project: String,
    environment: String,
}

#[derive(Deserialize)]
pub(crate) struct OnePath {
    project: String,
    environment: String,
    destination: String,
}

/// `{"kind": "webhook", "name": "Site rebuild", "url": "https://…"}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewDestination {
    #[serde(default = "webhook")]
    kind: String,
    name: String,
    url: String,
}

fn webhook() -> String {
    "webhook".to_owned()
}

/// `{"enabled": false}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Enabled {
    enabled: bool,
}

#[derive(Deserialize)]
pub(crate) struct JobQuery {
    generation: Option<i64>,
}

#[derive(Serialize)]
pub(crate) struct DestinationView {
    id: Uuid,
    kind: String,
    name: String,
    url: String,
    enabled: bool,
    created_at: String,
}

impl DestinationView {
    fn new(row: destinations::Destination) -> Result<Self, ApiError> {
        Ok(Self {
            id: row.id,
            kind: row.kind,
            name: row.name,
            url: row.url,
            enabled: row.enabled,
            created_at: rfc3339(row.created_at)?,
        })
    }
}

#[derive(Serialize)]
pub(crate) struct JobView {
    id: Uuid,
    destination_id: Uuid,
    destination_name: String,
    generation: i64,
    status: String,
    attempts: i32,
    last_http_status: Option<i16>,
    last_error: Option<String>,
    created_at: String,
    finished_at: Option<String>,
}

const WHO: &str =
    "only a project's owners and developers can see and change where releases are delivered";

/// `GET {P}/destinations`.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ListPath>,
) -> Result<Json<Page<DestinationView>>, ApiError> {
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    require(&tx.role, Action::ManageDestinations, WHO)?;
    let rows = destinations::list(&mut tx)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let data = rows
        .into_iter()
        .map(DestinationView::new)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(Page {
        data,
        next_cursor: None,
    }))
}

/// `POST {P}/destinations`.
pub(crate) async fn create(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ListPath>,
    body: Result<Json<NewDestination>, JsonRejection>,
) -> Result<(StatusCode, Json<Data<DestinationView>>), ApiError> {
    let allow_private = services(&state)?.allow_private_destinations;
    let Json(new) = body?;
    if new.kind != "webhook" && new.kind != "cdn" {
        return Err(ApiError::InvalidField {
            reason: "invalid_kind",
            pointer: "/kind",
            message: "a destination is a webhook or a cdn".to_owned(),
        });
    }
    let name = new.name.trim();
    if !(1..=100).contains(&name.chars().count()) || name.chars().any(char::is_control) {
        return Err(ApiError::InvalidField {
            reason: "invalid_name",
            pointer: "/name",
            message: "a name is 1 to 100 characters, without control characters".to_owned(),
        });
    }
    let url = new.url.trim();
    if url.chars().count() > 2000 {
        return Err(ApiError::InvalidField {
            reason: "invalid_url",
            pointer: "/url",
            message: "a URL is at most 2,000 characters".to_owned(),
        });
    }
    check_url(url, allow_private).map_err(|message| ApiError::InvalidField {
        reason: "invalid_url",
        pointer: "/url",
        message,
    })?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    require(&tx.role, Action::ManageDestinations, WHO)?;
    let row = destinations::create(&mut tx, &new.kind, name, url)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok((
        StatusCode::CREATED,
        Json(Data {
            data: DestinationView::new(row)?,
        }),
    ))
}

/// `PUT {P}/destinations/{destination}`.
pub(crate) async fn set_enabled(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<OnePath>,
    body: Result<Json<Enabled>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(change) = body?;
    let id = Uuid::try_parse(&path.destination).map_err(|_| ApiError::NotFound)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    require(&tx.role, Action::ManageDestinations, WHO)?;
    if destinations::set_enabled(&mut tx, id, change.enabled)
        .await
        .map_err(ApiError::from_db)?
    {
        tx.commit().await.map_err(ApiError::from_db)?;
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

/// `GET {P}/jobs?generation=`.
pub(crate) async fn jobs(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ListPath>,
    Query(query): Query<JobQuery>,
) -> Result<Json<Page<JobView>>, ApiError> {
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let rows = jobs::list(&mut tx, query.generation)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let data = rows
        .into_iter()
        .map(|row| {
            Ok(JobView {
                id: row.id,
                destination_id: row.destination_id,
                destination_name: row.destination_name,
                generation: row.generation,
                status: row.status,
                attempts: row.attempts,
                last_http_status: row.last_http_status,
                last_error: row.last_error,
                created_at: rfc3339(row.created_at)?,
                finished_at: row.finished_at.map(rfc3339).transpose()?,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    Ok(Json(Page {
        data,
        next_cursor: None,
    }))
}
