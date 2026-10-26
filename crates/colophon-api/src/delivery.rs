//! Public delivery, `/content/v1` (docs/modules/06-delivery.md; D-045, D-048): what a project
//! publishes, read from the published projection as `colophon_delivery`, which can't read drafts
//! at all. No sign-in: published content is public.
//!
//! - `GET /content/v1/{project}/{environment}/{collection}?generation=&limit=&cursor=`;
//! - `GET /content/v1/{project}/{environment}/{collection}/{entry}?generation=`.
//!
//! Without `generation`, reads are at the latest release; with it, pinned to that release, as it
//! was. Every answer names its generation in `Colophon-Generation`, and a list's cursor carries
//! the generation of its first page, so paging through a list never mixes two releases.
//!
//! Since step 3.1 (D-048) answers are cached by generation (`crate::cache`). The generation is
//! resolved from Postgres on every request, in a transaction of its own that ends before the
//! cache is asked, so no database connection waits on the cache or on another request's fill.
//! Answers carry an `ETag` (and a matching `If-None-Match` gets 304), the shared-cache policy for
//! their kind (latest 30 seconds, pinned 300, browsers always revalidate), `Cache-Tag` for purges,
//! and `Colophon-Cache: hit | miss | coalesced | bypass`.

use std::future::Future;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use colophon_db::delivery::{self, Published};
use colophon_domain::cache::Scope;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use uuid::Uuid;

use crate::{
    ApiError, AppState, Services,
    cache::{Cached, Filled, Refused},
    entries::{json, sha256},
    session::services,
    shape::{Data, Page},
};

/// The response header naming the generation an answer was read at.
pub(crate) const GENERATION: HeaderName = HeaderName::from_static("colophon-generation");
/// The response header saying where an answer came from.
pub(crate) const CACHE: HeaderName = HeaderName::from_static("colophon-cache");
/// The tags a CDN purges by (Cloudflare's header name).
pub(crate) const CACHE_TAG: HeaderName = HeaderName::from_static("cache-tag");

/// Module 06: live public queries may be shared for 30 seconds; browsers revalidate every time.
const LATEST: &str = "public, max-age=0, must-revalidate, s-maxage=30";
/// Module 06: generation-pinned content may be shared for 300 seconds.
const PINNED: &str = "public, max-age=0, must-revalidate, s-maxage=300";

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
            content_hash: crate::entries::content_hash(&row.content_hash),
            published_in: row.valid_from,
        })
    }
}

/// An answer read from Postgres: its body, and whether it is empty (served, not cached).
struct Answer {
    body: String,
    empty: bool,
}

/// A failed read that other requests were waiting for. Its cause was logged once, by the request
/// that read it.
#[derive(Debug, thiserror::Error)]
#[error("reading published content failed (logged by the request that read it)")]
struct SharedReadFailed;

/// The project's environment, from Postgres, in a transaction that ends here; or 404.
async fn resolve(
    services: &Services,
    project: Uuid,
    environment: &str,
) -> Result<delivery::Environment, ApiError> {
    let mut tx = delivery::begin(&services.pool, project)
        .await
        .map_err(ApiError::from_db)?;
    let found = tx
        .environment(environment)
        .await
        .map_err(ApiError::from_db)?;
    tx.finish().await.map_err(ApiError::from_db)?;
    found.ok_or(ApiError::NotFound)
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

/// A cursor names the list it came from and the generation its first page was read at. The
/// project is written in its canonical form, so one page has one body however the URL spelled it.
fn cursor(path: &CollectionPath, project: Uuid, generation: i64, last: Uuid) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "d1:{project}:{}:{}:{generation}:{last}",
        path.environment, path.collection
    ))
}

fn after_cursor(path: &CollectionPath, project: Uuid, text: &str) -> Result<(i64, Uuid), ApiError> {
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

/// A strong `ETag` for `body`: the first half of its SHA-256. The generation isn't part of it,
/// so an answer a later release didn't change still revalidates as unchanged.
fn etag(body: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = sha256(body.as_bytes());
    let mut text = String::with_capacity(34);
    text.push('"');
    for byte in &digest[..16] {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0xf)]));
    }
    text.push('"');
    text
}

/// Whether an `If-None-Match` value matches `etag` (RFC 9110 §13.1.2: weak comparison).
fn none_match(header: &str, etag: &str) -> bool {
    header.trim() == "*"
        || header
            .split(',')
            .map(|tag| tag.trim().trim_start_matches("W/"))
            .any(|tag| tag == etag)
}

/// One cacheable answer: from the cache, or filled from Postgres by `fill` (pinned to the
/// generation the key names), then rendered with its headers.
async fn serve<F, Fut>(
    services: &Services,
    request: &HeaderMap,
    key: &str,
    tags: &[String],
    generation: i64,
    pinned: bool,
    fill: F,
) -> Result<Response, ApiError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Option<Answer>, ApiError>>,
{
    let jitter = Uuid::now_v7().as_u64_pair().1;
    let (filled, served) = services
        .cache
        .get_or_fill(key, jitter, || async {
            match fill().await {
                Ok(Some(answer)) => {
                    let cached = Cached {
                        etag: etag(&answer.body),
                        body: answer.body,
                    };
                    Ok(if answer.empty {
                        Filled::Once(cached)
                    } else {
                        Filled::Keep(cached)
                    })
                }
                Ok(None) => Ok(Filled::Missing),
                Err(ApiError::Busy) => Err(Refused::Busy),
                Err(ApiError::Internal(cause)) => {
                    tracing::error!(%cause, "reading published content failed");
                    Err(Refused::Failed)
                }
                Err(other) => {
                    tracing::error!(error = %other, "reading published content failed");
                    Err(Refused::Failed)
                }
            }
        })
        .await
        .map_err(|refused| match refused {
            Refused::Busy => ApiError::Busy,
            Refused::Failed => ApiError::internal(SharedReadFailed),
        })?;
    let cached = match filled {
        Filled::Keep(cached) | Filled::Once(cached) => cached,
        Filled::Missing => return Err(ApiError::NotFound),
    };
    let unchanged = request
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| none_match(value, &cached.etag));
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        json(StatusCode::OK, cached.body, None, false)
    };
    let headers = response.headers_mut();
    headers.insert(GENERATION, HeaderValue::from(generation));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(if pinned { PINNED } else { LATEST }),
    );
    if let Ok(value) = HeaderValue::from_str(&cached.etag) {
        headers.insert(header::ETAG, value);
    }
    headers.insert(CACHE, HeaderValue::from_static(served.as_str()));
    if let Ok(value) = HeaderValue::from_str(&tags.join(",")) {
        headers.insert(CACHE_TAG, value);
    }
    Ok(response)
}

/// `GET /content/v1/{project}/{environment}/{collection}`.
pub(crate) async fn list(
    State(state): State<AppState>,
    Path(path): Path<CollectionPath>,
    Query(query): Query<ReadQuery>,
    request: HeaderMap,
) -> Result<Response, ApiError> {
    let limit = query.limit.unwrap_or(DEFAULT_PAGE);
    if !(1..=delivery::MAX_PAGE).contains(&limit) {
        return Err(ApiError::InvalidHeader("limit must be between 1 and 100"));
    }
    let services = services(&state)?;
    let project = Uuid::try_parse(&path.project).map_err(|_| ApiError::NotFound)?;
    let environment = resolve(services, project, &path.environment).await?;
    // A cursor's page is pinned to the generation of the first page, like `?generation=`.
    let (generation, before, pinned) = match query.cursor.as_deref() {
        Some(text) => {
            let (generation, last) = after_cursor(&path, project, text)?;
            (
                at(Some(generation), environment.generation)?,
                Some(last),
                true,
            )
        }
        None => (
            at(query.generation, environment.generation)?,
            None,
            query.generation.is_some(),
        ),
    };
    let scope = Scope {
        project: project.as_u128(),
        environment: environment.id.as_u128(),
        epoch: environment.epoch,
        generation,
    };
    let key = scope.list_key(&path.collection, limit, before.as_ref().map(Uuid::as_u128));
    let tags = scope.list_tags(&path.collection);
    serve(
        services,
        &request,
        &key,
        &tags,
        generation,
        pinned,
        || async {
            let mut rows = if generation == 0 {
                Vec::new()
            } else {
                let mut tx = delivery::begin(&services.pool, project)
                    .await
                    .map_err(ApiError::from_db)?;
                let rows = tx
                    .list(
                        environment.id,
                        &path.collection,
                        generation,
                        before,
                        limit + 1,
                    )
                    .await
                    .map_err(ApiError::from_db)?;
                tx.finish().await.map_err(ApiError::from_db)?;
                rows
            };
            let page = usize::try_from(limit).unwrap_or(usize::MAX);
            let more = rows.len() > page;
            rows.truncate(page);
            let next_cursor = more
                .then(|| {
                    rows.last()
                        .map(|row| cursor(&path, project, generation, row.entry_id))
                })
                .flatten();
            let data = rows
                .into_iter()
                .map(PublishedView::new)
                .collect::<Result<Vec<_>, ApiError>>()?;
            let empty = data.is_empty();
            let body =
                serde_json::to_string(&Page { data, next_cursor }).map_err(ApiError::internal)?;
            Ok(Some(Answer { body, empty }))
        },
    )
    .await
}

/// `GET /content/v1/{project}/{environment}/{collection}/{entry}`.
pub(crate) async fn get(
    State(state): State<AppState>,
    Path(path): Path<EntryPath>,
    Query(query): Query<ReadQuery>,
    request: HeaderMap,
) -> Result<Response, ApiError> {
    let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
    let services = services(&state)?;
    let project = Uuid::try_parse(&path.project).map_err(|_| ApiError::NotFound)?;
    let environment = resolve(services, project, &path.environment).await?;
    let generation = at(query.generation, environment.generation)?;
    if generation == 0 {
        return Err(ApiError::NotFound);
    }
    let scope = Scope {
        project: project.as_u128(),
        environment: environment.id.as_u128(),
        epoch: environment.epoch,
        generation,
    };
    let key = scope.entry_key(&path.collection, entry.as_u128());
    let tags = scope.entry_tags(&path.collection, entry.as_u128());
    let pinned = query.generation.is_some();
    serve(
        services,
        &request,
        &key,
        &tags,
        generation,
        pinned,
        || async {
            let mut tx = delivery::begin(&services.pool, project)
                .await
                .map_err(ApiError::from_db)?;
            let found = tx
                .get(environment.id, &path.collection, entry, generation)
                .await
                .map_err(ApiError::from_db)?;
            tx.finish().await.map_err(ApiError::from_db)?;
            let Some(row) = found else { return Ok(None) };
            let body = serde_json::to_string(&Data {
                data: PublishedView::new(row)?,
            })
            .map_err(ApiError::internal)?;
            Ok(Some(Answer { body, empty: false }))
        },
    )
    .await
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
        let (project, last) = (Uuid::now_v7(), Uuid::now_v7());
        let token = cursor(&here, project, 7, last);
        assert_eq!(after_cursor(&here, project, &token).ok(), Some((7, last)));
        assert!(
            after_cursor(&here, Uuid::now_v7(), &token).is_err(),
            "another project's list"
        );
        let elsewhere = CollectionPath {
            collection: "events".to_owned(),
            ..here
        };
        assert!(after_cursor(&elsewhere, project, &token).is_err());
        assert!(after_cursor(&elsewhere, project, "not-a-cursor").is_err());
    }

    #[test]
    fn an_etag_depends_on_the_body_alone_and_if_none_match_compares_weakly() {
        let tag = etag("{\"data\":[]}");
        assert_eq!(tag.len(), 34);
        assert_eq!(tag, etag("{\"data\":[]}"));
        assert_ne!(tag, etag("{\"data\":[1]}"));
        assert!(none_match(&tag, &tag));
        assert!(none_match(&format!("W/{tag}"), &tag));
        assert!(none_match(&format!("\"other\", {tag}"), &tag));
        assert!(none_match("*", &tag));
        assert!(!none_match("\"other\"", &tag));
        assert!(!none_match("", &tag));
    }
}
