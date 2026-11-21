//! SQL and GraphQL injection (docs/security/attack-surface.md): no value reaches SQL text — filters,
//! sorts, field names, cursors and GraphQL variables are bound or refused — so whatever is sent, the answer
//! is a 4xx or an empty page, never a 500, and nothing else is read or written.

use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use super::send;
use crate::common::{Server, article, entries, project, sign_in};

const HOSTILE: &[&str] = &[
    "' OR 1=1 --",
    "\"; DROP TABLE entries; --",
    "1) OR (1=1",
    "$1",
    "\\x00",
    "x'::text; SELECT pg_sleep(5); --",
    "__proto__",
    "{\"$ne\": null}",
    "%' OR '%'='",
];

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn hostile_queries_are_answered_never_executed() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let made = send(
        &owner,
        Method::POST,
        &entries(&project),
        Some(&article("Only one", "Body")),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let query = format!("{}/query", entries(&project));
    let mut bodies: Vec<Value> = Vec::new();
    for text in HOSTILE {
        bodies.push(json!({"filters": [{"field": "title", "op": "eq", "value": text}]}));
        bodies.push(json!({"filters": [{"field": text, "op": "eq", "value": "x"}]}));
        bodies.push(json!({"filters": [{"field": "title", "op": text, "value": "x"}]}));
        bodies.push(json!({"sort": [{"field": text, "dir": "asc"}]}));
        bodies.push(json!({"sort": [{"field": "title", "dir": text}]}));
        bodies.push(json!({"filters": [{"field": "title", "op": "in", "value": [text, text]}]}));
        bodies.push(json!({"fields": [text]}));
    }
    for body in &bodies {
        let reply = send(&owner, Method::POST, &query, Some(body)).await;
        assert!(
            reply.status.as_u16() < 500,
            "{body}: {} {}",
            reply.status,
            reply.body
        );
        if reply.status.is_success() {
            assert_eq!(
                reply.body["data"].as_array().map(Vec::len),
                Some(0),
                "{body}: {}",
                reply.body
            );
        }
        let listed = send(
            &owner,
            Method::GET,
            &format!(
                "{}?cursor={}",
                entries(&project),
                urlencode(&body.to_string())
            ),
            None,
        )
        .await;
        assert!(
            listed.status.as_u16() < 500,
            "cursor {body}: {}",
            listed.status
        );
    }
    let graphql = format!("{}/graphql", entries(&project).trim_end_matches("/entries"));
    for text in HOSTILE {
        for body in [
            json!({"query": "query($t: String) { nodes(filter: {title: {eq: $t}}) { id } }", "variables": {"t": text}}),
            json!({"query": format!("{{ nodes {{ id {text} }} }}")}),
            json!({"query": "{ __schema { types { name } } }", "variables": {"x": text}}),
        ] {
            let reply = send(&owner, Method::POST, &graphql, Some(&body)).await;
            assert!(
                reply.status.as_u16() < 500,
                "{body}: {} {}",
                reply.status,
                reply.body
            );
        }
    }
    // The one entry is still the only one, untouched.
    let listed = send(&owner, Method::GET, &entries(&project), None).await;
    assert_eq!(
        listed.body["data"].as_array().map(Vec::len),
        Some(1),
        "{}",
        listed.body
    );
}

fn urlencode(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}
