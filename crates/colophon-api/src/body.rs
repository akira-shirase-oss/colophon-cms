//! Request-body hygiene: never close a connection silently because a body went unread.
//!
//! When a handler answers without reading the request body (a 404 for a POST, or a refusal of
//! an oversized upload), the HTTP/1.1 connection still holds the unread bytes. Closing it
//! then, without saying so, makes clients that pool connections send their next request into
//! a dead socket (ECONNRESET, EPIPE), and resets clients that are still uploading, which can
//! lose the response they were about to read (RFC 9112 §9.6). Found by the testbed on
//! 2026-10-25.
//!
//! So this layer reads and discards the unread rest of a body, within [`DRAIN_LIMIT_BYTES`]
//! and [`DRAIN_DEADLINE`], which leaves the connection usable. When that isn't possible, the
//! response carries `Connection: close` and the client knows not to reuse it.

use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use axum::{
    body::{Body, Bytes},
    extract::Request,
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use http_body::{Frame, SizeHint};
use http_body_util::BodyExt as _;
use tokio::sync::oneshot;

/// The most unread body the server will read and throw away to keep a connection usable.
pub(crate) const DRAIN_LIMIT_BYTES: u64 = 32 * 1024 * 1024;

/// How long draining may take before the server gives up and closes the connection.
pub(crate) const DRAIN_DEADLINE: Duration = Duration::from_secs(2);

/// Wraps a request body and, if it is dropped before the end, hands the rest back so the
/// middleware can drain it.
struct Tracked {
    inner: Body,
    finished: bool,
    leftover: Option<oneshot::Sender<Body>>,
}

impl http_body::Body for Tracked {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let polled = Pin::new(&mut self.inner).poll_frame(cx);
        if matches!(polled, Poll::Ready(None)) {
            self.finished = true;
        }
        polled
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        if !self.finished
            && !http_body::Body::is_end_stream(&self.inner)
            && let Some(sender) = self.leftover.take()
        {
            let rest = std::mem::replace(&mut self.inner, Body::empty());
            // Nobody listening means the middleware already finished: nothing to drain.
            let _ = sender.send(rest);
        }
    }
}

/// Reads and discards `body` within the drain bounds. True when it reached the end.
async fn drain(body: Body) -> bool {
    let read_to_end = async move {
        let mut body = body;
        let mut seen: u64 = 0;
        while let Some(frame) = body.frame().await {
            let Ok(frame) = frame else { return false };
            if let Some(data) = frame.data_ref() {
                seen = seen.saturating_add(data.len() as u64);
                if seen > DRAIN_LIMIT_BYTES {
                    return false;
                }
            }
        }
        true
    };
    tokio::time::timeout(DRAIN_DEADLINE, read_to_end)
        .await
        .unwrap_or(false)
}

fn announce_close(response: &mut Response) {
    response
        .headers_mut()
        .insert(header::CONNECTION, HeaderValue::from_static("close"));
}

fn declared_length(request: &Request) -> Option<u64> {
    request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
}

/// The middleware. `max_body_bytes` is the request body limit.
pub(crate) async fn hygiene(max_body_bytes: usize, request: Request, next: Next) -> Response {
    let declared = declared_length(&request);
    let chunked = request.headers().contains_key(header::TRANSFER_ENCODING);
    if declared.unwrap_or(0) == 0 && !chunked {
        return next.run(request).await;
    }

    // A declared body over the limit: refuse it here, before any handler, after reading what
    // the client sends (a later layer turns the bare 413 into the error envelope).
    if let Some(length) = declared
        && length > max_body_bytes as u64
    {
        let drained = length <= DRAIN_LIMIT_BYTES && drain(request.into_body()).await;
        let mut response = StatusCode::PAYLOAD_TOO_LARGE.into_response();
        if !drained {
            announce_close(&mut response);
        }
        return response;
    }

    let (sender, mut leftover) = oneshot::channel();
    let request = request.map(|inner| {
        Body::new(Tracked {
            inner,
            finished: false,
            leftover: Some(sender),
        })
    });
    let mut response = next.run(request).await;
    // `Ok` only if the body was dropped before its end. Fully read, or still owned elsewhere
    // (a task the handler spawned), means there is nothing to do; never wait for it.
    if let Ok(rest) = leftover.try_recv()
        && !drain(rest).await
    {
        announce_close(&mut response);
    }
    response
}
