//! The `OpenAPI` document against the routes it describes, over the real binary on the real
//! stack (D-110). The releases list once answered the newest 100 with `next_cursor` always
//! `null` while the document said to pass the cursor back as `?cursor=`, and the receipt's
//! schema reached its release fields only through `allOf`. Here every answer is checked against
//! the schema the served document gives its operation — every field declared, every required
//! one present, every type and enum honoured — and the releases list pages as documented:
//! newest first, `?limit=` and `?cursor=`, neither skipping nor repeating a release when one is
//! published between pages. Needs the stack and `scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{ORIGIN, Server, Session, app, call, project, publish_article, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::Value;

const RELEASES: &str = "/api/v1/projects/{project}/environments/{environment}/releases";
const RECEIPT: &str = "/api/v1/projects/{project}/environments/{environment}/releases/{generation}";

/// The schema the document gives a `GET`'s 200 answer.
fn answer_schema<'a>(document: &'a Value, path: &str) -> &'a Value {
    &document["paths"][path]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
}

/// Every way `value` departs from `schema` (`$ref`, `type`, `enum`, `properties`, `required`,
/// `items`), each with its JSON pointer. Strict about objects: a field the schema doesn't
/// declare is a departure too, since a client generated from the document can't see it. An
/// `allOf` is one as well: readers and generators too often show only the schema's own fields.
fn departures(document: &Value, schema: &Value, value: &Value, at: &str, found: &mut Vec<String>) {
    if let Some(reference) = schema["$ref"].as_str() {
        let name = reference
            .strip_prefix("#/components/schemas/")
            .expect("a component reference");
        let target = &document["components"]["schemas"][name];
        assert!(!target.is_null(), "{at}: {reference} resolves to nothing");
        departures(document, target, value, at, found);
        return;
    }
    if schema.get("allOf").is_some() {
        found.push(format!("{at}: its schema composes with allOf"));
    }
    if let Some(types) = types_of(schema) {
        let kind = match value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(number) if number.is_i64() || number.is_u64() => "integer",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        };
        let fits = types
            .iter()
            .any(|wanted| *wanted == kind || (*wanted == "number" && kind == "integer"));
        if !fits {
            found.push(format!("{at}: a {kind}, the document says {types:?}"));
            return;
        }
    }
    if let Some(allowed) = schema["enum"].as_array()
        && !allowed.contains(value)
    {
        found.push(format!("{at}: {value} is not one of {allowed:?}"));
    }
    if let Value::Array(items) = value
        && schema.get("items").is_some()
    {
        for (index, item) in items.iter().enumerate() {
            departures(
                document,
                &schema["items"],
                item,
                &format!("{at}/{index}"),
                found,
            );
        }
    }
    if let (Value::Object(fields), Some(properties)) = (value, schema["properties"].as_object()) {
        for name in schema["required"].as_array().into_iter().flatten() {
            let name = name.as_str().expect("a required name");
            if !fields.contains_key(name) {
                found.push(format!("{at}: `{name}` is required but missing"));
            }
        }
        for (name, field) in fields {
            match properties.get(name) {
                Some(property) => {
                    departures(document, property, field, &format!("{at}/{name}"), found);
                }
                None => found.push(format!("{at}: `{name}` is not in the document")),
            }
        }
    }
}

fn types_of(schema: &Value) -> Option<Vec<&str>> {
    match &schema["type"] {
        Value::String(one) => Some(vec![one.as_str()]),
        Value::Array(many) => Some(many.iter().filter_map(Value::as_str).collect()),
        _ => None,
    }
}

/// `value` is exactly what the document says the operation at `path` answers.
fn conforms(document: &Value, path: &str, value: &Value) {
    let schema = answer_schema(document, path);
    assert!(
        !schema.is_null(),
        "the document has no 200 answer for {path}"
    );
    let mut found = Vec::new();
    departures(document, schema, value, "", &mut found);
    assert!(
        found.is_empty(),
        "{path} departs from its document: {found:#?}"
    );
}

/// One page of releases: the generations on it, its `next_cursor`, and the whole answer.
async fn page(owner: &Session, project: &str, query: &str) -> (Vec<i64>, Option<String>, Value) {
    let reply = call(
        owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/environments/production/releases{query}"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(reply.status, StatusCode::OK, "{query}: {}", reply.body);
    let generations = reply.body["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|release| release["generation"].as_i64().expect("a generation"))
        .collect();
    let cursor = reply.body["next_cursor"].as_str().map(str::to_owned);
    (generations, cursor, reply.body)
}

async fn status(owner: &Session, project: &str, query: &str) -> StatusCode {
    call(
        owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/environments/production/releases{query}"),
        None,
        &[],
    )
    .await
    .expect("request")
    .status
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn the_releases_list_and_the_receipt_answer_what_the_document_says() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let document: Value = app()
        .get(format!("{ORIGIN}/openapi.json"))
        .send()
        .await
        .expect("/openapi.json")
        .json()
        .await
        .expect("the document parses");

    // The parameters the list takes are the ones the document declares.
    let declared: Vec<&str> = document["paths"][RELEASES]["get"]["parameters"]
        .as_array()
        .expect("parameters")
        .iter()
        .filter(|parameter| parameter["in"] == "query")
        .filter_map(|parameter| parameter["name"].as_str())
        .collect();
    assert_eq!(declared, ["limit", "cursor"]);

    for title in ["One", "Two", "Three", "Four", "Five"] {
        publish_article(&owner, &project, title).await;
    }

    // Without a limit: everything up to 100, newest first, and no further page.
    let (all, cursor, body) = page(&owner, &project, "").await;
    assert_eq!((all, cursor), (vec![5, 4, 3, 2, 1], None));
    conforms(&document, RELEASES, &body);

    // Two at a time. A release published between pages lands above the first page: the walk
    // neither repeats nor skips one.
    let (first, cursor, body) = page(&owner, &project, "?limit=2").await;
    assert_eq!(first, [5, 4]);
    conforms(&document, RELEASES, &body);
    let cursor = cursor.expect("a second page");
    assert_eq!(publish_article(&owner, &project, "Six").await, 6);
    let (second, cursor, body) = page(&owner, &project, &format!("?limit=2&cursor={cursor}")).await;
    assert_eq!(second, [3, 2]);
    conforms(&document, RELEASES, &body);
    let (third, cursor, body) = page(
        &owner,
        &project,
        &format!("?limit=2&cursor={}", cursor.expect("a third page")),
    )
    .await;
    assert_eq!((third, cursor), (vec![1], None));
    conforms(&document, RELEASES, &body);

    // An exact last page says so: no cursor to an empty page.
    let (exact, cursor, _) = page(&owner, &project, "?limit=6").await;
    assert_eq!((exact.len(), cursor), (6, None));

    // What the document's 400 covers: a limit out of range, a cursor that isn't one, and a
    // cursor from another project's list.
    let other = common::project(&owner).await;
    publish_article(&owner, &other, "Elsewhere").await;
    publish_article(&owner, &other, "Elsewhere too").await;
    let (_, foreign, _) = page(&owner, &other, "?limit=1").await;
    let foreign = foreign.expect("a cursor");
    for query in [
        "?limit=0".to_owned(),
        "?limit=101".to_owned(),
        "?cursor=not-a-cursor".to_owned(),
        format!("?cursor={foreign}"),
    ] {
        assert_eq!(
            status(&owner, &project, &query).await,
            StatusCode::BAD_REQUEST,
            "{query}"
        );
    }
    assert!(
        document["paths"][RELEASES]["get"]["responses"]
            .get("400")
            .is_some(),
        "the document names the 400"
    );

    // The receipt: every release field at the top level, plus its two lists.
    let receipt = call(
        &owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/environments/production/releases/3"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(receipt.status, StatusCode::OK, "{}", receipt.body);
    assert_eq!(receipt.body["data"]["generation"], 3);
    conforms(&document, RECEIPT, &receipt.body);
}
