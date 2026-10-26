//! `POST /api/v1/rich-text/validate` through the full middleware stack, with every shared
//! fixture: the HTTP verdict must match the fixture exactly.
#![expect(
    clippy::expect_used,
    reason = "HTTP tests: an unreadable fixture or a transport failure is a test failure"
)]

use std::{fs, path::PathBuf};

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use colophon_api::{Dependency, HttpLimits, Readiness};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tower::ServiceExt as _;

fn app() -> axum::Router {
    let readiness = Readiness::new("all", false);
    readiness.record(Dependency::Database, true);
    colophon_api::app(
        colophon_api::AppState::without_services(readiness),
        HttpLimits::default(),
    )
}

async fn post(body: Body, content_type: Option<&str>) -> (StatusCode, String, Value) {
    let mut request = Request::post("/api/v1/rich-text/validate");
    if let Some(content_type) = content_type {
        request = request.header(header::CONTENT_TYPE, content_type);
    }
    let response = app()
        .oneshot(request.body(body).expect("valid request"))
        .await
        .expect("infallible");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let text = String::from_utf8(bytes.to_vec()).expect("UTF-8");
    let json = serde_json::from_str(&text).expect("JSON");
    (status, text, json)
}

async fn validate(value: &Value) -> (StatusCode, String, Value) {
    post(Body::from(value.to_string()), Some("application/json")).await
}

fn fixtures(kind: &str) -> Vec<(String, Value)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/rich-text/v1")
        .join(kind);
    let mut paths: Vec<_> = fs::read_dir(dir)
        .expect("fixture directory")
        .map(|entry| entry.expect("entry").path())
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let file: Value =
                serde_json::from_str(&fs::read_to_string(&path).expect("readable")).expect("JSON");
            (
                path.file_stem()
                    .expect("name")
                    .to_string_lossy()
                    .into_owned(),
                file,
            )
        })
        .collect()
}

#[tokio::test]
async fn every_valid_fixture_comes_back_in_canonical_form() {
    for (name, file) in fixtures("valid") {
        let (status, text, json) = validate(&file["value"]).await;
        assert_eq!(status, StatusCode::OK, "{name}: {text}");
        // The canonical bytes are embedded verbatim, so they appear in the body as is.
        let canonical = colophon_domain::rich_text::validate(&file["value"]).expect("valid");
        assert!(text.contains(canonical.canonical_json()), "{name}");
        assert_eq!(json["nodes"], canonical.nodes(), "{name}");
        assert_eq!(json["bytes"], canonical.bytes(), "{name}");
    }
}

#[tokio::test]
async fn every_invalid_fixture_is_a_422_naming_its_code_and_pointer() {
    for (name, file) in fixtures("invalid") {
        let (status, text, json) = validate(&file["value"]).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{name}: {text}");
        assert_eq!(json["error"]["code"], "validation_failed", "{name}");
        assert_eq!(
            json["error"]["details"]["reason"], file["reject"]["code"],
            "{name}"
        );
        assert_eq!(
            json["error"]["details"]["pointer"], file["reject"]["path"],
            "{name}"
        );
        assert!(json["error"]["request_id"].is_string(), "{name}");
    }
}

#[tokio::test]
async fn a_body_that_is_not_json_is_a_400() {
    let (status, _, json) = post(Body::from("{not json"), Some("application/json")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"]["code"], "malformed_json");
    assert!(
        json["error"].get("details").is_none(),
        "details only for validation failures"
    );
}

#[tokio::test]
async fn a_body_without_the_json_content_type_is_a_415() {
    let body = json!({ "schema": "colophon/rich-text", "version": 1, "doc": { "type": "doc", "content": [{ "type": "paragraph" }] } });
    let (status, _, json) = post(Body::from(body.to_string()), None).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(json["error"]["code"], "unsupported_media_type");
}

#[tokio::test]
async fn json_nested_past_the_parsers_limit_is_a_400_not_a_crash() {
    // serde_json refuses nesting past 128 levels while parsing; the server must answer cleanly.
    let body = format!("{}{}", "[".repeat(10_000), "]".repeat(10_000));
    let (status, _, json) = post(Body::from(body), Some("application/json")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"]["code"], "malformed_json");
}
