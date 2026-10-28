//! The project list a page at a time, against the real binary: `GET /api/v1/projects` with
//! `limit`, `cursor` and `q` (docs/modules/04-api.md). Needs the stack and `scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Server, call, sign_in, unique_slug};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

fn names(page: &Value) -> Vec<String> {
    page["data"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|project| project["name"].as_str().expect("a name").to_owned())
        .collect()
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh, which bootstraps dev-owner"]
async fn projects_come_a_page_at_a_time_and_a_search_finds_them_in_any_case() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let editor = sign_in("dev-editor").await;
    // A token only these projects' names hold, so other tests' projects never match.
    let token = unique_slug().replace('-', "");
    for letter in ["C", "A", "B"] {
        let created = call(
            &owner,
            Method::POST,
            "/api/v1/projects",
            Some(&json!({"slug": unique_slug(), "name": format!("Paging {} {letter}", token.to_uppercase())})),
            &[],
        )
        .await
        .expect("request");
        assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    }
    let list = |query: String| {
        let owner = &owner;
        async move {
            call(
                owner,
                Method::GET,
                &format!("/api/v1/projects?{query}"),
                None,
                &[],
            )
            .await
            .expect("request")
        }
    };

    // By name, two at a time; the search is lower case and the names upper case.
    let first = list(format!("q={token}&limit=2")).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);
    let upper = token.to_uppercase();
    assert_eq!(
        names(&first.body),
        [format!("Paging {upper} A"), format!("Paging {upper} B")]
    );
    let cursor = first.body["next_cursor"]
        .as_str()
        .expect("a next cursor")
        .to_owned();
    let second = list(format!("q={token}&limit=2&cursor={cursor}")).await;
    assert_eq!(names(&second.body), [format!("Paging {upper} C")]);
    assert_eq!(
        second.body["next_cursor"],
        Value::Null,
        "no page after the last"
    );

    // `%` and `_` are literal: no name holds the token followed by a percent sign.
    let literal = list(format!("q={token}%25")).await;
    assert_eq!(names(&literal.body), Vec::<String>::new());

    // Someone who isn't a member finds none of them.
    let theirs = call(
        &editor,
        Method::GET,
        &format!("/api/v1/projects?q={token}"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(names(&theirs.body), Vec::<String>::new());

    // A cursor is bound to whoever it was given to.
    let borrowed = call(
        &editor,
        Method::GET,
        &format!("/api/v1/projects?cursor={cursor}"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        borrowed.status,
        StatusCode::BAD_REQUEST,
        "{}",
        borrowed.body
    );

    // Cursors, limits and searches are checked.
    for query in [
        "cursor=not-a-cursor".to_owned(),
        "limit=0".to_owned(),
        "limit=101".to_owned(),
        format!("q={}", "x".repeat(101)),
    ] {
        let refused = list(query.clone()).await;
        assert_eq!(
            refused.status,
            StatusCode::BAD_REQUEST,
            "{query}: {}",
            refused.body
        );
    }
}
