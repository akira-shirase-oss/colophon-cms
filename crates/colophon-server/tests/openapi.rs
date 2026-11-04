//! `GET /openapi.json`, over the real binary on the real stack: anyone may read the published
//! `OpenAPI` contract, it is the document the route table emits (D-110), it names the release
//! `/version` reports, and it answers `If-None-Match` with 304. Needs the local stack and
//! migrations: `scripts/stack.sh up && scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "stack tests: a failure to answer is a test failure"
)]

mod common;

use common::{ORIGIN, Server, app};
use reqwest::{StatusCode, header};
use serde_json::Value;

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn the_openapi_document_is_served_to_anyone() {
    let _server = Server::start().await;
    let client = app();

    // No session, no token: a client reads the contract before it can sign in.
    let response = client
        .get(format!("{ORIGIN}/openapi.json"))
        .send()
        .await
        .expect("/openapi.json");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "public, max-age=300"
    );
    let etag = response.headers()[header::ETAG]
        .to_str()
        .expect("an ASCII ETag")
        .to_owned();
    assert!(etag.starts_with("\"sha256:"), "a hashed ETag, got {etag}");

    let document: Value = response.json().await.expect("the document parses");
    assert_eq!(document["openapi"], "3.1.0");
    let paths = document["paths"].as_object().expect("paths");
    assert!(
        paths.len() > 70,
        "the whole surface is described, got {}",
        paths.len()
    );
    for path in [
        "/openapi.json",
        "/version",
        "/livez",
        "/me",
        "/api/v1/projects",
        "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/entries",
        "/content/v1/{project}/{environment}/{collection}",
        "/media/v1/{project}/{environment}/{asset}",
    ] {
        assert!(paths.contains_key(path), "the document misses {path}");
    }
    for scheme in ["sessionCookie", "bearerToken", "apiToken"] {
        assert!(
            document["components"]["securitySchemes"]
                .get(scheme)
                .is_some(),
            "the document misses the {scheme} security scheme"
        );
    }

    // The document names the same release /version reports.
    let version: Value = client
        .get(format!("{ORIGIN}/version"))
        .send()
        .await
        .expect("/version")
        .json()
        .await
        .expect("/version parses");
    assert_eq!(document["info"]["version"], version["version"]);

    // A matching If-None-Match gets a 304; HEAD gets the headers and no body.
    let conditional = client
        .get(format!("{ORIGIN}/openapi.json"))
        .header(header::IF_NONE_MATCH, &etag)
        .send()
        .await
        .expect("a conditional request");
    assert_eq!(conditional.status(), StatusCode::NOT_MODIFIED);

    let head = client
        .head(format!("{ORIGIN}/openapi.json"))
        .send()
        .await
        .expect("HEAD");
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(head.headers()[header::ETAG], etag.as_str());
}
