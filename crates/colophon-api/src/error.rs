//! One error type and one JSON envelope for every error Colophon's HTTP server emits,
//! including errors produced by middleware it does not control:
//!
//! ```json
//! { "error": { "code": "route_not_found", "message": "no route matches this path", "request_id": "…" } }
//! ```
//!
//! Clients branch on `code`, never on `message`. `request_id` matches the `x-request-id`
//! response header, so a user can quote one string and the logs find the request.

use std::error::Error as StdError;

use axum::{
    Json,
    extract::rejection::JsonRejection,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use colophon_db::DbError;
use colophon_domain::{entry::ArticleError, rich_text::Rejection};
use serde::{Serialize, Serializer};
use uuid::Uuid;

/// Stable, machine-readable error codes. Renaming one is a breaking API change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorCode {
    /// A malformed request that no more specific code describes.
    BadRequest,
    /// The body isn't valid JSON.
    MalformedJson,
    /// The body is well-formed but breaks a rule; `details` says which, and where.
    ValidationFailed,
    /// Not signed in, or the session ended.
    Unauthorized,
    /// Signed in, but not allowed (for example, a write without a valid CSRF token).
    Forbidden,
    /// No such resource, or one the caller may not see: the two look the same.
    NotFound,
    /// The request conflicts with the current state (for example, a taken slug, or a save on a
    /// revision that is no longer the draft); `details.reason` says which.
    Conflict,
    /// A required precondition header (`If-Match`, `Idempotency-Key`) is missing.
    PreconditionRequired,
    /// Sign-in failed, was cancelled, or its state expired or was already used.
    SignInFailed,
    /// No route matches the path.
    RouteNotFound,
    /// The path exists, but not for this method.
    MethodNotAllowed,
    /// The request body is larger than the configured limit.
    PayloadTooLarge,
    /// The request's `Content-Type` is not accepted here.
    UnsupportedMediaType,
    /// The server could not produce a response before the request deadline.
    DeadlineExceeded,
    /// The server is temporarily unable to handle the request.
    Unavailable,
    /// A bug or an unexpected failure. Details are logged, never returned.
    Internal,
}

impl ErrorCode {
    /// The wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BadRequest => "bad_request",
            Self::MalformedJson => "malformed_json",
            Self::ValidationFailed => "validation_failed",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::PreconditionRequired => "precondition_required",
            Self::SignInFailed => "sign_in_failed",
            Self::RouteNotFound => "route_not_found",
            Self::MethodNotAllowed => "method_not_allowed",
            Self::PayloadTooLarge => "payload_too_large",
            Self::UnsupportedMediaType => "unsupported_media_type",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::Unavailable => "unavailable",
            Self::Internal => "internal",
        }
    }

    /// The best code for an error response that code we don't control produced: axum's 405,
    /// tower-http's 413 and timeout responses.
    pub fn from_status(status: StatusCode) -> Self {
        match status {
            StatusCode::NOT_FOUND => Self::RouteNotFound,
            StatusCode::UNAUTHORIZED => Self::Unauthorized,
            StatusCode::FORBIDDEN => Self::Forbidden,
            StatusCode::CONFLICT => Self::Conflict,
            StatusCode::PRECONDITION_REQUIRED => Self::PreconditionRequired,
            StatusCode::METHOD_NOT_ALLOWED => Self::MethodNotAllowed,
            StatusCode::PAYLOAD_TOO_LARGE => Self::PayloadTooLarge,
            StatusCode::UNSUPPORTED_MEDIA_TYPE => Self::UnsupportedMediaType,
            StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => Self::DeadlineExceeded,
            StatusCode::SERVICE_UNAVAILABLE => Self::Unavailable,
            status if status.is_client_error() => Self::BadRequest,
            _ => Self::Internal,
        }
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Machine-readable detail for an error about part of the request body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorDetails {
    /// A code specific to the failure, such as a rich-text rejection code (`unsafe_link`).
    pub reason: &'static str,
    /// RFC 6901 JSON Pointer into the request body; empty means the whole body.
    pub pointer: String,
}

/// The part of an error a client may see. Stored in the response extensions so the edge
/// middleware can render it together with the request id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicError {
    /// Stable code.
    pub code: ErrorCode,
    /// Safe, human-readable message.
    pub message: String,
    /// Optional detail, for errors about part of the body.
    pub details: Option<ErrorDetails>,
}

impl PublicError {
    /// A generic public error for a status code.
    pub fn from_status(status: StatusCode) -> Self {
        Self {
            code: ErrorCode::from_status(status),
            message: status
                .canonical_reason()
                .unwrap_or("request failed")
                .to_ascii_lowercase(),
            details: None,
        }
    }
}

/// Every way a request can fail. `Display` is the public message; the private cause of an
/// `Internal` error is reachable only through `source()`, and is logged, never returned.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// No route matches the path.
    #[error("no route matches this path")]
    RouteNotFound,
    /// The body isn't valid JSON. The detail is the parser's message (a position in the
    /// client's own input), truncated.
    #[error("the request body is not valid JSON: {0}")]
    MalformedJson(String),
    /// The body needs `Content-Type: application/json`.
    #[error("expected `Content-Type: application/json`")]
    UnsupportedMediaType,
    /// The body is larger than the limit.
    #[error("request body exceeds the configured limit")]
    PayloadTooLarge,
    /// The body couldn't be read.
    #[error("the request body could not be read")]
    BadRequest,
    /// A rich-text document was refused.
    #[error("{0}")]
    InvalidRichText(Rejection),
    /// An article was refused.
    #[error("{0}")]
    InvalidArticle(ArticleError),
    /// A header is malformed; the reason is safe to show.
    #[error("{0}")]
    InvalidHeader(&'static str),
    /// A required precondition header is missing; the message names it.
    #[error("{0}")]
    PreconditionRequired(&'static str),
    /// A save named a revision that is no longer the draft. The response's `ETag` names the
    /// current one.
    #[error("the draft has changed since that revision; load the current one and try again")]
    StaleRevision {
        /// The revision the draft points at now.
        current: Uuid,
    },
    /// An `Idempotency-Key` was used before for a different request.
    #[error("this Idempotency-Key was already used for a different request")]
    IdempotencyKeyReused,
    /// A field of the request body was refused.
    #[error("{message}")]
    InvalidField {
        /// A stable code for the problem.
        reason: &'static str,
        /// JSON Pointer to the field.
        pointer: &'static str,
        /// What is wrong, for people.
        message: String,
    },
    /// Not signed in, or the session ended.
    #[error("sign in first")]
    Unauthorized,
    /// Not allowed; the reason is safe to show.
    #[error("{0}")]
    Forbidden(&'static str),
    /// No such resource (or one the caller may not see).
    #[error("no such resource")]
    NotFound,
    /// A conflict with the current state; the reason is safe to show.
    #[error("{0}")]
    Conflict(&'static str),
    /// Temporarily unable to serve; the reason is safe to show.
    #[error("{0}")]
    Unavailable(&'static str),
    /// At capacity: no database connection became free in time, and nothing was attempted.
    /// 503 with `Retry-After`, so clients back off instead of piling on.
    #[error("the server is busy; try again shortly")]
    Busy,
    /// Sign-in failed. Details are logged, never shown: they can help an attacker.
    #[error("sign-in failed or expired; start again")]
    SignInFailed,
    /// An unexpected failure; the boxed error is the private cause.
    #[error("internal error")]
    Internal(#[source] Box<dyn StdError + Send + Sync>),
}

impl ApiError {
    /// Wraps any error as an internal failure.
    pub fn internal(cause: impl Into<Box<dyn StdError + Send + Sync>>) -> Self {
        Self::Internal(cause.into())
    }

    /// A database error: an exhausted pool is [`Self::Busy`]; anything else is internal.
    /// Callers that expect a particular error, such as a conflict, match it first.
    pub fn from_db(error: DbError) -> Self {
        match error {
            DbError::Busy => Self::Busy,
            other => Self::internal(other),
        }
    }

    /// HTTP status for this error. Total over the variants, so every new variant must choose.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::RouteNotFound | Self::NotFound => StatusCode::NOT_FOUND,
            Self::MalformedJson(_) | Self::BadRequest | Self::SignInFailed => {
                StatusCode::BAD_REQUEST
            }
            Self::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::InvalidRichText(_) | Self::InvalidArticle(_) | Self::InvalidField { .. } => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            Self::InvalidHeader(_) => StatusCode::BAD_REQUEST,
            Self::PreconditionRequired(_) => StatusCode::PRECONDITION_REQUIRED,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::Conflict(_) | Self::StaleRevision { .. } | Self::IdempotencyKeyReused => {
                StatusCode::CONFLICT
            }
            Self::Unavailable(_) | Self::Busy => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Stable machine-readable code.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::RouteNotFound => ErrorCode::RouteNotFound,
            Self::MalformedJson(_) => ErrorCode::MalformedJson,
            Self::UnsupportedMediaType => ErrorCode::UnsupportedMediaType,
            Self::PayloadTooLarge => ErrorCode::PayloadTooLarge,
            Self::BadRequest | Self::InvalidHeader(_) => ErrorCode::BadRequest,
            Self::InvalidRichText(_) | Self::InvalidArticle(_) | Self::InvalidField { .. } => {
                ErrorCode::ValidationFailed
            }
            Self::PreconditionRequired(_) => ErrorCode::PreconditionRequired,
            Self::Unauthorized => ErrorCode::Unauthorized,
            Self::Forbidden(_) => ErrorCode::Forbidden,
            Self::NotFound => ErrorCode::NotFound,
            Self::Conflict(_) | Self::StaleRevision { .. } | Self::IdempotencyKeyReused => {
                ErrorCode::Conflict
            }
            Self::Unavailable(_) | Self::Busy => ErrorCode::Unavailable,
            Self::SignInFailed => ErrorCode::SignInFailed,
            Self::Internal(_) => ErrorCode::Internal,
        }
    }

    /// What the client sees.
    pub fn public(&self) -> PublicError {
        let details = match self {
            Self::InvalidRichText(rejection) => Some(ErrorDetails {
                reason: rejection.code.as_str(),
                pointer: rejection.pointer.clone(),
            }),
            Self::InvalidArticle(error) => Some(ErrorDetails {
                reason: error.code(),
                pointer: error.pointer(),
            }),
            Self::StaleRevision { .. } => Some(ErrorDetails {
                reason: "stale_revision",
                pointer: String::new(),
            }),
            Self::IdempotencyKeyReused => Some(ErrorDetails {
                reason: "idempotency_key_reused",
                pointer: String::new(),
            }),
            Self::InvalidField {
                reason, pointer, ..
            } => Some(ErrorDetails {
                reason,
                pointer: (*pointer).to_owned(),
            }),
            _ => None,
        };
        PublicError {
            code: self.code(),
            message: self.to_string(),
            details,
        }
    }

    /// Logs the error once, at a level that matches who is at fault.
    fn log(&self) {
        let code = self.code().as_str();
        if let Self::Internal(cause) = self {
            let cause = error_chain(cause.as_ref());
            tracing::error!(code, %cause, "request failed");
        } else if matches!(self, Self::Busy) {
            // Operators need to see saturation. Each of these waited out the acquire timeout,
            // so their rate is bounded by in-flight requests over that timeout.
            tracing::warn!(code, "request refused: no database connection free");
        } else {
            // A client's mistake: worth a debug line, never a page.
            tracing::debug!(code, error = %self, "request rejected");
        }
    }
}

/// `outer: inner: innermost`, the way `anyhow`'s `{:#}` prints a chain.
fn error_chain(error: &(dyn StdError + 'static)) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// Seconds a client should wait before retrying a [`ApiError::Busy`] request.
pub const BUSY_RETRY_AFTER_SECONDS: u32 = 1;

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        self.log();
        let mut response = render(self.status(), &self.public(), None);
        match self {
            Self::Busy => {
                response.headers_mut().insert(
                    header::RETRY_AFTER,
                    HeaderValue::from(BUSY_RETRY_AFTER_SECONDS),
                );
            }
            Self::StaleRevision { current } => {
                if let Ok(etag) = HeaderValue::from_str(&format!("\"{current}\"")) {
                    response.headers_mut().insert(header::ETAG, etag);
                }
            }
            _ => {}
        }
        response
    }
}

#[derive(Serialize)]
struct Envelope<'a> {
    error: EnvelopeBody<'a>,
}

#[derive(Serialize)]
struct EnvelopeBody<'a> {
    code: ErrorCode,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<&'a ErrorDetails>,
    request_id: Option<&'a str>,
}

/// Renders the envelope. The only function that knows its shape.
pub fn render(status: StatusCode, error: &PublicError, request_id: Option<&str>) -> Response {
    let envelope = Envelope {
        error: EnvelopeBody {
            code: error.code,
            message: &error.message,
            details: error.details.as_ref(),
            request_id,
        },
    };
    let mut response = (status, Json(envelope)).into_response();
    // `IntoResponse` cannot see the request, so the id is usually unknown here; the edge
    // middleware finds this extension and renders again once it knows the id.
    response.extensions_mut().insert(error.clone());
    response
}

/// True when the response already declares a JSON body.
pub fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"))
}

/// The longest client-supplied detail echoed back in a message.
const MAX_DETAIL_CHARS: usize = 200;

fn truncate(text: &str) -> String {
    match text.char_indices().nth(MAX_DETAIL_CHARS) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text.to_owned(),
    }
}

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        match rejection {
            JsonRejection::JsonSyntaxError(_) | JsonRejection::JsonDataError(_) => {
                Self::MalformedJson(truncate(&rejection.body_text()))
            }
            JsonRejection::MissingJsonContentType(_) => Self::UnsupportedMediaType,
            other if other.status() == StatusCode::PAYLOAD_TOO_LARGE => Self::PayloadTooLarge,
            _ => Self::BadRequest,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    #[test]
    fn internal_errors_never_show_their_cause() {
        let cause = io::Error::other("password authentication failed for user colophon");
        let error = ApiError::internal(cause);
        assert_eq!(error.to_string(), "internal error");
        assert_eq!(error.public().message, "internal error");
        assert!(
            StdError::source(&error).is_some(),
            "the cause is kept for the logs"
        );
    }

    #[test]
    fn statuses_and_codes_are_stable() {
        assert_eq!(ApiError::RouteNotFound.status(), StatusCode::NOT_FOUND);
        assert_eq!(ApiError::RouteNotFound.code().as_str(), "route_not_found");
        let internal = ApiError::internal(io::Error::other("x"));
        assert_eq!(internal.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(internal.code().as_str(), "internal");
    }

    #[test]
    fn foreign_statuses_map_to_codes() {
        let cases = [
            (StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed"),
            (StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large"),
            (StatusCode::GATEWAY_TIMEOUT, "deadline_exceeded"),
            (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
            (StatusCode::IM_A_TEAPOT, "bad_request"),
            (StatusCode::BAD_GATEWAY, "internal"),
        ];
        for (status, code) in cases {
            assert_eq!(ErrorCode::from_status(status).as_str(), code, "{status}");
        }
    }

    #[test]
    fn error_chains_print_every_cause() {
        #[derive(Debug, thiserror::Error)]
        #[error("loading revision")]
        struct Outer(#[source] io::Error);
        let chain = error_chain(&Outer(io::Error::other("disk full")));
        assert_eq!(chain, "loading revision: disk full");
    }
}
