//! A noisy agent's refusals cost its neighbours nothing (D-214): once an API token is refused
//! for its quota, its next requests are refused with the same 429, envelope and `Retry-After`
//! after one question to the limiter and none to Postgres, so they never queue on the pool. The
//! "database" here is a listener that counts every connection made to it and answers none: a
//! request that reaches the pool shows up in the count (and ends in 503, the pool's busy answer).
#![expect(
    clippy::expect_used,
    reason = "test helpers: a bind, a request or a body that fails is a test failure"
)]

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use colophon_api::{
    AppState, HttpLimits, Readiness, Services, SessionSettings,
    admission::{Admission, AdmissionSettings},
    middleware,
};
use colophon_db::PoolSettings;
use http_body_util::BodyExt as _;
use serde_json::Value;
use tower::ServiceExt as _;
use uuid::Uuid;

/// A token's secret, as a script would hold it.
const SECRET: &str = "colophon_api_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

struct World {
    app: Router,
    admission: Arc<Admission>,
    connections: Arc<AtomicUsize>,
}

/// The API in front of a database that counts connections and never answers, with the quota
/// window `window_ms` and no shaping limits (so the only limiter questions are the quota's).
async fn world(window_ms: u64) -> World {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("address").port();
    let connections = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&connections);
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            counted.fetch_add(1, Ordering::SeqCst);
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
    let admission = Arc::new(
        Admission::new(
            AdmissionSettings {
                per_address: None,
                per_credential: None,
                delivery_per_address: None,
                password_per_address: None,
                trusted_proxies: Vec::new(),
                quota_window_ms: window_ms,
            },
            None,
        )
        .expect("no URL"),
    );
    let state = AppState {
        readiness: Readiness::new("all", true),
        services: Some(Arc::new(Services {
            pool,
            oidc: None,
            sessions: SessionSettings::default(),
            trusted_origins: vec!["http://localhost:58000".to_owned()],
            allow_private_destinations: true,
            dev_mail: false,
            mail: None,
            cache: colophon_api::DeliveryCache::disabled(),
            content_types: colophon_api::ContentTypeCache::new(),
            assets: None,
            events: colophon_api::events::EventBus::new(
                colophon_api::events::EventLimits::default(),
            ),
            mcp: colophon_api::mcp::Shared::new(colophon_api::mcp::Limits::default()),
            passwords: colophon_db::passwords::PasswordGate::new(1),
            magic_links: colophon_db::audience::MagicLinkLimits::DEFAULT,
            media_origin: "http://localhost:58000".to_owned(),
            install_kind: None,
        })),
        build: colophon_api::BuildInfo::unstamped("0.0.0-test"),
        admission: Some(Arc::clone(&admission)),
    };
    let build = state.build;
    let app = middleware::apply(
        colophon_api::routes(state),
        HttpLimits {
            request_timeout: Duration::from_secs(5),
            max_body_bytes: 1024,
        },
        build,
    );
    World {
        app,
        admission,
        connections,
    }
}

fn entries(project: Uuid) -> String {
    format!("/api/v1/projects/{project}/environments/production/collections/articles/entries")
}

async fn get(app: &Router, path: &str) -> (StatusCode, Option<String>, Value) {
    send(app, Method::GET, path).await
}

async fn send(app: &Router, method: Method, path: &str) -> (StatusCode, Option<String>, Value) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("idempotency-key", Uuid::new_v4().to_string())
        .header(header::AUTHORIZATION, format!("Bearer {SECRET}"))
        .body(Body::empty())
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("a response");
    let status = response.status();
    let retry_after = response
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (
        status,
        retry_after,
        serde_json::from_slice(&body).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn a_throttled_token_is_refused_again_without_postgres_and_one_limiter_question() {
    let world = world(60_000).await;
    let (project, token) = (Uuid::new_v4(), Uuid::new_v4());
    let hash = colophon_auth::tokens::hash(SECRET);
    // Three a minute, all spent: as after the full path refused the token once.
    let first = world
        .admission
        .throttle_for_tests(hash, project, token, 3)
        .await;
    let before = world.admission.queries();

    for _ in 0..5 {
        let asked = world.admission.queries();
        let (status, retry_after, body) = get(&world.app, &entries(project)).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
        assert_eq!(
            body["error"]["code"], "rate_limited",
            "the usual envelope: {body}"
        );
        assert!(body["error"]["request_id"].is_string(), "{body}");
        let retry_after: u32 = retry_after
            .expect("Retry-After")
            .parse()
            .expect("whole seconds");
        assert!(
            (1..=first).contains(&retry_after),
            "Retry-After {retry_after} within the first refusal's {first}"
        );
        assert_eq!(
            world.admission.queries() - asked,
            1,
            "one question to the limiter per refusal"
        );
    }
    assert_eq!(
        world.connections.load(Ordering::SeqCst),
        0,
        "no refusal opened a database connection"
    );
    assert_eq!(world.admission.queries() - before, 5);
}

#[tokio::test]
async fn a_remembered_refusal_never_answers_for_another_project_or_a_path_the_quota_skips() {
    let world = world(60_000).await;
    let (project, token) = (Uuid::new_v4(), Uuid::new_v4());
    world
        .admission
        .throttle_for_tests(colophon_auth::tokens::hash(SECRET), project, token, 3)
        .await;

    // Another project: the full path, where the token lookup decides (here: the pool, busy).
    let (status, _, body) = get(&world.app, &entries(Uuid::new_v4())).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    // Scheduling is refused to tokens by name, not counted by the quota: the full path too.
    let (status, _, body) = send(
        &world.app,
        Method::POST,
        &format!(
            "/api/v1/projects/{project}/environments/production/changesets/{}/schedule",
            Uuid::new_v4()
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(world.connections.load(Ordering::SeqCst) >= 1);
}

#[tokio::test]
async fn once_the_window_has_room_the_memo_lets_go_and_the_full_path_decides() {
    // One request every 300 ms.
    let world = world(300).await;
    let (project, token) = (Uuid::new_v4(), Uuid::new_v4());
    world
        .admission
        .throttle_for_tests(colophon_auth::tokens::hash(SECRET), project, token, 1)
        .await;
    assert_eq!(world.admission.refused_len(), 1);
    tokio::time::sleep(Duration::from_millis(400)).await;

    let (status, _, body) = get(&world.app, &entries(project)).await;
    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "the memo never admits: the request went on to the token lookup: {body}"
    );
    assert!(world.connections.load(Ordering::SeqCst) >= 1);
    assert_eq!(world.admission.refused_len(), 0, "the refusal is forgotten");
}
