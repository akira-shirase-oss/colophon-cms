//! Replay of one-time things (docs/security/attack-surface.md): an invitation joins one person once,
//! whatever was tried with it first. A member who opens it is refused (`AlreadyMember`) without spending
//! it, so the person it was sent to can still join — once — and nobody after them.

use reqwest::{Method, StatusCode};
use serde_json::json;

use super::send;
use crate::common::{Server, call, project, sign_in};

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn an_invitation_joins_one_person_once() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let guest = sign_in("dev-editor").await;
    let project = project(&owner).await;
    let invited = send(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/invitations"),
        Some(&json!({"role": "editor", "label": "Guest"})),
    )
    .await;
    assert_eq!(invited.status, StatusCode::CREATED, "{}", invited.body);
    let token = invited.body["data"]["link"]
        .as_str()
        .and_then(|link| link.split_once('#'))
        .map(|(_, token)| token.to_owned())
        .expect("a token");
    let accept = async |session| {
        call(
            session,
            Method::POST,
            "/api/v1/invitations/accept",
            Some(&json!({"token": token})),
            &[],
        )
        .await
        .expect("request")
    };
    // The owner, a member already: refused, and the invitation is not spent.
    let by_member = accept(&owner).await;
    assert!(
        by_member.status.is_client_error(),
        "{} {}",
        by_member.status,
        by_member.body
    );
    // The guest joins with it, once.
    let joined = accept(&guest).await;
    assert_eq!(joined.status, StatusCode::OK, "{}", joined.body);
    let again = accept(&guest).await;
    assert!(
        again.status.is_client_error(),
        "{} {}",
        again.status,
        again.body
    );
    let members = send(
        &owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/members"),
        None,
    )
    .await;
    assert_eq!(
        members.body["data"].as_array().map(Vec::len),
        Some(2),
        "{}",
        members.body
    );
}
