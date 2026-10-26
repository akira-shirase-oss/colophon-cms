//! Members and invitations end to end: the real binary, two real people. An owner invites, the
//! invitee sees what they're joining and accepts, roles decide what each may do, the last owner
//! can't step down alone, and a member can leave. Needs the stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Server, Session, article, call, entries, key, project, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::json;

fn members(project: &str) -> String {
    format!("/api/v1/projects/{project}/members")
}

async fn status(
    session: &Session,
    method: Method,
    path: &str,
    body: Option<&serde_json::Value>,
) -> StatusCode {
    call(session, method, path, body, &[("idempotency-key", &key())])
        .await
        .expect("request")
        .status
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: invite, join, roles, leave"
)]
async fn an_invitation_brings_someone_in_and_their_role_decides_what_they_may_do() {
    let _server = Server::start().await;
    let owner = sign_in("dev-editor").await;
    let guest = sign_in("dev-owner").await;
    let project = project(&owner).await;

    // The owner invites; the token only ever travels after '#', or in a body.
    let invited = call(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/invitations"),
        Some(&json!({"role": "editor", "label": "A guest editor"})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(invited.status, StatusCode::CREATED, "{}", invited.body);
    let link = invited.body["data"]["link"]
        .as_str()
        .expect("a link")
        .to_owned();
    let (page, token) = link.split_once('#').expect("the token after #");
    assert!(page.ends_with("/studio/invite"), "{link}");
    let pending = call(
        &owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/invitations"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(pending.body["data"][0]["label"], "A guest editor");
    assert!(
        pending.body["data"][0].get("link").is_none(),
        "the link is shown once"
    );

    // The guest sees what they're joining, then joins.
    let offer = call(
        &guest,
        Method::POST,
        "/api/v1/invitations/lookup",
        Some(&json!({"token": token})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        (
            offer.body["data"]["role"].as_str(),
            offer.body["data"]["invited_by"].as_str()
        ),
        (Some("editor"), Some("Dev Editor"))
    );
    assert_eq!(offer.body["data"]["project_name"], "Test project");
    let joined = call(
        &guest,
        Method::POST,
        "/api/v1/invitations/accept",
        Some(&json!({"token": token})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(joined.status, StatusCode::OK, "{}", joined.body);
    let again = call(
        &guest,
        Method::POST,
        "/api/v1/invitations/accept",
        Some(&json!({"token": token})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        again.status,
        StatusCode::NOT_FOUND,
        "an invitation works once"
    );
    let listed = call(&guest, Method::GET, &members(&project), None, &[])
        .await
        .expect("request");
    let roles: Vec<&str> = listed.body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|m| m["role"].as_str())
        .collect();
    assert_eq!(roles, ["owner", "editor"]);

    // As an editor: content yes, content types and members no.
    assert_eq!(
        status(
            &guest,
            Method::POST,
            &entries(&project),
            Some(&article("By the guest", "x"))
        )
        .await,
        StatusCode::CREATED
    );
    let types = format!("/api/v1/projects/{project}/environments/production/content-types");
    let notes = json!({"slug": "notes", "definition": {"name": "Notes", "title_field": "t", "fields": [{"id": "t", "label": "T", "type": "text", "required": true}]}});
    assert_eq!(
        status(&guest, Method::POST, &types, Some(&notes)).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(
            &guest,
            Method::POST,
            &format!("/api/v1/projects/{project}/invitations"),
            Some(&json!({"role": "owner", "label": "me again"}))
        )
        .await,
        StatusCode::FORBIDDEN
    );
    let guest_id = listed.body["data"][1]["identity_id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let owner_id = listed.body["data"][0]["identity_id"]
        .as_str()
        .expect("an id")
        .to_owned();
    assert_eq!(
        status(
            &guest,
            Method::PUT,
            &format!("{}/{guest_id}", members(&project)),
            Some(&json!({"role": "owner"}))
        )
        .await,
        StatusCode::FORBIDDEN,
        "no promoting yourself"
    );

    // Promoted to developer, the guest may define content types.
    assert_eq!(
        status(
            &owner,
            Method::PUT,
            &format!("{}/{guest_id}", members(&project)),
            Some(&json!({"role": "developer"}))
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        status(&guest, Method::POST, &types, Some(&notes)).await,
        StatusCode::CREATED
    );

    // The last owner can't step down alone; with a second owner, they can.
    let me = format!("{}/{owner_id}", members(&project));
    assert_eq!(
        status(&owner, Method::PUT, &me, Some(&json!({"role": "editor"}))).await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        status(&owner, Method::DELETE, &me, None).await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        status(
            &owner,
            Method::PUT,
            &format!("{}/{guest_id}", members(&project)),
            Some(&json!({"role": "owner"}))
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        status(&owner, Method::PUT, &me, Some(&json!({"role": "editor"}))).await,
        StatusCode::NO_CONTENT
    );

    // A member can leave, and then the project is gone for them.
    assert_eq!(
        status(&owner, Method::DELETE, &me, None).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        status(&owner, Method::GET, &members(&project), None).await,
        StatusCode::NOT_FOUND
    );
    let unknown = call(
        &guest,
        Method::POST,
        "/api/v1/invitations/lookup",
        Some(&json!({"token": "x".repeat(43)})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
}
