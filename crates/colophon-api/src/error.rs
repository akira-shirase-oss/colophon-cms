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
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use colophon_domain::rich_text::Rejection;
use serde::{Serialize, Serializer};

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
    /// An unexpected failure; the boxed error is the private cause.
    #[error("internal error")]
    Internal(#[source] Box<dyn StdError + Send + Sync>),
}

impl ApiError {
    /// Wraps any error as an internal failure.
    pub fn internal(cause: impl Into<Box<dyn StdError + Send + Sync>>) -> Self {
        Self::Internal(cause.into())
    }

    /// HTTP status for this error. Total over the variants, so every new variant must choose.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::RouteNotFound => StatusCode::NOT_FOUND,
            Self::MalformedJson(_) | Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::InvalidRichText(_) => StatusCode::UNPROCESSABLE_ENTITY,
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
            Self::BadRequest => ErrorCode::BadRequest,
            Self::InvalidRichText(_) => ErrorCode::ValidationFailed,
            Self::Internal(_) => ErrorCode::Internal,
        }
    }

    /// What the client sees.
    pub fn public(&self) -> PublicError {
        let details = if let Self::InvalidRichText(rejection) = self {
            Some(ErrorDetails {
                reason: rejection.code.as_str(),
                pointer: rejection.pointer.clone(),
            })
        } else {
            None
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

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        self.log();
        render(self.status(), &self.public(), None)
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
