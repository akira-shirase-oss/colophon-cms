//! Content schema migrations end to end (step M.5a, D-100), against the real stack: a change
//! a `PUT` refuses with `needs_migration` gets a plan instead; the plan's dry run names the
//! draft that wouldn't fit; approval binds the plan's hash like a review binds a changeset's;
//! apply starts the run — `202 running`, truthful even though the worker that would finish it
//! is step M.5b — and a running plan blocks schema writes on its collection until it finishes.
//! Needs the stack and `scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Reply, Server, Session, article, call, entries, join, key, project, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
}

fn migrations(project: &str) -> String {
    format!("{}/migrations", base(project))
}

fn content_type(project: &str, slug: &str) -> String {
    format!("{}/content-types/{slug}", base(project))
}

/// The `articles` type's current definition and version (its `ETag` `"v<N>"`).
async fn current_type(session: &Session, project: &str) -> (Value, String) {
    let reply = call(
        session,
        Method::GET,
        &content_type(project, "articles"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    (
        reply.body["data"]["definition"].clone(),
        reply.etag().expect("an ETag"),
    )
}

/// A definition that adds a required `subtitle` field: the kind of change a `PUT` can only
/// refuse while entries lack it.
fn with_required_subtitle(definition: &Value) -> Value {
    let mut next = definition.clone();
    next["fields"].as_array_mut().expect("fields").push(json!({
        "id": "subtitle",
        "label": "Subtitle",
        "type": "text",
        "required": true,
        "max_chars": 100,
        "multiline": false
    }));
    next
}

/// `POST {P}/migrations` with an idempotency key.
async fn plan(session: &Session, project: &str, definition: &Value, key: &str) -> Reply {
    call(
        session,
        Method::POST,
        &migrations(project),
        Some(&json!({"collection": "articles", "definition": definition})),
        &[("idempotency-key", key)],
    )
    .await
    .expect("request")
}

/// `POST {path}` on a plan route, with `If-Match` and an idempotency key.
async fn act(session: &Session, path: &str, hash: Option<&str>) -> Reply {
    let mut headers = vec![("idempotency-key", key())];
    let etag = hash.map(|hash| format!("\"{hash}\""));
    if let Some(etag) = &etag {
        headers.push(("if-match", etag.clone()));
    }
    let owned: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    call(session, Method::POST, path, None, &owned)
        .await
        .expect("request")
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: refuse, plan, resolve, approve, apply, blocked"
)]
async fn a_plan_dry_runs_blocks_writes_and_applies() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let editor = sign_in("dev-editor").await;
    let project = project(&owner).await;
    join(&owner, &editor, &project, "editor").await;

    // An entry the target can't accept: it lacks the required field the target adds.
    let created = call(
        &editor,
        Method::POST,
        &entries(&project),
        Some(&article("Launch post", "first")),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let entry = created.body["data"]["id"].as_str().expect("id").to_owned();
    let (definition, version) = current_type(&owner, &project).await;
    let target = with_required_subtitle(&definition);

    // The plain update refuses: the draft is in the way.
    let refused = call(
        &owner,
        Method::PUT,
        &content_type(&project, "articles"),
        Some(&target),
        &[("if-match", &version), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(refused.body["error"]["code"], "validation_failed");

    // So it gets a plan. The same key again is the same plan.
    let create_key = key();
    let planned = plan(&owner, &project, &target, &create_key).await;
    assert_eq!(planned.status, StatusCode::CREATED, "{}", planned.body);
    let id = planned.body["data"]["id"].as_str().expect("id").to_owned();
    let path = format!("{}/{id}", migrations(&project));
    let hash = planned.body["data"]["hash"]
        .as_str()
        .expect("a plan hash")
        .to_owned();
    assert_eq!(planned.etag(), Some(format!("\"{hash}\"")));
    assert_eq!(planned.body["data"]["state"], "proposed");
    assert_eq!(planned.body["data"]["collection"], "articles");
    assert_eq!(planned.body["data"]["items"]["expected"], 1);
    assert_eq!(planned.body["data"]["items"]["blocked"], 1);
    let replayed = plan(&owner, &project, &target, &create_key).await;
    assert!(replayed.replayed());
    assert_eq!(replayed.body["data"]["id"], id.as_str());

    // An editor reads the plan but can't make one or approve it.
    let denied_plan = plan(&editor, &project, &target, &key()).await;
    assert_eq!(
        denied_plan.status,
        StatusCode::FORBIDDEN,
        "{}",
        denied_plan.body
    );
    assert_eq!(
        denied_plan.body["error"]["details"]["reason"],
        "missing_permission"
    );

    // Its dry run names the entry that needs input, with the refusal.
    let items = call(
        &editor,
        Method::GET,
        &format!("{path}/items?outcome=needs_input"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(items.status, StatusCode::OK, "{}", items.body);
    let list = items.body["data"].as_array().expect("items");
    assert_eq!(list.len(), 1, "{}", items.body);
    assert_eq!(list[0]["entry_id"], entry.as_str());
    assert_eq!(list[0]["outcome"], "needs_input");
    assert!(list[0]["problem"]["reason"].is_string(), "{}", list[0]);
    let all = call(&editor, Method::GET, &format!("{path}/items"), None, &[])
        .await
        .expect("request");
    assert_eq!(all.status, StatusCode::OK, "{}", all.body);
    assert_eq!(all.body["data"].as_array().map(Vec::len), Some(1));

    // Give the item a document the target accepts: resolved, unblocking the plan.
    let resolved = call(
        &owner,
        Method::PUT,
        &format!("{path}/items/{entry}"),
        Some(&json!({
            "resolution": {
                "title": "Launch post",
                "subtitle": "the short version",
                "body": {"schema": "colophon/rich-text", "version": 1, "doc": {"type": "doc",
                    "content": [{"type": "paragraph", "content": [{"type": "text", "text": "first"}]}]}}
            }
        })),
        &[("if-match", &format!("\"{hash}\"")), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(resolved.status, StatusCode::OK, "{}", resolved.body);
    assert_eq!(resolved.body["data"]["item"]["outcome"], "resolved");
    let after = call(&editor, Method::GET, &path, None, &[])
        .await
        .expect("request");
    assert_eq!(after.body["data"]["items"]["blocked"], 0, "{}", after.body);

    // A resolution the target doesn't accept is refused like a save.
    let bad = call(
        &owner,
        Method::PUT,
        &format!("{path}/items/{entry}"),
        Some(&json!({"resolution": {"title": "Launch post"}})),
        &[
            ("if-match", &format!("\"{hash}\"")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request");
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", bad.body);

    // Every action needs the hash you looked at: no If-Match, a wrong one, then the right one.
    let missing = act(&owner, &format!("{path}/approve"), None).await;
    assert_eq!(
        missing.status,
        StatusCode::PRECONDITION_REQUIRED,
        "{}",
        missing.body
    );
    let moved = act(
        &owner,
        &format!("{path}/approve"),
        Some("sha256:0000000000000000000000000000000000000000000000000000000000000000"),
    )
    .await;
    assert_eq!(moved.status, StatusCode::CONFLICT, "{}", moved.body);
    assert_eq!(moved.body["error"]["details"]["reason"], "plan_moved");
    let denied_approve = act(&editor, &format!("{path}/approve"), Some(&hash)).await;
    assert_eq!(
        denied_approve.status,
        StatusCode::FORBIDDEN,
        "{}",
        denied_approve.body
    );
    let approved = act(&owner, &format!("{path}/approve"), Some(&hash)).await;
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.body);
    assert_eq!(approved.body["data"]["state"], "approved");
    let again = act(&owner, &format!("{path}/approve"), Some(&hash)).await;
    assert_eq!(again.status, StatusCode::CONFLICT, "{}", again.body);
    assert_eq!(
        again.body["error"]["details"]["reason"],
        "plan_not_proposed"
    );

    // Applying starts the run: 202, running, started. While it runs the collection's schema is
    // the run's own.
    let applied = act(&owner, &format!("{path}/apply"), Some(&hash)).await;
    assert_eq!(applied.status, StatusCode::ACCEPTED, "{}", applied.body);
    assert_eq!(applied.body["data"]["state"], "running");
    let blocked = call(
        &owner,
        Method::PUT,
        &content_type(&project, "articles"),
        Some(&target),
        &[("if-match", &version), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(blocked.status, StatusCode::CONFLICT, "{}", blocked.body);
    assert_eq!(
        blocked.body["error"]["details"]["reason"],
        "migration_in_progress"
    );

    // A running plan can't be cancelled in v1: let it finish.
    let cancel = act(&owner, &format!("{path}/cancel"), None).await;
    assert_eq!(cancel.status, StatusCode::CONFLICT, "{}", cancel.body);
    assert_eq!(
        cancel.body["error"]["details"]["reason"],
        "cannot_cancel_running"
    );

    // And the plan is still running: a dry run never was a reservation (D-100).
    let running = call(&owner, Method::GET, &path, None, &[])
        .await
        .expect("request");
    assert_eq!(running.body["data"]["state"], "running");
    let listed = call(
        &owner,
        Method::GET,
        &format!("{}?state=running&collection=articles", migrations(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        listed.body["data"].as_array().map(Vec::len),
        Some(1),
        "{}",
        listed.body
    );
}

/// A change a plain `PUT` could apply takes no plan; a compatible change is refused
/// `no_migration_needed`.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_compatible_change_needs_no_plan() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let (definition, version) = current_type(&owner, &project).await;
    let mut widened = definition.clone();
    widened["fields"]
        .as_array_mut()
        .expect("fields")
        .iter_mut()
        .for_each(|field| {
            if field["id"] == "title" {
                field["max_chars"] = json!(400);
            }
        });
    // The plain update accepts it.
    let updated = call(
        &owner,
        Method::PUT,
        &content_type(&project, "articles"),
        Some(&widened),
        &[("if-match", &version), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);
    // So a plan for it has nothing to do.
    let refused = plan(&owner, &project, &widened, &key()).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"]["reason"],
        "no_migration_needed"
    );
    // Bad filters say so.
    let listed = call(
        &owner,
        Method::GET,
        &format!("{}?state=nonsense", migrations(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        listed.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        listed.body
    );
    assert_eq!(listed.body["error"]["details"]["reason"], "invalid_state");
    let unknown = call(
        &owner,
        Method::GET,
        &format!("{}/{}", migrations(&project), uuid::Uuid::now_v7()),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(unknown.status, StatusCode::NOT_FOUND, "{}", unknown.body);
}
