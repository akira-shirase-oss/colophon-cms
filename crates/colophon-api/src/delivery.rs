//! Public delivery, `/content/v1` (docs/modules/06-delivery.md; D-045): what a project publishes,
//! read from the published projection as `colophon_delivery`, which can't read drafts at all.
//! No sign-in: published content is public.
//!
//! - `GET /content/v1/{project}/{environment}/{collection}?generation=&limit=&cursor=`;
//! - `GET /content/v1/{project}/{environment}/{collection}/{entry}?generation=`.
//!
//! Without `generation`, reads are at the latest release; with it, pinned to that release, as it
//! was. Every answer names its generation in `Colophon-Generation`, and a list's cursor carries
//! the generation of its first page, so paging through a list never mixes two releases. No cache
//! headers yet: caching comes in P3.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderName, HeaderValue, StatusCode, header},
    response::Response,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use colophon_db::delivery::{self, DeliveryTransaction, Published};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    entries::{content_hash, json},
    session::services,
    shape::{Data, Page},
};

/// The response header naming the generation an answer was read at.
pub(crate) const GENERATION: HeaderName = HeaderName::from_static("colophon-generation");

const DEFAULT_PAGE: i64 = 20;

#[derive(Deserialize)]
pub(crate) struct CollectionPath {
    project: String,
    environment: String,
    collection: String,
}

#[derive(Deserialize)]
pub(crate) struct EntryPath {
    project: String,
    environment: String,
    collection: String,
    entry: String,
}

#[derive(Deserialize)]
pub(crate) struct ReadQuery {
    generation: Option<i64>,
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct PublishedView {
    id: Uuid,
    revision_id: Uuid,
    collection: String,
    title: String,
    content: Box<RawValue>,
    content_hash: String,
    /// The release it was published in.
    published_in: i64,
}

impl PublishedView {
    fn new(row: Published) -> Result<Self, ApiError> {
        Ok(Self {
            id: row.entry_id,
            revision_id: row.revision_id,
            collection: row.collection,
            title: row.title,
            content: RawValue::from_string(row.content).map_err(ApiError::internal)?,
            content_hash: content_hash(&row.content_hash),
            published_in: row.valid_from,
        })
    }
}

/// Opens a delivery transaction for `project` and finds the environment, or 404.
async fn open(
    state: &AppState,
    project: &str,
    environment: &str,
) -> Result<(DeliveryTransaction, delivery::Environment), ApiError> {
    let services = services(state)?;
    let project = Uuid::try_parse(project).map_err(|_| ApiError::NotFound)?;
    let mut tx = delivery::begin(&services.pool, project)
        .await
        .map_err(ApiError::from_db)?;
    let found = tx
        .environment(environment)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    Ok((tx, found))
}

/// The generation to read at: the one asked for, which must have committed, or the latest.
fn at(asked: Option<i64>, latest: i64) -> Result<i64, ApiError> {
    match asked {
        None => Ok(latest),
        Some(generation) if (1..=latest).contains(&generation) => Ok(generation),
        Some(_) => Err(ApiError::InvalidField {
            reason: "invalid_generation",
            pointer: "/generation",
            message: format!("this environment's releases are generations 1 to {latest}"),
        }),
    }
}

/// A cursor names the list it came from and the generation its first page was read at.
fn cursor(path: &CollectionPath, project: &str, generation: i64, last: Uuid) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "d1:{project}:{}:{}:{generation}:{last}",
        path.environment, path.collection
    ))
}

fn after_cursor(path: &CollectionPath, project: &str, text: &str) -> Result<(i64, Uuid), ApiError> {
    let prefix = format!("d1:{project}:{}:{}:", path.environment, path.collection);
    URL_SAFE_NO_PAD
        .decode(text)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|decoded| {
            let rest = decoded.strip_prefix(&prefix)?.to_owned();
            let (generation, last) = rest.split_once(':')?;
            Some((generation.parse().ok()?, Uuid::try_parse(last).ok()?))
        })
        .ok_or(ApiError::InvalidHeader(
            "this cursor is not valid for this list",
        ))
}

fn answer(body: String, generation: i64) -> Response {
    let mut response = json(StatusCode::OK, body, None, false);
    let headers = response.headers_mut();
    headers.insert(GENERATION, HeaderValue::from(generation));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// `GET /content/v1/{project}/{environment}/{collection}`.
pub(crate) async fn list(
    State(state): State<AppState>,
    Path(path): Path<CollectionPath>,
    Query(query): Query<ReadQuery>,
) -> Result<Response, ApiError> {
    let limit = query.limit.unwrap_or(DEFAULT_PAGE);
    if !(1..=delivery::MAX_PAGE).contains(&limit) {
        return Err(ApiError::InvalidHeader("limit must be between 1 and 100"));
    }
    let (mut tx, environment) = open(&state, &path.project, &path.environment).await?;
    let (generation, before) = match query.cursor.as_deref() {
        Some(text) => {
            let (generation, last) = after_cursor(&path, &path.project, text)?;
            (at(Some(generation), environment.generation)?, Some(last))
        }
        None => (at(query.generation, environment.generation)?, None),
    };
    let mut rows = if generation == 0 {
        Vec::new()
    } else {
        tx.list(
            environment.id,
            &path.collection,
            generation,
            before,
            limit + 1,
        )
        .await
        .map_err(ApiError::from_db)?
    };
    tx.finish().await.map_err(ApiError::from_db)?;
    let page = usize::try_from(limit).unwrap_or(usize::MAX);
    let more = rows.len() > page;
    rows.truncate(page);
    let next_cursor = more
        .then(|| {
            rows.last()
                .map(|row| cursor(&path, &path.project, generation, row.entry_id))
        })
        .flatten();
    let data = rows
        .into_iter()
        .map(PublishedView::new)
        .collect::<Result<Vec<_>, ApiError>>()?;
    let body = serde_json::to_string(&Page { data, next_cursor }).map_err(ApiError::internal)?;
    Ok(answer(body, generation))
}

/// `GET /content/v1/{project}/{environment}/{collection}/{entry}`.
pub(crate) async fn get(
    State(state): State<AppState>,
    Path(path): Path<EntryPath>,
    Query(query): Query<ReadQuery>,
) -> Result<Response, ApiError> {
    let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
    let (mut tx, environment) = open(&state, &path.project, &path.environment).await?;
    let generation = at(query.generation, environment.generation)?;
    let found = if generation == 0 {
        None
    } else {
        tx.get(environment.id, &path.collection, entry, generation)
            .await
            .map_err(ApiError::from_db)?
    };
    tx.finish().await.map_err(ApiError::from_db)?;
    let row = found.ok_or(ApiError::NotFound)?;
    let body = serde_json::to_string(&Data {
        data: PublishedView::new(row)?,
    })
    .map_err(ApiError::internal)?;
    Ok(answer(body, generation))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generation_must_have_committed() {
        assert_eq!(at(None, 4).ok(), Some(4));
        assert_eq!(at(Some(1), 4).ok(), Some(1));
        assert_eq!(at(Some(4), 4).ok(), Some(4));
        assert!(at(Some(5), 4).is_err(), "not yet");
        assert!(at(Some(0), 4).is_err());
        assert_eq!(
            at(None, 0).ok(),
            Some(0),
            "nothing published yet: an empty read"
        );
    }

    #[test]
    fn cursors_keep_their_list_and_their_generation() {
        let here = CollectionPath {
            project: "p".to_owned(),
            environment: "production".to_owned(),
            collection: "articles".to_owned(),
        };
        let last = Uuid::now_v7();
        let token = cursor(&here, "p", 7, last);
        assert_eq!(after_cursor(&here, "p", &token).ok(), Some((7, last)));
        let elsewhere = CollectionPath {
            collection: "events".to_owned(),
            ..here
        };
        assert!(after_cursor(&elsewhere, "p", &token).is_err());
        assert!(after_cursor(&elsewhere, "p", "not-a-cursor").is_err());
    }
}
