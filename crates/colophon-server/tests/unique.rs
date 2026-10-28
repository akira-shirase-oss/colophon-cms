//! Unique fields, end to end (step M.4, D-070): no two entries of a collection hold the same
//! value of a unique field, among drafts or among what is published, and the database is what
//! says so. A second entry with a taken value is refused, naming the holder only to a role that
//! reads its title; making a field unique is refused while entries share a value; publishing a
//! value another entry still publishes is refused, and swapping two values in one release works.
//! Needs the stack and migrations.
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

async fn send(session: &Session, method: Method, path: &str, body: Option<&Value>) -> Reply {
    call(session, method, path, body, &[("idempotency-key", &key())])
        .await
        .expect("request")
}

async fn content_type(owner: &Session, project: &str, slug: &str, definition: &Value) {
    let made = send(
        owner,
        Method::POST,
        &format!("{}/content-types", base(project)),
        Some(&json!({"slug": slug, "definition": definition})),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
}

/// Replaces `slug`'s definition, expected at `version`.
async fn change_type(
    owner: &Session,
    project: &str,
    slug: &str,
    version: u32,
    definition: &Value,
) -> Reply {
    call(
        owner,
        Method::PUT,
        &format!("{}/content-types/{slug}", base(project)),
        Some(definition),
        &[
            ("if-match", &format!("\"v{version}\"")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request")
}

async fn create(session: &Session, project: &str, collection: &str, content: &Value) -> Reply {
    send(
        session,
        Method::POST,
        &format!("{}/collections/{collection}/entries", base(project)),
        Some(content),
    )
    .await
}

/// Saves `content` over the draft `made` answered with.
async fn save(
    owner: &Session,
    project: &str,
    collection: &str,
    made: &Reply,
    content: &Value,
) -> Reply {
    call(
        owner,
        Method::PUT,
        &format!(
            "{}/collections/{collection}/entries/{}",
            base(project),
            id(made)
        ),
        Some(content),
        &[
            ("if-match", &made.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request")
}

fn id(reply: &Reply) -> String {
    assert!(reply.status.is_success(), "{}", reply.body);
    reply.body["data"]["id"].as_str().expect("an id").to_owned()
}

/// The refusal's reason, pointer, and the entry it names with that entry's state, if any.
fn refusal(reply: &Reply) -> (StatusCode, Value, Value, Value, Value) {
    let details = &reply.body["error"]["details"];
    (
        reply.status,
        details["reason"].clone(),
        details["pointer"].clone(),
        details["entries"][0]["entry"].clone(),
        details["entries"][0]["state"].clone(),
    )
}

/// Submits, approves and tries to publish `entries` as `owner`; the publish answer.
async fn try_release(owner: &Session, project: &str, entries: &[&str]) -> Reply {
    let set = send(
        owner,
        Method::POST,
        &format!("{}/changesets", base(project)),
        Some(&json!({"title": "Release", "entries": entries})),
    )
    .await;
    assert_eq!(set.status, StatusCode::CREATED, "{}", set.body);
    let path = format!(
        "{}/changesets/{}",
        base(project),
        set.body["data"]["id"].as_str().expect("an id")
    );
    let if_match = format!("\"{}\"", set.body["data"]["hash"].as_str().expect("a hash"));
    for (step, body) in [
        ("submit", None),
        ("reviews", Some(json!({"decision": "approve"}))),
    ] {
        let done = call(
            owner,
            Method::POST,
            &format!("{path}/{step}"),
            body.as_ref(),
            &[("if-match", &if_match)],
        )
        .await
        .expect("request");
        assert_eq!(done.status, StatusCode::OK, "{step}: {}", done.body);
    }
    let releases = call(
        owner,
        Method::GET,
        &format!("{}/releases", base(project)),
        None,
        &[],
    )
    .await
    .expect("request");
    let current: i64 = releases
        .headers
        .get("colophon-generation")
        .and_then(|value| value.to_str().ok()?.parse().ok())
        .expect("a generation");
    call(
        owner,
        Method::POST,
        &format!("{path}/publish"),
        Some(&json!({"base_generation": current})),
        &[("if-match", &if_match), ("idempotency-key", &key())],
    )
    .await
    .expect("request")
}

fn people() -> Value {
    json!({
        "name": "People",
        "title_field": "name",
        "fields": [
            {"id": "name", "label": "Name", "type": "text", "required": true},
            {"id": "email", "label": "Email", "type": "email", "unique": true}
        ]
    })
}

/// Codes with a `code` field, unique or not.
fn codes(unique: bool) -> Value {
    let mut code = json!({"id": "code", "label": "Code", "type": "text"});
    if unique {
        code["unique"] = json!(true);
    }
    json!({
        "name": "Codes",
        "title_field": "title",
        "fields": [{"id": "title", "label": "Title", "type": "text", "required": true}, code]
    })
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_unique_value_belongs_to_one_draft_and_one_publication() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    content_type(&owner, &project, "people", &people()).await;

    let ada = create(
        &owner,
        &project,
        "people",
        &json!({"name": "Ada", "email": "ada@example.com"}),
    )
    .await;
    let ada_id = id(&ada);
    let taken = create(
        &owner,
        &project,
        "people",
        &json!({"name": "Bea", "email": "ada@example.com"}),
    )
    .await;
    assert_eq!(
        refusal(&taken),
        (
            StatusCode::CONFLICT,
            json!("value_taken"),
            json!("/email"),
            json!(ada_id),
            json!("draft")
        )
    );
    // Values compare exactly as stored, and leaving one out claims nothing.
    let bea = create(
        &owner,
        &project,
        "people",
        &json!({"name": "Bea", "email": "Ada@example.com"}),
    )
    .await;
    for name in ["Cy", "Di"] {
        id(&create(&owner, &project, "people", &json!({"name": name})).await);
    }
    let onto = save(
        &owner,
        &project,
        "people",
        &bea,
        &json!({"name": "Bea", "email": "ada@example.com"}),
    )
    .await;
    assert_eq!(refusal(&onto).1, json!("value_taken"), "{}", onto.body);
    // An entry keeps its own value.
    let kept = save(
        &owner,
        &project,
        "people",
        &ada,
        &json!({"name": "Ada L.", "email": "ada@example.com"}),
    )
    .await;
    assert_eq!(kept.status, StatusCode::OK, "{}", kept.body);

    // Once Ada is published with it, her draft may move on, and the value stays hers.
    let published = try_release(&owner, &project, &[&ada_id]).await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);
    let moved = save(
        &owner,
        &project,
        "people",
        &kept,
        &json!({"name": "Ada L.", "email": "ada@new.example"}),
    )
    .await;
    assert_eq!(moved.status, StatusCode::OK, "{}", moved.body);
    let still = save(
        &owner,
        &project,
        "people",
        &bea,
        &json!({"name": "Bea", "email": "ada@example.com"}),
    )
    .await;
    assert_eq!(
        refusal(&still),
        (
            StatusCode::CONFLICT,
            json!("value_taken"),
            json!("/email"),
            json!(ada_id),
            json!("published")
        )
    );
    // The value Ada's draft left is free among drafts.
    let free = create(
        &owner,
        &project,
        "people",
        &json!({"name": "Eve", "email": "ada@new.example"}),
    )
    .await;
    assert_eq!(
        refusal(&free).4,
        json!("draft"),
        "her draft has it now: {}",
        free.body
    );
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn making_a_field_unique_waits_until_no_two_entries_share_a_value() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    content_type(&owner, &project, "codes", &codes(false)).await;
    let first = create(
        &owner,
        &project,
        "codes",
        &json!({"title": "First", "code": "same"}),
    )
    .await;
    let second = create(
        &owner,
        &project,
        "codes",
        &json!({"title": "Second", "code": "same"}),
    )
    .await;
    id(&create(
        &owner,
        &project,
        "codes",
        &json!({"title": "Third", "code": "other"}),
    )
    .await);

    let refused = change_type(&owner, &project, "codes", 1, &codes(true)).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    let details = &refused.body["error"]["details"];
    assert_eq!(
        (&details["reason"], &details["pointer"], &details["total"]),
        (
            &json!("needs_migration"),
            &json!("/fields/1/unique"),
            &json!(2)
        )
    );
    let mut in_the_way: Vec<&str> = details["entries"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|entry| {
            assert_eq!(entry["problem"]["reason"], "value_taken");
            assert_eq!(entry["problem"]["pointer"], "/code");
            entry["entry"].as_str().expect("an id")
        })
        .collect();
    in_the_way.sort_unstable();
    let mut both = [id(&first), id(&second)];
    both.sort_unstable();
    assert_eq!(in_the_way, both);

    let changed = save(
        &owner,
        &project,
        "codes",
        &second,
        &json!({"title": "Second", "code": "else"}),
    )
    .await;
    assert_eq!(changed.status, StatusCode::OK, "{}", changed.body);
    let accepted = change_type(&owner, &project, "codes", 1, &codes(true)).await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.body);
    let again = create(
        &owner,
        &project,
        "codes",
        &json!({"title": "Fourth", "code": "same"}),
    )
    .await;
    assert_eq!(refusal(&again).1, json!("value_taken"), "{}", again.body);

    // Dropping it frees every value; making it unique again checks them all over again.
    let dropped = change_type(&owner, &project, "codes", 2, &codes(false)).await;
    assert_eq!(dropped.status, StatusCode::OK, "{}", dropped.body);
    id(&create(
        &owner,
        &project,
        "codes",
        &json!({"title": "Fourth", "code": "same"}),
    )
    .await);
    let twice = change_type(&owner, &project, "codes", 3, &codes(true)).await;
    assert_eq!(
        twice.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        twice.body
    );
    assert_eq!(twice.body["error"]["details"]["total"], 2);
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_release_cannot_publish_a_value_another_entry_still_publishes() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    content_type(&owner, &project, "codes", &codes(false)).await;
    // Before the field is unique: X publishes "k", then its draft moves to "m"; Y's draft is "k".
    let x = create(
        &owner,
        &project,
        "codes",
        &json!({"title": "X", "code": "k"}),
    )
    .await;
    let x_id = id(&x);
    let published = try_release(&owner, &project, &[&x_id]).await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);
    let x_moved = save(
        &owner,
        &project,
        "codes",
        &x,
        &json!({"title": "X", "code": "m"}),
    )
    .await;
    assert_eq!(x_moved.status, StatusCode::OK, "{}", x_moved.body);
    let y_id = id(&create(
        &owner,
        &project,
        "codes",
        &json!({"title": "Y", "code": "k"}),
    )
    .await);
    // Drafts (m, k) and publications (k) share nothing, so the field can become unique.
    let unique = change_type(&owner, &project, "codes", 1, &codes(true)).await;
    assert_eq!(unique.status, StatusCode::OK, "{}", unique.body);

    let alone = try_release(&owner, &project, &[&y_id]).await;
    assert_eq!(alone.status, StatusCode::CONFLICT, "{}", alone.body);
    assert_eq!(alone.body["error"]["details"]["reason"], "value_taken");
    let together = try_release(&owner, &project, &[&x_id, &y_id]).await;
    assert_eq!(together.status, StatusCode::CREATED, "{}", together.body);
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_refusal_names_the_entry_in_the_way_only_to_roles_that_read_its_title() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let writer = sign_in("dev-editor").await;
    let project = project(&owner).await;
    let mut definition = people();
    definition["fields"]
        .as_array_mut()
        .expect("a list")
        .push(json!({"id": "slug", "label": "URL name", "type": "slug", "source": "name"}));
    content_type(&owner, &project, "people", &definition).await;
    let made = send(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/roles"),
        Some(&json!({
            "key": "own-writer",
            "name": "Own writer",
            "permissions": [
                {"action": "content.read", "subject": "people", "condition": "own"},
                {"action": "content.create", "subject": "people"},
                {"action": "content.update", "subject": "people", "condition": "own"}
            ]
        })),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    join(&owner, &writer, &project, "own-writer").await;
    let ada = id(&create(
        &owner,
        &project,
        "people",
        &json!({"name": "Ada", "email": "ada@example.com", "slug": "ada"}),
    )
    .await);

    // The owner reads Ada's title, so the refusal names her; the writer only learns it's taken.
    let to_owner = create(
        &owner,
        &project,
        "people",
        &json!({"name": "Bea", "email": "ada@example.com"}),
    )
    .await;
    assert_eq!(refusal(&to_owner).3, json!(ada));
    for (content, reason) in [
        (
            json!({"name": "Bea", "email": "ada@example.com"}),
            "value_taken",
        ),
        (json!({"name": "Bea", "slug": "ada"}), "slug_taken"),
    ] {
        let refused = create(&writer, &project, "people", &content).await;
        assert_eq!(
            refusal(&refused),
            (
                StatusCode::CONFLICT,
                json!(reason),
                refused.body["error"]["details"]["pointer"].clone(),
                Value::Null,
                Value::Null
            ),
            "{}",
            refused.body
        );
        assert!(
            !refused.body.to_string().contains("Ada"),
            "{}",
            refused.body
        );
    }
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn racing_writes_of_one_value_leave_it_to_exactly_one_entry() {
    let _server = Server::start().await;
    let owner = std::sync::Arc::new(sign_in("dev-owner").await);
    let project = project(&owner).await;
    content_type(&owner, &project, "people", &people()).await;
    // Every precheck runs before any claim commits, so the database alone decides.
    let mut tasks = tokio::task::JoinSet::new();
    for n in 0..8 {
        let (owner, project) = (std::sync::Arc::clone(&owner), project.clone());
        tasks.spawn(async move {
            create(
                &owner,
                &project,
                "people",
                &json!({"name": format!("Racer {n}"), "email": "race@example.com"}),
            )
            .await
        });
    }
    let (mut created, mut taken) = (0, 0);
    while let Some(joined) = tasks.join_next().await {
        let answer = joined.expect("task");
        match answer.status {
            StatusCode::CREATED => created += 1,
            StatusCode::CONFLICT => {
                assert_eq!(
                    answer.body["error"]["details"]["reason"], "value_taken",
                    "{}",
                    answer.body
                );
                taken += 1;
            }
            other => panic!("{other}: {}", answer.body),
        }
    }
    assert_eq!((created, taken), (1, 7));
}
