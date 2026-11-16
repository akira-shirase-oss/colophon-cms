//! GraphQL over real HTTP (step DX.10, D-115): `POST {P}/collections/{collection}/graphql`
//! runs the caller's document against a per-collection schema — `entries(where, orderBy,
//! first, after)` compiling to the same canonical document `entries/query` runs, `entry(id)`
//! reading one draft — every refusal a GraphQL error whose `extensions` carry the query
//! language's own `code` and `pointer`. Mutations stay REST. Needs the stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Reply, Server, Session, call, join, key, project, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
}

/// A request that may write: content types, entries, roles carry an idempotency key.
async fn send(session: &Session, method: Method, path: &str, body: Option<&Value>) -> Reply {
    call(session, method, path, body, &[("idempotency-key", &key())])
        .await
        .expect("request")
}

/// `POST …/graphql` with `document` — and `variables` when the document names some.
async fn gql(
    session: &Session,
    project: &str,
    collection: &str,
    document: &str,
    variables: Option<&Value>,
) -> Reply {
    let mut body = json!({"query": document});
    if let Some(variables) = variables {
        body["variables"] = variables.clone();
    }
    send(
        session,
        Method::POST,
        &format!("{}/collections/{collection}/graphql", base(project)),
        Some(&body),
    )
    .await
}

/// `POST …/entries/query` — the REST twin a GraphQL read is checked against.
async fn ask(session: &Session, project: &str, collection: &str, document: &Value) -> Reply {
    send(
        session,
        Method::POST,
        &format!("{}/collections/{collection}/entries/query", base(project)),
        Some(document),
    )
    .await
}

/// `staff`: `name` the title; `mentor` a single reference to its own collection — the
/// shape a two-level selection walks.
fn staff() -> Value {
    json!({
        "name": "Staff",
        "title_field": "name",
        "fields": [
            {"id": "name", "label": "Name", "type": "text", "required": true},
            {"id": "mentor", "label": "Mentor", "type": "reference", "to": "staff"}
        ]
    })
}

/// `posts`: `headline` the title, `rating` and `section` indexed, `author` a single
/// reference to `staff` — the collection the endpoint is scoped to.
fn posts() -> Value {
    json!({
        "name": "Posts",
        "title_field": "headline",
        "fields": [
            {"id": "headline", "label": "Headline", "type": "text", "required": true, "index": true},
            {"id": "rating", "label": "Rating", "type": "number", "integer": true, "min": 0, "max": 5, "index": true},
            {"id": "section", "label": "Section", "type": "enum", "values": ["News", "Guides"], "index": true},
            {"id": "author", "label": "Author", "type": "reference", "to": "staff"}
        ]
    })
}

/// One collection per type, `staff` then `posts`, in creation order: the entry ids.
async fn seed(owner: &Session) -> (String, Vec<String>, Vec<String>) {
    let project = project(owner).await;
    for (collection, definition) in [("staff", staff()), ("posts", posts())] {
        let made = send(
            owner,
            Method::POST,
            &format!("{}/content-types", base(&project)),
            Some(&json!({"slug": collection, "definition": definition})),
        )
        .await;
        assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    }
    let mut writers = Vec::new();
    for content in [
        json!({"name": "Ada"}),
        json!({"name": "Grace"}),
        json!({"name": "Edsger"}),
    ] {
        let made = send(
            owner,
            Method::POST,
            &format!("{}/collections/staff/entries", base(&project)),
            Some(&content),
        )
        .await;
        assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
        writers.push(made.body["data"]["id"].as_str().expect("an id").to_owned());
    }
    // Grace mentored by Ada; every post by Grace, so `author.mentor` is Ada.
    let grace = writers[1].clone();
    let path = format!("{}/collections/staff/entries/{grace}", base(&project));
    let read = send(owner, Method::GET, &path, None).await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    let saved = call(
        owner,
        Method::PUT,
        &path,
        Some(&json!({
            "name": "Grace",
            "mentor": writers[0].clone(),
        })),
        &[
            ("if-match", &read.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request");
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    let mut entries = Vec::new();
    for content in [
        json!({"headline": "Alpha news", "rating": 5, "section": "News", "author": grace.clone()}),
        json!({"headline": "Beta guide", "rating": 3, "section": "Guides", "author": grace.clone()}),
        json!({"headline": "Gamma news", "rating": 5, "section": "News", "author": grace.clone()}),
    ] {
        let made = send(
            owner,
            Method::POST,
            &format!("{}/collections/posts/entries", base(&project)),
            Some(&content),
        )
        .await;
        assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
        entries.push(made.body["data"]["id"].as_str().expect("an id").to_owned());
    }
    (project, writers, entries)
}

/// The node's `title` values, in the order the page answered them.
fn titles(page: &Value) -> Vec<String> {
    page["nodes"]
        .as_array()
        .unwrap_or_else(|| panic!("nodes: {page}"))
        .iter()
        .map(|node| node["title"].as_str().expect("a title").to_owned())
        .collect()
}

/// The `name`s of a list of introspected members.
fn names(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|field| field["name"].as_str())
        .collect()
}

/// The GraphQL error the field that ran answered: its `message`, and the `code` and
/// `pointer` `extensions` carries — the same values REST's `details` would.
fn error(reply: &Reply) -> (String, Value, Value) {
    let errors = reply.body["errors"].as_array().expect("GraphQL errors");
    assert!(!errors.is_empty(), "{}", reply.body);
    let first = &errors[0];
    (
        first["message"].as_str().expect("a message").to_owned(),
        first["extensions"]["code"].clone(),
        first["extensions"]["pointer"].clone(),
    )
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn an_entries_read_is_the_same_query_rest_runs() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, _, _) = seed(&owner).await;

    // The GraphQL document — a filter and a descending sort on indexed fields, the
    // node's title and two content fields selected.
    let reply = gql(
        &owner,
        &project,
        "posts",
        "{ entries(where: {rating: {gte: 3}, section: {eq: \"News\"}}, \
             orderBy: [{field: \"rating\", dir: DESC}]) { \
           nodes { title headline rating } \
           pageInfo { endCursor hasNextPage } \
           applied { query queryHash } } }",
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let connection = &reply.body["data"]["entries"];
    assert_eq!(titles(connection), ["Gamma news", "Alpha news"]);
    let node = &connection["nodes"][0];
    assert_eq!(node["headline"], "Gamma news");
    // `rating` is a `number` field — GraphQL's Float, which answers `5.0`.
    assert_eq!(node["rating"], 5.0);

    // `applied` echoes the canonical document and `sha256:` hash — byte-equal to what
    // `entries/query` reports for the twin document — the content fields the
    // selection named are the document's `fields`, `title` a meta the node type
    // answers without narrowing — proving one compiled form ran.
    let rest = ask(
        &owner,
        &project,
        "posts",
        &json!({
            "filters": [
                {"field": "rating", "op": "gte", "value": 3},
                {"field": "section", "op": "eq", "value": "News"}
            ],
            "sort": [{"field": "rating", "dir": "desc"}],
            "fields": ["headline", "rating"]
        }),
    )
    .await;
    assert_eq!(rest.status, StatusCode::OK, "{}", rest.body);
    assert_eq!(
        connection["applied"]["query"], rest.body["applied"]["query"],
        "the canonical document is the same document's"
    );
    assert_eq!(
        connection["applied"]["queryHash"], rest.body["applied"]["query_hash"],
        "the hash a cursor binds to is the same document's"
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn entry_by_id_and_a_missing_one_is_null() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, _, entries) = seed(&owner).await;

    let reply = gql(
        &owner,
        &project,
        "posts",
        &format!(
            "{{ entry(id: \"{}\") {{ id title headline }} }}",
            entries[0]
        ),
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["data"]["entry"]["id"], entries[0]);
    assert_eq!(reply.body["data"]["entry"]["title"], "Alpha news");

    // An unknown id — and a value that isn't a uuid at all — is `null`, the honest
    // answer a nullable field gives a miss.
    for id in [
        "123e4567-e89b-12d3-a456-426614174000".to_owned(),
        "not-a-uuid".to_owned(),
    ] {
        let reply = gql(
            &owner,
            &project,
            "posts",
            &format!("{{ entry(id: \"{id}\") {{ title }} }}"),
            None,
        )
        .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        assert_eq!(reply.body["data"]["entry"], Value::Null, "{id}");
    }
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn aliases_run_the_call_each_names() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, _, _) = seed(&owner).await;

    // Two `entries` calls in one document, aliased apart; each answers its own page.
    let reply = gql(
        &owner,
        &project,
        "posts",
        "{ first: entries(first: 1) { nodes { title } hasNext: pageInfo { hasNextPage } } \
           rest: entries { nodes { title } } }",
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(titles(&reply.body["data"]["first"]), ["Alpha news"]);
    assert_eq!(
        titles(&reply.body["data"]["rest"]),
        ["Alpha news", "Beta guide", "Gamma news"]
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_nested_reference_expands_to_its_bound() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, _, _) = seed(&owner).await;

    // `author`'s selection expands the reference one level; `mentor`'s inside it a
    // second — the document's `depth` of 2, populated in the one transaction.
    let reply = gql(
        &owner,
        &project,
        "posts",
        "{ entries(first: 1) { nodes { author { name mentor { title } } } \
           applied { query } } }",
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let node = &reply.body["data"]["entries"]["nodes"][0];
    assert_eq!(node["author"]["name"], "Grace");
    assert_eq!(node["author"]["mentor"]["title"], "Ada");
    assert_eq!(
        reply.body["data"]["entries"]["applied"]["query"]["depth"], 2,
        "the nested selections compiled to the document's depth"
    );

    // A third level — `mentor`'s own `mentor`'s field — refuses where the document
    // would: `depth` out of range, pointed where a client names it.
    let reply = gql(
        &owner,
        &project,
        "posts",
        "{ entries { nodes { author { mentor { mentor { title } } } } } }",
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let (_, code, pointer) = error(&reply);
    assert_eq!(code, "out_of_range", "{}", reply.body);
    assert_eq!(pointer, "/depth", "{}", reply.body);
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn page_info_carries_the_cursor_the_next_call_takes() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, _, _) = seed(&owner).await;

    let first = gql(
        &owner,
        &project,
        "posts",
        "{ entries(first: 2) { nodes { title } \
           pageInfo { endCursor hasNextPage } } }",
        None,
    )
    .await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);
    let info = &first.body["data"]["entries"]["pageInfo"];
    assert_eq!(info["hasNextPage"], true);
    let cursor = info["endCursor"]
        .as_str()
        .expect("an end cursor")
        .to_owned();
    assert_eq!(
        titles(&first.body["data"]["entries"]),
        ["Alpha news", "Beta guide"]
    );

    // `after:` is the `?cursor=` of the same query: the next page resumes where the
    // boundary sealed, with no more pages beyond.
    let second = gql(
        &owner,
        &project,
        "posts",
        "query ($cursor: String) { entries(first: 2, after: $cursor) { nodes { title } \
           pageInfo { endCursor hasNextPage } } }",
        Some(&json!({"cursor": cursor})),
    )
    .await;
    assert_eq!(second.status, StatusCode::OK, "{}", second.body);
    assert_eq!(titles(&second.body["data"]["entries"]), ["Gamma news"]);
    assert_eq!(
        second.body["data"]["entries"]["pageInfo"]["hasNextPage"],
        false
    );

    // A token that isn't this query's cursor is a GraphQL error carrying the
    // document's own refusal — `invalid_cursor` at the argument that named it.
    let refused = gql(
        &owner,
        &project,
        "posts",
        "{ entries(after: \"not-a-cursor\") { nodes { title } } }",
        None,
    )
    .await;
    assert_eq!(refused.status, StatusCode::OK, "{}", refused.body);
    let (_, code, _) = error(&refused);
    assert_eq!(code, "invalid_cursor", "{}", refused.body);
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_filter_on_a_field_the_role_cant_read_is_refused_like_rests() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let guest = sign_in("dev-editor").await;
    let (project, _, _) = seed(&owner).await;

    // `analyst` reads only `headline` and `rating` of posts, nothing of staff.
    let made = send(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/roles"),
        Some(&json!({
            "key": "analyst",
            "name": "Analyst",
            "permissions": [
                {"action": "content.read", "subject": "posts", "fields": ["headline", "rating"]}
            ]
        })),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    join(&owner, &guest, &project, "analyst").await;

    // A filter on `section` — which the role can't read on every row — refuses with
    // the same `code` and `pointer` `entries/query` answers, as a GraphQL error.
    let reply = gql(
        &guest,
        &project,
        "posts",
        "{ entries(where: {section: {eq: \"News\"}}) { nodes { title } } }",
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let (_, code, pointer) = error(&reply);
    assert_eq!(code, "field_not_readable", "{}", reply.body);
    assert_eq!(pointer, "/filters/0/field", "{}", reply.body);

    // Selecting readable fields narrows as before; the fields the role can't read
    // are named in `hiddenFields`, never leaked.
    let reply = gql(
        &guest,
        &project,
        "posts",
        "{ entries { nodes { title headline hiddenFields } } }",
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let node = &reply.body["data"]["entries"]["nodes"][0];
    assert_eq!(node["title"], "Alpha news");
    assert!(node["headline"].is_string(), "{}", node);
    assert!(
        node["hiddenFields"]
            .as_array()
            .expect("hidden fields")
            .iter()
            .any(|field| field == "section"),
        "{}",
        node
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn mutations_are_not_this_surface() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, _, _) = seed(&owner).await;

    // A `mutation` operation names a root the schema doesn't have: GraphQL's own
    // refusal — writes stay REST's.
    let reply = gql(
        &owner,
        &project,
        "posts",
        "mutation { entries { nodes { title } } }",
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let (message, _, _) = error(&reply);
    assert!(
        message.to_lowercase().contains("mutation"),
        "{message}: {}",
        reply.body
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn introspection_names_the_collections_shape() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, _, _) = seed(&owner).await;

    // The schema is generated from the content type, so `__schema` names it —
    // the tooling a developer points at the endpoint asks nothing more.
    let reply = gql(
        &owner,
        &project,
        "posts",
        "{ __schema { queryType { fields { name } } types { name } } }",
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let fields = names(&reply.body["data"]["__schema"]["queryType"]["fields"]);
    assert_eq!(fields, ["entries", "entry"]);
    let types = names(&reply.body["data"]["__schema"]["types"]);
    for expected in [
        "posts",
        "staff",
        "postsConnection",
        "PageInfo",
        "AppliedQuery",
    ] {
        assert!(types.contains(&expected), "{expected} in {types:?}");
    }
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn the_route_wants_a_signed_in_caller() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (project, _, _) = seed(&owner).await;

    // No session: the API envelope's 401, like every `{P}` route — the GraphQL
    // document is never reached.
    let response = common::app()
        .post(format!(
            "{}{}/collections/posts/graphql",
            common::ORIGIN,
            base(&project)
        ))
        .json(&json!({"query": "{ entries { nodes { title } } }"}))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
