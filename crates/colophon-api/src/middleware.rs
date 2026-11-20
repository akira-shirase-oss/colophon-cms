//! The one Tower stack every request passes through (`docs/modules/04-api.md`, "Server
//! shape"). `ServiceBuilder` lists layers outermost first: a request travels down the list,
//! its response travels back up.

use std::{any::Any, borrow::Cow, time::Duration};

use axum::{
    Router,
    body::Body,
    extract::{DefaultBodyLimit, MatchedPath, Request},
    http::{HeaderValue, StatusCode, header},
    middleware::{self as axum_mw, Next},
    response::{IntoResponse, Response},
};
use tower::ServiceBuilder;
use tower_http::{
    LatencyUnit,
    catch_panic::CatchPanicLayer,
    limit::RequestBodyLimitLayer,
    request_id::{MakeRequestId, PropagateRequestIdLayer, RequestId, SetRequestIdLayer},
    trace::{DefaultOnResponse, TraceLayer},
};
use tracing::{Level, Span};

use crate::{
    BuildInfo, body,
    error::{self, ApiError, PublicError},
};

/// The header that carries the request id in both directions.
pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// The longest client-supplied request id that is kept.
pub const MAX_REQUEST_ID_LEN: usize = 64;

/// Status for a request that misses its deadline. 504 rather than 408: the client sent its
/// request in time; the server was too slow answering it.
pub const TIMEOUT_STATUS: StatusCode = StatusCode::GATEWAY_TIMEOUT;

/// Bounds that apply to every request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpLimits {
    /// Deadline for producing the response headers.
    pub request_timeout: Duration,
    /// Largest accepted request body, in bytes.
    pub max_body_bytes: usize,
}

impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(10),
            // 1 MiB (module 04); uploads stream separately and never pass through here.
            max_body_bytes: 1024 * 1024,
        }
    }
}

/// The request deadline, handed to an upload handler instead of timing its body (D-215).
#[derive(Debug, Clone, Copy)]
pub struct UploadDeadline(pub Duration);

/// Whether `request` sends an upload's bytes: `POST {P}/assets` or `POST {P}/assets/{asset}/replace`
/// (`{P}` = `/api/v1/projects/{project}/environments/{environment}`).
pub fn is_upload(request: &Request) -> bool {
    if request.method() != axum::http::Method::POST {
        return false;
    }
    let parts: Vec<&str> = request
        .uri()
        .path()
        .trim_end_matches('/')
        .split('/')
        .collect();
    let environment = matches!(parts.as_slice(), ["", "api", "v1", "projects", project, "environments", env, ..] if !project.is_empty() && !env.is_empty());
    environment
        && match &parts[7..] {
            ["assets"] => true,
            ["assets", asset, "replace"] => !asset.is_empty(),
            _ => false,
        }
}

/// The deadline layer: `timeout` around every request but an upload, which instead carries it as an
/// [`UploadDeadline`] for its handler (its body may legitimately take longer to arrive).
async fn deadline(timeout: Duration, mut request: Request, next: Next) -> Response {
    if is_upload(&request) {
        request.extensions_mut().insert(UploadDeadline(timeout));
        return next.run(request).await;
    }
    match tokio::time::timeout(timeout, next.run(request)).await {
        Ok(response) => response,
        Err(_) => TIMEOUT_STATUS.into_response(),
    }
}

/// Generates time-ordered UUID v7 request ids, which sort by time in log stores.
#[derive(Debug, Clone, Copy, Default)]
pub struct MakeRequestUuidV7;

impl MakeRequestId for MakeRequestUuidV7 {
    fn make_request_id<B>(&mut self, _request: &Request<B>) -> Option<RequestId> {
        let id = uuid::Uuid::now_v7().to_string();
        HeaderValue::from_str(&id).ok().map(RequestId::new)
    }
}

/// True for ids that are safe to copy into logs and responses: 1 to 64 characters of
/// `[A-Za-z0-9._-]`.
pub fn is_acceptable_request_id(value: &str) -> bool {
    (1..=MAX_REQUEST_ID_LEN).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// Removes a client-supplied request id that is too long or contains other characters, so
/// that `SetRequestIdLayer`, which keeps an existing id, generates a fresh one instead.
async fn sanitize_request_id(mut request: Request) -> Request {
    let acceptable = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .map(|value| value.to_str().is_ok_and(is_acceptable_request_id));
    if acceptable == Some(false) {
        request.headers_mut().remove(REQUEST_ID_HEADER);
    }
    request
}

/// Path parameters whose values are credentials (D-152): a magic link's `{token}` and a
/// device sign-in's `{code}`. The request span writes their segments as these names, so no
/// line logged while handling such a request — at any level — carries the secret.
const SECRET_PARAMS: &[&str] = &["{token}", "{code}"];

/// For a path no route matched — a trailing slash after a token, a wrong prefix — the
/// segment after one of these is written as the credential it would have been (D-152):
/// `magic-link/{token}`, `device/{code}`.
const SECRET_AFTER: &[(&str, &str)] = &[("magic-link", "{token}"), ("device", "{code}")];

/// The path as the request span records it: as sent, except that each segment the matched
/// route's template names as a credential is written as that name
/// (`/audience/v1/{uuid}/magic-link/{token}`). An unmatched path has no template, so any
/// segment after `magic-link` or `device` is written as its name instead ([`SECRET_AFTER`]).
fn loggable_path<'a>(path: &'a str, route: Option<&str>) -> Cow<'a, str> {
    let Some(route) = route else {
        return unmatched_path(path);
    };
    if !SECRET_PARAMS.iter().any(|name| route.contains(name)) {
        return Cow::Borrowed(path);
    }
    let mut template = route.split('/');
    let segments: Vec<&str> = path
        .split('/')
        .map(|segment| match template.next() {
            Some(name) if SECRET_PARAMS.contains(&name) => name,
            _ => segment,
        })
        .collect();
    Cow::Owned(segments.join("/"))
}

/// [`loggable_path`] for a path no route matched.
fn unmatched_path(path: &str) -> Cow<'_, str> {
    if !SECRET_AFTER.iter().any(|(after, _)| path.contains(after)) {
        return Cow::Borrowed(path);
    }
    let mut previous = "";
    let segments: Vec<&str> = path
        .split('/')
        .map(|segment| {
            let written = SECRET_AFTER
                .iter()
                .find(|(after, _)| *after == previous && !segment.is_empty())
                .map_or(segment, |(_, name)| name);
            previous = segment;
            written
        })
        .collect();
    Cow::Owned(segments.join("/"))
}

/// The root span of every request. Everything logged while handling the request inherits
/// these fields. `route` is the matched template (low cardinality); `path` omits the query
/// string, which can carry tokens, and writes credential segments as their names
/// ([`loggable_path`]). `version` and `commit` are the deployment identity the correlation
/// fields call for (module 18, spec §32): every request-attributed line names the exact
/// build that served it.
fn make_span(request: &Request<Body>, build: BuildInfo) -> Span {
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-");
    let matched = request.extensions().get::<MatchedPath>();
    let route = matched.map_or("<unmatched>", MatchedPath::as_str);
    let path = loggable_path(request.uri().path(), matched.map(MatchedPath::as_str));
    tracing::info_span!(
        "request",
        method = %request.method(),
        route,
        path = %path,
        request_id = %request_id,
        version = build.version,
        commit = build.commit,
    )
}

/// Gives every 4xx and 5xx response leaving the server the JSON envelope with the request id:
/// errors from handlers (which carry a [`PublicError`]) and errors from layers we don't
/// control (axum's 405, tower-http's 413 and 504). JSON bodies without a `PublicError`, such
/// as a 503 from `/readyz`, are left alone.
async fn error_envelope(request: Request, next: Next) -> Response {
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let response = next.run(request).await;

    let status = response.status();
    if !(status.is_client_error() || status.is_server_error()) {
        return response;
    }
    let public = match response.extensions().get::<PublicError>() {
        Some(public) => public.clone(),
        None if error::is_json(response.headers()) => return response,
        None => {
            if status.is_server_error() {
                tracing::warn!(%status, "request failed in middleware");
            } else {
                tracing::debug!(%status, "request rejected by middleware");
            }
            PublicError::from_status(status)
        }
    };

    let (mut parts, _discarded_body) = response.into_parts();
    let rendered = error::render(status, &public, request_id.as_deref());
    // Keep headers such as `Allow` and `x-request-id`; replace the body and the headers that
    // describe it.
    parts.headers.remove(header::CONTENT_LENGTH);
    let (rendered_parts, body) = rendered.into_parts();
    parts.headers.extend(rendered_parts.headers);
    parts.extensions.extend(rendered_parts.extensions);
    Response::from_parts(parts, body)
}

#[derive(Debug, thiserror::Error)]
#[error("handler panicked: {0}")]
struct Panicked(String);

/// Turns a caught panic into a 500 envelope. The panic message is logged, because it may
/// contain internal detail, and never returned.
fn panic_response(payload: Box<dyn Any + Send + 'static>) -> Response {
    // `panic!("{x}")` carries a `String`, `panic!("literal")` a `&'static str`.
    let message = match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => payload.downcast::<&'static str>().map_or_else(
            |_| "non-string panic payload".to_owned(),
            |message| (*message).to_owned(),
        ),
    };
    ApiError::internal(Panicked(message)).into_response()
}

/// A request this slow logs its phases (D-221).
const SLOW_REQUEST: Duration = Duration::from_millis(200);

/// D-221's evidence: a request slower than [`SLOW_REQUEST`] logs one DEBUG line on the target
/// `colophon_api::slow`, inside its request span (id, method, route): its total, its time getting
/// connections and beginning transactions (pool wait and `BEGIN`, [`colophon_db::Phases`]), how many,
/// and the rest (the handler's statements and work). Nothing is counted unless that target is on.
async fn slow_requests(request: Request, next: Next) -> Response {
    if !tracing::enabled!(target: "colophon_api::slow", Level::DEBUG) {
        return next.run(request).await;
    }
    let started = std::time::Instant::now();
    let phases = std::sync::Arc::new(colophon_db::Phases::default());
    let response =
        colophon_db::with_phases(std::sync::Arc::clone(&phases), next.run(request)).await;
    let took = started.elapsed();
    if took >= SLOW_REQUEST {
        let total_ms = took.as_millis();
        let begin_ms = u128::from(phases.begin_micros()) / 1000;
        tracing::debug!(
            target: "colophon_api::slow",
            status = response.status().as_u16(),
            total_ms,
            begin_ms,
            begins = phases.begins(),
            rest_ms = total_ms.saturating_sub(begin_ms),
            "slow request"
        );
    }
    response
}

/// Applies the global stack to a router, with `build` recorded on every request span.
/// Read it top to bottom as the path of a request.
pub fn apply(router: Router, limits: HttpLimits, build: BuildInfo) -> Router {
    let stack = ServiceBuilder::new()
        // 0. Drop unsafe client ids before anything copies them into logs.
        .layer(axum_mw::map_request(sanitize_request_id))
        // 1. Assign an id if there is none (header and request extensions).
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuidV7))
        // 2. Copy it onto every response, including those that inner layers generate.
        .layer(PropagateRequestIdLayer::x_request_id())
        // 3. The root span, which can see the id because layer 1 already ran. One line per
        //    request at DEBUG: at the request rates Colophon is built for, per-request INFO
        //    lines would cost more than the requests. Errors are logged where they happen.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(move |request: &Request<Body>| make_span(request, build))
                .on_response(
                    DefaultOnResponse::new()
                        .level(Level::DEBUG)
                        .latency_unit(LatencyUnit::Micros),
                )
                .on_failure(()),
        )
        // 3b. A slow request's phases, inside its span (D-221; off unless its DEBUG target is on).
        .layer(axum_mw::from_fn(slow_requests))
        // 4. Every error below becomes the envelope, inside the span so its logs carry the id.
        .layer(axum_mw::from_fn(error_envelope))
        // 5. Never close a connection silently over an unread body: refuse declared oversized
        //    bodies after draining them, drain what handlers leave unread, or say
        //    `Connection: close` (body.rs). Outside the panic catcher, so it still runs after a
        //    panic, and inside the envelope, so its bare 413 gets rendered.
        .layer(axum_mw::from_fn(move |request: Request, next: Next| {
            body::hygiene(limits.max_body_bytes, request, next)
        }))
        // 6. A panic below becomes a 500 envelope instead of a dropped connection.
        .layer(CatchPanicLayer::custom(panic_response))
        // 7. The deadline for producing response headers: every request's, except an upload's,
        //    whose body may still be arriving (D-215); its handler applies it once the body is in.
        .layer(axum_mw::from_fn(move |request: Request, next: Next| {
            deadline(limits.request_timeout, request, next)
        }))
        // 8. Reject bodies that grow past the limit while streaming (no Content-Length)...
        .layer(RequestBodyLimitLayer::new(limits.max_body_bytes))
        // ...and give axum's extractors, whose own default is 2 MB, the same limit.
        .layer(DefaultBodyLimit::max(limits.max_body_bytes));
    router.layer(stack)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, path: &str) -> Request {
        Request::builder()
            .method(method)
            .uri(path)
            .body(Body::empty())
            .expect("request")
    }

    #[test]
    fn only_the_two_upload_routes_skip_the_deadline() {
        let p = "/api/v1/projects/abc/environments/production";
        assert!(is_upload(&request("POST", &format!("{p}/assets"))));
        assert!(is_upload(&request(
            "POST",
            &format!("{p}/assets/01a1/replace")
        )));
        assert!(!is_upload(&request("GET", &format!("{p}/assets"))));
        assert!(!is_upload(&request("POST", &format!("{p}/assets/01a1"))));
        assert!(!is_upload(&request(
            "POST",
            &format!("{p}/assets/01a1/metadata")
        )));
        assert!(!is_upload(&request(
            "POST",
            &format!("{p}/collections/assets/entries")
        )));
        assert!(!is_upload(&request("POST", "/api/v1/projects/abc/assets")));
        assert!(!is_upload(&request("POST", "/auth/signup")));
    }

    #[test]
    fn credential_segments_are_written_as_their_names() {
        let route = Some("/audience/v1/{project}/magic-link/{token}");
        assert_eq!(
            loggable_path("/audience/v1/01a1/magic-link/s3cret", route),
            "/audience/v1/01a1/magic-link/{token}"
        );
        assert_eq!(
            loggable_path("/auth/device/WXYZ-1234", Some("/auth/device/{code}")),
            "/auth/device/{code}"
        );
        // Routes without a credential keep their ids; unmatched paths have no template.
        let plain = "/api/v1/projects/01a1/environments/production";
        assert_eq!(
            loggable_path(
                plain,
                Some("/api/v1/projects/{project}/environments/{environment}")
            ),
            plain
        );
        assert_eq!(loggable_path("/nowhere/s3cret", None), "/nowhere/s3cret");
        // Unmatched, a credential's segment is still known by what precedes it: a trailing
        // slash or a wrong prefix never logs a live token.
        assert_eq!(
            loggable_path("/audience/v1/01a1/magic-link/s3cret/", None),
            "/audience/v1/01a1/magic-link/{token}/"
        );
        assert_eq!(
            loggable_path("/v1/magic-link/s3cret", None),
            "/v1/magic-link/{token}"
        );
        assert_eq!(
            loggable_path("/auth/device/WXYZ-1234/", None),
            "/auth/device/{code}/"
        );
    }

    #[test]
    fn request_id_policy() {
        let longest = "a".repeat(MAX_REQUEST_ID_LEN);
        let too_long = "a".repeat(MAX_REQUEST_ID_LEN + 1);
        let cases = [
            ("018f2f0e-3c1a-7cc0-9e1e-3f1f6b7f9a01", true),
            ("client.trace_42", true),
            (longest.as_str(), true),
            ("", false),
            ("has space", false),
            ("new\nline", false),
            ("quote\"", false),
            (too_long.as_str(), false),
        ];
        for (input, expected) in cases {
            assert_eq!(is_acceptable_request_id(input), expected, "{input:?}");
        }
    }

    #[test]
    fn generated_ids_are_uuid_v7() {
        let id = MakeRequestUuidV7
            .make_request_id(&Request::new(Body::empty()))
            .expect("an id is always generated");
        let text = id.header_value().to_str().expect("UUIDs are ASCII");
        let parsed = uuid::Uuid::parse_str(text).expect("a valid UUID");
        assert_eq!(parsed.get_version_num(), 7);
    }

    #[test]
    fn panic_payloads_of_any_type_become_500s() {
        for payload in [
            Box::new("static str") as Box<dyn Any + Send>,
            Box::new(String::from("owned")),
            Box::new(42_u32),
        ] {
            assert_eq!(
                panic_response(payload).status(),
                StatusCode::INTERNAL_SERVER_ERROR
            );
        }
    }
}
