//! The HTTP contract, tested in-process through the full middleware stack: health states,
//! the error envelope for errors from handlers and from middleware, and request ids.

use std::time::Duration;

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Method, Request, StatusCode, header},
    routing::{get, post},
};
use colophon_api::{Dependency, HttpLimits, Readiness, middleware};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tower::ServiceExt as _;

const LIMITS: HttpLimits = HttpLimits {
    request_timeout: Duration::from_millis(200),
    max_body_bytes: 64,
};

/// The production router plus three routes that only tests need: one that panics, one that
/// is too slow, and one that reads a body.
fn app(readiness: Readiness) -> Router {
    let test_routes = Router::new()
        .route("/test/panic", get(panics))
        .route(
            "/test/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                "too late"
            }),
        )
        .route("/test/echo", post(|body: String| async move { body }));
    middleware::apply(colophon_api::routes(readiness).merge(test_routes), LIMITS)
}

async fn panics() -> &'static str {
    panic!("secret internal detail")
}

fn ready() -> Readiness {
    let readiness = Readiness::new("all", true);
    readiness.record(Dependency::Database, true);
    readiness.record(Dependency::Cache, true);
    readiness
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Reply {
    #[expect(
        clippy::expect_used,
        reason = "test helper: a missing header is a test failure"
    )]
    fn request_id(&self) -> &str {
        self.headers
            .get("x-request-id")
            .expect("every response carries x-request-id")
            .to_str()
            .expect("request ids are ASCII")
    }

    /// Asserts the envelope shape and that its request id matches the header.
    fn assert_envelope(&self, status: StatusCode, code: &str) {
        assert_eq!(self.status, status, "body: {}", self.body);
        assert_eq!(self.body["error"]["code"], code, "body: {}", self.body);
        assert!(
            self.body["error"]["message"].is_string(),
            "body: {}",
            self.body
        );
        assert_eq!(self.body["error"]["request_id"], self.request_id());
        assert_eq!(
            self.headers
                .get(header::CONTENT_TYPE)
                .map(axum::http::HeaderValue::as_bytes),
            Some(&b"application/json"[..])
        );
    }
}

#[expect(
    clippy::expect_used,
    reason = "test helper: transport failures are test failures"
)]
async fn send(app: Router, request: Request<Body>) -> Reply {
    let response = app
        .oneshot(request)
        .await
        .expect("the router is infallible");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body collects")
        .to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("every body is JSON")
    };
    Reply {
        status,
        headers,
        body,
    }
}

#[expect(
    clippy::expect_used,
    reason = "test helper: the request is well-formed"
)]
fn get_request(path: &str) -> Request<Body> {
    Request::get(path)
        .body(Body::empty())
        .expect("valid request")
}

#[tokio::test]
async fn livez_answers_without_checking_dependencies() {
    // The database has never been probed, and liveness doesn't care.
    let reply = send(app(Readiness::new("all", true)), get_request("/livez")).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body, json!({ "status": "live" }));
    assert_eq!(reply.headers[header::CACHE_CONTROL], "no-store");
}

#[tokio::test]
async fn readyz_reports_every_state_with_the_right_status_code() {
    let readiness = Readiness::new("all", true);
    let reply = send(app(readiness.clone()), get_request("/readyz")).await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(reply.body["status"], "starting");

    readiness.record(Dependency::Database, true);
    readiness.record(Dependency::Cache, true);
    let reply = send(app(readiness.clone()), get_request("/readyz")).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.body,
        json!({ "status": "ready", "role": "all", "checks": { "database": "up", "cache": "up" } })
    );
    assert_eq!(reply.headers[header::CACHE_CONTROL], "no-store");

    readiness.record(Dependency::Cache, false);
    let reply = send(app(readiness.clone()), get_request("/readyz")).await;
    assert_eq!(
        reply.status,
        StatusCode::OK,
        "a cache outage degrades, it doesn't unready"
    );
    assert_eq!(reply.body["status"], "degraded");

    readiness.record(Dependency::Database, false);
    let reply = send(app(readiness.clone()), get_request("/readyz")).await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(reply.body["status"], "not_ready");
    assert!(
        reply.body.get("error").is_none(),
        "a readiness report, not an error envelope"
    );

    readiness.record(Dependency::Database, true);
    readiness.begin_drain();
    let reply = send(app(readiness), get_request("/readyz")).await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(reply.body["status"], "draining");
}

#[tokio::test]
async fn head_requests_work_for_health_checks() {
    let request = Request::head("/livez")
        .body(Body::empty())
        .expect("valid request");
    let reply = send(app(ready()), request).await;
    assert_eq!(reply.status, StatusCode::OK);
}

#[tokio::test]
async fn unknown_routes_get_the_envelope() {
    let reply = send(app(ready()), get_request("/api/v1/nope")).await;
    reply.assert_envelope(StatusCode::NOT_FOUND, "route_not_found");
    assert_eq!(reply.body["error"]["message"], "no route matches this path");
}

#[tokio::test]
async fn wrong_methods_get_the_envelope_and_keep_the_allow_header() {
    let request = Request::builder()
        .method(Method::DELETE)
        .uri("/livez")
        .body(Body::empty())
        .expect("valid request");
    let reply = send(app(ready()), request).await;
    reply.assert_envelope(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    assert!(
        reply.headers.contains_key(header::ALLOW),
        "axum's Allow header survives"
    );
}

#[tokio::test]
async fn oversized_bodies_are_refused_whether_declared_or_streamed() {
    let declared = Request::post("/test/echo")
        .header(header::CONTENT_LENGTH, "65")
        .body(Body::from("x".repeat(65)))
        .expect("valid request");
    send(app(ready()), declared)
        .await
        .assert_envelope(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large");

    // No Content-Length: the limit has to be enforced while the body streams in.
    let chunks =
        futures_util::stream::iter([vec![b'x'; 40], vec![b'x'; 40]].map(Ok::<_, std::io::Error>));
    let streamed = Request::post("/test/echo")
        .body(Body::from_stream(chunks))
        .expect("valid request");
    send(app(ready()), streamed)
        .await
        .assert_envelope(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large");

    let small = Request::post("/test/echo")
        .body(Body::from("fits"))
        .expect("valid request");
    let response = app(ready()).oneshot(small).await.expect("infallible");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test(start_paused = true)]
async fn slow_requests_end_at_the_deadline_with_the_envelope() {
    let reply = send(app(ready()), get_request("/test/slow")).await;
    reply.assert_envelope(StatusCode::GATEWAY_TIMEOUT, "deadline_exceeded");
}

#[tokio::test]
async fn panics_become_500s_that_hide_the_panic_message() {
    let reply = send(app(ready()), get_request("/test/panic")).await;
    reply.assert_envelope(StatusCode::INTERNAL_SERVER_ERROR, "internal");
    assert_eq!(reply.body["error"]["message"], "internal error");
    assert!(
        !reply.body.to_string().contains("secret"),
        "no internals in the response"
    );
}

#[tokio::test]
async fn generated_request_ids_are_uuid_v7() {
    let reply = send(app(ready()), get_request("/livez")).await;
    let id = uuid::Uuid::parse_str(reply.request_id()).expect("a UUID");
    assert_eq!(id.get_version_num(), 7);
}

#[tokio::test]
async fn a_clients_well_formed_request_id_is_kept_and_echoed() {
    let request = Request::get("/nope")
        .header("x-request-id", "client.trace-42")
        .body(Body::empty())
        .expect("valid request");
    let reply = send(app(ready()), request).await;
    assert_eq!(reply.request_id(), "client.trace-42");
    reply.assert_envelope(StatusCode::NOT_FOUND, "route_not_found");
}

#[tokio::test]
async fn hostile_request_ids_are_replaced() {
    for hostile in ["a".repeat(65), "has space".to_owned(), "x\"y".to_owned()] {
        let request = Request::get("/livez")
            .header("x-request-id", &hostile)
            .body(Body::empty())
            .expect("valid request");
        let reply = send(app(ready()), request).await;
        assert_ne!(reply.request_id(), hostile);
        let id = uuid::Uuid::parse_str(reply.request_id()).expect("a fresh UUID");
        assert_eq!(id.get_version_num(), 7);
    }
}
