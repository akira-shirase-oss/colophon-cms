//! The audit log's routes (docs/modules/04-api.md; D-058, D-063, step R.4).
//!
//! `GET /api/v1/projects/{project}/audit` lists who changed roles, members, invitations and API
//! tokens, newest first, for people whose role holds `audit.read` (owners, by default). Handlers
//! record their own changes with [`record`], in the transaction that makes them, so a change and
//! its record commit together. Nothing here updates or deletes an event: the database refuses it.

use axum::{
    Json,
    extract::{Path, Query, State},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use colophon_db::{audit, scope::MemberTransaction};
use colophon_domain::policy::Action;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use uuid::Uuid;

use crate::{
    AppState,
    entries::rfc3339,
    error::ApiError,
    members::{ProjectPath, fresh_policy, open, require},
    session::SignedIn,
    shape::Page,
};

/// The page size when the caller doesn't say.
const DEFAULT_PAGE: i64 = 50;

/// Records `action` on `subject` by the transaction's identity. `detail` is what the change was,
/// as the API shows it, never a secret.
pub(crate) async fn record(
    tx: &mut MemberTransaction,
    action: &str,
    subject: &str,
    detail: &serde_json::Value,
) -> Result<(), ApiError> {
    audit::record(tx, action, subject, &detail.to_string())
        .await
        .map_err(ApiError::from_db)
}

/// `?limit=&cursor=&action=&actor=`.
#[derive(Deserialize)]
pub(crate) struct AuditQuery {
    limit: Option<i64>,
    cursor: Option<String>,
    action: Option<String>,
    actor: Option<String>,
}

#[derive(Serialize)]
struct ActorView {
    id: Uuid,
    name: Option<String>,
    /// `person` or `token`.
    kind: &'static str,
}

#[derive(Serialize)]
pub(crate) struct EventView {
    id: Uuid,
    at: String,
    actor: ActorView,
    action: String,
    subject: String,
    /// What the change was, as recorded.
    detail: Box<RawValue>,
}

/// A cursor names the project and the last event seen.
fn cursor(project: Uuid, last: Uuid) -> String {
    URL_SAFE_NO_PAD.encode(format!("audit:v1:{project}:{last}"))
}

fn after_cursor(project: Uuid, cursor: &str) -> Result<Uuid, ApiError> {
    let prefix = format!("audit:v1:{project}:");
    URL_SAFE_NO_PAD
        .decode(cursor)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| Uuid::try_parse(text.strip_prefix(&prefix)?).ok())
        .ok_or(ApiError::InvalidHeader(
            "this cursor is not valid for this list",
        ))
}

/// `GET /api/v1/projects/{project}/audit`.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ProjectPath>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Page<EventView>>, ApiError> {
    let limit = query.limit.unwrap_or(DEFAULT_PAGE);
    if !(1..=audit::MAX_PAGE).contains(&limit) {
        return Err(ApiError::InvalidHeader("limit must be between 1 and 100"));
    }
    let actor = query
        .actor
        .as_deref()
        .map(|text| {
            Uuid::try_parse(text)
                .map_err(|_| ApiError::InvalidHeader("actor must be an identity id"))
        })
        .transpose()?;
    let action = query
        .action
        .as_deref()
        .filter(|text| !text.is_empty() && text.len() <= 50);
    let mut tx = open(&state, caller, &path.project).await?;
    require(
        &fresh_policy(&mut tx).await?,
        Action::AuditRead,
        None,
        "your role can't read the audit log",
    )?;
    let project = tx.project;
    let before = query
        .cursor
        .as_deref()
        .map(|text| after_cursor(project, text))
        .transpose()?;
    let mut events = audit::list(&mut tx, before, limit + 1, action, actor)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let more = events.len() > usize::try_from(limit).unwrap_or(usize::MAX);
    events.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    let next_cursor = more
        .then(|| events.last().map(|event| cursor(project, event.id)))
        .flatten();
    let data = events
        .into_iter()
        .map(|event| {
            Ok(EventView {
                id: event.id,
                at: rfc3339(event.at)?,
                actor: ActorView {
                    id: event.actor_id,
                    name: event.actor_name,
                    kind: if event.actor_is_token {
                        "token"
                    } else {
                        "person"
                    },
                },
                action: event.action,
                subject: event.subject,
                detail: RawValue::from_string(event.detail).map_err(ApiError::internal)?,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    Ok(Json(Page { data, next_cursor }))
}
