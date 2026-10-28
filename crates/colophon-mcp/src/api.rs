//! The Colophon HTTP API, as an agent calls it: the same routes the Studio and the CLI use, with
//! the project API token as the only credential (docs/modules/04-api.md). Nothing here decides
//! what the agent may do; the API does, and its refusals come back intact.
//!
//! Every request is bounded: a connect timeout, a deadline for each attempt that covers reading
//! the whole answer, an overall deadline for the call, and a size limit on the answer. A request
//! that gets no answer, or a 503, is tried again, at most [`ATTEMPTS`] times within the deadline.
//! That is safe because every request this crate sends is safe to repeat: reads, writes that carry
//! an `Idempotency-Key` (the API replays the recorded answer), and asking for review, which is a
//! no-op once asked.

use std::time::Duration;

use reqwest::{Method, StatusCode, Url, header};
use serde_json::{Map, Value};
use tokio::time::{Instant, sleep, timeout_at};
use uuid::Uuid;

use crate::config::{Config, Secret};

/// The environment every tool works in.
pub const ENVIRONMENT: &str = "production";
/// The most bytes of an answer read; a larger one is refused rather than held.
pub const MAX_ANSWER_BYTES: usize = 4 * 1024 * 1024;
/// How long one tool call may spend on the API, retries included.
pub const CALL_DEADLINE: Duration = Duration::from_secs(30);
/// How long one attempt may take, from connecting to the last byte of the answer.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(15);
/// How long connecting may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Attempts per request, the first included.
const ATTEMPTS: usize = 3;
/// Waits between attempts.
const BACKOFF: [Duration; ATTEMPTS - 1] = [Duration::from_millis(250), Duration::from_secs(1)];

/// Why a request didn't succeed.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The API answered with an error envelope (or, rarely, a status without one).
    #[error("refused with HTTP {}", .0.status)]
    Refused(Box<Refusal>),
    /// No connection could be made.
    #[error("could not reach the Colophon server: {0}")]
    Unreachable(String),
    /// No complete answer before the deadline. A write may or may not have happened.
    #[error("no answer within {0:?}")]
    TimedOut(Duration),
    /// The answer was larger than [`MAX_ANSWER_BYTES`]; nothing of it is used.
    #[error("the answer was larger than {limit} bytes")]
    TooLarge {
        /// The limit.
        limit: usize,
    },
    /// A success whose body isn't the JSON the API documents.
    #[error("the answer was not the JSON the API documents")]
    BadAnswer,
    /// The request's URL could not be built (its parts are validated first, so this is a bug).
    #[error("could not build the request's URL")]
    BadUrl,
}

impl ApiError {
    /// The API's refusal with this status, if that is what this is.
    pub fn refused_with(&self, status: u16) -> bool {
        matches!(self, Self::Refused(refusal) if refusal.status == status)
    }

    /// Whether trying again could help, and is safe for the requests this crate sends.
    fn retryable(&self) -> bool {
        match self {
            Self::Unreachable(_) | Self::TimedOut(_) => true,
            Self::Refused(refusal) => refusal.status == 503,
            Self::TooLarge { .. } | Self::BadAnswer | Self::BadUrl => false,
        }
    }
}

/// An error answer from the API: its envelope, `{"error": {"code", "message", "request_id",
/// "details": {"reason", "pointer", "permission", …}}}`, plus the `ETag` a stale-version conflict
/// names the current version in.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    /// The HTTP status.
    pub status: u16,
    /// The stable error code, such as `forbidden` or `conflict`.
    pub code: String,
    /// The API's message, safe to show.
    pub message: String,
    /// `details.reason`: the stable reason clients act on, such as `missing_permission`.
    pub reason: Option<String>,
    /// `details.pointer`: where in the request body, as a JSON Pointer; `None` when empty.
    pub pointer: Option<String>,
    /// `details.permission`: the permission the token lacks, for `missing_permission`.
    pub permission: Option<String>,
    /// `details` as the API sent it (entries in the way, candidates, …).
    pub details: Value,
    /// The current version a conflict names in its `ETag`, without the quotes.
    pub current: Option<String>,
    /// The request id, to quote to an operator.
    pub request_id: Option<String>,
}

impl Refusal {
    /// Reads an error answer. A body that isn't the envelope still gives a refusal: the status,
    /// a code for it and a plain message.
    pub fn from_answer(status: StatusCode, etag: Option<&str>, body: &[u8]) -> Self {
        let envelope: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
        let error = &envelope["error"];
        let text = |value: &Value| {
            value
                .as_str()
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        let details = &error["details"];
        Self {
            status: status.as_u16(),
            code: text(&error["code"]).unwrap_or_else(|| {
                status
                    .canonical_reason()
                    .unwrap_or("error")
                    .to_ascii_lowercase()
                    .replace(' ', "_")
            }),
            message: text(&error["message"])
                .unwrap_or_else(|| format!("the server answered HTTP {}", status.as_u16())),
            reason: text(&details["reason"]),
            pointer: text(&details["pointer"]),
            permission: text(&details["permission"]),
            details: if details.is_object() {
                details.clone()
            } else {
                Value::Null
            },
            current: etag.and_then(|etag| {
                let inner = etag.trim().strip_prefix('"')?.strip_suffix('"')?;
                (!inner.is_empty()).then(|| inner.to_owned())
            }),
            request_id: text(&error["request_id"]),
        }
    }
}

/// A successful answer.
#[derive(Debug)]
pub struct Answer {
    /// The JSON body.
    pub body: Value,
    /// Whether the API replayed the recorded answer to an earlier request with the same key.
    pub replayed: bool,
}

/// One request: a method, a path under `{P}`, and what goes with it.
#[derive(Debug)]
pub struct Request<'a> {
    method: Method,
    path: String,
    query: Vec<(&'static str, &'a str)>,
    body: Option<&'a Value>,
    if_match: Option<String>,
    idempotency_key: Option<&'a str>,
}

impl<'a> Request<'a> {
    /// A `GET` of `path`, relative to `{P}` (such as `/content-types`).
    pub fn get(path: impl Into<String>) -> Self {
        Self::new(Method::GET, path)
    }

    /// Any method on `path`, relative to `{P}`.
    pub fn new(method: Method, path: impl Into<String>) -> Self {
        Self {
            method,
            path: path.into(),
            query: Vec::new(),
            body: None,
            if_match: None,
            idempotency_key: None,
        }
    }

    /// Adds a query parameter.
    #[must_use]
    pub fn query(mut self, name: &'static str, value: &'a str) -> Self {
        self.query.push((name, value));
        self
    }

    /// Sends `body` as JSON.
    #[must_use]
    pub fn json(mut self, body: &'a Value) -> Self {
        self.body = Some(body);
        self
    }

    /// Sends `If-Match` with `tag` as one strong `ETag`.
    #[must_use]
    pub fn if_match(mut self, tag: &str) -> Self {
        self.if_match = Some(format!("\"{tag}\""));
        self
    }

    /// Sends `Idempotency-Key`.
    #[must_use]
    pub fn idempotency_key(mut self, key: &'a str) -> Self {
        self.idempotency_key = Some(key);
        self
    }
}

/// A fresh idempotency key: 36 characters, within the API's 16 to 128 of `A-Z a-z 0-9 - _`.
pub fn new_idempotency_key() -> String {
    format!("mcp-{}", Uuid::now_v7().simple())
}

/// The API, as one token in one project's production environment sees it.
#[derive(Debug, Clone)]
pub struct Api {
    http: reqwest::Client,
    base: Url,
    token: Secret,
    project: Uuid,
}

impl Api {
    /// One client for the process: connection pooling, no redirects (a redirect could carry the
    /// token elsewhere).
    pub fn new(config: &Config) -> Result<Self, ApiError> {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(ATTEMPT_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("colophon-mcp/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| ApiError::Unreachable(error.to_string()))?;
        Ok(Self {
            http,
            base: config.server.clone(),
            token: config.token.clone(),
            project: config.project,
        })
    }

    /// The project this API serves.
    pub fn project(&self) -> Uuid {
        self.project
    }

    /// Sends `request`, trying again when that is safe, within [`CALL_DEADLINE`].
    pub async fn send(&self, request: &Request<'_>) -> Result<Answer, ApiError> {
        let deadline = Instant::now() + CALL_DEADLINE;
        let mut attempt = 0;
        loop {
            let error = match self.attempt(request, deadline).await {
                Ok(answer) => return Ok(answer),
                Err(error) => error,
            };
            let wait = BACKOFF.get(attempt).copied();
            match wait {
                Some(wait) if error.retryable() && Instant::now() + wait < deadline => {
                    tracing::debug!(%error, attempt, "trying again");
                    sleep(wait).await;
                    attempt += 1;
                }
                _ => return Err(error),
            }
        }
    }

    /// One attempt, bounded by its own timeout and by `deadline`.
    async fn attempt(&self, request: &Request<'_>, deadline: Instant) -> Result<Answer, ApiError> {
        let mut url = self
            .base
            .join(&format!(
                "/api/v1/projects/{}/environments/{ENVIRONMENT}{}",
                self.project, request.path
            ))
            .map_err(|_| ApiError::BadUrl)?;
        if !request.query.is_empty() {
            url.query_pairs_mut().extend_pairs(&request.query);
        }
        let mut builder = self
            .http
            .request(request.method.clone(), url)
            .bearer_auth(self.token.expose());
        if let Some(tag) = &request.if_match {
            builder = builder.header(header::IF_MATCH, tag);
        }
        if let Some(key) = request.idempotency_key {
            builder = builder.header("idempotency-key", key);
        }
        if let Some(body) = request.body {
            builder = builder.json(body);
        }
        let limit = deadline.min(Instant::now() + ATTEMPT_TIMEOUT);
        let exchange = async {
            let response = builder.send().await.map_err(transport)?;
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
            let body = read_bounded(response, MAX_ANSWER_BYTES).await?;
            Ok::<_, ApiError>((status, etag, replayed, body))
        };
        let (status, etag, replayed, body) = timeout_at(limit, exchange)
            .await
            .map_err(|_| ApiError::TimedOut(ATTEMPT_TIMEOUT))??;
        if status.is_success() {
            let body = serde_json::from_slice(&body).map_err(|_| ApiError::BadAnswer)?;
            Ok(Answer { body, replayed })
        } else {
            Err(ApiError::Refused(Box::new(Refusal::from_answer(
                status,
                etag.as_deref(),
                &body,
            ))))
        }
    }
}

/// A transport failure: a timeout, or no connection.
fn transport(error: reqwest::Error) -> ApiError {
    if error.is_timeout() {
        ApiError::TimedOut(ATTEMPT_TIMEOUT)
    } else {
        ApiError::Unreachable(error.without_url().to_string())
    }
}

/// Reads the answer, refusing one larger than `limit` before holding more than that.
async fn read_bounded(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>, ApiError> {
    if response
        .content_length()
        .is_some_and(|length| length > u64::try_from(limit).unwrap_or(u64::MAX))
    {
        return Err(ApiError::TooLarge { limit });
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport)? {
        if body.len() + chunk.len() > limit {
            return Err(ApiError::TooLarge { limit });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// The `data` of an answer, which every success has.
pub fn data(answer: &Answer) -> Result<&Value, ApiError> {
    answer.body.get("data").ok_or(ApiError::BadAnswer)
}

/// Builds a JSON object from pairs; for tool results.
pub fn object(pairs: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Object(
        pairs
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect::<Map<_, _>>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_envelope_gives_its_reason_pointer_and_permission() {
        let body = br#"{"error":{"code":"forbidden","message":"your role can't create entries here","request_id":"r-1","details":{"reason":"missing_permission","pointer":"","permission":"content.create"}}}"#;
        let refusal = Refusal::from_answer(StatusCode::FORBIDDEN, None, body);
        assert_eq!(refusal.status, 403);
        assert_eq!(refusal.code, "forbidden");
        assert_eq!(refusal.reason.as_deref(), Some("missing_permission"));
        assert_eq!(
            refusal.pointer, None,
            "an empty pointer means the whole body"
        );
        assert_eq!(refusal.permission.as_deref(), Some("content.create"));
        assert_eq!(refusal.request_id.as_deref(), Some("r-1"));
    }

    #[test]
    fn a_stale_revision_names_the_current_one_from_its_etag() {
        let body = br#"{"error":{"code":"conflict","message":"the draft has changed","request_id":"r","details":{"reason":"stale_revision","pointer":""}}}"#;
        let refusal = Refusal::from_answer(
            StatusCode::CONFLICT,
            Some("\"0190f0c2-7a4e-7cc1-9d1e-2b8a4c6f1a01\""),
            body,
        );
        assert_eq!(refusal.reason.as_deref(), Some("stale_revision"));
        assert_eq!(
            refusal.current.as_deref(),
            Some("0190f0c2-7a4e-7cc1-9d1e-2b8a4c6f1a01")
        );
    }

    #[test]
    fn a_body_without_the_envelope_still_gives_a_refusal() {
        let refusal = Refusal::from_answer(StatusCode::BAD_GATEWAY, None, b"<html>oops</html>");
        assert_eq!(refusal.status, 502);
        assert_eq!(refusal.code, "bad_gateway");
        assert_eq!(refusal.message, "the server answered HTTP 502");
        assert_eq!(refusal.reason, None);
        assert_eq!(refusal.details, Value::Null);
    }

    #[test]
    fn only_failures_that_can_pass_are_tried_again() {
        let refused = |status| {
            ApiError::Refused(Box::new(Refusal::from_answer(
                StatusCode::from_u16(status).expect("status"),
                None,
                b"",
            )))
        };
        assert!(ApiError::Unreachable("refused".into()).retryable());
        assert!(ApiError::TimedOut(ATTEMPT_TIMEOUT).retryable());
        assert!(refused(503).retryable());
        for status in [400, 401, 403, 404, 409, 422, 428, 429, 500] {
            assert!(!refused(status).retryable(), "{status}");
        }
        assert!(!ApiError::TooLarge { limit: 1 }.retryable());
    }

    #[test]
    fn idempotency_keys_fit_the_api_s_rule() {
        let key = new_idempotency_key();
        assert!((16..=128).contains(&key.len()), "{key}");
        assert!(
            key.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        );
        assert_ne!(key, new_idempotency_key());
    }
}
