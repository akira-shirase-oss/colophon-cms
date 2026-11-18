//! Public delivery, `/content/v1` (docs/modules/06-delivery.md; D-045, D-048): what a project
//! publishes, read from the published projection as `colophon_delivery`, which can't read drafts
//! at all. No sign-in: published content is public.
//!
//! - `GET /content/v1/{project}/{environment}/{collection}?generation=&limit=&cursor=`;
//! - `GET /content/v1/{project}/{environment}/{collection}/{entry}?generation=`;
//! - `POST /content/v1/{project}/{environment}/{collection}/query?generation=&cursor=`
//!   (DX.4): the query language over the published projection — the same document the
//!   draft route takes, the same `applied` echo, answered from `publications`, never
//!   drafts. `depth` populates references (DX.5, D-107): each level's targets are the
//!   publications pinned at the page's own generation, resolved inside the fill's
//!   transaction — so a populated reference reads exactly the release its row does, and
//!   a publication that doesn't resolve at it stays the stored id. A query page's cache
//!   key names the query's own hash — the normalized query — so two different queries
//!   can never share an answer (spec §15).
//!
//! Without `generation`, reads are at the latest release; with it, pinned to that release, as it
//! was. Every answer names its generation in `Colophon-Generation`, and a list's cursor carries
//! the generation of its first page, so paging through a list never mixes two releases.
//!
//! Since step 3.1 (D-048) answers are cached by generation (`crate::cache`). The generation is
//! resolved from Postgres on every request, in one statement that ends before the cache is asked
//! (`delivery::environment`), so no database connection waits on the cache or on another
//! request's fill.
//! Answers carry an `ETag` (and a matching `If-None-Match` gets 304), the shared-cache policy for
//! their kind (latest 30 seconds, pinned 300, browsers always revalidate), `Cache-Tag` for purges,
//! and `Colophon-Cache: hit | miss | coalesced | bypass`.
//!
//! Since step 3.2 (D-049) an unpublished entry is never delivered, at any generation: lists leave
//! it out and a read of it is 410 `gone`, uncached. Cache keys name the environment's latest
//! unpublish, so answers cached before it are never served again.
//!
//! Locales (D-181, D-182): every row names its `locale` — its own tag, or the default language's
//! for an untagged entry — and an entry read lists its published translations as `alternates`.
//! `?locale=` (with `?fallback=policy|none|<tags>`) answers in one language: lists and queries
//! give one row per translation group, the best published member through the chain, labelled
//! with `localization`; an entry read gives the entry's best translation. Without `?locale=`
//! every language comes back, as before. Resolution runs inside the reader's own row-level
//! security, so a member-only translation is never an anonymous reader's fallback.

use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
};

use axum::{
    Json,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use colophon_db::{
    content_types,
    delivery::{self, Audience, Published},
    query as db_query,
};
use colophon_domain::{
    cache::Scope,
    locale::{self, Fallback},
    query::Query as Document,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, value::RawValue};
use uuid::Uuid;

use crate::{
    ApiError, AppState, Services, audience,
    cache::{Cached, Filled, Refused},
    entries::{AppliedQuery, json, narrow, sha256},
    session::services,
    shape::{Data, Page},
};

/// The response header naming the generation an answer was read at.
pub(crate) const GENERATION: HeaderName = HeaderName::from_static("colophon-generation");
/// The response header saying where an answer came from.
pub(crate) const CACHE: HeaderName = HeaderName::from_static("colophon-cache");
/// The tags a CDN purges by (Cloudflare's header name).
pub(crate) const CACHE_TAG: HeaderName = HeaderName::from_static("cache-tag");
/// The response header naming the locale policy version an answer resolved with (D-182).
pub(crate) const LOCALE_POLICY: HeaderName = HeaderName::from_static("colophon-locale-policy");

/// Module 06: live public queries may be shared for 30 seconds; browsers revalidate every time.
const LATEST: &str = "public, max-age=0, must-revalidate, s-maxage=30";
/// Module 06: generation-pinned content may be shared for 300 seconds.
const PINNED: &str = "public, max-age=0, must-revalidate, s-maxage=300";
/// D-137: a member read is theirs alone — the browser may hold it but must revalidate, and no
/// shared cache may store it. The generation-keyed cache is the anonymous audience's; a gated
/// row must never enter it.
const MEMBER: &str = "private, no-cache";

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
    locale: Option<String>,
    fallback: Option<String>,
}

/// The `?generation=`, `?cursor=`, `?locale=` and `?fallback=` a delivery query takes (DX.4,
/// D-182): the release pin, the page boundary and the language travel outside the document,
/// which the validator holds to its own keys — `limit` among them, the document's own, not a
/// parameter's.
#[derive(Deserialize)]
pub(crate) struct QueryParams {
    generation: Option<i64>,
    cursor: Option<String>,
    locale: Option<String>,
    fallback: Option<String>,
}

/// A `?locale=` read, checked against the environment's policy (D-182).
struct Asked {
    /// The requested tag, as the environment's list spells it (or as asked, without a list).
    tag: String,
    /// The chain and default the read resolves through.
    resolution: delivery::Resolution,
}

/// Checks `?locale=` and `?fallback=` against `environment`'s policy: a well-formed tag, one
/// the list has when there is a list (`unknown_locale` otherwise — the list bounds the keys a
/// cache can hold), and fallbacks that are `policy`, `none` or up to four tags, listed when
/// there is a list. `None` without `?locale=`, which reads every language.
fn asked(
    environment: &delivery::Environment,
    locale: Option<&str>,
    fallback: Option<&str>,
) -> Result<Option<Asked>, ApiError> {
    let Some(requested) = locale else {
        if fallback.is_some() {
            return Err(ApiError::InvalidField {
                reason: "invalid_fallback",
                pointer: "/fallback",
                message: "name a ?locale= to fall back from".to_owned(),
            });
        }
        return Ok(None);
    };
    if !locale::is_tag(requested) {
        return Err(ApiError::InvalidField {
            reason: "invalid_locale",
            pointer: "/locale",
            message: format!(
                "\"{requested}\" isn't a BCP-47 language tag (like \"en\" or \"pt-BR\")"
            ),
        });
    }
    let listed = !environment.locales.is_empty();
    let tag = match (&environment.listed_locale, listed) {
        (Some(spelled), _) => spelled.clone(),
        (None, false) => requested.to_owned(),
        (None, true) => {
            return Err(ApiError::InvalidField {
                reason: "unknown_locale",
                pointer: "/locale",
                message: format!(
                    "{requested} isn't one of this environment's languages ({})",
                    environment.locales.join(", ")
                ),
            });
        }
    };
    let mut fallback = Fallback::parse(fallback).ok_or_else(|| ApiError::InvalidField {
        reason: "invalid_fallback",
        pointer: "/fallback",
        message: format!(
            "fallback is policy, none, or up to {} language tags separated by commas",
            locale::MAX_FALLBACKS
        ),
    })?;
    // Without a list, a made-up tag matches nothing and its empty page is never stored; named
    // fallbacks would give every made-up tag a real answer to store, so they need a list.
    if !listed && matches!(fallback, Fallback::Tags(_)) {
        return Err(ApiError::InvalidField {
            reason: "invalid_fallback",
            pointer: "/fallback",
            message: "list this environment's languages before naming fallbacks".to_owned(),
        });
    }
    if let Fallback::Tags(tags) = &mut fallback {
        for tag in tags.iter_mut() {
            let Some(spelled) = environment
                .locales
                .iter()
                .find(|listed| listed.eq_ignore_ascii_case(tag))
            else {
                return Err(ApiError::InvalidField {
                    reason: "unknown_locale",
                    pointer: "/fallback",
                    message: format!("{tag} isn't one of this environment's languages"),
                });
            };
            tag.clone_from(spelled);
        }
    }
    let chain = locale::chain(&tag, &fallback, &environment.chain);
    Ok(Some(Asked {
        tag,
        resolution: delivery::Resolution {
            chain,
            default: environment.default_locale.clone(),
        },
    }))
}

/// The cache scope of a read at `generation` in `environment`, in the language `asked` names.
fn scope_of(
    project: Uuid,
    environment: &delivery::Environment,
    generation: i64,
    asked: Option<&Asked>,
    media: u64,
) -> Scope {
    Scope {
        project: project.as_u128(),
        environment: environment.id.as_u128(),
        epoch: environment.epoch,
        generation,
        unpublished: environment.last_unpublish,
        media,
        locale_policy: environment.locale_policy,
        locale: asked.map(|asked| locale::chain_digest(&asked.resolution.chain)),
    }
}

/// Names the policy an answer resolved with, and — for a `?locale=` read — its language.
fn with_locale(
    mut response: Response,
    environment: &delivery::Environment,
    asked: Option<&Asked>,
) -> Response {
    let headers = response.headers_mut();
    headers.insert(LOCALE_POLICY, HeaderValue::from(environment.locale_policy));
    if let Some(asked) = asked
        && let Ok(value) = HeaderValue::from_str(&asked.tag)
    {
        headers.insert(header::CONTENT_LANGUAGE, value);
    }
    response
}

/// How a `?locale=` row was resolved (D-182).
#[derive(Serialize)]
struct Localization {
    /// The tag asked for.
    requested: String,
    /// The tag served: the row's own, or the default language's for an untagged entry.
    resolved: Option<String>,
    /// Whether that is a fallback rather than the tag asked for.
    fallback: bool,
}

/// A published translation of an entry (D-182): what a site needs for `hreflang` links.
#[derive(Serialize)]
struct Alternate {
    id: Uuid,
    locale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    slug: Option<String>,
}

/// One published entry as `/content/v1` answers it — and, with a `preview` member beside it,
/// as `/preview/v1` does (D-172): one serialization for both.
#[derive(Serialize)]
pub(crate) struct PublishedView {
    id: Uuid,
    revision_id: Uuid,
    collection: String,
    title: String,
    /// Left out when the entry has none, so answers cached before slugs existed read the same.
    #[serde(skip_serializing_if = "Option::is_none")]
    slug: Option<String>,
    content: Box<RawValue>,
    content_hash: String,
    /// The release it was published in.
    published_in: i64,
    /// Who may read it (D-137): a site that caches pages needs to know which answers are
    /// member-only even before it signs anyone in.
    visibility: String,
    /// Its language (D-182): its own tag, or the default language's for an untagged entry;
    /// `null` when it has none and the environment lists no default.
    locale: Option<String>,
    /// On a `?locale=` read: what was asked for, what was served, and whether it fell back.
    #[serde(skip_serializing_if = "Option::is_none")]
    localization: Option<Localization>,
    /// On a `?locale=` entry read: the id in the URL, when the answer is a translation of it.
    #[serde(skip_serializing_if = "Option::is_none")]
    requested_id: Option<Uuid>,
    /// On an entry read: its other published translations at the same generation.
    #[serde(skip_serializing_if = "Option::is_none")]
    alternates: Option<Vec<Alternate>>,
    /// The uploads it names — asset fields and rich-text images — at the versions its release
    /// recorded (D-178), by asset id. Left out when it names none.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    media: BTreeMap<String, MediaView>,
}

/// What a release recorded of one upload, as `publications.media` holds it (D-178).
/// Every field is optional on the way in: a stamp missing one is left out of the answer rather
/// than failing the whole row.
#[derive(Deserialize)]
struct Stamp {
    sha: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    #[serde(default)]
    renditions: Vec<String>,
}

/// One upload as a site sees it (D-178): `url` and the renditions name the exact bytes the
/// release published and never change (cached a year, D-157); `current_url` follows
/// replacements, for a site that wants the newest bytes without publishing again.
#[derive(Serialize)]
pub(crate) struct MediaView {
    url: String,
    current_url: String,
    #[serde(rename = "type")]
    kind: String,
    width: u32,
    height: u32,
    renditions: BTreeMap<String, String>,
}

/// Where an answer's media URLs start: `{origin}/media/v1/{project}/{environment}`.
pub(crate) struct MediaBase(String);

impl MediaBase {
    pub(crate) fn new(origin: &str, project: Uuid, environment: &str) -> Self {
        Self(format!("{origin}/media/v1/{project}/{environment}"))
    }

    /// The views of a publication's recorded `media`. Only what the server itself wrote is
    /// stored there, but it still goes into URLs: an id that isn't a UUID, a digest that isn't
    /// 64 hex characters or a rendition name that isn't lowercase letters and digits is left
    /// out rather than written into a link.
    fn views(&self, media: &str) -> Result<BTreeMap<String, MediaView>, ApiError> {
        let stamps: BTreeMap<String, Stamp> =
            serde_json::from_str(media).map_err(ApiError::internal)?;
        let base = &self.0;
        Ok(stamps
            .into_iter()
            .filter_map(|(asset, stamp)| {
                let sha = stamp
                    .sha
                    .filter(|sha| colophon_media::is_version_sha(sha))?;
                Uuid::try_parse(&asset).ok()?;
                Some((
                    asset,
                    sha,
                    stamp.kind?,
                    stamp.width?,
                    stamp.height?,
                    stamp.renditions,
                ))
            })
            .map(|(asset, sha, kind, width, height, renditions)| {
                let url = format!("{base}/{asset}/{sha}");
                let renditions = renditions
                    .iter()
                    .filter(|name| {
                        !name.is_empty()
                            && name.len() <= 16
                            && name
                                .bytes()
                                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                    })
                    .map(|name| (name.clone(), format!("{url}/{name}")))
                    .collect();
                let view = MediaView {
                    current_url: format!("{base}/{asset}"),
                    url,
                    kind,
                    width,
                    height,
                    renditions,
                };
                (asset, view)
            })
            .collect())
    }
}

/// A query row's `media` (D-178): its own recorded versions plus those of the publications
/// `depth` populated into it, kept to the uploads its answered `content` still names — so
/// `fields` narrowing drops the stamps of the fields it left out, and a populated reference's
/// images carry the versions its release recorded. An asset id is a UUID, so finding it in the
/// content's JSON is finding where it is named.
pub(crate) fn media_named(
    own: &str,
    populated: &serde_json::Map<String, Value>,
    content: &str,
) -> Result<String, ApiError> {
    let mut stamps: serde_json::Map<String, Value> =
        serde_json::from_str(own).map_err(ApiError::internal)?;
    for (asset, stamp) in populated {
        if !stamps.contains_key(asset) {
            stamps.insert(asset.clone(), stamp.clone());
        }
    }
    stamps.retain(|asset, _| content.contains(asset.as_str()));
    serde_json::to_string(&stamps).map_err(ApiError::internal)
}

impl PublishedView {
    pub(crate) fn new(row: Published, media: &MediaBase) -> Result<Self, ApiError> {
        Self::in_locale(row, media, None, None)
    }

    /// The row as a reader of `asked`'s language gets it, with `default` the language of an
    /// untagged entry.
    fn in_locale(
        row: Published,
        media: &MediaBase,
        default: Option<&str>,
        asked: Option<&Asked>,
    ) -> Result<Self, ApiError> {
        let locale = locale::effective(row.locale.as_deref(), default).map(str::to_owned);
        let localization = asked.map(|asked| Localization {
            requested: asked.tag.clone(),
            fallback: !locale
                .as_deref()
                .is_some_and(|served| served.eq_ignore_ascii_case(&asked.tag)),
            resolved: locale.clone(),
        });
        Ok(Self {
            id: row.entry_id,
            revision_id: row.revision_id,
            collection: row.collection,
            title: row.title,
            slug: row.slug,
            content: RawValue::from_string(row.content).map_err(ApiError::internal)?,
            content_hash: crate::entries::content_hash(&row.content_hash),
            published_in: row.valid_from,
            visibility: row.visibility,
            media: media.views(&row.media)?,
            locale,
            localization,
            requested_id: None,
            alternates: None,
        })
    }
}

/// An answer read from Postgres: its body, and whether it is empty (served, not cached).
struct Answer {
    body: String,
    empty: bool,
}

/// What a read from Postgres found.
enum Read {
    Found(Answer),
    Missing,
    Gone,
    /// Published, but not in the language asked for or its fallbacks: in these (D-182).
    NotInLocale(Vec<String>),
}

/// A failed read that other requests were waiting for. Its cause was logged once, by the request
/// that read it.
#[derive(Debug, thiserror::Error)]
#[error("reading published content failed (logged by the request that read it)")]
struct SharedReadFailed;

/// The project's environment, its latest generation and its locale policy for `requested`,
/// from Postgres in one statement; or 404.
async fn resolve(
    services: &Services,
    project: Uuid,
    environment: &str,
    requested: Option<&str>,
) -> Result<delivery::Environment, ApiError> {
    // A malformed tag never reaches the statement; `asked` refuses it with its own code.
    let requested = requested.filter(|tag| locale::is_tag(tag));
    delivery::environment_in(&services.pool, project, environment, requested)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)
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

/// A `?locale=` list's cursor (D-182): `d2:` adds the chain's digest, so a page of one
/// language never continues another's, and its position is the last row's translation group.
fn locale_cursor(
    path: &CollectionPath,
    project: Uuid,
    generation: i64,
    chain: u128,
    last: Uuid,
) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "d2:{project}:{}:{}:{generation}:{chain:032x}:{last}",
        path.environment, path.collection
    ))
}

fn after_locale_cursor(
    path: &CollectionPath,
    project: Uuid,
    chain: u128,
    text: &str,
) -> Result<(i64, Uuid), ApiError> {
    let prefix = format!("d2:{project}:{}:{}:", path.environment, path.collection);
    let chain = format!("{chain:032x}");
    URL_SAFE_NO_PAD
        .decode(text)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|decoded| {
            let rest = decoded.strip_prefix(&prefix)?.to_owned();
            let (generation, rest) = rest.split_once(':')?;
            let (digest, last) = rest.split_once(':')?;
            (digest == chain).then_some(())?;
            Some((generation.parse().ok()?, Uuid::try_parse(last).ok()?))
        })
        .ok_or(ApiError::InvalidHeader(
            "this cursor is not valid for this list in this language",
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

/// A query cursor's envelope `q1:<generation>:<v1 token>`: the release its first page
/// read at, then the query-bound position the domain sealed — what
/// `Query::after_cursor` refuses (`foreign_cursor` and the rest) stays this query's own
/// refusal, inside an envelope that isn't.
fn after_query_cursor(text: &str) -> Result<(i64, &str), ApiError> {
    let foreign = || ApiError::InvalidHeader("this cursor is not valid for this query");
    let Some(rest) = text.strip_prefix("q1:") else {
        return Err(foreign());
    };
    rest.split_once(':')
        .and_then(|(generation, inner)| generation.parse().ok().map(|g| (g, inner)))
        .ok_or_else(foreign)
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
    Fut: Future<Output = Result<Read, ApiError>>,
{
    let jitter = Uuid::now_v7().as_u64_pair().1;
    let (filled, served) = services
        .cache
        .get_or_fill(key, jitter, || async {
            match fill().await {
                Ok(Read::Found(answer)) => {
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
                Ok(Read::Missing) => Ok(Filled::Missing),
                Ok(Read::Gone) => Ok(Filled::Gone),
                Ok(Read::NotInLocale(available)) => Ok(Filled::NotInLocale(available)),
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
        Filled::Gone => return Err(ApiError::Gone),
        Filled::NotInLocale(available) => return Err(ApiError::NotInLocale { available }),
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

/// A member's answer: `fill` reads Postgres as their role, every time — the shared
/// generation-keyed cache is the anonymous audience's, so a gated row is never served to,
/// or stored for, another visitor (D-137). The answer still carries an `ETag` over the
/// member's own body, and `private` marks what no shared cache may keep.
async fn serve_member<F, Fut>(
    request: &HeaderMap,
    generation: i64,
    fill: F,
) -> Result<Response, ApiError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Read, ApiError>>,
{
    let answer = match fill().await? {
        Read::Found(answer) => answer,
        Read::Missing => return Err(ApiError::NotFound),
        Read::Gone => return Err(ApiError::Gone),
        Read::NotInLocale(available) => return Err(ApiError::NotInLocale { available }),
    };
    let tag = etag(&answer.body);
    let unchanged = request
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| none_match(value, &tag));
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        json(StatusCode::OK, answer.body, None, false)
    };
    let headers = response.headers_mut();
    headers.insert(GENERATION, HeaderValue::from(generation));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(MEMBER));
    headers.insert(
        header::VARY,
        HeaderValue::from_static("Authorization, Cookie"),
    );
    if let Ok(value) = HeaderValue::from_str(&tag) {
        headers.insert(header::ETAG, value);
    }
    headers.insert(CACHE, HeaderValue::from_static("bypass"));
    Ok(response)
}

/// `GET /content/v1/{project}/{environment}/{collection}`.
#[expect(
    clippy::too_many_lines,
    reason = "one request's own sequence: pin, ask, page, key, fill"
)]
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
    let environment = resolve(
        services,
        project,
        &path.environment,
        query.locale.as_deref(),
    )
    .await?;
    let media = MediaBase::new(&services.media_origin, project, &path.environment);
    let asked = asked(
        &environment,
        query.locale.as_deref(),
        query.fallback.as_deref(),
    )?;
    let digest = asked
        .as_ref()
        .map(|asked| locale::chain_digest(&asked.resolution.chain));
    // A cursor's page is pinned to the generation of the first page, like `?generation=`.
    let (generation, before, pinned) = match query.cursor.as_deref() {
        Some(text) => {
            let (generation, last) = match digest {
                Some(chain) => after_locale_cursor(&path, project, chain, text)?,
                None => after_cursor(&path, project, text)?,
            };
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
    // Who the read is for: a live site session makes it a member's, reading as its role
    // (D-137) — otherwise anonymous, which the generation-keyed cache is for.
    let audience = audience::reader(services, project, &request).await?;
    let scope = scope_of(
        project,
        &environment,
        generation,
        asked.as_ref(),
        colophon_domain::cache::media_digest(&services.media_origin),
    );
    let key = scope.list_key(&path.collection, limit, before.as_ref().map(Uuid::as_u128));
    let tags = scope.list_tags(&path.collection);
    let fill = || {
        let audience = &audience;
        let asked = asked.as_ref();
        let environment = &environment;
        async move {
            let mut rows = if generation == 0 {
                Vec::new()
            } else {
                let mut tx = delivery::begin(&services.pool, project, audience)
                    .await
                    .map_err(ApiError::from_db)?;
                let rows = match asked {
                    Some(asked) => {
                        tx.list_in_locale(
                            environment.id,
                            &path.collection,
                            generation,
                            &asked.resolution,
                            before,
                            limit + 1,
                        )
                        .await
                    }
                    None => {
                        tx.list(
                            environment.id,
                            &path.collection,
                            generation,
                            before,
                            limit + 1,
                        )
                        .await
                    }
                }
                .map_err(ApiError::from_db)?;
                tx.finish().await.map_err(ApiError::from_db)?;
                rows
            };
            let page = usize::try_from(limit).unwrap_or(usize::MAX);
            let more = rows.len() > page;
            rows.truncate(page);
            let next_cursor = more
                .then(|| {
                    rows.last().map(|row| match digest {
                        Some(chain) => {
                            locale_cursor(&path, project, generation, chain, row.group_key)
                        }
                        None => cursor(&path, project, generation, row.entry_id),
                    })
                })
                .flatten();
            let default = environment.default_locale.as_deref();
            let data = rows
                .into_iter()
                .map(|row| PublishedView::in_locale(row, &media, default, asked))
                .collect::<Result<Vec<_>, ApiError>>()?;
            let empty = data.is_empty();
            let body =
                serde_json::to_string(&Page { data, next_cursor }).map_err(ApiError::internal)?;
            Ok(Read::Found(Answer { body, empty }))
        }
    };
    let response = if matches!(audience, Audience::Member { .. }) {
        serve_member(&request, generation, fill).await?
    } else {
        serve(services, &request, &key, &tags, generation, pinned, fill).await?
    };
    Ok(with_locale(response, &environment, asked.as_ref()))
}

/// `GET /content/v1/{project}/{environment}/{collection}/{entry}`. With `?locale=`, the entry's
/// best published translation through the chain (D-182): `id` is the translation's,
/// `requested_id` the one asked for; nothing in the chain is 404 `not_in_locale`, naming the
/// languages that are published. Every answer lists the other published translations as
/// `alternates`.
#[expect(
    clippy::too_many_lines,
    reason = "one request's own sequence: pin, ask, key, resolve, read"
)]
pub(crate) async fn get(
    State(state): State<AppState>,
    Path(path): Path<EntryPath>,
    Query(query): Query<ReadQuery>,
    request: HeaderMap,
) -> Result<Response, ApiError> {
    let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
    let services = services(&state)?;
    let project = Uuid::try_parse(&path.project).map_err(|_| ApiError::NotFound)?;
    let environment = resolve(
        services,
        project,
        &path.environment,
        query.locale.as_deref(),
    )
    .await?;
    let media = MediaBase::new(&services.media_origin, project, &path.environment);
    let asked = asked(
        &environment,
        query.locale.as_deref(),
        query.fallback.as_deref(),
    )?;
    let generation = at(query.generation, environment.generation)?;
    if generation == 0 {
        return Err(ApiError::NotFound);
    }
    // Who the read is for (D-137), as in `list`.
    let audience = audience::reader(services, project, &request).await?;
    let scope = scope_of(
        project,
        &environment,
        generation,
        asked.as_ref(),
        colophon_domain::cache::media_digest(&services.media_origin),
    );
    let key = scope.entry_key(&path.collection, entry.as_u128());
    let tags = scope.entry_tags(&path.collection, entry.as_u128());
    let pinned = query.generation.is_some();
    let fill = || {
        let audience = &audience;
        let asked = asked.as_ref();
        let environment = &environment;
        async move {
            let default = environment.default_locale.as_deref();
            let mut tx = delivery::begin(&services.pool, project, audience)
                .await
                .map_err(ApiError::from_db)?;
            // The translation to answer with, and its published siblings — all read inside the
            // audience's own row-level security, so a member-only translation is neither the
            // answer nor an alternate for anyone who can't read it.
            let (chosen, siblings) = match asked {
                None => {
                    let found = tx
                        .get(environment.id, &path.collection, entry, generation)
                        .await
                        .map_err(ApiError::from_db)?;
                    let siblings = match &found {
                        Some(row) => tx
                            .siblings(environment.id, &path.collection, row.group_key, generation)
                            .await
                            .map_err(ApiError::from_db)?,
                        None => Vec::new(),
                    };
                    (found, siblings)
                }
                Some(asked) => {
                    let group = tx
                        .group_of(environment.id, &path.collection, entry)
                        .await
                        .map_err(ApiError::from_db)?;
                    let siblings = match group {
                        Some(group) => tx
                            .siblings(environment.id, &path.collection, group, generation)
                            .await
                            .map_err(ApiError::from_db)?,
                        None => Vec::new(),
                    };
                    let stored: Vec<Option<&str>> = siblings
                        .iter()
                        .map(|sibling| sibling.locale.as_deref())
                        .collect();
                    match locale::resolve(&asked.resolution.chain, default, &stored) {
                        Some((index, _)) => {
                            let found = tx
                                .get(
                                    environment.id,
                                    &path.collection,
                                    siblings[index].entry_id,
                                    generation,
                                )
                                .await
                                .map_err(ApiError::from_db)?;
                            (found, siblings)
                        }
                        None if !siblings.is_empty() => {
                            tx.finish().await.map_err(ApiError::from_db)?;
                            let mut available: Vec<String> = Vec::new();
                            for sibling in &siblings {
                                if let Some(tag) =
                                    locale::effective(sibling.locale.as_deref(), default)
                                    && !available.iter().any(|seen| seen == tag)
                                {
                                    available.push(tag.to_owned());
                                }
                            }
                            available.sort();
                            return Ok(Read::NotInLocale(available));
                        }
                        None => (None, siblings),
                    }
                }
            };
            let gone = match chosen {
                Some(_) => false,
                None => tx
                    .gone(environment.id, &path.collection, entry, generation)
                    .await
                    .map_err(ApiError::from_db)?,
            };
            tx.finish().await.map_err(ApiError::from_db)?;
            let Some(row) = chosen else {
                return Ok(if gone { Read::Gone } else { Read::Missing });
            };
            let answered = row.entry_id;
            let mut view = PublishedView::in_locale(row, &media, default, asked)?;
            if asked.is_some() {
                view.requested_id = Some(entry);
            }
            view.alternates = Some(
                siblings
                    .into_iter()
                    .filter(|sibling| sibling.entry_id != answered)
                    .map(|sibling| Alternate {
                        id: sibling.entry_id,
                        locale: locale::effective(sibling.locale.as_deref(), default)
                            .map(str::to_owned),
                        slug: sibling.slug,
                    })
                    .collect(),
            );
            let body = serde_json::to_string(&Data { data: view }).map_err(ApiError::internal)?;
            Ok(Read::Found(Answer { body, empty: false }))
        }
    };
    let response = if matches!(audience, Audience::Member { .. }) {
        serve_member(&request, generation, fill).await?
    } else {
        serve(services, &request, &key, &tags, generation, pinned, fill).await?
    };
    Ok(with_locale(response, &environment, asked.as_ref()))
}

/// The page a `POST …/query` answers with: the published rows, and `applied`, the
/// query's own name — the canonical document and the hash its cursors and cache key
/// bind to (D-103).
#[derive(Serialize)]
struct QueryPage {
    data: Vec<PublishedView>,
    next_cursor: Option<String>,
    applied: AppliedQuery,
}

/// `POST /content/v1/{project}/{environment}/{collection}/query` — the query language
/// over the published projection (DX.4, D-103): the same document the draft route
/// takes, validated against the collection's content type, answered as
/// `colophon_delivery` from `publications` pinned at the generation — never drafts.
/// `depth` populates references (DX.5, D-107): each level's targets resolve under the
/// same pin, inside the fill's transaction.
///
/// A cursor is `q1:<generation>:<v1 token>`: the generation its first page read at —
/// so paging a query, like paging a list, never mixes two releases — and the position
/// the domain sealed to this query. `?generation=` names the release without one.
///
/// The definition lives outside the delivery role's reach — it must, or the role could
/// read management tables — so the read that validates the document runs as the API
/// role in the project's context, before the cache key exists: the query's own hash is
/// the key's normalized query (spec §15), so validation must precede it.
#[expect(
    clippy::too_many_lines,
    reason = "one request's own sequence: pin, define, validate, key, fill"
)]
pub(crate) async fn query(
    State(state): State<AppState>,
    Path(path): Path<CollectionPath>,
    Query(parameters): Query<QueryParams>,
    request: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(document) = body?;
    let services = services(&state)?;
    let project = Uuid::try_parse(&path.project).map_err(|_| ApiError::NotFound)?;
    let environment = resolve(
        services,
        project,
        &path.environment,
        parameters.locale.as_deref(),
    )
    .await?;
    let media = MediaBase::new(&services.media_origin, project, &path.environment);
    let asked = asked(
        &environment,
        parameters.locale.as_deref(),
        parameters.fallback.as_deref(),
    )?;
    // A cursor's page is pinned to the generation of its first page, like `?generation=`.
    let (generation, inner) = match parameters.cursor.as_deref() {
        Some(text) => {
            let (generation, inner) = after_query_cursor(text)?;
            (at(Some(generation), environment.generation)?, Some(inner))
        }
        None => (at(parameters.generation, environment.generation)?, None),
    };
    let pinned = parameters.generation.is_some() || parameters.cursor.is_some();
    // The collection's current definition — as the API role, since `colophon_delivery`
    // may not read it. An unknown collection is 404, like an empty list's.
    let definition = content_types::current_definition(
        &services.pool,
        project,
        environment.id,
        &path.collection,
    )
    .await
    .map_err(ApiError::from_db)?
    .ok_or(ApiError::NotFound)?;
    let content_type = services.content_types.parse(&definition)?;
    let language = Document::validate(&content_type, &document).map_err(ApiError::Invalid)?;
    let after = inner
        .map(|token| language.after_cursor(token))
        .transpose()
        .map_err(ApiError::Invalid)?;
    // A position's literals must be ones their fields could hold — checked before the
    // cache's shared fill, where a refusal would come back as an internal error.
    if let Some(position) = &after {
        db_query::check_position(&content_type, &language, position).map_err(
            |error| match error {
                db_query::Error::Cursor(refusal) => ApiError::Invalid(refusal),
                db_query::Error::Db(error) => ApiError::from_db(error),
            },
        )?;
    }
    // The key's boundary is the canonical position re-sealed into the envelope — not
    // the request's own spelling — so one page has one key however `?cursor=` came.
    let cursor = after
        .as_ref()
        .map(|position| language.cursor(position))
        .transpose()
        .map_err(ApiError::Invalid)?
        .map(|inner| format!("q1:{generation}:{inner}"));
    // Who the read is for (D-137), as in `list`.
    let audience = audience::reader(services, project, &request).await?;
    let scope = scope_of(
        project,
        &environment,
        generation,
        asked.as_ref(),
        colophon_domain::cache::media_digest(&services.media_origin),
    );
    let key = scope.query_key(&path.collection, &language.query_hash(), cursor.as_deref());
    let tags = scope.list_tags(&path.collection);
    let fill = || {
        let audience = &audience;
        let asked = asked.as_ref();
        let environment = &environment;
        async move {
            let depth = language.depth();
            let keep = language.fields();
            let mut resolved = crate::populate::Resolved::default();
            let mut populated = serde_json::Map::new();
            let found = if generation == 0 {
                db_query::PublishedPage {
                    rows: Vec::new(),
                    next_cursor: None,
                }
            } else {
                let mut tx = delivery::begin(&services.pool, project, audience)
                    .await
                    .map_err(ApiError::from_db)?;
                let mut found = db_query::published_page_in(
                    &mut tx,
                    environment.id,
                    &path.collection,
                    &content_type,
                    &language,
                    after.as_ref(),
                    generation,
                    asked.map(|asked| &asked.resolution),
                )
                .await
                .map_err(|error| match error {
                    db_query::Error::Db(error) => ApiError::from_db(error),
                    // `check_position` refused a bad cursor above; reaching this is a bug.
                    db_query::Error::Cursor(refusal) => ApiError::internal(refusal),
                })?;
                // Narrow each row to `fields`, then collect the ids `depth` expands and
                // resolve them — inside the pin's transaction, so every level reads the
                // same snapshot, pinned at the page's own generation.
                let mut wanted = BTreeSet::new();
                for row in &mut found.rows {
                    if let Some(keep) = keep {
                        row.content = narrow(&row.content, keep)?;
                    }
                    if depth > 0 {
                        crate::populate::collect_wanted(&content_type, &row.content, &mut wanted)?;
                    }
                }
                if !wanted.is_empty() {
                    (resolved, populated) = crate::populate::resolve_publications(
                        services,
                        &mut tx,
                        project,
                        environment.id,
                        &wanted,
                        generation,
                        depth,
                    )
                    .await?;
                }
                tx.finish().await.map_err(ApiError::from_db)?;
                found
            };
            let default = environment.default_locale.as_deref();
            let data = found
                .rows
                .into_iter()
                .map(|mut row| {
                    row.content =
                        crate::populate::expand_document(&content_type, &row.content, &resolved)?;
                    row.media = media_named(&row.media, &populated, &row.content)?;
                    PublishedView::in_locale(row, &media, default, asked)
                })
                .collect::<Result<Vec<_>, ApiError>>()?;
            let empty = data.is_empty();
            let body = serde_json::to_string(&QueryPage {
                data,
                next_cursor: found
                    .next_cursor
                    .map(|inner| format!("q1:{generation}:{inner}")),
                applied: AppliedQuery::of(&language)?,
            })
            .map_err(ApiError::internal)?;
            Ok(Read::Found(Answer { body, empty }))
        }
    };
    let response = if matches!(audience, Audience::Member { .. }) {
        serve_member(&request, generation, fill).await?
    } else {
        serve(services, &request, &key, &tags, generation, pinned, fill).await?
    };
    Ok(with_locale(response, &environment, asked.as_ref()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_query_rows_media_is_what_its_answered_content_names() {
        let a = "0199f0a2-6c1e-7b3a-9d42-00000000000a";
        let b = "0199f0a2-6c1e-7b3a-9d42-00000000000b";
        let c = "0199f0a2-6c1e-7b3a-9d42-00000000000c";
        let own = serde_json::json!({ a: {"sha": "x"}, b: {"sha": "y"} }).to_string();
        let mut populated = serde_json::Map::new();
        populated.insert(c.to_owned(), serde_json::json!({"sha": "z"}));
        populated.insert(a.to_owned(), serde_json::json!({"sha": "other"}));
        // `fields` kept only the field naming `a`; the populated author's avatar names `c`.
        let content =
            format!(r#"{{"cover":"{a}","author":{{"id":"e","content":{{"avatar":"{c}"}}}}}}"#);
        let named: Value =
            serde_json::from_str(&super::media_named(&own, &populated, &content).expect("merges"))
                .expect("json");
        assert_eq!(
            named,
            serde_json::json!({ a: {"sha": "x"}, c: {"sha": "z"} }),
            "the row's own stamp wins; what the content no longer names goes"
        );
    }

    #[test]
    fn media_views_name_the_recorded_version_and_follow_the_current_one() {
        let base = super::MediaBase::new("https://media.example", uuid::Uuid::nil(), "production");
        let asset = "0199f0a2-6c1e-7b3a-9d42-5e8f1a2b3c4d";
        let sha = "ab".repeat(32);
        let stored = serde_json::json!({
            asset: {"sha": sha, "type": "image/png", "width": 2400, "height": 1600,
                    "renditions": ["w320", "w1280", "../x", "W9"]},
            "not-a-uuid": {"sha": sha, "type": "image/png", "width": 1, "height": 1, "renditions": []},
            "0199f0a2-6c1e-7b3a-9d42-5e8f1a2b3c4e": {"sha": "../../x", "type": "image/png",
                    "width": 1, "height": 1, "renditions": []},
            "0199f0a2-6c1e-7b3a-9d42-5e8f1a2b3c4f": {"sha": sha, "type": "image/png", "width": null}
        })
        .to_string();
        let views = base.views(&stored).expect("parses");
        let view = serde_json::to_value(&views).expect("serializes");
        let prefix = format!(
            "https://media.example/media/v1/{}/production/{asset}",
            uuid::Uuid::nil()
        );
        assert_eq!(
            view,
            serde_json::json!({asset: {
                "url": format!("{prefix}/{sha}"),
                "current_url": prefix,
                "type": "image/png",
                "width": 2400,
                "height": 1600,
                "renditions": {
                    "w1280": format!("{prefix}/{sha}/w1280"),
                    "w320": format!("{prefix}/{sha}/w320"),
                }
            }}),
            "only what could be a URL segment the server wrote is linked"
        );
        assert!(base.views("{}").expect("empty").is_empty());
    }

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
    fn a_locale_cursor_keeps_its_list_its_generation_and_its_language() {
        let here = CollectionPath {
            project: "p".to_owned(),
            environment: "production".to_owned(),
            collection: "articles".to_owned(),
        };
        let (project, group) = (Uuid::now_v7(), Uuid::now_v7());
        let token = locale_cursor(&here, project, 7, 0xfe, group);
        assert_eq!(
            after_locale_cursor(&here, project, 0xfe, &token).ok(),
            Some((7, group))
        );
        assert!(
            after_locale_cursor(&here, project, 0xff, &token).is_err(),
            "another language's page"
        );
        assert!(
            after_cursor(&here, project, &token).is_err(),
            "a page of every language"
        );
        let every = cursor(&here, project, 7, group);
        assert!(after_locale_cursor(&here, project, 0xfe, &every).is_err());
    }

    #[test]
    fn a_query_cursor_names_its_generation_and_keeps_the_domains_token() {
        assert_eq!(
            after_query_cursor("q1:7:v1x2hhc2hlYQ==").ok(),
            Some((7, "v1x2hhc2hlYQ=="))
        );
        for bad in [
            "v1:abc:W10=",      // the domain's own token, without the envelope
            "d1:p:e:c:7:entry", // a list's cursor
            "q1:x:token",       // a generation that isn't a number
            "q1:7",             // no token at all
            "",
        ] {
            assert!(after_query_cursor(bad).is_err(), "{bad}");
        }
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
