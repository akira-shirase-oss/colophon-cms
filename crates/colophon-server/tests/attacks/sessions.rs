//! Sessions, CSRF and stored XSS at the edge (docs/security/attack-surface.md): a cross-site form can't
//! sign anyone in (the sign-in routes read JSON only, which a form can't send), a cookie write needs the
//! exact origin and the CSRF token, and rich text that could run script is refused before it is stored.

use reqwest::{Method, StatusCode, header};
use serde_json::json;

use super::{anonymous, refused};
use crate::common::{ORIGIN, Server, app, entries, project, sign_in};

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_cross_site_form_signs_nobody_in() {
    let _server = Server::start().await;
    for path in ["/auth/signin", "/auth/signup"] {
        for kind in [
            "text/plain",
            "application/x-www-form-urlencoded",
            "multipart/form-data; boundary=x",
        ] {
            let response = app()
                .post(format!("{ORIGIN}{path}"))
                .header(header::ORIGIN, "https://attacker.example")
                .header(header::CONTENT_TYPE, kind)
                .body(r#"{"email":"dev-owner@colophon.local","password":"x"}"#)
                .send()
                .await
                .expect("request");
            assert!(
                response.status().is_client_error(),
                "{path} as {kind}: {}",
                response.status()
            );
            assert!(
                response.headers().get(header::SET_COOKIE).is_none(),
                "{path} as {kind} set a cookie"
            );
        }
    }
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_cookie_write_needs_the_origin_and_the_csrf_token() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let body = json!({"title": "Forged", "body": null});
    for (origin, csrf) in [
        (Some("https://attacker.example"), Some(owner.csrf.as_str())),
        (None, Some(owner.csrf.as_str())),
        (Some(ORIGIN), None),
        (Some(ORIGIN), Some("forged")),
        (Some("null"), Some(owner.csrf.as_str())),
    ] {
        let mut request = app()
            .post(format!("{ORIGIN}{}", entries(&project)))
            .header(header::COOKIE, &owner.cookie)
            .header("idempotency-key", crate::common::key())
            .json(&body);
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        if let Some(csrf) = csrf {
            request = request.header("x-csrf-token", csrf);
        }
        let response = request.send().await.expect("request");
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{origin:?} {csrf:?}"
        );
    }
}

/// The validator is the first gate before anything renders: it refuses what isn't the format, with a
/// code, and it is open to anyone, so it is the first thing an attacker reaches.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn rich_text_that_could_run_script_is_refused() {
    let _server = Server::start().await;
    let doc = |content: serde_json::Value| json!({"schema": "colophon/rich-text", "version": 1, "doc": {"type": "doc", "content": content}});
    let link = |href: &str| {
        doc(json!([{"type": "paragraph", "content": [
            {"type": "text", "text": "click", "marks": [{"type": "link", "attrs": {"href": href}}]}
        ]}]))
    };
    for value in [
        link("javascript:alert(1)"),
        link("JaVaScRiPt:alert(1)"),
        link("java\tscript:alert(1)"),
        link("data:text/html,<script>alert(1)</script>"),
        link("vbscript:msgbox(1)"),
        link("//attacker.example"),
        link("/\\attacker.example"),
        doc(
            json!([{"type": "html", "content": [{"type": "text", "text": "<script>alert(1)</script>"}]}]),
        ),
        doc(json!([{"type": "paragraph", "attrs": {"onclick": "alert(1)"}, "content": []}])),
        doc(json!([{"type": "image", "attrs": {"assetId": "javascript:alert(1)", "alt": ""}}])),
        doc(
            json!([{"type": "codeBlock", "attrs": {"language": "x\" onmouseover=\"alert(1)"}, "content": []}]),
        ),
    ] {
        let reply = anonymous(Method::POST, "/api/v1/rich-text/validate", Some(&value)).await;
        refused(&reply, &value.to_string());
        assert!(reply.body["error"]["code"].is_string(), "{}", reply.body);
    }
}
