//! The remote MCP endpoint (D-121, docs/modules/09-automation.md): `POST|DELETE {P}/mcp`,
//! where MCP's Streamable HTTP transport (protocol 2025-11-25) meets the API a project API
//! token may call.
//!
//! The shape, per the transport:
//!
//! - `POST` carries one JSON-RPC message. A request's response comes back on the same POST as
//!   `application/json`; a notification or a stray response is `202`; input that isn't
//!   acceptable JSON-RPC is `400`. The first `initialize` mints an `Mcp-Session-Id`, which
//!   every later request carries; an unknown id is `404`.
//! - `GET` is not offered: the server never initiates messages, so there is no SSE stream to
//!   open and the route's own `405` — which the table does not declare — is the transport's
//!   sanctioned answer. Nothing is resumable, so `Last-Event-ID` is meaningless and ignored,
//!   exactly as `{P}/events` treats it (D-108).
//! - `DELETE` ends a session early; sessions also expire after [`Limits::idle`] unused. The
//!   session ends its in-flight requests with `request cancelled`.
//!
//! Who may call it: a project API token and nothing else — a browser session and a personal
//! bearer token are 403 `api_token_required` (AU2: agents authenticate as agents, never as
//! people, and never with a person's credential either). The `SignedIn`
//! extractor already does the rest of the credential work: a revoked or expired token is 401,
//! another project's path is 404, so every request re-proves the token in Postgres rather than
//! trusting the session's age.
//!
//! Every request must also pass the transport's checks: `Origin`, when present, must be one
//! this server trusts exactly (the DNS-rebinding defence the transport requires), and
//! `MCP-Protocol-Version` must name [`PROTOCOL_VERSION`] — it is required once a session
//! exists, and a wrong value is refused even on `initialize`.
//!
//! What a tool call does here: the same thing it does on stdio — [`RemoteSession`] runs the
//! shared dispatcher, and the tools' requests are dispatched in-process through the server's
//! own router with the same `Authorization` header, so the API alone decides every call. The
//! endpoint itself holds no authorization beyond "is this the token's own session".

use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError},
    time::Duration,
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use colophon_mcp::{
    PROTOCOL_VERSION,
    api::{self, Backend},
    remote::{RemoteSession, Reply},
};
use serde::Deserialize;
use tokio::time::{Instant, timeout_at};
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    session::{Credential, SignedIn, services},
    state::Services,
};

/// The header an `initialize` answer mints and every later request carries.
const SESSION_ID: header::HeaderName = header::HeaderName::from_static("mcp-session-id");
/// The header naming the MCP protocol version a client speaks (`2025-11-25` here).
const PROTOCOL: header::HeaderName = header::HeaderName::from_static("mcp-protocol-version");

/// How MCP sessions are bounded (`COLOPHON_MCP_*`; D-121, invariant 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Sessions one API token may hold on this server.
    pub per_token: usize,
    /// Sessions of one project this server carries.
    pub per_project: usize,
    /// Sessions this server carries in all.
    pub max_sessions: usize,
    /// A session this long unused is forgotten; the client's next request is a 404 and it
    /// initializes again.
    pub idle: Duration,
}

impl Default for Limits {
    /// The documented bounds: 8 sessions per token, 256 per project on one server, 2048 on a
    /// server in all, and a half hour unused forgets a session.
    fn default() -> Self {
        Self {
            per_token: 8,
            per_project: 256,
            max_sessions: 2048,
            idle: Duration::from_mins(30),
        }
    }
}

/// The sessions this server carries, plus the router tool calls dispatch through. Lives in
/// [`Services`] — one per server, behind the state `Arc`.
pub struct Shared {
    limits: Limits,
    /// The server's own router, built on the first tool call that needs it. The router holds
    /// the state that holds this, so the reference is a cycle — which untangles only at
    /// process exit, when everything here would drop anyway.
    router: OnceLock<Router>,
    registry: Mutex<Registry>,
}

#[derive(Default)]
struct Registry {
    /// Live sessions by their `Mcp-Session-Id`.
    sessions: HashMap<Uuid, Arc<Hosted>>,
    /// Live sessions per token id.
    per_token: HashMap<Uuid, usize>,
    /// Live sessions per project.
    per_project: HashMap<Uuid, usize>,
}

fn lock(registry: &Mutex<Registry>) -> MutexGuard<'_, Registry> {
    registry.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One client's session and the credential it was made with. `token` binds the session to the
/// API token that created it — no other credential may use the id — and `dispatch` carries the
/// tool calls to the API under that same credential.
struct Hosted {
    session: RemoteSession,
    dispatch: Dispatch,
    token: Uuid,
    project: Uuid,
    environment: String,
    /// When a request last touched the session; `idle` past now forgets it. Locked because the
    /// session may be shared while one request refreshes it.
    touched: Mutex<Instant>,
}

/// Why a new session was refused, mapped to the envelope by the handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refused {
    /// The token already holds `per_token` sessions: 429.
    Token,
    /// This project already has `per_project` sessions: 503.
    Project,
    /// This server already carries `max_sessions` sessions: 503.
    Server,
}

impl Shared {
    /// An empty registry under `limits`.
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            router: OnceLock::new(),
            registry: Mutex::new(Registry::default()),
        }
    }

    /// The server's own router for in-process dispatch, built once. It is the same table the
    /// public surface serves, so a tool's request meets the same routes and admission limits
    /// as an agent's request over the network.
    fn router(&self, state: &AppState) -> Router {
        self.router
            .get_or_init(|| crate::routes(state.clone()))
            .clone()
    }

    /// The session `id`, when it exists, belongs to `token` on this project and environment,
    /// and was touched inside `idle`. Stale sessions are forgotten on the way.
    fn get(&self, id: Uuid, token: Uuid, project: Uuid, environment: &str) -> Option<Arc<Hosted>> {
        let mut registry = lock(&self.registry);
        self.sweep(&mut registry);
        let hosted = registry.sessions.get(&id)?;
        if hosted.token != token || hosted.project != project || hosted.environment != environment {
            return None;
        }
        *hosted
            .touched
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Instant::now();
        Some(Arc::clone(hosted))
    }

    /// Registers `hosted` under a fresh session id, refused over the caps *before* it is
    /// recorded — the same before-subscribing check the event streams use.
    fn register(&self, hosted: Hosted) -> Result<Uuid, Refused> {
        let mut registry = lock(&self.registry);
        self.sweep(&mut registry);
        if registry.per_token.get(&hosted.token).copied().unwrap_or(0) >= self.limits.per_token {
            return Err(Refused::Token);
        }
        if registry
            .per_project
            .get(&hosted.project)
            .copied()
            .unwrap_or(0)
            >= self.limits.per_project
        {
            return Err(Refused::Project);
        }
        if registry.sessions.len() >= self.limits.max_sessions {
            return Err(Refused::Server);
        }
        let id = Uuid::now_v7();
        *registry.per_token.entry(hosted.token).or_insert(0) += 1;
        *registry.per_project.entry(hosted.project).or_insert(0) += 1;
        registry.sessions.insert(id, Arc::new(hosted));
        Ok(id)
    }

    /// Ends the session `id` when it is `token`'s on this project and environment. `None` when
    /// there is no such session — another token's id looks the same as one never minted.
    fn remove(
        &self,
        id: Uuid,
        token: Uuid,
        project: Uuid,
        environment: &str,
    ) -> Option<Arc<Hosted>> {
        let mut registry = lock(&self.registry);
        self.sweep(&mut registry);
        let belongs = registry.sessions.get(&id).is_some_and(|hosted| {
            hosted.token == token && hosted.project == project && hosted.environment == environment
        });
        if !belongs {
            return None;
        }
        let hosted = registry.sessions.remove(&id);
        if let Some(hosted) = &hosted {
            decrement(&mut registry.per_token, hosted.token);
            decrement(&mut registry.per_project, hosted.project);
        }
        hosted
    }

    /// Forgets the sessions `idle` has outlived, ending their in-flight requests. Called under
    /// the lock from every operation, so a full table can never hold only dead entries.
    fn sweep(&self, registry: &mut Registry) {
        let now = Instant::now();
        let idle = self.limits.idle;
        registry.sessions.retain(|_, hosted| {
            let touched = *hosted
                .touched
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let live = now.duration_since(touched) < idle;
            if !live {
                hosted.session.close();
            }
            live
        });
        // The counts are derived, so rebuild them rather than adjust under `retain`.
        registry.per_token = count_by(&registry.sessions, |hosted| hosted.token);
        registry.per_project = count_by(&registry.sessions, |hosted| hosted.project);
    }

    /// Live sessions in all, for the tests.
    #[cfg(test)]
    fn sessions(&self) -> usize {
        lock(&self.registry).sessions.len()
    }
}

/// One fewer session under `key`; the entry leaves at zero.
fn decrement(map: &mut HashMap<Uuid, usize>, key: Uuid) {
    if let Some(count) = map.get_mut(&key) {
        *count = count.saturating_sub(1);
        if *count == 0 {
            map.remove(&key);
        }
    }
}

/// The count map rebuilt from the sessions themselves, after a sweep.
fn count_by(
    sessions: &HashMap<Uuid, Arc<Hosted>>,
    key: impl Fn(&Hosted) -> Uuid,
) -> HashMap<Uuid, usize> {
    let mut counts = HashMap::new();
    for hosted in sessions.values() {
        *counts.entry(key(hosted)).or_insert(0) += 1;
    }
    counts
}

/// The `{P}` of `{P}/mcp`.
#[derive(Deserialize)]
pub(crate) struct McpPath {
    project: String,
    environment: String,
}

/// `POST {P}/mcp`: one JSON-RPC message on the session its `Mcp-Session-Id` names — or, with
/// none, on a fresh session that registers only if `initialize` succeeds (D-121).
pub(crate) async fn post(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<McpPath>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let services = services(&state)?;
    let id = admitted(services, &caller, &headers, false)?;
    let (project, environment) = target(&path)?;

    let mut fresh = None;
    let reply = if let Some(id) = id {
        let hosted = services
            .mcp
            .get(id, caller.session, project, &environment)
            .ok_or(ApiError::NotFound)?;
        hosted.session.handle(&body, &hosted.dispatch).await
    } else {
        let dispatch = Dispatch::new(&state, &headers, project, &environment)?;
        let session = RemoteSession::new();
        let reply = session.handle(&body, &dispatch).await;
        fresh = session.is_initialized().then_some(Hosted {
            session,
            dispatch,
            token: caller.session,
            project,
            environment,
            touched: Mutex::new(Instant::now()),
        });
        reply
    };
    // A fresh session that handshook joins the registry; its id rides back on the answer.
    let minted =
        match fresh {
            Some(hosted) => Some(services.mcp.register(hosted).map_err(
                |refused| match refused {
                    Refused::Token => ApiError::TooManyRequests { retry_after: 30 },
                    Refused::Project | Refused::Server => ApiError::Unavailable(
                        "this server is carrying as many MCP sessions as it can",
                    ),
                },
            )?),
            None => None,
        };
    Ok(respond(reply, minted))
}

/// `DELETE {P}/mcp`: ends the session early (D-121). The session's in-flight requests answer
/// `request cancelled`; a later request with the id is a 404.
pub(crate) async fn delete(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<McpPath>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let services = services(&state)?;
    let id = admitted(services, &caller, &headers, true)?;
    let (project, environment) = target(&path)?;
    let hosted = services
        .mcp
        .remove(
            id.ok_or(ApiError::InvalidHeader(
                "the Mcp-Session-Id header is required",
            ))?,
            caller.session,
            project,
            &environment,
        )
        .ok_or(ApiError::NotFound)?;
    hosted.session.close();
    Ok(StatusCode::OK.into_response())
}

/// The transport's door checks, shared by `POST` and `DELETE`: only a project API token (a
/// person is 401); a present `Origin` must be trusted exactly (the DNS-rebinding check the
/// transport requires); `MCP-Protocol-Version` must name the one version this server speaks —
/// required once a session exists — and a `POST` answer is always `application/json`, which a
/// stated `Accept` must allow. Answers the session id the request carries, when it parses as
/// one this server could have minted.
fn admitted(
    services: &Services,
    caller: &SignedIn,
    headers: &HeaderMap,
    session_required: bool,
) -> Result<Option<Uuid>, ApiError> {
    if caller.credential != Credential::ApiToken {
        return Err(ApiError::forbidden(
            "api_token_required",
            "the MCP endpoint speaks to agents: sign a project API token in, not a person",
        ));
    }
    if let Some(origin) = headers.get(header::ORIGIN) {
        let trusted = origin
            .to_str()
            .is_ok_and(|origin| services.trusted_origins.iter().any(|t| t == origin));
        if !trusted {
            return Err(ApiError::forbidden(
                "origin_not_allowed",
                "this origin may not call the MCP endpoint",
            ));
        }
    }
    let id = session_id(headers)?;
    if session_required && id.is_none() {
        return Err(ApiError::InvalidHeader(
            "the Mcp-Session-Id header is required",
        ));
    }
    match headers.get(&PROTOCOL).and_then(|value| value.to_str().ok()) {
        Some(version) if version == PROTOCOL_VERSION => {}
        Some(_) => {
            return Err(ApiError::InvalidHeader(
                "the MCP-Protocol-Version header names a version this server doesn't speak (2025-11-25)",
            ));
        }
        None if id.is_some() || session_required => {
            return Err(ApiError::InvalidHeader(
                "the MCP-Protocol-Version header is required once a session exists",
            ));
        }
        None => {}
    }
    if let Some(accept) = headers.get(header::ACCEPT)
        && !accepts_json(accept)
    {
        return Err(ApiError::InvalidHeader(
            "the MCP endpoint answers application/json only: Accept must allow it",
        ));
    }
    Ok(id)
}

/// The `Mcp-Session-Id` header's session id, when the request carries one. A malformed value
/// is a 404 like any unknown id: the ids are UUIDs and this one names none.
fn session_id(headers: &HeaderMap) -> Result<Option<Uuid>, ApiError> {
    let Some(value) = headers.get(&SESSION_ID) else {
        return Ok(None);
    };
    let id = value
        .to_str()
        .ok()
        .and_then(|text| Uuid::try_parse(text).ok())
        .ok_or(ApiError::NotFound)?;
    Ok(Some(id))
}

/// Whether `Accept` allows `application/json` — by it, by a wildcard, or by `*/*`.
fn accepts_json(accept: &HeaderValue) -> bool {
    accept.to_str().is_ok_and(|value| {
        value.split(',').any(|part| {
            let media = part.split(';').next().unwrap_or_default().trim();
            media == "*/*" || media == "application/*" || media == "application/json"
        })
    })
}

/// The project and environment the path names; either is 404 when it can't exist — a project
/// that isn't an id, an environment that isn't a slug.
fn target(path: &McpPath) -> Result<(Uuid, String), ApiError> {
    let project = path
        .project
        .parse::<Uuid>()
        .map_err(|_| ApiError::NotFound)?;
    if !is_slug(&path.environment) {
        return Err(ApiError::NotFound);
    }
    Ok((project, path.environment.clone()))
}

/// Whether `text` is an environment slug: 1–63 of `a-z 0-9 -`, starting and ending plainly.
fn is_slug(text: &str) -> bool {
    let bytes = text.as_bytes();
    let plain = |byte: &u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    (1..=63).contains(&bytes.len())
        && bytes.first().is_some_and(plain)
        && bytes.last().is_some_and(plain)
        && bytes.iter().all(|byte| plain(byte) || *byte == b'-')
}

/// The HTTP answer one [`Reply`] is: a JSON-RPC response's `200`, an accepted message's `202`,
/// a refused message's `400` — carrying its JSON-RPC error when there is one. `minted` is the
/// `Mcp-Session-Id` an `initialize` answer announces.
fn respond(reply: Reply, minted: Option<Uuid>) -> Response {
    let mut response = match reply {
        Reply::Respond(text) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            text,
        )
            .into_response(),
        Reply::Accepted => StatusCode::ACCEPTED.into_response(),
        Reply::Rejected(Some(text)) => (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "application/json")],
            text,
        )
            .into_response(),
        Reply::Rejected(None) => StatusCode::BAD_REQUEST.into_response(),
    };
    if let Some(id) = minted {
        // The id is a UUID: 32 hex characters, visible ASCII as the transport requires.
        if let Ok(value) = HeaderValue::from_str(&id.simple().to_string()) {
            response.headers_mut().insert(&SESSION_ID, value);
        }
    }
    response
}

/// Carries a tool's request to the API in-process: the same `Authorization` header the client
/// called MCP with, the same routes an agent over the network would hit, so the API alone
/// decides the call — this is dispatch, never authorization (D-121).
struct Dispatch {
    state: AppState,
    /// The `Authorization` header value as it arrived: `Bearer colophon_api_…`.
    token: HeaderValue,
    project: Uuid,
    environment: String,
}

impl Dispatch {
    /// The dispatch a new session runs its tool calls through: the caller's own credential,
    /// the project and environment the endpoint was addressed on.
    fn new(
        state: &AppState,
        headers: &HeaderMap,
        project: Uuid,
        environment: &str,
    ) -> Result<Self, ApiError> {
        let token = headers
            .get(header::AUTHORIZATION)
            .ok_or(ApiError::Unauthorized)?
            .clone();
        Ok(Self {
            state: state.clone(),
            token,
            project,
            environment: environment.to_owned(),
        })
    }
}

impl Backend for Dispatch {
    fn project(&self) -> Uuid {
        self.project
    }

    /// One attempt, inside this process: the request's path under `{P}`, the same headers the
    /// HTTP client would set, through the same router — bounded by `deadline` and by
    /// [`api::ATTEMPT_TIMEOUT`], and the answer read under [`api::MAX_ANSWER_BYTES`].
    async fn attempt<'a>(
        &'a self,
        request: &'a api::Request<'a>,
        deadline: Instant,
    ) -> Result<api::Answer, api::ApiError> {
        let services = services(&self.state)
            .map_err(|_| api::ApiError::Unreachable("the server has no database".into()))?;
        let router = services.mcp.router(&self.state);
        let mut uri = format!(
            "/api/v1/projects/{}/environments/{}{}",
            self.project,
            self.environment,
            request.path()
        );
        if !request.query_pairs().is_empty() {
            let mut pairs = url::form_urlencoded::Serializer::new(String::new());
            for (name, value) in request.query_pairs() {
                pairs.append_pair(name, value);
            }
            uri.push('?');
            uri.push_str(&pairs.finish());
        }
        let mut builder = Request::builder()
            .method(request.method().clone())
            .uri(&uri)
            .header(header::AUTHORIZATION, self.token.clone());
        if let Some(tag) = request.if_match_tag() {
            builder = builder.header(header::IF_MATCH, tag);
        }
        if let Some(key) = request.idempotency_key_value() {
            builder = builder.header("idempotency-key", key);
        }
        let body = match request.body() {
            Some(body) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(serde_json::to_vec(body).unwrap_or_default())
            }
            None => Body::empty(),
        };
        let request = builder.body(body).map_err(|_| api::ApiError::BadUrl)?;
        let exchange = async {
            // The router's service is infallible: it answers with a status, never an error.
            let response = router
                .oneshot(request)
                .await
                .map_err(|_: Infallible| api::ApiError::Unreachable("the router erred".into()))?;
            let status = response.status();
            let etag = response
                .headers()
                .get(header::ETAG)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            let replayed = response
                .headers()
                .get("idempotent-replayed")
                .is_some_and(|value| value == "true");
            // An in-process body can only fail the read by exceeding the limit.
            let body = axum::body::to_bytes(response.into_body(), api::MAX_ANSWER_BYTES)
                .await
                .map_err(|_| api::ApiError::TooLarge {
                    limit: api::MAX_ANSWER_BYTES,
                })?;
            Ok::<_, api::ApiError>((status, etag, replayed, body))
        };
        let limit = deadline.min(Instant::now() + api::ATTEMPT_TIMEOUT);
        let (status, etag, replayed, body) = timeout_at(limit, exchange)
            .await
            .map_err(|_| api::ApiError::TimedOut(api::ATTEMPT_TIMEOUT))??;
        api::answer(status, etag.as_deref(), replayed, &body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosted(token: Uuid, project: Uuid) -> Hosted {
        let state = AppState::without_services(crate::Readiness::new("all", true));
        Hosted {
            session: RemoteSession::new(),
            dispatch: Dispatch {
                state,
                token: HeaderValue::from_static("Bearer colophon_api_x"),
                project,
                environment: "production".to_owned(),
            },
            token,
            project,
            environment: "production".to_owned(),
            touched: Mutex::new(Instant::now()),
        }
    }

    #[test]
    fn sessions_are_capped_before_they_are_kept() {
        let (token, other, project) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        let shared = Shared::new(Limits {
            per_token: 2,
            per_project: 3,
            max_sessions: 4,
            ..Limits::default()
        });
        shared.register(hosted(token, project)).expect("room");
        shared.register(hosted(token, project)).expect("room");
        assert_eq!(
            shared.register(hosted(token, project)),
            Err(Refused::Token),
            "the token's own cap bites first"
        );
        shared.register(hosted(other, project)).expect("room");
        assert_eq!(
            shared.register(hosted(other, project)),
            Err(Refused::Project),
            "then the project's cap"
        );
        assert_eq!(shared.sessions(), 3);
    }

    #[test]
    fn a_session_is_only_its_own_tokens() {
        let (token, other, project) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        let shared = Shared::new(Limits::default());
        let id = shared.register(hosted(token, project)).expect("room");
        assert!(shared.get(id, token, project, "production").is_some());
        assert!(
            shared.get(id, other, project, "production").is_none(),
            "another token's request is a 404, as if the id never existed"
        );
        assert!(
            shared.get(id, token, project, "staging").is_none(),
            "another environment's path is a 404 too"
        );
        assert!(shared.remove(id, other, project, "production").is_none());
        assert!(shared.remove(id, token, project, "production").is_some());
        assert!(shared.get(id, token, project, "production").is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn an_idle_session_is_forgotten() {
        let (token, project) = (Uuid::now_v7(), Uuid::now_v7());
        let shared = Shared::new(Limits {
            idle: Duration::from_secs(60),
            ..Limits::default()
        });
        let id = shared.register(hosted(token, project)).expect("room");
        tokio::time::advance(Duration::from_secs(30)).await;
        assert!(shared.get(id, token, project, "production").is_some());
        tokio::time::advance(Duration::from_secs(31)).await;
        // The last touch was the get, half an idle ago — still inside it.
        assert!(shared.get(id, token, project, "production").is_some());
        tokio::time::advance(Duration::from_secs(61)).await;
        assert!(
            shared.get(id, token, project, "production").is_none(),
            "a full idle since the last touch forgets the session"
        );
        assert_eq!(shared.sessions(), 0);
    }

    #[test]
    fn header_checks_match_the_transport() {
        let headers = |pairs: &[(&'static str, &str)]| {
            let mut headers = HeaderMap::new();
            for (name, value) in pairs {
                headers.insert(
                    header::HeaderName::from_static(name),
                    HeaderValue::from_str(value).expect("ASCII"),
                );
            }
            headers
        };
        for value in [
            "application/json, text/event-stream",
            "application/json",
            "*/*",
            "application/*",
        ] {
            assert!(
                accepts_json(
                    headers(&[("accept", value)])
                        .get(header::ACCEPT)
                        .expect("the header")
                ),
                "{value}"
            );
        }
        for value in ["text/event-stream", "text/plain"] {
            assert!(
                !accepts_json(
                    headers(&[("accept", value)])
                        .get(header::ACCEPT)
                        .expect("the header")
                ),
                "{value}"
            );
        }

        let id: Uuid = "0190f0c2-7a4e-7cc1-9d1e-2b8a4c6f1a01"
            .parse()
            .expect("a UUID");
        assert!(matches!(
            session_id(&headers(&[("mcp-session-id", &id.to_string())])),
            Ok(Some(found)) if found == id
        ));
        assert!(matches!(
            session_id(&headers(&[("mcp-session-id", "nonsense")])),
            Err(ApiError::NotFound)
        ));
    }
}
