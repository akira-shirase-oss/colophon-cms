//! Draft preview on the editor's own site (D-171, D-172; docs/modules/03-auth.md "Preview
//! grants", 06-delivery.md "Preview reads").
//!
//! **Management, signed in as a person** (`{P}`; project API tokens are refused — agents
//! don't show people drafts in v1):
//! - `GET | PUT {P}/preview-target`: the site address a preview opens (`destinations.manage`
//!   to change it; any member may read it, so the editor's Preview button knows);
//! - `POST {P}/previews {entry | changeset, ttl_seconds?}`: a grant and its one-time code;
//! - `GET {P}/previews?entry=`: the caller's live grants;
//! - `POST {P}/previews/{id}/renew`, `POST {P}/previews/{id}/code`, `DELETE {P}/previews/{id}`.
//!
//! **The read surface, with no session** — the customer's site holds a bearer token:
//! - `POST /preview/v1/{project}/{environment}/exchange {code}`: the code for a token;
//! - `GET /preview/v1/{project}/{environment}/session`: the grant's scope and end;
//! - `GET …/{collection}`, `GET …/{collection}/{entry}`, `POST …/{collection}/query`: the
//!   same shapes as `/content/v1`, serialized by delivery's own [`PublishedView`], with the
//!   scope's drafts laid over the latest release and a `preview` member on each row.
//!
//! **Leaking a draft is the worst failure here.** Every preview answer, error or not, is
//! `private, no-store`, `noindex`, `no-referrer`, `Vary: Authorization`; it never carries a
//! `Cache-Tag` or an `ETag` and never goes near the delivery cache (`crate::cache`). Every read
//! re-checks the grant (expired, revoked), opens its transaction *as the issuer* — so a removed
//! member or a narrowed role ends the preview on the next request — and narrows each row by
//! the issuer's field policy. A per-server semaphore keeps preview reads from starving
//! delivery; admission counts `/preview/` per address and per bearer like the management API.

use axum::{
    Json,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use colophon_auth::tokens;
use colophon_db::{
    audit,
    previews::{self, Grant, OverlayRow, Scope},
    query::{self as db_query, Overlay},
    scope::{Isolation, ProjectTransaction, begin_in_project_isolated},
};
use colophon_domain::{
    policy::Action,
    preview::{Fill, Template, ttl},
    query::Query as Document,
    schema::ContentType,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    delivery::{GENERATION, MediaBase, PublishedView, media_named},
    entries::{AppliedQuery, content_type, field_ids, json, may_read, narrow, open, rfc3339},
    members::require,
    session::{SignedIn, services},
    shape::{Data, Page},
    visibility::{self, Access},
};

/// Names the scope a preview answer was read in: `entry` or `changeset`.
pub(crate) const PREVIEW: HeaderName = HeaderName::from_static("colophon-preview");
const ROBOTS: HeaderName = HeaderName::from_static("x-robots-tag");

/// No cache, shared or private, may keep a preview answer.
const NO_STORE: &str = "private, no-store";
const NOINDEX: &str = "noindex, nofollow, noarchive";

/// Preview reads one server serves at once; the next is refused 503 with `Retry-After`, so a
/// burst of previews can never take the database connections public delivery needs.
pub(crate) const MAX_READS: usize = 32;
static READS: Semaphore = Semaphore::const_new(MAX_READS);

/// The most bytes an exchange reads: a code is 47 characters, in a small JSON object.
pub(crate) const EXCHANGE_BYTES: usize = 1024;

/// The one-time code's and the token's prefixes, so a leaked one is recognizable.
const CODE_PREFIX: &str = "cpc_";
const TOKEN_PREFIX: &str = "cpv_";

fn secret(prefix: &str) -> Result<String, ApiError> {
    let random = tokens::random_token()
        .map_err(|_| ApiError::internal(std::io::Error::other("no secure random source")))?;
    Ok(format!("{prefix}{random}"))
}

const fn refused(status: StatusCode, reason: &'static str, message: &'static str) -> ApiError {
    ApiError::Preview {
        status,
        reason,
        message,
        generation: None,
    }
}

fn person(caller: &SignedIn) -> Result<(), ApiError> {
    if caller.project.is_some() {
        return Err(ApiError::forbidden(
            "api_token_not_allowed",
            "previews are opened by people; an API token can't issue one",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Management

#[derive(Deserialize)]
pub(crate) struct EnvPath {
    project: String,
    environment: String,
}

#[derive(Deserialize)]
pub(crate) struct GrantPath {
    project: String,
    environment: String,
    preview: String,
}

/// The environment's preview address.
#[derive(Serialize)]
pub(crate) struct TargetView {
    url_template: Option<String>,
    /// The origin every preview address shares: what the Studio posts messages to.
    origin: Option<String>,
    updated_at: Option<String>,
}

fn target_view(
    target: Option<previews::Target>,
    allow_private: bool,
) -> Result<TargetView, ApiError> {
    Ok(match target {
        None => TargetView {
            url_template: None,
            origin: None,
            updated_at: None,
        },
        Some(target) => TargetView {
            origin: Template::parse(&target.url_template, allow_private)
                .ok()
                .map(|parsed| parsed.origin().to_owned()),
            updated_at: Some(rfc3339(target.updated_at)?),
            url_template: Some(target.url_template),
        },
    })
}

/// `GET {P}/preview-target`.
pub(crate) async fn get_target(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<EnvPath>,
) -> Result<Json<Data<TargetView>>, ApiError> {
    person(&caller)?;
    let allow_private = services(&state)?.allow_private_destinations;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let target = previews::target(&mut tx).await.map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(Json(Data {
        data: target_view(target, allow_private)?,
    }))
}

/// `{"url_template": "https://…?code={code}"}`, or `null` to clear it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetTarget {
    url_template: Option<String>,
}

/// `PUT {P}/preview-target`: owners and developers (`destinations.manage`), since the address
/// decides where one-time codes are sent.
pub(crate) async fn set_target(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<EnvPath>,
    body: Result<Json<SetTarget>, JsonRejection>,
) -> Result<Json<Data<TargetView>>, ApiError> {
    person(&caller)?;
    let Json(asked) = body?;
    let allow_private = services(&state)?.allow_private_destinations;
    let template = asked
        .url_template
        .as_deref()
        .map(|text| Template::parse(text, allow_private))
        .transpose()
        .map_err(|refusal| ApiError::InvalidField {
            reason: refusal.reason,
            pointer: "/url_template",
            message: refusal.message.to_owned(),
        })?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    require(
        &tx.policy,
        Action::DestinationsManage,
        None,
        "only owners and developers can change where previews open",
    )?;
    let stored = previews::set_target(&mut tx, template.as_ref().map(Template::as_str))
        .await
        .map_err(ApiError::from_db)?;
    let detail = serde_json::json!({ "url_template": template.as_ref().map(Template::as_str) });
    audit::record_in(
        &mut tx.transaction,
        tx.project,
        tx.identity,
        "preview.target_updated",
        &tx.environment.to_string(),
        &detail.to_string(),
    )
    .await
    .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(Json(Data {
        data: target_view(stored, allow_private)?,
    }))
}

/// What a grant reads, as answers name it.
#[derive(Serialize, Clone)]
pub(crate) struct ScopeView {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    entry: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    changeset: Option<Uuid>,
}

impl From<&Scope> for ScopeView {
    fn from(scope: &Scope) -> Self {
        match scope {
            Scope::Entry(entry) => Self {
                kind: "entry",
                entry: Some(*entry),
                changeset: None,
            },
            Scope::Changeset { id, .. } => Self {
                kind: "changeset",
                entry: None,
                changeset: Some(*id),
            },
        }
    }
}

/// `{"entry": "<id>"}` or `{"changeset": "<id>"}`, with `ttl_seconds` optional.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewPreview {
    entry: Option<Uuid>,
    changeset: Option<Uuid>,
    ttl_seconds: Option<i64>,
}

/// A grant just issued: the code travels once, here, and in the address it fills.
#[derive(Serialize)]
pub(crate) struct IssuedView {
    id: Uuid,
    code: String,
    code_expires_at: String,
    expires_at: String,
    /// The site address with the code in it.
    url: String,
    /// The address's origin: the exact `targetOrigin` for the Studio's messages.
    origin: String,
    scope: ScopeView,
}

/// The issuer's address, filled for `scope` and `code`.
async fn address(
    tx: &mut ProjectTransaction,
    template: &Template,
    scope: &Scope,
    code: &str,
) -> Result<String, ApiError> {
    let (collection, id, slug, locale) = match scope {
        Scope::Entry(entry) => {
            let facts = previews::entry_facts(tx, *entry)
                .await
                .map_err(ApiError::from_db)?
                .ok_or(ApiError::NotFound)?;
            (
                facts.collection,
                entry.to_string(),
                facts.slug.unwrap_or_default(),
                facts.locale.unwrap_or_default(),
            )
        }
        Scope::Changeset { id, .. } => {
            let state = previews::changeset_state(tx, *id)
                .await
                .map_err(ApiError::from_db)?
                .ok_or(ApiError::NotFound)?;
            match state.first {
                Some((entry, collection)) => {
                    let facts = previews::entry_facts(tx, entry)
                        .await
                        .map_err(ApiError::from_db)?;
                    (
                        collection,
                        entry.to_string(),
                        facts
                            .as_ref()
                            .and_then(|f| f.slug.clone())
                            .unwrap_or_default(),
                        facts.and_then(|f| f.locale).unwrap_or_default(),
                    )
                }
                None => (String::new(), String::new(), String::new(), String::new()),
            }
        }
    };
    Ok(template.fill(&Fill {
        code,
        collection: &collection,
        id: &id,
        slug: &slug,
        locale: &locale,
    }))
}

/// `POST {P}/previews`.
#[expect(
    clippy::too_many_lines,
    reason = "one request's own sequence: check, scope, bound, mint, store, fill, record"
)]
pub(crate) async fn issue(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<EnvPath>,
    body: Result<Json<NewPreview>, JsonRejection>,
) -> Result<Response, ApiError> {
    person(&caller)?;
    let Json(asked) = body?;
    let ttl_seconds = ttl(asked.ttl_seconds).ok_or(ApiError::InvalidField {
        reason: "invalid_ttl",
        pointer: "/ttl_seconds",
        message: "a preview lasts 60 to 3600 seconds".to_owned(),
    })?;
    let allow_private = services(&state)?.allow_private_destinations;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let template = previews::target(&mut tx)
        .await
        .map_err(ApiError::from_db)?
        .and_then(|target| Template::parse(&target.url_template, allow_private).ok())
        .ok_or(refused(
            StatusCode::UNPROCESSABLE_ENTITY,
            "preview_not_configured",
            "this project has no preview address yet; set one in Settings → Website connections",
        ))?;
    let scope = match (asked.entry, asked.changeset) {
        (Some(entry), None) => {
            let facts = previews::entry_facts(&mut tx, entry)
                .await
                .map_err(ApiError::from_db)?
                .ok_or(ApiError::NotFound)?;
            // An entry the issuer can't read doesn't exist to them.
            Access::of(&tx.policy, tx.identity, &facts.collection, facts.created_by)
                .read
                .ok_or(ApiError::NotFound)?;
            Scope::Entry(entry)
        }
        (None, Some(changeset)) => {
            let found = previews::changeset_state(&mut tx, changeset)
                .await
                .map_err(ApiError::from_db)?
                .ok_or(ApiError::NotFound)?;
            if let Some((_, collection)) = &found.first {
                may_read(&tx, collection)?;
            }
            if found.released_in.is_some() {
                return Err(ApiError::conflict(
                    "already_published",
                    "this publish request is already live; open the site itself",
                ));
            }
            Scope::Changeset {
                id: changeset,
                hash: found.hash,
            }
        }
        _ => {
            return Err(ApiError::InvalidField {
                reason: "invalid_scope",
                pointer: "",
                message: "a preview names one entry or one publish request".to_owned(),
            });
        }
    };
    if previews::live(&mut tx).await.map_err(ApiError::from_db)? >= previews::MAX_LIVE {
        return Err(refused(
            StatusCode::TOO_MANY_REQUESTS,
            "preview_limit",
            "this project has 1,000 previews open; end some or wait for them to expire",
        ));
    }
    let code = secret(CODE_PREFIX)?;
    let id = Uuid::now_v7();
    let issued = previews::issue(
        &mut tx,
        &previews::NewGrant {
            id,
            scope: &scope,
            ttl_seconds,
            code_hash: &tokens::hash(&code),
        },
    )
    .await
    .map_err(ApiError::from_db)?;
    let url = address(&mut tx, &template, &scope, &code).await?;
    let view = ScopeView::from(&scope);
    let detail = serde_json::json!({ "scope": &view, "ttl_seconds": ttl_seconds });
    audit::record_in(
        &mut tx.transaction,
        tx.project,
        tx.identity,
        "preview.issued",
        &id.to_string(),
        &detail.to_string(),
    )
    .await
    .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let body = serde_json::to_string(&Data {
        data: IssuedView {
            id,
            code,
            code_expires_at: rfc3339(issued.code_expires_at)?,
            expires_at: rfc3339(issued.expires_at)?,
            url,
            origin: template.origin().to_owned(),
            scope: view,
        },
    })
    .map_err(ApiError::internal)?;
    let mut response = json(StatusCode::CREATED, body, None, false);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

#[derive(Deserialize)]
pub(crate) struct MineQuery {
    entry: Option<Uuid>,
}

/// One of the caller's live grants.
#[derive(Serialize)]
pub(crate) struct LiveView {
    id: Uuid,
    scope: ScopeView,
    created_at: String,
    expires_at: String,
    exchanged: bool,
}

/// `GET {P}/previews?entry=`: the caller's own live grants, newest first.
pub(crate) async fn mine(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<EnvPath>,
    Query(query): Query<MineQuery>,
) -> Result<Json<Page<LiveView>>, ApiError> {
    person(&caller)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let live = previews::mine(&mut tx, query.entry)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let data = live
        .into_iter()
        .map(|grant| {
            Ok(LiveView {
                id: grant.id,
                scope: ScopeView::from(&grant.scope),
                created_at: rfc3339(grant.created_at)?,
                expires_at: rfc3339(grant.expires_at)?,
                exchanged: grant.exchanged,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    Ok(Json(Page {
        data,
        next_cursor: None,
    }))
}

fn grant_id(text: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(text).map_err(|_| ApiError::NotFound)
}

/// A renewed grant's new end.
#[derive(Serialize)]
pub(crate) struct RenewedView {
    id: Uuid,
    expires_at: String,
}

/// `POST {P}/previews/{id}/renew`: the issuer's own, while it is live.
pub(crate) async fn renew(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<GrantPath>,
) -> Result<Json<Data<RenewedView>>, ApiError> {
    person(&caller)?;
    let id = grant_id(&path.preview)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let expires_at = previews::renew(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(Json(Data {
        data: RenewedView {
            id,
            expires_at: rfc3339(expires_at)?,
        },
    }))
}

/// A fresh code for a live grant.
#[derive(Serialize)]
pub(crate) struct CodeView {
    code: String,
    code_expires_at: String,
    url: String,
}

/// `POST {P}/previews/{id}/code`: a fresh one-time code for the issuer's own live grant — the
/// Studio's Reload goes through the site's handler again without a second grant. Exchanging it
/// replaces the grant's token.
pub(crate) async fn new_code(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<GrantPath>,
) -> Result<Response, ApiError> {
    person(&caller)?;
    let id = grant_id(&path.preview)?;
    let allow_private = services(&state)?.allow_private_destinations;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let template = previews::target(&mut tx)
        .await
        .map_err(ApiError::from_db)?
        .and_then(|target| Template::parse(&target.url_template, allow_private).ok())
        .ok_or(refused(
            StatusCode::UNPROCESSABLE_ENTITY,
            "preview_not_configured",
            "this project has no preview address yet; set one in Settings → Website connections",
        ))?;
    let code = secret(CODE_PREFIX)?;
    let (code_expires_at, scope) = previews::new_code(&mut tx, id, &tokens::hash(&code))
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let url = address(&mut tx, &template, &scope, &code).await?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let body = serde_json::to_string(&Data {
        data: CodeView {
            code,
            code_expires_at: rfc3339(code_expires_at)?,
            url,
        },
    })
    .map_err(ApiError::internal)?;
    let mut response = json(StatusCode::CREATED, body, None, false);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

/// `DELETE {P}/previews/{id}`: ends the issuer's own grant now.
pub(crate) async fn revoke(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<GrantPath>,
) -> Result<StatusCode, ApiError> {
    person(&caller)?;
    let id = grant_id(&path.preview)?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    if !previews::revoke(&mut tx, id)
        .await
        .map_err(ApiError::from_db)?
    {
        return Err(ApiError::NotFound);
    }
    audit::record_in(
        &mut tx.transaction,
        tx.project,
        tx.identity,
        "preview.revoked",
        &id.to_string(),
        "{}",
    )
    .await
    .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// The read surface

/// Adds what every preview answer carries — success or refusal alike, so no cache keeps a
/// refusal either — and the scope and generation when known.
fn seal(mut response: Response, kind: Option<&'static str>, generation: Option<i64>) -> Response {
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(NO_STORE));
    headers.insert(ROBOTS, HeaderValue::from_static(NOINDEX));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(header::VARY, HeaderValue::from_static("Authorization"));
    headers.remove(header::ETAG);
    headers.remove(crate::delivery::CACHE_TAG);
    if let Some(kind) = kind {
        headers.insert(PREVIEW, HeaderValue::from_static(kind));
    }
    if let Some(generation) = generation {
        headers.insert(GENERATION, HeaderValue::from(generation));
    }
    response
}

/// A preview answer, sealed whichever way it went.
fn answer(result: Result<(Response, &'static str, i64), ApiError>) -> Response {
    match result {
        Ok((response, kind, generation)) => seal(response, Some(kind), Some(generation)),
        Err(error) => seal(error.into_response(), None, None),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExchangeBody {
    code: String,
}

/// The token a site holds, as the exchange hands it over.
#[derive(Serialize)]
pub(crate) struct TokenView {
    token: String,
    expires_at: String,
    scope: ScopeView,
}

/// `POST /preview/v1/{project}/{environment}/exchange`: the one-time code for a bearer token.
/// Unknown, spent and expired codes all answer the same 404.
pub(crate) async fn exchange(
    State(state): State<AppState>,
    Path(path): Path<EnvPath>,
    body: Result<Json<ExchangeBody>, JsonRejection>,
) -> Response {
    let result = async {
        let Json(asked) = body?;
        let services = services(&state)?;
        let not_found = || {
            refused(
                StatusCode::NOT_FOUND,
                "preview_not_found",
                "this preview link was already used or has expired; open the preview again from the Studio",
            )
        };
        let project = Uuid::try_parse(&path.project).map_err(|_| not_found())?;
        if !asked.code.starts_with(CODE_PREFIX) || asked.code.len() > 128 {
            return Err(not_found());
        }
        let token = secret(TOKEN_PREFIX)?;
        let exchanged = previews::exchange(
            &services.pool,
            project,
            &path.environment,
            &tokens::hash(&asked.code),
            &tokens::hash(&token),
        )
        .await
        .map_err(ApiError::from_db)?
        .ok_or_else(not_found)?;
        let body = serde_json::to_string(&Data {
            data: TokenView {
                token,
                expires_at: rfc3339(exchanged.expires_at)?,
                scope: ScopeView::from(&exchanged.scope),
            },
        })
        .map_err(ApiError::internal)?;
        Ok(json(StatusCode::OK, body, None, false))
    }
    .await;
    match result {
        Ok(response) => seal(response, None, None),
        Err(error) => seal(error.into_response(), None, None),
    }
}

/// A preview read's context: the issuer's transaction, the grant, and the overlay it reads.
struct Reading {
    tx: ProjectTransaction,
    grant: Grant,
    generation: i64,
    entries: Vec<Uuid>,
    revisions: Vec<Option<Uuid>>,
}

impl Reading {
    /// What a scope row says it is.
    const fn source(&self) -> &'static str {
        match self.grant.scope {
            Scope::Entry(_) => "draft",
            Scope::Changeset { .. } => "changeset",
        }
    }

    const fn kind(&self) -> &'static str {
        self.grant.scope.kind()
    }
}

/// The bearer token, if the request carries one that could be a preview token.
fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?
        .trim();
    (token.starts_with(TOKEN_PREFIX) && token.len() <= 128).then_some(token)
}

/// Checks the token and opens the read as its issuer: every request, every time.
async fn reading(
    state: &AppState,
    project: &str,
    environment: &str,
    headers: &HeaderMap,
) -> Result<Reading, ApiError> {
    let services = services(state)?;
    let token = bearer(headers).ok_or(refused(
        StatusCode::UNAUTHORIZED,
        "preview_invalid",
        "this request carries no preview token",
    ))?;
    let project = Uuid::try_parse(project).map_err(|_| ApiError::NotFound)?;
    let grant = previews::by_token(&services.pool, project, &tokens::hash(token))
        .await
        .map_err(ApiError::from_db)?
        .ok_or(refused(
            StatusCode::UNAUTHORIZED,
            "preview_invalid",
            "this preview token isn't one this project issued",
        ))?;
    if grant.revoked_at.is_some() {
        return Err(refused(
            StatusCode::UNAUTHORIZED,
            "preview_revoked",
            "this preview was ended from the Studio",
        ));
    }
    if grant.expired {
        return Err(refused(
            StatusCode::UNAUTHORIZED,
            "preview_expired",
            "this preview has expired; open it again from the Studio",
        ));
    }
    // Scope comes from the grant, never the URL: another environment is not found.
    if grant.environment != environment {
        return Err(ApiError::NotFound);
    }
    // As the issuer, in one snapshot: their membership and role, read now.
    let mut tx = begin_in_project_isolated(
        &services.pool,
        grant.issuer,
        project,
        environment,
        Isolation::Snapshot,
    )
    .await
    .map_err(ApiError::from_db)?
    .ok_or(refused(
        StatusCode::UNAUTHORIZED,
        "preview_revoked",
        "the person who opened this preview can no longer read this project",
    ))?;
    let generation = previews::generation(&mut tx)
        .await
        .map_err(ApiError::from_db)?;
    if let Scope::Changeset { id, hash } = &grant.scope {
        let found = previews::changeset_state(&mut tx, *id)
            .await
            .map_err(ApiError::from_db)?
            .ok_or(ApiError::NotFound)?;
        if let Some(released) = found.released_in {
            return Err(ApiError::Preview {
                status: StatusCode::GONE,
                reason: "preview_published",
                message: "this publish request is live now; read the site itself",
                generation: Some(released),
            });
        }
        if found.hash != *hash {
            return Err(refused(
                StatusCode::CONFLICT,
                "preview_stale",
                "this publish request changed after the preview was opened; preview it again",
            ));
        }
    }
    let (entries, revisions) = previews::scope_rows(&mut tx, &grant.scope)
        .await
        .map_err(ApiError::from_db)?;
    Ok(Reading {
        tx,
        grant,
        generation,
        entries,
        revisions,
    })
}

/// What a preview row adds to delivery's: where it came from, and which of the content
/// type's fields the issuer's role hid.
#[derive(Serialize)]
pub(crate) struct PreviewMeta {
    source: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    hidden_fields: Vec<String>,
}

/// One preview row: exactly delivery's view, plus `preview`.
#[derive(Serialize)]
pub(crate) struct PreviewRow {
    #[serde(flatten)]
    view: PublishedView,
    preview: PreviewMeta,
}

/// `row` as the issuer may see it, or `None` when their role can't read it at all.
fn shown(
    tx: &ProjectTransaction,
    content_type: &ContentType,
    fields: &[String],
    keep: Option<&[String]>,
    media: &MediaBase,
    found: OverlayRow,
) -> Result<Option<PreviewRow>, ApiError> {
    let OverlayRow {
        mut row,
        source,
        created_by,
    } = found;
    let access = Access::of(
        &tx.policy,
        tx.identity,
        &row.collection,
        created_by.unwrap_or(Uuid::nil()),
    );
    let Some(read) = access.read else {
        return Ok(None);
    };
    let (content, hidden_fields) = visibility::project(row.content, &read, fields)?;
    row.content = match keep {
        Some(keep) => narrow(&content, keep)?,
        None => content,
    };
    if !read.includes(content_type.title_field()) {
        row.title = String::new();
    }
    // The uploads the shown content still names: a field the role hides, or `fields` leaves
    // out, takes its images' versions with it (D-178). A draft row's are the current
    // versions, as drafts show them.
    row.media = media_named(&row.media, &serde_json::Map::new(), &row.content)?;
    Ok(Some(PreviewRow {
        view: PublishedView::new(row, media)?,
        preview: PreviewMeta {
            source,
            hidden_fields,
        },
    }))
}

/// A preview read may not be pinned: it always reads the latest release.
#[derive(Deserialize)]
pub(crate) struct ReadParams {
    generation: Option<i64>,
    limit: Option<i64>,
    cursor: Option<String>,
}

fn latest_only(generation: Option<i64>) -> Result<(), ApiError> {
    match generation {
        Some(_) => Err(refused(
            StatusCode::UNPROCESSABLE_ENTITY,
            "preview_reads_latest",
            "a preview always reads the latest release; leave out ?generation=",
        )),
        None => Ok(()),
    }
}

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

/// A preview list's cursor names its project, environment and collection, and the last
/// entry: a preview always reads the latest release, so no generation travels in it.
fn cursor(path: &CollectionPath, project: Uuid, last: Uuid) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "p1:{project}:{}:{}:{last}",
        path.environment, path.collection
    ))
}

fn after_cursor(path: &CollectionPath, project: Uuid, text: &str) -> Result<Uuid, ApiError> {
    let prefix = format!("p1:{project}:{}:{}:", path.environment, path.collection);
    URL_SAFE_NO_PAD
        .decode(text)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|decoded| Uuid::try_parse(decoded.strip_prefix(&prefix)?).ok())
        .ok_or(ApiError::InvalidHeader(
            "this cursor is not valid for this list",
        ))
}

/// What the session route answers.
#[derive(Serialize)]
pub(crate) struct SessionView {
    scope: ScopeView,
    expires_at: String,
}

/// `GET /preview/v1/{project}/{environment}/session`: the grant's scope and end, for the
/// site's "Preview mode" banner.
pub(crate) async fn session(
    State(state): State<AppState>,
    Path(path): Path<EnvPath>,
    headers: HeaderMap,
) -> Response {
    let Ok(_permit) = READS.try_acquire() else {
        return seal(ApiError::Busy.into_response(), None, None);
    };
    answer(
        async {
            let reading = reading(&state, &path.project, &path.environment, &headers).await?;
            let body = serde_json::to_string(&Data {
                data: SessionView {
                    scope: ScopeView::from(&reading.grant.scope),
                    expires_at: rfc3339(reading.grant.expires_at)?,
                },
            })
            .map_err(ApiError::internal)?;
            let (kind, generation) = (reading.kind(), reading.generation);
            reading.tx.commit().await.map_err(ApiError::from_db)?;
            Ok((json(StatusCode::OK, body, None, false), kind, generation))
        }
        .await,
    )
}

/// `GET /preview/v1/{project}/{environment}/{collection}`: delivery's list, with the scope laid
/// over the latest release.
pub(crate) async fn list(
    State(state): State<AppState>,
    Path(path): Path<CollectionPath>,
    Query(params): Query<ReadParams>,
    headers: HeaderMap,
) -> Response {
    let Ok(_permit) = READS.try_acquire() else {
        return seal(ApiError::Busy.into_response(), None, None);
    };
    answer(
        async {
            latest_only(params.generation)?;
            let limit = params.limit.unwrap_or(20);
            if !(1..=colophon_db::delivery::MAX_PAGE).contains(&limit) {
                return Err(ApiError::InvalidHeader("limit must be between 1 and 100"));
            }
            let mut reading = reading(&state, &path.project, &path.environment, &headers).await?;
            let media = MediaBase::new(
                &services(&state)?.media_origin,
                reading.tx.project,
                &path.environment,
            );
            let project = reading.tx.project;
            let before = params
                .cursor
                .as_deref()
                .map(|text| after_cursor(&path, project, text))
                .transpose()?;
            may_read(&reading.tx, &path.collection).map_err(|_| ApiError::NotFound)?;
            let (content_type, _) = content_type(&state, &mut reading.tx, &path.collection).await?;
            let fields = field_ids(&content_type);
            let (entries, revisions) = (reading.entries.clone(), reading.revisions.clone());
            let overlay = Overlay {
                generation: reading.generation,
                entries: &entries,
                revisions: &revisions,
                source: reading.source(),
            };
            let mut rows = previews::list(
                &mut reading.tx,
                &path.collection,
                &overlay,
                before,
                limit + 1,
            )
            .await
            .map_err(ApiError::from_db)?;
            let page = usize::try_from(limit).unwrap_or(usize::MAX);
            let more = rows.len() > page;
            rows.truncate(page);
            let next_cursor = more
                .then(|| {
                    rows.last()
                        .map(|row| cursor(&path, project, row.row.entry_id))
                })
                .flatten();
            let mut data = Vec::with_capacity(rows.len());
            for row in rows {
                if let Some(row) = shown(&reading.tx, &content_type, &fields, None, &media, row)? {
                    data.push(row);
                }
            }
            let body =
                serde_json::to_string(&Page { data, next_cursor }).map_err(ApiError::internal)?;
            let (kind, generation) = (reading.kind(), reading.generation);
            reading.tx.commit().await.map_err(ApiError::from_db)?;
            Ok((json(StatusCode::OK, body, None, false), kind, generation))
        }
        .await,
    )
}

/// `GET /preview/v1/{project}/{environment}/{collection}/{entry}`.
pub(crate) async fn get(
    State(state): State<AppState>,
    Path(path): Path<EntryPath>,
    Query(params): Query<ReadParams>,
    headers: HeaderMap,
) -> Response {
    let Ok(_permit) = READS.try_acquire() else {
        return seal(ApiError::Busy.into_response(), None, None);
    };
    answer(
        async {
            latest_only(params.generation)?;
            let entry = Uuid::try_parse(&path.entry).map_err(|_| ApiError::NotFound)?;
            let mut reading = reading(&state, &path.project, &path.environment, &headers).await?;
            let media = MediaBase::new(
                &services(&state)?.media_origin,
                reading.tx.project,
                &path.environment,
            );
            may_read(&reading.tx, &path.collection).map_err(|_| ApiError::NotFound)?;
            let (content_type, _) = content_type(&state, &mut reading.tx, &path.collection).await?;
            let fields = field_ids(&content_type);
            let (entries, revisions) = (reading.entries.clone(), reading.revisions.clone());
            let overlay = Overlay {
                generation: reading.generation,
                entries: &entries,
                revisions: &revisions,
                source: reading.source(),
            };
            let found = previews::get(&mut reading.tx, &path.collection, &overlay, entry)
                .await
                .map_err(ApiError::from_db)?
                .ok_or(ApiError::NotFound)?;
            let row = shown(&reading.tx, &content_type, &fields, None, &media, found)?
                .ok_or(ApiError::NotFound)?;
            let body = serde_json::to_string(&Data { data: row }).map_err(ApiError::internal)?;
            let (kind, generation) = (reading.kind(), reading.generation);
            reading.tx.commit().await.map_err(ApiError::from_db)?;
            Ok((json(StatusCode::OK, body, None, false), kind, generation))
        }
        .await,
    )
}

/// The page a preview query answers with: delivery's query page, row for row.
#[derive(Serialize)]
struct QueryPage {
    data: Vec<PreviewRow>,
    next_cursor: Option<String>,
    applied: AppliedQuery,
}

/// `POST /preview/v1/{project}/{environment}/{collection}/query?cursor=`: the query language
/// over the overlay. A filter or sort on a field the issuer's role can't read is refused, as
/// the Studio's own query refuses it (D-109). `depth` isn't served in preview yet.
pub(crate) async fn query(
    State(state): State<AppState>,
    Path(path): Path<CollectionPath>,
    Query(params): Query<ReadParams>,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Response {
    let Ok(_permit) = READS.try_acquire() else {
        return seal(ApiError::Busy.into_response(), None, None);
    };
    answer(
        async {
            latest_only(params.generation)?;
            let Json(document) = body?;
            let mut reading = reading(&state, &path.project, &path.environment, &headers).await?;
            let media = MediaBase::new(
                &services(&state)?.media_origin,
                reading.tx.project,
                &path.environment,
            );
            may_read(&reading.tx, &path.collection).map_err(|_| ApiError::NotFound)?;
            let (content_type, _) = content_type(&state, &mut reading.tx, &path.collection).await?;
            let language =
                Document::validate(&content_type, &document).map_err(ApiError::Invalid)?;
            if language.depth() > 0 {
                return Err(refused(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "preview_depth",
                    "a preview doesn't populate references yet; leave out depth",
                ));
            }
            visibility::refuse_unreadable(
                &document,
                reading
                    .tx
                    .policy
                    .fields(Action::ContentRead, &path.collection, false)
                    .as_ref(),
            )?;
            let after = params
                .cursor
                .as_deref()
                .map(|token| language.after_cursor(token))
                .transpose()
                .map_err(ApiError::Invalid)?;
            let fields = field_ids(&content_type);
            let (entries, revisions) = (reading.entries.clone(), reading.revisions.clone());
            let overlay = Overlay {
                generation: reading.generation,
                entries: &entries,
                revisions: &revisions,
                source: reading.source(),
            };
            let found = db_query::overlay_page(
                &mut reading.tx,
                &path.collection,
                &content_type,
                &language,
                after.as_ref(),
                &overlay,
            )
            .await
            .map_err(|error| match error {
                db_query::Error::Cursor(refusal) => ApiError::Invalid(refusal),
                db_query::Error::Db(error) => ApiError::from_db(error),
            })?;
            let keep = language.fields();
            let mut data = Vec::with_capacity(found.rows.len());
            for row in found.rows {
                if let Some(row) = shown(&reading.tx, &content_type, &fields, keep, &media, row)? {
                    data.push(row);
                }
            }
            let body = serde_json::to_string(&QueryPage {
                data,
                next_cursor: found.next_cursor,
                applied: AppliedQuery::of(&language)?,
            })
            .map_err(ApiError::internal)?;
            let (kind, generation) = (reading.kind(), reading.generation);
            reading.tx.commit().await.map_err(ApiError::from_db)?;
            Ok((json(StatusCode::OK, body, None, false), kind, generation))
        }
        .await,
    )
}

// ---------------------------------------------------------------------------
// Browsers on the site's own origin (CORS)

/// The project and environment a `/preview/v1/{project}/{environment}/…` path names.
fn preview_scope_of(path: &str) -> Option<(Uuid, &str)> {
    let mut parts = path.strip_prefix("/preview/v1/")?.split('/');
    let project = Uuid::try_parse(parts.next()?).ok()?;
    let environment = parts.next().filter(|slug| !slug.is_empty())?;
    Some((project, environment))
}

/// Whether `origin` is the origin of the environment's preview address — the one origin a
/// browser may read preview answers from (D-172). Asked only for requests that carry `Origin`:
/// a site's server fetches without one and pays nothing.
async fn allowed_origin(state: &AppState, path: &str, origin: &str) -> bool {
    let Some((project, environment)) = preview_scope_of(path) else {
        return false;
    };
    let Ok(services) = services(state) else {
        return false;
    };
    match previews::target_of(&services.pool, project, environment).await {
        Ok(Some(template)) => Template::parse(&template, services.allow_private_destinations)
            .is_ok_and(|parsed| parsed.origin() == origin),
        Ok(None) => false,
        Err(error) => {
            tracing::warn!(%error, "could not read the preview address for a cross-origin request");
            false
        }
    }
}

/// CORS for `/preview/v1` (D-172): a browser on the environment's preview origin — a static or
/// client-rendered site — may exchange a code and read with a bearer token; any other origin
/// gets no `Access-Control-Allow-Origin`, so the browser keeps the answer from its page. Never
/// `Allow-Credentials`: preview reads are bearer, never cookie. A preflight is answered here,
/// before the router (whose routes don't take `OPTIONS`).
pub(crate) async fn cors(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let path = request.uri().path().to_owned();
    if !path.starts_with("/preview/v1/") {
        return next.run(request).await;
    }
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let preflight = request.method() == axum::http::Method::OPTIONS;
    let allowed = match &origin {
        Some(origin) => allowed_origin(&state, &path, origin).await,
        None => false,
    };
    let mut response = if preflight {
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(request).await
    };
    let headers = response.headers_mut();
    headers.append(header::VARY, HeaderValue::from_static("Origin"));
    if let (true, Some(origin)) = (allowed, origin)
        && let Ok(value) = HeaderValue::from_str(&origin)
    {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
        if preflight {
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static("GET, POST"),
            );
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_HEADERS,
                HeaderValue::from_static("Authorization, Content-Type"),
            );
            headers.insert(
                header::ACCESS_CONTROL_MAX_AGE,
                HeaderValue::from_static("600"),
            );
        } else {
            headers.insert(
                header::ACCESS_CONTROL_EXPOSE_HEADERS,
                HeaderValue::from_static("Colophon-Preview, Colophon-Generation"),
            );
        }
    }
    if preflight {
        // A preflight's own answer is no more cacheable than what it guards.
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(NO_STORE));
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_row_is_delivery_row_plus_preview() {
        let row = colophon_db::delivery::Published {
            entry_id: Uuid::nil(),
            revision_id: Uuid::nil(),
            collection: "articles".to_owned(),
            title: "Hello".to_owned(),
            slug: Some("hello".to_owned()),
            content: r#"{"title":"Hello","body":{"type":"doc"}}"#.to_owned(),
            content_hash: vec![0; 32],
            valid_from: 3,
            visibility: "public".to_owned(),
            media: "{}".to_owned(),
        };
        let media = MediaBase::new("http://localhost", Uuid::nil(), "production");
        let delivered =
            serde_json::to_value(PublishedView::new(row.clone(), &media).expect("view"))
                .expect("delivery json");
        let previewed = serde_json::to_value(PreviewRow {
            view: PublishedView::new(row, &media).expect("view"),
            preview: PreviewMeta {
                source: "draft".to_owned(),
                hidden_fields: Vec::new(),
            },
        })
        .expect("preview json");
        let mut without = previewed.as_object().expect("object").clone();
        assert_eq!(
            without.remove("preview"),
            Some(serde_json::json!({"source": "draft"}))
        );
        assert_eq!(
            Value::Object(without),
            delivered,
            "the rest is delivery's own shape"
        );
    }

    #[test]
    fn only_a_preview_token_is_taken_from_the_header() {
        let mut headers = HeaderMap::new();
        assert_eq!(bearer(&headers), None);
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer cpv_abc"),
        );
        assert_eq!(bearer(&headers), Some("cpv_abc"));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer colophon_api_abc"),
        );
        assert_eq!(
            bearer(&headers),
            None,
            "another credential is no preview token"
        );
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Basic cpv_abc"),
        );
        assert_eq!(bearer(&headers), None);
    }

    #[test]
    fn every_preview_answer_is_sealed_against_caches_and_crawlers() {
        let mut response = json(StatusCode::OK, "{}".to_owned(), Some(Uuid::nil()), false);
        response
            .headers_mut()
            .insert(crate::delivery::CACHE_TAG, HeaderValue::from_static("p:x"));
        let sealed = seal(response, Some("entry"), Some(4));
        let headers = sealed.headers();
        assert_eq!(headers[header::CACHE_CONTROL], "private, no-store");
        assert_eq!(headers[ROBOTS], "noindex, nofollow, noarchive");
        assert_eq!(headers[header::REFERRER_POLICY], "no-referrer");
        assert_eq!(headers[header::VARY], "Authorization");
        assert_eq!(headers[PREVIEW], "entry");
        assert_eq!(headers[GENERATION], "4");
        assert!(headers.get(header::ETAG).is_none());
        assert!(headers.get(crate::delivery::CACHE_TAG).is_none());
        let refused = seal(
            refused(StatusCode::UNAUTHORIZED, "preview_expired", "ended").into_response(),
            None,
            None,
        );
        assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            refused.headers()[header::CACHE_CONTROL],
            "private, no-store"
        );
        assert_eq!(
            refused.headers()[header::WWW_AUTHENTICATE],
            "Bearer error=\"invalid_token\""
        );
    }

    #[test]
    fn cors_reads_the_project_and_environment_from_the_path_alone() {
        let project = Uuid::now_v7();
        assert_eq!(
            preview_scope_of(&format!("/preview/v1/{project}/production/articles")),
            Some((project, "production"))
        );
        assert_eq!(
            preview_scope_of(&format!("/preview/v1/{project}/production/exchange")),
            Some((project, "production"))
        );
        assert_eq!(
            preview_scope_of("/preview/v1/not-a-uuid/production/x"),
            None
        );
        assert_eq!(preview_scope_of(&format!("/preview/v1/{project}/")), None);
        assert_eq!(preview_scope_of("/content/v1/x/y"), None);
    }

    #[test]
    fn a_list_cursor_is_bound_to_its_list() {
        let here = CollectionPath {
            project: "p".to_owned(),
            environment: "production".to_owned(),
            collection: "articles".to_owned(),
        };
        let (project, last) = (Uuid::now_v7(), Uuid::now_v7());
        let token = cursor(&here, project, last);
        assert_eq!(after_cursor(&here, project, &token).ok(), Some(last));
        assert!(after_cursor(&here, Uuid::now_v7(), &token).is_err());
        let other = CollectionPath {
            collection: "events".to_owned(),
            ..here
        };
        assert!(after_cursor(&other, project, &token).is_err());
    }
}
