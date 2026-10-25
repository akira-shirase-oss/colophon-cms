//! The one Tower stack every request passes through (`docs/modules/04-api.md`, "Server
//! shape"). `ServiceBuilder` lists layers outermost first: a request travels down the list,
//! its response travels back up.

use std::{any::Any, time::Duration};

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
    timeout::TimeoutLayer,
    trace::{DefaultOnResponse, TraceLayer},
};
use tracing::{Level, Span};

use crate::{
    body,
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

/// The root span of every request. Everything logged while handling the request inherits
/// these fields. `route` is the matched template (low cardinality); `path` omits the query
/// string, which can carry tokens.
fn make_span(request: &Request<Body>) -> Span {
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-");
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("<unmatched>", MatchedPath::as_str);
    tracing::info_span!(
        "request",
        method = %request.method(),
        route,
        path = %request.uri().path(),
        request_id = %request_id,
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

/// Applies the global stack to a router. Read it top to bottom as the path of a request.
pub fn apply(router: Router, limits: HttpLimits) -> Router {
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
                .make_span_with(make_span)
                .on_response(
                    DefaultOnResponse::new()
                        .level(Level::DEBUG)
                        .latency_unit(LatencyUnit::Micros),
                )
                .on_failure(()),
        )
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
        // 7. The deadline for producing response headers.
        .layer(TimeoutLayer::with_status_code(
            TIMEOUT_STATUS,
            limits.request_timeout,
        ))
        // 8. Reject bodies that grow past the limit while streaming (no Content-Length)...
        .layer(RequestBodyLimitLayer::new(limits.max_body_bytes))
        // ...and give axum's extractors, whose own default is 2 MB, the same limit.
        .layer(DefaultBodyLimit::max(limits.max_body_bytes));
    router.layer(stack)
}

#[cfg(test)]
mod tests {
    use super::*;

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
