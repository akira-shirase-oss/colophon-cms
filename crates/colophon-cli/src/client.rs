//! Talking to a Colophon server as a signed-in person: one HTTP client with timeouts, the
//! bearer token from the credentials file, and every outcome mapped to the CLI contract's exit
//! codes (docs/modules/08-cli.md).

use std::time::Duration;

use reqwest::{Method, StatusCode};
use serde_json::Value;

use crate::Exit;

/// Every request gives up after this long.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Why a request didn't succeed, with what to tell the person.
#[derive(Debug)]
pub(crate) struct Failure {
    pub(crate) exit: Exit,
    pub(crate) message: String,
}

impl Failure {
    pub(crate) fn new(exit: Exit, message: impl Into<String>) -> Self {
        Self {
            exit,
            message: message.into(),
        }
    }
}

/// A successful reply: headers and body.
pub(crate) struct Reply {
    pub(crate) headers: reqwest::header::HeaderMap,
    pub(crate) body: Value,
}

/// A server and, once signed in, a token.
pub(crate) struct Client {
    http: reqwest::Client,
    base: reqwest::Url,
    token: Option<String>,
}

impl Client {
    pub(crate) fn new(server: &str, token: Option<String>) -> Result<Self, Failure> {
        let base = reqwest::Url::parse(server)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .ok_or_else(|| {
                Failure::new(Exit::Usage, format!("{server:?} is not an http(s) URL"))
            })?;
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| {
                Failure::new(
                    Exit::Internal,
                    format!("could not build an HTTP client: {error}"),
                )
            })?;
        Ok(Self { http, base, token })
    }

    /// Sends a request. A non-success status becomes a [`Failure`] with the server's message
    /// and the matching exit code; so does no answer at all (exit 5, safe to retry).
    pub(crate) async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
        headers: &[(&str, &str)],
    ) -> Result<Reply, Failure> {
        let url = self
            .base
            .join(path)
            .map_err(|error| Failure::new(Exit::Usage, format!("bad path {path:?}: {error}")))?;
        let mut request = self.http.request(method, url);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(|error| {
            Failure::new(
                Exit::Unavailable,
                if error.is_timeout() {
                    format!("no answer from {} within {REQUEST_TIMEOUT:?}", self.base)
                } else {
                    format!("could not reach {}: {error}", self.base)
                },
            )
        })?;
        let status = response.status();
        let headers = response.headers().clone();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(Reply { headers, body });
        }
        let message = body["error"]["message"]
            .as_str()
            .unwrap_or("request failed")
            .to_owned();
        let detail = body["error"]["details"]["pointer"]
            .as_str()
            .filter(|pointer| !pointer.is_empty())
            .map(|pointer| format!(" (at {pointer})"))
            .unwrap_or_default();
        Err(Failure::new(
            exit_for(status),
            format!("{message}{detail} [HTTP {}]", status.as_u16()),
        ))
    }
}

/// The CLI contract's exit code for an HTTP status.
pub(crate) fn exit_for(status: StatusCode) -> Exit {
    match status.as_u16() {
        401 | 403 | 404 => Exit::Denied,
        409 => Exit::Conflict,
        429 | 500..=599 => Exit::Unavailable,
        400..=499 => Exit::Usage,
        _ => Exit::Internal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_to_the_contract_s_exit_codes() {
        for (status, exit) in [
            (401, Exit::Denied),
            (403, Exit::Denied),
            (404, Exit::Denied),
            (409, Exit::Conflict),
            (422, Exit::Usage),
            (428, Exit::Usage),
            (429, Exit::Unavailable),
            (503, Exit::Unavailable),
            (302, Exit::Internal),
        ] {
            assert_eq!(
                exit_for(StatusCode::from_u16(status).expect("status")),
                exit,
                "{status}"
            );
        }
    }
}
