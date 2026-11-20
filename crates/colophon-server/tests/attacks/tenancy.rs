//! Cross-project access and IDOR (docs/security/attack-surface.md): another project's entries, releases,
//! receipts, assets and schedules are not found — 404, never 403, so nothing confirms they exist — for a
//! stranger, for a member of another project and for another project's token; and a token's path can't be
//! bent past its project's `environments/` routes.

use reqwest::{Method, StatusCode};
use serde_json::json;

use super::{anonymous, as_token, refused, send, token};
use crate::common::{Server, article, entries, project, sign_in};

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn another_project_s_rows_are_not_found_by_anyone_outside_it() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let stranger = sign_in("dev-editor").await;
    let theirs = project(&owner).await;
    let mine = project(&stranger).await;
    let created = send(
        &owner,
        Method::POST,
        &entries(&theirs),
        Some(&article("Private", "Body")),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let entry = created.body["data"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let other_token = token(
        &stranger,
        &mine,
        &json!({"name": "Mine", "kind": "full-access", "expires_in_days": 1}),
    )
    .await;

    let p = format!("/api/v1/projects/{theirs}");
    let e = format!("{p}/environments/production");
    let paths = [
        format!("{}/{entry}", entries(&theirs)),
        entries(&theirs),
        format!("{e}/releases/1"),
        format!("{e}/releases/1/changes"),
        format!("{e}/schedules"),
        format!("{e}/changesets"),
        format!("{e}/assets"),
        format!("{e}/content-types"),
        format!("{p}/members"),
        format!("{p}/tokens"),
        format!("{p}/audit"),
    ];
    for path in &paths {
        let as_member_elsewhere = send(&stranger, Method::GET, path, None).await;
        assert_eq!(
            as_member_elsewhere.status,
            StatusCode::NOT_FOUND,
            "{path}: {}",
            as_member_elsewhere.body
        );
        let as_other_token = as_token(&other_token, Method::GET, path, None).await;
        assert!(
            matches!(
                as_other_token.status,
                StatusCode::NOT_FOUND | StatusCode::FORBIDDEN
            ),
            "{path}: {} {}",
            as_other_token.status,
            as_other_token.body
        );
        let as_nobody = anonymous(Method::GET, path, None).await;
        assert_eq!(as_nobody.status, StatusCode::UNAUTHORIZED, "{path}");
    }
    // Writes the same way: nothing lands in their project.
    let write = send(
        &stranger,
        Method::POST,
        &entries(&theirs),
        Some(&article("Planted", "x")),
    )
    .await;
    assert_eq!(write.status, StatusCode::NOT_FOUND, "{}", write.body);
    let planted = as_token(
        &other_token,
        Method::POST,
        &entries(&theirs),
        Some(&article("Planted", "x")),
    )
    .await;
    refused(&planted, "another project's token writing here");
    let listed = send(&owner, Method::GET, &entries(&theirs), None).await;
    assert_eq!(
        listed.body["data"].as_array().map(Vec::len),
        Some(1),
        "{}",
        listed.body
    );
}

/// A token is confined to its project's `environments/` routes (D-063) by its path as the router sees
/// it: a trailing slash, a percent-encoded slash, a dot segment or a slug in place of the project's id
/// never reaches a route the token may not use.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_token_s_path_can_t_be_bent_past_its_scope() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let secret = token(
        &owner,
        &project,
        &json!({"name": "Agent", "kind": "full-access", "expires_in_days": 1}),
    )
    .await;
    let p = format!("/api/v1/projects/{project}");
    for path in [
        format!("{p}/members"),
        format!("{p}/members/"),
        format!("{p}/tokens"),
        format!("{p}/environments/production/../../tokens"),
        format!("{p}/environments%2Fproduction%2F..%2F..%2Ftokens"),
        format!("{p}/environments/production/%2E%2E/%2E%2E/tokens"),
        format!("{p}/audit"),
        "/api/v1/projects".to_owned(),
        "/me".to_owned(),
    ] {
        let reply = as_token(&secret, Method::GET, &path, None).await;
        refused(&reply, &path);
    }
    // A project named by anything but its id is not found.
    let slug = send(&owner, Method::GET, &p, None).await;
    let slug = slug.body["data"]["slug"]
        .as_str()
        .expect("a slug")
        .to_owned();
    let by_slug = as_token(
        &secret,
        Method::GET,
        &format!("/api/v1/projects/{slug}/environments/production/content-types"),
        None,
    )
    .await;
    assert_eq!(by_slug.status, StatusCode::NOT_FOUND, "{}", by_slug.body);
    // Scheduling is a person's act, however the path is spelled (D-063).
    for suffix in ["schedule", "schedule/", "schedule?x=1"] {
        let reply = as_token(
            &secret,
            Method::POST,
            &format!("{p}/environments/production/changesets/00000000-0000-7000-8000-000000000000/{suffix}"),
            Some(&json!({"at": "2030-01-01T00:00", "zone": "UTC"})),
        )
        .await;
        refused(&reply, suffix);
    }
}
