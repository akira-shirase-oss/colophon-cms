//! Shared components (step M.4, D-073). Under
//! `{P}` = `/api/v1/projects/{project}/environments/{environment}`:
//!
//! - `GET  {P}/components`: the current version of each;
//! - `POST {P}/components`: `{"slug", "definition"}` (`Idempotency-Key` required);
//! - `GET  {P}/components/{slug}`: the current version, with `ETag: "v<version>"`;
//! - `PUT  {P}/components/{slug}`: a new definition (`If-Match` and `Idempotency-Key`
//!   required). Every content type that uses the component gets a new version in the same
//!   transaction, each checked against its stored content like any change to it (D-055); if one
//!   is refused, nothing changes, and the refusal names that content type.
//!
//! A content type uses a component with a field `{"type": "component", "component": slug}`,
//! which the server replaces with the component's fields (`content_types::expand`). Any member
//! may read components; changing them takes `schema.change` everywhere, since a change reaches
//! every content type that uses one.

use std::{collections::BTreeMap, sync::Arc};

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use colophon_db::{
    DbError, components,
    content_types::{self as type_store, Updated},
    idempotency,
    scope::ProjectTransaction,
};
use colophon_domain::{
    policy::Action,
    project::Slug,
    schema::{self, Component, ContentType, Refusal, Successor, Unlisted},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, value::RawValue};

use crate::{
    ApiError, AppState,
    content_types::{TypesPath, check_stored, claim, if_match, respond},
    entries::{content_hash, idempotency_key, json, open, request_hash, rfc3339, sha256},
    error::InTheWay,
    session::SignedIn,
    shape::{Data, Page},
};

const CREATE: &str = "component.create";
const UPDATE: &str = "component.update";

/// The path of the single-component routes.
#[derive(Deserialize)]
pub(crate) struct ComponentPath {
    project: String,
    environment: String,
    slug: String,
}

/// `{"slug": ..., "definition": {...}}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewComponent {
    slug: String,
    definition: Value,
}

#[derive(Serialize)]
struct ComponentView {
    slug: String,
    version: i32,
    /// The canonical definition, embedded as stored.
    definition: Box<RawValue>,
    definition_hash: String,
    created_at: String,
    /// After a change: the content types it gave a new version, in slug order.
    #[serde(skip_serializing_if = "Option::is_none")]
    content_types: Option<Vec<TypeVersion>>,
}

/// A content type version a component change wrote.
#[derive(Serialize)]
struct TypeVersion {
    slug: String,
    version: i32,
}

impl ComponentView {
    fn new(
        version: components::Version,
        content_types: Option<Vec<TypeVersion>>,
    ) -> Result<Self, ApiError> {
        Ok(Self {
            slug: version.component,
            version: version.version,
            definition: RawValue::from_string(version.definition).map_err(ApiError::internal)?,
            definition_hash: content_hash(&version.definition_hash),
            created_at: rfc3339(version.created_at)?,
            content_types,
        })
    }
}

fn may_change(tx: &ProjectTransaction) -> Result<(), ApiError> {
    crate::members::require(
        &tx.policy,
        Action::SchemaChange,
        None,
        "your role can't change shared components: a change reaches every content type that uses one",
    )
}

/// Records the answer and commits before responding.
async fn finish(
    mut tx: ProjectTransaction,
    operation: &str,
    key: &str,
    status: StatusCode,
    view: ComponentView,
) -> Result<Response, ApiError> {
    let number = view.version;
    let body = serde_json::to_string(&Data { data: view }).map_err(ApiError::internal)?;
    idempotency::complete(&mut tx, operation, key, status.as_u16(), &body)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(respond(status, body, number, false))
}

/// `GET {P}/components`.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<TypesPath>,
) -> Result<Response, ApiError> {
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let versions = components::list(&mut tx).await.map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let data = versions
        .into_iter()
        .map(|version| ComponentView::new(version, None))
        .collect::<Result<Vec<_>, ApiError>>()?;
    let body = serde_json::to_string(&Page {
        data,
        next_cursor: None,
    })
    .map_err(ApiError::internal)?;
    Ok(json(StatusCode::OK, body, None, false))
}

/// `GET {P}/components/{slug}`.
pub(crate) async fn get(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ComponentPath>,
) -> Result<Response, ApiError> {
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let version = components::current(&mut tx, &path.slug)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let number = version.version;
    let body = serde_json::to_string(&Data {
        data: ComponentView::new(version, None)?,
    })
    .map_err(ApiError::internal)?;
    Ok(respond(StatusCode::OK, body, number, false))
}

/// `POST {P}/components`.
pub(crate) async fn create(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<TypesPath>,
    headers: HeaderMap,
    body: Result<Json<NewComponent>, JsonRejection>,
) -> Result<Response, ApiError> {
    let key = idempotency_key(&headers)?;
    let Json(body) = body?;
    let slug = Slug::parse(&body.slug).map_err(|error| ApiError::InvalidField {
        reason: error.code(),
        pointer: "/slug",
        message: error.to_string(),
    })?;
    let component = Component::parse(&body.definition).map_err(|refusal| {
        ApiError::Invalid(Refusal {
            pointer: format!("/definition{}", refusal.pointer),
            ..refusal
        })
    })?;
    let canonical = component.canonical_json();
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_change(&tx)?;
    let request = request_hash(&[CREATE, slug.as_str(), &canonical]);
    if let Some(replay) = claim(&mut tx, CREATE, key, &request).await? {
        return Ok(replay);
    }
    let version = components::create(
        &mut tx,
        slug.as_str(),
        &canonical,
        &sha256(canonical.as_bytes()),
    )
    .await
    .map_err(|error| match error {
        DbError::Conflict => ApiError::conflict(
            "component_exists",
            "a component with this slug already exists",
        ),
        other => ApiError::from_db(other),
    })?;
    finish(
        tx,
        CREATE,
        key,
        StatusCode::CREATED,
        ComponentView::new(version, None)?,
    )
    .await
}

/// `PUT {P}/components/{slug}`.
pub(crate) async fn update(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<ComponentPath>,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Response, ApiError> {
    let expected = if_match(&headers)?;
    let key = idempotency_key(&headers)?;
    let Json(value) = body?;
    let component = Component::parse(&value).map_err(ApiError::Invalid)?;
    let canonical = component.canonical_json();
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_change(&tx)?;
    let request = request_hash(&[UPDATE, &path.slug, &expected.to_string(), &canonical]);
    if let Some(replay) = claim(&mut tx, UPDATE, key, &request).await? {
        return Ok(replay);
    }
    // Before anything else: content type writes that use it wait from here (D-073).
    components::lock_for_change(&mut tx, &path.slug)
        .await
        .map_err(ApiError::from_db)?;
    let version = match components::update(
        &mut tx,
        &path.slug,
        expected,
        &canonical,
        &sha256(canonical.as_bytes()),
    )
    .await
    .map_err(ApiError::from_db)?
    {
        components::Updated::Updated(version) => version,
        components::Updated::Stale { current } => {
            return Err(ApiError::StaleVersion { current });
        }
        components::Updated::NotFound => return Err(ApiError::NotFound),
    };
    let rewritten = rewrite_users(&mut tx, &path.slug, component.fields_json()).await?;
    finish(
        tx,
        UPDATE,
        key,
        StatusCode::OK,
        ComponentView::new(version, Some(rewritten))?,
    )
    .await
}

/// Gives every content type that uses `slug` a new version holding `fields`, in slug order, each
/// under its own change lock and checked like any change to it. The first that can't take it
/// refuses the whole change, named.
async fn rewrite_users(
    tx: &mut ProjectTransaction,
    slug: &str,
    fields: Value,
) -> Result<Vec<TypeVersion>, ApiError> {
    let marker = format!(r#""component":"{slug}""#);
    let new_fields = BTreeMap::from([(slug.to_owned(), fields)]);
    let mut rewritten = Vec::new();
    for user in components::users(tx, slug)
        .await
        .map_err(ApiError::from_db)?
    {
        let collection = user.collection;
        type_store::lock_for_change(tx, &collection)
            .await
            .map_err(ApiError::from_db)?;
        // Read again under the lock: it may have stopped using the component meanwhile.
        let Some(current) = type_store::current(tx, &collection)
            .await
            .map_err(ApiError::from_db)?
            .filter(|current| current.definition.contains(&marker))
        else {
            continue;
        };
        let stored: Value =
            serde_json::from_str(&current.definition).map_err(ApiError::internal)?;
        let previous = ContentType::parse(&stored).map_err(ApiError::internal)?;
        let expanded = schema::expand_components(&stored, &new_fields, Unlisted::Keep)
            .map_err(ApiError::internal)?;
        let next = ContentType::parse(&expanded)
            .map_err(|refusal| in_type(&collection, ApiError::Invalid(refusal)))?;
        let canonical = next.canonical_json();
        if canonical == current.definition {
            continue;
        }
        match previous.successor(&next) {
            Successor::Compatible => {}
            Successor::CheckContent(refusal) => {
                check_stored(tx, &collection, Arc::new(next), refusal)
                    .await
                    .map_err(|error| in_type(&collection, error))?;
            }
            Successor::Refused(refusal) => {
                return Err(in_type(&collection, ApiError::Invalid(refusal)));
            }
        }
        match type_store::update(
            tx,
            &collection,
            current.version,
            &canonical,
            &sha256(canonical.as_bytes()),
        )
        .await
        .map_err(ApiError::from_db)?
        {
            Updated::Updated(version) => rewritten.push(TypeVersion {
                slug: version.collection,
                version: version.version,
            }),
            // Its change lock is held: nothing else can write it.
            Updated::Stale { .. } | Updated::NotFound => {
                return Err(ApiError::internal(std::io::Error::other(
                    "a content type changed under its change lock",
                )));
            }
        }
    }
    Ok(rewritten)
}

/// A refusal of the change to `collection` a component change would make, said as the component
/// change's: its pointer is into that content type's definition, not into this request.
fn in_type(collection: &str, error: ApiError) -> ApiError {
    let say = |message: &str| format!("`{collection}` uses this component, and {message}");
    match error {
        ApiError::Invalid(refusal) => ApiError::Invalid(Refusal {
            message: say(&refusal.message),
            pointer: String::new(),
            ..refusal
        }),
        ApiError::NeedsMigration(way) => ApiError::NeedsMigration(Box::new(InTheWay {
            message: say(&way.message),
            pointer: String::new(),
            ..*way
        })),
        other => other,
    }
}
