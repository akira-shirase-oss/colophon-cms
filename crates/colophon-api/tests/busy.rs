//! At capacity the API says so: when no database connection becomes free in time, a request
//! gets 503 with `Retry-After` and the error envelope, not a 500, and nothing was attempted.

use std::{sync::Arc, time::Duration};

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use colophon_api::{
    AppState, HttpLimits, Readiness, Services, SessionSettings, error::BUSY_RETRY_AFTER_SECONDS,
    middleware,
};
use colophon_db::PoolSettings;
use http_body_util::BodyExt as _;
use serde_json::Value;
use tower::ServiceExt as _;

#[tokio::test]
async fn a_request_that_cannot_get_a_connection_is_told_to_retry() {
    // A "database" that accepts connections and never answers: the pool can't hand one out.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("address").port();
    let _hold = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });
    let pool = colophon_db::connect_lazy(
        &format!("postgres://colophon_app:x@127.0.0.1:{port}/colophon"),
        PoolSettings {
            max_connections: 1,
            acquire_timeout: Duration::from_millis(200),
            ..PoolSettings::default()
        },
    )
    .expect("a valid URL");
    let state = AppState {
        readiness: Readiness::new("all", true),
        services: Some(Arc::new(Services {
            pool,
            oidc: None,
            sessions: SessionSettings::default(),
            trusted_origins: vec!["http://localhost:58000".to_owned()],
            allow_private_destinations: true,
            cache: colophon_api::DeliveryCache::disabled(),
        })),
        build: colophon_api::BuildInfo::unstamped("0.0.0-test"),
    };
    let app = middleware::apply(
        colophon_api::routes(state),
        HttpLimits {
            request_timeout: Duration::from_secs(5),
            max_body_bytes: 1024,
        },
    );

    // A well-formed session cookie, so the request reaches the session lookup.
    let request = Request::get("/api/v1/projects")
        .header(
            header::COOKIE,
            format!("colophon_session={}", "a".repeat(43)),
        )
        .body(Body::empty())
        .expect("request");
    let response = app.oneshot(request).await.expect("a response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response.headers().get(header::RETRY_AFTER),
        Some(&BUSY_RETRY_AFTER_SECONDS.into())
    );
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: Value = serde_json::from_slice(&body).expect("JSON");
    assert_eq!(body["error"]["code"], "unavailable", "{body}");
    assert!(
        body["error"]["request_id"].is_string(),
        "the envelope carries the request id: {body}"
    );
}
