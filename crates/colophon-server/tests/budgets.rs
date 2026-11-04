//! Token budgets, end to end (migration 0045; D-113, invariant AU3): limits are set on a token
//! at creation and replaced or removed with `PATCH`, read back with the token's usage by the
//! people who manage tokens — a token itself can't reach any of it. An exhausted
//! `documents_per_day` refuses the token's create or save as `budget_exceeded`, while a person
//! writes without a budget at all; and a publish reserves a slot per job it would dispatch, so
//! too many destinations means the release is refused, not queued. Needs the stack and
//! migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{
    ORIGIN, Reply, Server, Session, app, article, call, destination, entries, key, project, sign_in,
};
use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};

fn tokens(project: &str) -> String {
    format!("/api/v1/projects/{project}/tokens")
}

async fn send(session: &Session, method: Method, path: &str, body: Option<&Value>) -> Reply {
    call(session, method, path, body, &[])
        .await
        .expect("request")
}

/// A request made with an API token's secret, as a script would send it.
async fn as_token(
    secret: &str,
    method: Method,
    path: &str,
    body: Option<&Value>,
    extra: &[(&str, &str)],
) -> Reply {
    let mut request = app()
        .request(method, format!("{ORIGIN}{path}"))
        .header(header::AUTHORIZATION, format!("Bearer {secret}"))
        .header("idempotency-key", key());
    for (name, value) in extra {
        request = request.header(*name, *value);
    }
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request.send().await.expect("request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json().await.unwrap_or(Value::Null);
    Reply {
        status,
        headers,
        body,
    }
}

/// Makes a token; its secret and its view.
async fn make(session: &Session, project: &str, body: &Value) -> (String, Value) {
    let made = send(session, Method::POST, &tokens(project), Some(body)).await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let secret = made.body["data"]["secret"]
        .as_str()
        .expect("the secret, once")
        .to_owned();
    (secret, made.body["data"].clone())
}

fn reason(reply: &Reply) -> &Value {
    &reply.body["error"]["details"]["reason"]
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: set, read, replace, remove, audit"
)]
async fn limits_are_set_read_changed_and_audited_for_people_only() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;

    // Limits travel with the creation body and come back in the view.
    let (secret, view) = make(
        &owner,
        &project,
        &json!({
            "name": "Budgeted", "kind": "full-access", "expires_in_days": 30,
            "limits": {"documents_per_day": 5, "max_concurrent_jobs": 2}
        }),
    )
    .await;
    let id = view["id"].as_str().expect("an id").to_owned();
    assert_eq!(
        (
            &view["limits"]["documents_per_day"],
            &view["limits"]["max_concurrent_jobs"],
            view["limits"].get("requests_per_minute")
        ),
        (&json!(5), &json!(2), Some(&Value::Null)),
        "{view}"
    );

    // The single-token read adds what it holds or spent; the list does not.
    let one = format!("{}/{id}", tokens(&project));
    let read = send(&owner, Method::GET, &one, None).await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    assert_eq!(
        (
            &read.body["data"]["usage"]["documents_last_day"],
            &read.body["data"]["usage"]["jobs_open"],
            &read.body["data"]["usage"]["requests_last_minute"]
        ),
        (&json!(0), &json!(0), &json!(0))
    );
    let list = send(&owner, Method::GET, &tokens(&project), None).await;
    assert!(
        list.body["data"][0].get("usage").is_none(),
        "usage is a single-token answer: {}",
        list.body
    );

    // PATCH replaces the row, whole: a dimension left out is unconstrained again.
    let replaced = send(
        &owner,
        Method::PATCH,
        &one,
        Some(&json!({"limits": {"requests_per_minute": 100}})),
    )
    .await;
    assert_eq!(replaced.status, StatusCode::OK, "{}", replaced.body);
    assert_eq!(
        (
            replaced.body["data"]["limits"].get("documents_per_day"),
            &replaced.body["data"]["limits"]["requests_per_minute"]
        ),
        (Some(&Value::Null), &json!(100))
    );

    // `null` removes the ceilings; a missing key is a refusal, not a no-op.
    let removed = send(&owner, Method::PATCH, &one, Some(&json!({"limits": null}))).await;
    assert_eq!(
        (removed.status, removed.body["data"].get("limits")),
        (StatusCode::OK, Some(&Value::Null)),
        "{}",
        removed.body
    );
    let nothing = send(&owner, Method::PATCH, &one, Some(&json!({}))).await;
    assert_eq!(
        (nothing.status, reason(&nothing)),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            &json!("nothing_to_change")
        ),
        "{}",
        nothing.body
    );

    // A value out of range or an empty object is refused at the door, on create and patch.
    for (limits, why) in [
        (json!({"documents_per_day": 0}), "invalid_limit"),
        (json!({"max_concurrent_jobs": 65}), "invalid_limit"),
        (json!({}), "limits_empty"),
    ] {
        let patched = send(
            &owner,
            Method::PATCH,
            &one,
            Some(&json!({"limits": limits.clone()})),
        )
        .await;
        assert_eq!(
            (patched.status, reason(&patched)),
            (StatusCode::UNPROCESSABLE_ENTITY, &json!(why)),
            "{limits}: {}",
            patched.body
        );
        let made = send(
            &owner,
            Method::POST,
            &tokens(&project),
            Some(&json!({
                "name": "Bad limits", "kind": "read-only", "expires_in_days": 1,
                "limits": limits
            })),
        )
        .await;
        assert_eq!(
            (made.status, reason(&made)),
            (StatusCode::UNPROCESSABLE_ENTITY, &json!(why)),
            "{limits}: {}",
            made.body
        );
    }

    // Limits and usage are people-facing: the token's own credential is refused.
    for path in [tokens(&project), one.clone()] {
        let reply = as_token(&secret, Method::GET, &path, None, &[]).await;
        assert_eq!(
            (reply.status, reason(&reply)),
            (StatusCode::FORBIDDEN, &json!("api_token_not_allowed")),
            "{path}: {}",
            reply.body
        );
    }
    let patched_by_token = as_token(
        &secret,
        Method::PATCH,
        &one,
        Some(&json!({"limits": {"documents_per_day": 9}})),
        &[],
    )
    .await;
    assert_eq!(
        (patched_by_token.status, reason(&patched_by_token)),
        (StatusCode::FORBIDDEN, &json!("api_token_not_allowed")),
        "{}",
        patched_by_token.body
    );

    // Every change is in the log: the create recorded its limits, each change its before/after.
    let log = send(
        &owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/audit"),
        None,
    )
    .await;
    let actions: Vec<&str> = log.body["data"]
        .as_array()
        .expect("events")
        .iter()
        .filter_map(|event| event["action"].as_str())
        .collect();
    assert_eq!(
        actions,
        [
            "token.limits_changed",
            "token.limits_changed",
            "token.created"
        ],
        "{}",
        log.body
    );
    let changed = &log.body["data"][0];
    assert_eq!(
        (
            &changed["detail"]["limits"]["before"]["requests_per_minute"],
            &changed["detail"]["limits"]["after"]
        ),
        (&json!(100), &Value::Null),
        "{changed}"
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn an_exhausted_documents_budget_refuses_creates_and_saves_but_not_people() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let (secret, view) = make(
        &owner,
        &project,
        &json!({
            "name": "Two a day", "kind": "full-access", "expires_in_days": 30,
            "limits": {"documents_per_day": 2}
        }),
    )
    .await;
    let id = view["id"].as_str().expect("an id").to_owned();

    // Two writes fit; the third is refused before it starts — the same for a save.
    let mut first = None;
    for n in 0..2 {
        let written = as_token(
            &secret,
            Method::POST,
            &entries(&project),
            Some(&article(&format!("Piece {n}"), "x")),
            &[],
        )
        .await;
        assert_eq!(written.status, StatusCode::CREATED, "{}", written.body);
        if n == 0 {
            first = Some((
                format!(
                    "{}/{}",
                    entries(&project),
                    written.body["data"]["id"].as_str().expect("an id")
                ),
                written.etag().expect("an ETag"),
            ));
        }
    }
    let refused = as_token(
        &secret,
        Method::POST,
        &entries(&project),
        Some(&article("One too many", "x")),
        &[],
    )
    .await;
    assert_eq!(
        (refused.status, reason(&refused)),
        (StatusCode::FORBIDDEN, &json!("budget_exceeded")),
        "a hard quota, not rate shaping: {}",
        refused.body
    );
    let (path, etag) = first.expect("the first write's path and ETag");
    let saved = as_token(
        &secret,
        Method::PUT,
        &path,
        Some(&article("Over the budget", "x")),
        &[("if-match", &etag)],
    )
    .await;
    assert_eq!(
        (saved.status, reason(&saved)),
        (StatusCode::FORBIDDEN, &json!("budget_exceeded")),
        "{}",
        saved.body
    );

    // The ledger says two spent; a person, who has no limits row, writes unaffected.
    let one = send(
        &owner,
        Method::GET,
        &format!("{}/{id}", tokens(&project)),
        None,
    )
    .await;
    assert_eq!(one.body["data"]["usage"]["documents_last_day"], json!(2));
    let by_owner = call(
        &owner,
        Method::POST,
        &entries(&project),
        Some(&article("The owner's own", "x")),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(by_owner.status, StatusCode::CREATED, "{}", by_owner.body);

    // Raising the ceiling lets it write again; the next write counts against the new limit.
    let raised = send(
        &owner,
        Method::PATCH,
        &format!("{}/{id}", tokens(&project)),
        Some(&json!({"limits": {"documents_per_day": 3}})),
    )
    .await;
    assert_eq!(raised.status, StatusCode::OK, "{}", raised.body);
    let third = as_token(
        &secret,
        Method::POST,
        &entries(&project),
        Some(&article("Now it fits", "x")),
        &[],
    )
    .await;
    assert_eq!(third.status, StatusCode::CREATED, "{}", third.body);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: reserve, exhaust, release, then allow again"
)]
async fn a_publish_reserves_a_slot_per_job_or_is_refused() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let base = format!("/api/v1/projects/{project}/environments/production");
    let sets = format!("{base}/changesets");

    let (agent, view) = make(
        &owner,
        &project,
        &json!({
            "name": "Publisher", "kind": "full-access", "expires_in_days": 1,
            "limits": {"max_concurrent_jobs": 1, "documents_per_day": 20}
        }),
    )
    .await;
    let id = view["id"].as_str().expect("an id").to_owned();

    // Two destinations: one dispatch needs two slots.
    destination(&owner, &project, "https://one.example.com/hook").await;
    destination(&owner, &project, "https://two.example.com/hook").await;

    // The token proposes; a person approves (AU2); the token publishes.
    let entry = {
        let drafted = as_token(
            &agent,
            Method::POST,
            &entries(&project),
            Some(&article("Bound for the sites", "x")),
            &[],
        )
        .await;
        assert_eq!(drafted.status, StatusCode::CREATED, "{}", drafted.body);
        drafted.body["data"]["id"]
            .as_str()
            .expect("an id")
            .to_owned()
    };
    let set = as_token(
        &agent,
        Method::POST,
        &sets,
        Some(&json!({"title": "Announce it", "entries": [entry]})),
        &[],
    )
    .await;
    assert_eq!(set.status, StatusCode::CREATED, "{}", set.body);
    let path = format!("{sets}/{}", set.body["data"]["id"].as_str().expect("an id"));
    let hash = format!("\"{}\"", set.body["data"]["hash"].as_str().expect("a hash"));
    let submitted = as_token(
        &agent,
        Method::POST,
        &format!("{path}/submit"),
        None,
        &[("if-match", hash.as_str())],
    )
    .await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);
    let approved = call(
        &owner,
        Method::POST,
        &format!("{path}/reviews"),
        Some(&json!({"decision": "approve"})),
        &[("if-match", &hash)],
    )
    .await
    .expect("request");
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.body);
    let releases = send(&owner, Method::GET, &format!("{base}/releases"), None).await;
    let current: i64 = releases
        .headers
        .get("colophon-generation")
        .and_then(|value| value.to_str().ok()?.parse().ok())
        .expect("a generation");

    // Two jobs, one slot: the release is refused outright rather than dispatched half-way.
    let refused = as_token(
        &agent,
        Method::POST,
        &format!("{path}/publish"),
        Some(&json!({"base_generation": current})),
        &[("if-match", &hash), ("idempotency-key", &key())],
    )
    .await;
    assert_eq!(
        (refused.status, reason(&refused)),
        (StatusCode::FORBIDDEN, &json!("budget_exceeded")),
        "{}",
        refused.body
    );

    // Room for both: the publish stands, and the usage read shows two slots held.
    let raised = send(
        &owner,
        Method::PATCH,
        &format!("{}/{id}", tokens(&project)),
        Some(&json!({"limits": {"max_concurrent_jobs": 2, "documents_per_day": 20}})),
    )
    .await;
    assert_eq!(raised.status, StatusCode::OK, "{}", raised.body);
    let published = as_token(
        &agent,
        Method::POST,
        &format!("{path}/publish"),
        Some(&json!({"base_generation": current})),
        &[("if-match", &hash), ("idempotency-key", &key())],
    )
    .await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);
    let one = send(
        &owner,
        Method::GET,
        &format!("{}/{id}", tokens(&project)),
        None,
    )
    .await;
    assert_eq!(
        one.body["data"]["usage"]["jobs_open"],
        json!(2),
        "a held slot per dispatched job: {}",
        one.body
    );
}
