//! Rate-limit evasion (docs/security/attack-surface.md, gap G): every route that reads Postgres counts
//! against a limit. Before, a preview preflight's origin lookup ran in the CORS layer outside the limiter,
//! and `/me` and the media origin weren't limited at all. And an `X-Forwarded-For` a client writes itself
//! buys no fresh address: only the hop the trusted proxy appended names the client.
//!
//! Each test's requests come through the trusted proxy (`127.0.0.1`) as a fresh documentation address,
//! so the limits it drains in the shared Valkey are its own, not the next test's.

use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::{Method, StatusCode, header};
use uuid::Uuid;

use crate::common::{ORIGIN, Server, app, sign_in};

/// A documentation address (RFC 5737) no other run has used lately.
fn fresh_address() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after 1970")
        .subsec_nanos();
    format!("198.51.{}.{}", (nanos >> 8) % 256, nanos % 254 + 1)
}

/// Sends `count` requests made by `build`; how many were refused `429`.
async fn limited(count: usize, build: impl Fn(usize) -> reqwest::RequestBuilder) -> usize {
    let mut refused = 0;
    for index in 0..count {
        let response = build(index).send().await.expect("request");
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            refused += 1;
        } else {
            assert!(response.status().as_u16() < 500, "{}", response.status());
        }
    }
    refused
}

const SLOW: [(&str, &str); 4] = [
    ("COLOPHON_RATE_LIMIT_PER_ADDRESS", "1/5"),
    ("COLOPHON_RATE_LIMIT_DELIVERY", "1/5"),
    ("COLOPHON_RATE_LIMIT_PER_CREDENTIAL", "off"),
    ("COLOPHON_TRUSTED_PROXIES", "127.0.0.1"),
];

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn preflights_me_and_media_are_limited_like_every_read() {
    let _server = Server::start_with(&SLOW).await;
    let owner = sign_in("dev-owner").await;
    let project = Uuid::now_v7();
    // A flood of preflights, each one a database read in the CORS layer before: limited now.
    let client = fresh_address();
    let preflights = limited(30, |_| {
        app()
            .request(
                Method::OPTIONS,
                format!("{ORIGIN}/preview/v1/{project}/production/articles"),
            )
            .header("x-forwarded-for", &client)
            .header(header::ORIGIN, "https://attacker.example")
            .header("access-control-request-method", "GET")
    })
    .await;
    assert!(
        preflights >= 20,
        "only {preflights} of 30 preflights were limited"
    );
    let client = fresh_address();
    let me = limited(30, |_| {
        app()
            .get(format!("{ORIGIN}/me"))
            .header("x-forwarded-for", &client)
            .header(header::COOKIE, &owner.cookie)
    })
    .await;
    assert!(me >= 20, "only {me} of 30 /me reads were limited");
    let client = fresh_address();
    let media = limited(30, |_| {
        app()
            .get(format!(
                "{ORIGIN}/media/v1/{project}/production/{}",
                Uuid::now_v7()
            ))
            .header("x-forwarded-for", &client)
    })
    .await;
    assert!(media >= 20, "only {media} of 30 media reads were limited");
}

/// A client behind the trusted proxy writes its own `X-Forwarded-For` entries, a new one each time;
/// the proxy appends the real address. Only that last hop counts, so the forged ones escape nothing.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_forged_forwarded_for_buys_no_fresh_address() {
    let _server = Server::start_with(&SLOW).await;
    let client = fresh_address();
    let refused = limited(30, |index| {
        app().get(format!("{ORIGIN}/api/v1/projects")).header(
            "x-forwarded-for",
            format!("203.0.113.{}, {client}", index % 250 + 1),
        )
    })
    .await;
    assert!(
        refused >= 20,
        "a forged X-Forwarded-For escaped the limit: {refused} of 30 refused"
    );
}
