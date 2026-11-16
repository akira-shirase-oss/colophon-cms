//! Entry locales and localization groups end to end (D-087): the real binary, a real sign-in.
//! `?locale=` labels entries on create and save and filters lists; `POST …/localizations?locale=`
//! makes a sibling from the source's draft, one per locale per group. Needs the stack and
//! migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Reply, Server, Session, call, key, project, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

const PAGES: &str = "pages";
const PAGES_DEF: &str = r#"{"name": "Pages", "title_field": "title", "fields": [
    {"id": "title", "label": "Title", "type": "text", "required": true},
    {"id": "slug", "label": "URL name", "type": "slug"}
]}"#;

fn types(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production/content-types")
}

fn entries(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production/collections/{PAGES}/entries")
}

async fn send(session: &Session, method: Method, path: &str, body: Option<&Value>) -> Reply {
    call(session, method, path, body, &[("idempotency-key", &key())])
        .await
        .expect("request")
}

async fn save(session: &Session, path: &str, etag: &str, body: &Value) -> Reply {
    call(
        session,
        Method::PUT,
        path,
        Some(body),
        &[
            ("if-match", &format!("\"{etag}\"")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request")
}

async fn set_up() -> (Server, Session, String) {
    let server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let definition: Value = serde_json::from_str(PAGES_DEF).expect("definition");
    let created = send(
        &owner,
        Method::POST,
        &types(&project),
        Some(&json!({"slug": PAGES, "definition": definition})),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    (server, owner, project)
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn entries_carry_and_filter_locales() {
    let (_server, owner, project) = set_up().await;
    let list = entries(&project);

    // A create takes its locale; the answer carries it back, unlocalized for now.
    let created = send(
        &owner,
        Method::POST,
        &format!("{list}?locale=en-US"),
        Some(&json!({"title": "Home", "slug": "home"})),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(created.body["data"]["locale"], "en-US");
    assert_eq!(created.body["data"]["localizations"], json!([]));
    let home = created.body["data"]["id"].as_str().expect("id").to_owned();
    let home_etag = created.body["data"]["revision"]["id"]
        .as_str()
        .expect("revision")
        .to_owned();
    send(
        &owner,
        Method::POST,
        &format!("{list}?locale=fr"),
        Some(&json!({"title": "Accueil", "slug": "accueil"})),
    )
    .await;
    send(
        &owner,
        Method::POST,
        &list,
        Some(&json!({"title": "Untagged", "slug": "untagged"})),
    )
    .await;

    // A list filters on the tag; summaries carry it.
    let filtered = send(&owner, Method::GET, &format!("{list}?locale=en-US"), None).await;
    assert_eq!(filtered.status, StatusCode::OK);
    let rows = filtered.body["data"].as_array().expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["locale"], "en-US");

    // A malformed tag is refused on reads and writes alike.
    for path in [
        format!("{list}?locale=not_a_tag"),
        format!("{list}?locale=x"),
    ] {
        let refused = send(&owner, Method::GET, &path, None).await;
        assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY, "{path}");
        assert_eq!(refused.body["error"]["details"]["reason"], "invalid_locale");
    }

    // A save writes the tag in the same transaction; an empty `?locale=` clears it again.
    let tagged = save(
        &owner,
        &format!("{list}/{home}?locale=de"),
        &home_etag,
        &json!({"title": "Home", "slug": "home"}),
    )
    .await;
    assert_eq!(tagged.status, StatusCode::OK);
    assert_eq!(tagged.body["data"]["locale"], "de");
    let etag = tagged.body["data"]["revision"]["id"]
        .as_str()
        .expect("revision")
        .to_owned();
    let cleared = save(
        &owner,
        &format!("{list}/{home}?locale="),
        &etag,
        &json!({"title": "Home", "slug": "home"}),
    )
    .await;
    assert_eq!(cleared.status, StatusCode::OK);
    assert_eq!(cleared.body["data"]["locale"], Value::Null);
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: tag, group, refusal, freed tag"
)]
async fn localization_groups() {
    let (_server, owner, project) = set_up().await;
    let list = entries(&project);

    // An untagged source can't start a group: its own locale must be named first.
    let untagged = send(
        &owner,
        Method::POST,
        &list,
        Some(&json!({"title": "Welcome", "slug": "welcome"})),
    )
    .await;
    let untagged_id = untagged.body["data"]["id"].as_str().expect("id").to_owned();
    let untagged_etag = untagged.body["data"]["revision"]["id"]
        .as_str()
        .expect("revision")
        .to_owned();
    let refused = send(
        &owner,
        Method::POST,
        &format!("{list}/{untagged_id}/localizations?locale=fr"),
        None,
    )
    .await;
    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert_eq!(
        refused.body["error"]["details"]["reason"],
        "locale_required"
    );

    // Tag it, then add the French version: the sibling starts as a copy of the draft with a
    // slug of its own, and both are in one group.
    let tagged = save(
        &owner,
        &format!("{list}/{untagged_id}?locale=en"),
        &untagged_etag,
        &json!({"title": "Welcome", "slug": "welcome"}),
    )
    .await;
    assert_eq!(tagged.status, StatusCode::OK);
    let made = send(
        &owner,
        Method::POST,
        &format!("{list}/{untagged_id}/localizations?locale=fr"),
        None,
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED);
    let sibling = made.body["data"].clone();
    assert_eq!(sibling["locale"], "fr");
    let content = sibling["content"].as_object().expect("content");
    assert_eq!(content["title"], "Welcome");
    assert_eq!(content["slug"], "welcome-fr");
    let group = sibling["localizations"].as_array().expect("group");
    assert_eq!(group.len(), 2);
    assert_eq!(group[0]["locale"], "en");
    assert_eq!(group[0]["id"], untagged_id);
    assert_eq!(group[1]["locale"], "fr");
    assert_eq!(group[1]["id"], sibling["id"]);

    // The source's own read lists the same group.
    let read = send(&owner, Method::GET, &format!("{list}/{untagged_id}"), None).await;
    assert_eq!(
        read.body["data"]["localizations"]
            .as_array()
            .expect("group")
            .len(),
        2
    );

    // The same locale again is refused as taken; the same key replays instead of duplicating.
    let occupied = send(
        &owner,
        Method::POST,
        &format!("{list}/{untagged_id}/localizations?locale=fr"),
        None,
    )
    .await;
    assert_eq!(occupied.status, StatusCode::CONFLICT);
    assert_eq!(occupied.body["error"]["details"]["reason"], "locale_taken");
    let replay_key = key();
    let replay = call(
        &owner,
        Method::POST,
        &format!("{list}/{untagged_id}/localizations?locale=de"),
        None,
        &[("idempotency-key", &replay_key)],
    )
    .await
    .expect("request");
    assert_eq!(replay.status, StatusCode::CREATED);
    let again = call(
        &owner,
        Method::POST,
        &format!("{list}/{untagged_id}/localizations?locale=de"),
        None,
        &[("idempotency-key", &replay_key)],
    )
    .await
    .expect("request");
    assert!(
        again.replayed(),
        "a replayed key answers the recorded draft"
    );
    assert_eq!(again.body["data"]["id"], replay.body["data"]["id"]);

    // A missing locale parameter, a malformed tag and a missing source are refused.
    let missing_param = send(
        &owner,
        Method::POST,
        &format!("{list}/{untagged_id}/localizations"),
        None,
    )
    .await;
    assert_eq!(missing_param.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        missing_param.body["error"]["details"]["reason"],
        "missing_locale"
    );
    let bad_tag = send(
        &owner,
        Method::POST,
        &format!("{list}/{untagged_id}/localizations?locale=!!!"),
        None,
    )
    .await;
    assert_eq!(bad_tag.status, StatusCode::UNPROCESSABLE_ENTITY);
    let missing = send(
        &owner,
        Method::POST,
        &format!("{list}/{}/localizations?locale=fr", uuid::Uuid::now_v7()),
        None,
    )
    .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);

    // A grouped member's locale can't move onto a sibling's, nor be cleared.
    let source_etag = read.body["data"]["revision"]["id"]
        .as_str()
        .expect("revision")
        .to_owned();
    let taken = save(
        &owner,
        &format!("{list}/{untagged_id}?locale=fr"),
        &source_etag,
        &json!({"title": "Welcome", "slug": "welcome"}),
    )
    .await;
    assert_eq!(taken.status, StatusCode::CONFLICT);
    assert_eq!(taken.body["error"]["details"]["reason"], "locale_taken");
    let grouped = save(
        &owner,
        &format!("{list}/{untagged_id}?locale="),
        &source_etag,
        &json!({"title": "Welcome", "slug": "welcome"}),
    )
    .await;
    assert_eq!(grouped.status, StatusCode::CONFLICT);
    assert_eq!(
        grouped.body["error"]["details"]["reason"],
        "locale_required"
    );
}
