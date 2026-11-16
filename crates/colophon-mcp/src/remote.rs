//! One MCP session (protocol 2025-11-25) on the server itself, behind the Streamable HTTP
//! endpoint `{P}/mcp` (D-121, docs/modules/09-automation.md).
//!
//! Where `server` serves one process's stdin and stdout, [`RemoteSession`] serves one HTTP
//! session: each POST carries one JSON-RPC message and gets its response — or a `202` when the
//! message is a notification or a stray response, or a `400` when it isn't acceptable
//! JSON-RPC. The HTTP layer (colophon-api's `mcp` module) owns sessions, authorization and the
//! wire; this owns what a session does with a message.
//!
//! The differences from the stdio session are only the transport's:
//!
//! - **Concurrency:** POSTs on one session run concurrently, so `initialized` is atomic and
//!   `running` sits behind a lock. The same [`MAX_IN_FLIGHT`] backpressure applies: a request
//!   that finds no free slot waits for one.
//! - **Cancellation:** there is no spawned task to abort — a request's work runs inside its
//!   own POST's future, selected against a `oneshot` that `notifications/cancelled` (or the
//!   session's end) signals. Dropping the work mid-flight unwinds it; a write it started may
//!   still happen, and retrying with its idempotency key is safe, exactly as on stdio.
//! - **Replies:** the response to a request travels back on its own POST rather than through a
//!   shared writer. The server never initiates messages, so the GET stream has nothing but
//!   keep-alives to carry — that is the transport's affair, not this module's.
//!
//! Everything else — the `initialize` handshake, `ping`, `tools/list`, `tools/call`, the
//! refusal codes — is literally `server`'s code, shared, so the two transports cannot drift.

use std::{
    collections::HashMap,
    sync::{
        Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

use serde_json::{Map, Value, json};
use tokio::sync::{Semaphore, oneshot};

use crate::{
    api::Backend,
    jsonrpc::{
        self, INTERNAL_ERROR, INVALID_PARAMS, INVALID_REQUEST, Incoming, MAX_MESSAGE_BYTES,
        METHOD_NOT_FOUND, NOT_INITIALIZED, Notification, REQUEST_CANCELLED, Request, RequestId,
    },
    server::{MAX_IN_FLIGHT, Work, handshake, tool_call},
};

/// What one `POST`ed message produced: the HTTP answer's status and body in one.
pub enum Reply {
    /// A JSON-RPC response message: `200 application/json`.
    Respond(String),
    /// A notification or a response, accepted with nothing to answer: `202`, no body.
    Accepted,
    /// Input that isn't acceptable JSON-RPC: `400`, carrying the JSON-RPC error when there is
    /// one (a broken notification has no id to answer to, so it carries none).
    Rejected(Option<String>),
}

/// The in-flight requests, behind the lock so `notifications/cancelled` reaches them.
type Running = Mutex<HashMap<RequestId, oneshot::Sender<()>>>;

/// One client's session, behind the HTTP endpoint. `Sync`: POSTs on the same session may run
/// concurrently.
pub struct RemoteSession {
    /// The `initialize` handshake is done.
    initialized: AtomicBool,
    /// Requests against the API at once — the same bound the stdio session keeps.
    permits: Semaphore,
    /// Requests in flight, by id: signal one and its POST answers "request cancelled".
    running: Running,
}

impl Default for RemoteSession {
    fn default() -> Self {
        Self::new()
    }
}

impl RemoteSession {
    /// A session before `initialize`.
    pub fn new() -> Self {
        Self {
            initialized: AtomicBool::new(false),
            permits: Semaphore::new(MAX_IN_FLIGHT),
            running: Mutex::new(HashMap::new()),
        }
    }

    /// Whether `initialize` has been answered — the registry registers a session only once it
    /// has, so a failed handshake creates no session.
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }

    /// Handles one `POST`ed message; what the HTTP layer answers.
    pub async fn handle<B: Backend>(&self, message: &[u8], backend: &B) -> Reply {
        if message.len() > MAX_MESSAGE_BYTES {
            return Reply::Rejected(Some(jsonrpc::error(
                None,
                INVALID_REQUEST,
                "message too large: the limit is 2 MiB",
            )));
        }
        match jsonrpc::parse(message) {
            Err(reply) => Reply::Rejected(reply),
            Ok(Incoming::Response) => Reply::Accepted,
            Ok(Incoming::Notification(notification)) => {
                self.notified(&notification);
                Reply::Accepted
            }
            Ok(Incoming::Request(request)) => {
                Reply::Respond(self.requested(request, backend).await)
            }
        }
    }

    /// Ends every in-flight request (the session was deleted or expired): each waiting POST
    /// answers `request cancelled`.
    pub fn close(&self) {
        for (_, cancel) in lock(&self.running).drain() {
            let _sent = cancel.send(());
        }
    }

    /// A notification: `notifications/cancelled` stops the request it names; others are noted
    /// or ignored, as on stdio.
    fn notified(&self, notification: &Notification) {
        match notification.method.as_str() {
            "notifications/initialized" => tracing::debug!("the client is ready"),
            "notifications/cancelled" => {
                let id = notification
                    .params
                    .get("requestId")
                    .and_then(RequestId::new);
                let cancel = id.and_then(|id| lock(&self.running).remove(&id));
                if let Some(cancel) = cancel {
                    let _sent = cancel.send(());
                    tracing::info!("a request was cancelled by the client");
                }
            }
            // Unknown notifications are ignored, as JSON-RPC allows.
            _ => {}
        }
    }

    /// The response to one request.
    async fn requested<B: Backend>(&self, request: Request, backend: &B) -> String {
        let Request { id, method, params } = request;
        match method.as_str() {
            "initialize" => self.initialize(&id, &params),
            "ping" => jsonrpc::success(&id, &json!({})),
            _ if !self.is_initialized() => jsonrpc::error(
                Some(&id),
                NOT_INITIALIZED,
                "the session isn't initialized: send initialize first",
            ),
            "tools/list" if params.get("cursor").is_some_and(|cursor| !cursor.is_null()) => {
                jsonrpc::error(
                    Some(&id),
                    INVALID_PARAMS,
                    "invalid cursor: the tool list has one page",
                )
            }
            "tools/list" => self.run_work(id, Work::List, backend).await,
            "tools/call" => match tool_call(params) {
                Ok((tool, arguments)) => {
                    self.run_work(id, Work::Call(tool, arguments), backend)
                        .await
                }
                Err(problem) => jsonrpc::error(Some(&id), INVALID_PARAMS, &problem),
            },
            _ => jsonrpc::error(Some(&id), METHOD_NOT_FOUND, "method not found"),
        }
    }

    /// The `initialize` response: the shared handshake, marked once. A second `initialize`,
    /// even racing a first, gets the same refusal stdio gives.
    fn initialize(&self, id: &RequestId, params: &Map<String, Value>) -> String {
        match handshake(id, params) {
            Ok(result) => {
                if self
                    .initialized
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    jsonrpc::success(id, &result)
                } else {
                    already_initialized(id)
                }
            }
            Err(reply) => reply,
        }
    }

    /// Runs `work` inside this POST's own future: it answers directly, it waits for a free
    /// slot under [`MAX_IN_FLIGHT`], and `notifications/cancelled` (or [`close`]) answers it
    /// `request cancelled` while the work is dropped mid-flight.
    async fn run_work<B: Backend>(&self, id: RequestId, work: Work, backend: &B) -> String {
        let (cancel, cancelled) = oneshot::channel();
        {
            let mut running = lock(&self.running);
            if running.contains_key(&id) {
                return jsonrpc::error(
                    Some(&id),
                    INVALID_REQUEST,
                    "this id belongs to a request still in progress",
                );
            }
            running.insert(id.clone(), cancel);
        }
        // Backpressure: the next request waits until one of the running ones ends. A cancel
        // sent while waiting is buffered by the oneshot and answered the moment the work
        // starts racing it.
        let Ok(permit) = self.permits.acquire().await else {
            lock(&self.running).remove(&id);
            return jsonrpc::error(Some(&id), INTERNAL_ERROR, "the server is stopping");
        };
        let reply = tokio::select! {
            reply = work.run(backend, &id) => reply,
            _ = cancelled => jsonrpc::error(
                Some(&id),
                REQUEST_CANCELLED,
                "the request was cancelled",
            ),
        };
        drop(permit);
        lock(&self.running).remove(&id);
        reply
    }

    /// The requests registered right now; the tests wait on it.
    #[cfg(test)]
    fn in_flight(&self) -> usize {
        lock(&self.running).len()
    }
}

fn lock(running: &Running) -> MutexGuard<'_, HashMap<RequestId, oneshot::Sender<()>>> {
    running.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The refusal a second `initialize` gets — the same text the stdio session answers.
fn already_initialized(id: &RequestId) -> String {
    jsonrpc::error(
        Some(id),
        INVALID_REQUEST,
        "the session is already initialized",
    )
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, future::Future, sync::Arc};

    use tokio::time::Instant;
    use uuid::Uuid;

    use super::*;
    use crate::api::{Answer, ApiError, Request as ApiRequest};

    /// A backend whose answers are scripted; an empty script is "the API isn't there".
    struct Stub {
        project: Uuid,
        answers: Mutex<VecDeque<Result<Answer, ApiError>>>,
    }

    impl Stub {
        fn silent() -> Self {
            Self::scripted(Vec::new())
        }

        fn scripted(answers: Vec<Result<Answer, ApiError>>) -> Self {
            Self {
                project: Uuid::now_v7(),
                answers: Mutex::new(answers.into()),
            }
        }
    }

    impl Backend for Stub {
        fn project(&self) -> Uuid {
            self.project
        }

        fn attempt<'a>(
            &'a self,
            _request: &'a ApiRequest<'a>,
            _deadline: Instant,
        ) -> impl Future<Output = Result<Answer, ApiError>> + Send + 'a {
            let answer = self
                .answers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front()
                .unwrap_or_else(|| Err(ApiError::Unreachable("no stubbed answer".into())));
            std::future::ready(answer)
        }
    }

    fn permissions(actions: &[&str]) -> Answer {
        Answer {
            body: json!({"data": {"permissions":
                actions.iter().map(|action| json!({"action": action, "subject": "articles"})).collect::<Vec<_>>()
            }}),
            replayed: false,
        }
    }

    fn entry() -> Answer {
        Answer {
            body: json!({"data": {"id": "0190f0c2-7a4e-7cc1-9d1e-2b8a4c6f1a01", "title": "Hello"}}),
            replayed: false,
        }
    }

    const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#;

    /// The response `reply` carries as JSON, for assertions.
    fn replied(reply: Reply) -> Value {
        let Reply::Respond(text) = reply else {
            panic!("expected a response message");
        };
        serde_json::from_str(&text).expect("JSON")
    }

    /// Waits until `n` requests are registered on `session`.
    async fn until_in_flight(session: &RemoteSession, n: usize) {
        for _ in 0..100 {
            if session.in_flight() == n {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("the request was never registered");
    }

    #[tokio::test]
    async fn the_handshake_runs_once_and_then_requests_serve() {
        let session = RemoteSession::new();
        let backend = Stub::silent();
        let reply = session.handle(INITIALIZE.as_bytes(), &backend).await;
        let reply = replied(reply);
        assert_eq!(reply["result"]["protocolVersion"], "2025-11-25");
        assert!(session.is_initialized());
        // The same handshake answered on stdio names itself colophon-mcp.
        assert_eq!(reply["result"]["serverInfo"]["name"], "colophon-mcp");

        let reply = session.handle(INITIALIZE.as_bytes(), &backend).await;
        assert_eq!(
            replied(reply)["error"]["code"],
            INVALID_REQUEST,
            "a second initialize is refused"
        );
        let ping = session
            .handle(br#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#, &backend)
            .await;
        assert_eq!(replied(ping)["result"], json!({}));
    }

    #[tokio::test]
    async fn malformed_input_is_rejected_and_notifications_are_accepted() {
        let session = RemoteSession::new();
        let backend = Stub::silent();
        let Reply::Rejected(Some(text)) = session.handle(b"not json", &backend).await else {
            panic!("malformed JSON is rejected");
        };
        assert_eq!(
            serde_json::from_str::<Value>(&text).expect("JSON")["error"]["code"],
            -32_700
        );

        // A notification, even a meaningless one, is accepted with nothing to answer.
        let reply = session
            .handle(
                br#"{"jsonrpc":"2.0","method":"notifications/whatever"}"#,
                &backend,
            )
            .await;
        assert!(matches!(reply, Reply::Accepted));
        // A response to a request the server never sent is too.
        let reply = session
            .handle(br#"{"jsonrpc":"2.0","id":9,"result":{}}"#, &backend)
            .await;
        assert!(matches!(reply, Reply::Accepted));
        // A broken notification is rejected without a JSON-RPC error to carry.
        let reply = session
            .handle(
                br#"{"jsonrpc":"2.0","method":"notifications/x","params":1}"#,
                &backend,
            )
            .await;
        assert!(matches!(reply, Reply::Rejected(None)));
    }

    #[tokio::test]
    async fn a_too_large_message_is_rejected_before_parsing() {
        let session = RemoteSession::new();
        let backend = Stub::silent();
        let mut message = vec![b'{'; MAX_MESSAGE_BYTES];
        message.extend_from_slice(b"aa");
        let Reply::Rejected(Some(text)) = session.handle(&message, &backend).await else {
            panic!("oversized input is rejected");
        };
        let error: Value = serde_json::from_str(&text).expect("JSON");
        assert_eq!(error["error"]["code"], INVALID_REQUEST);
    }

    #[tokio::test]
    async fn requests_before_initialize_get_not_initialized() {
        let session = RemoteSession::new();
        let backend = Stub::silent();
        let reply = session
            .handle(
                br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                &backend,
            )
            .await;
        assert_eq!(replied(reply)["error"]["code"], NOT_INITIALIZED);
    }

    #[tokio::test]
    async fn unknown_methods_and_tools_get_the_stdio_refusals() {
        let session = RemoteSession::new();
        let backend = Stub::silent();
        let _ignored = session.handle(INITIALIZE.as_bytes(), &backend).await;
        let reply = session
            .handle(
                br#"{"jsonrpc":"2.0","id":1,"method":"resources/list"}"#,
                &backend,
            )
            .await;
        assert_eq!(replied(reply)["error"]["code"], METHOD_NOT_FOUND);
        // A tool the platform does not allow an agent — the boundary every transport keeps.
        for name in [
            "publish_release",
            "approve_publish_request",
            "resolve_comment",
        ] {
            let call = format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{name}","arguments":{{}}}}}}"#
            );
            let reply = session.handle(call.as_bytes(), &backend).await;
            assert_eq!(
                replied(reply)["error"]["code"],
                INVALID_PARAMS,
                "{name} is refused as an unknown tool, -32602"
            );
        }
    }

    #[tokio::test]
    async fn tools_list_and_call_run_through_the_same_table() {
        let session = RemoteSession::new();
        let backend = Stub::scripted(vec![Ok(permissions(&["content.read"])), Ok(entry())]);
        let _ignored = session.handle(INITIALIZE.as_bytes(), &backend).await;
        let list = session
            .handle(
                br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                &backend,
            )
            .await;
        let tools = replied(list)["result"]["tools"]
            .as_array()
            .expect("a tool list")
            .clone();
        assert_eq!(tools.len(), 12, "a read-only token sees the read tools");
        // The six write tools are absent: nothing that changes content is offered.
        for writer in [
            "create_draft",
            "update_draft",
            "create_publish_request",
            "request_review",
            "create_comment",
            "create_task",
        ] {
            assert!(
                tools
                    .iter()
                    .all(|tool| tool["name"].as_str() != Some(writer)),
                "{writer} is not listed for a read-only token"
            );
        }

        let call = session
            .handle(
                br#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_entry","arguments":{"collection":"articles","id":"0190f0c2-7a4e-7cc1-9d1e-2b8a4c6f1a01"}}}"#,
                &backend,
            )
            .await;
        let result = replied(call);
        assert_eq!(result["result"]["isError"], false, "{result}");
        assert_eq!(
            result["result"]["structuredContent"]["entry"]["title"],
            "Hello"
        );
    }

    #[tokio::test]
    async fn a_cancelled_request_answers_request_cancelled() {
        let session = RemoteSession::new();
        // The stub never answers well: the request retries until cancelled.
        let backend = Stub::silent();
        let _ignored = session.handle(INITIALIZE.as_bytes(), &backend).await;
        let (running, notified) = tokio::join!(
            session.handle(
                br#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
                &backend,
            ),
            session.handle(
                br#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3}}"#,
                &backend,
            )
        );
        assert!(matches!(notified, Reply::Accepted));
        assert_eq!(replied(running)["error"]["code"], REQUEST_CANCELLED);
    }

    #[tokio::test]
    async fn a_reused_id_while_running_is_refused() {
        let session = RemoteSession::new();
        let backend = Stub::silent();
        let _ignored = session.handle(INITIALIZE.as_bytes(), &backend).await;
        // Poll the first handle until its id registers, then the duplicate is refused.
        let (running, duplicate) = tokio::join!(
            session.handle(
                br#"{"jsonrpc":"2.0","id":5,"method":"tools/list"}"#,
                &backend,
            ),
            async {
                until_in_flight(&session, 1).await;
                session
                    .handle(
                        br#"{"jsonrpc":"2.0","id":5,"method":"tools/list"}"#,
                        &backend,
                    )
                    .await
            }
        );
        let _ = running;
        assert_eq!(replied(duplicate)["error"]["code"], INVALID_REQUEST);
    }

    #[tokio::test]
    async fn closing_the_session_ends_in_flight_requests() {
        let session = Arc::new(RemoteSession::new());
        let backend = Arc::new(Stub::silent());
        let _ignored = session.handle(INITIALIZE.as_bytes(), &*backend).await;
        let running = {
            let (session, backend) = (Arc::clone(&session), Arc::clone(&backend));
            tokio::spawn(async move {
                session
                    .handle(
                        br#"{"jsonrpc":"2.0","id":4,"method":"tools/list"}"#,
                        &*backend,
                    )
                    .await
            })
        };
        until_in_flight(&session, 1).await;
        session.close();
        let reply = running.await.expect("the task ran");
        assert_eq!(replied(reply)["error"]["code"], REQUEST_CANCELLED);
    }
}
