//! Token scope escalation (docs/security/attack-surface.md): a token never does more than the person who
//! made it still may (D-228). Before, an owner demoted to editor kept publishing through the token they
//! had made, and one removed from the project kept getting in with it.

use reqwest::{Method, StatusCode};
use serde_json::json;

use super::{as_token, identity_of, send, token};
use crate::common::{Server, join, project, sign_in};

fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story: a token through its creator's demotion and removal"
)]
async fn a_token_narrows_with_its_creator_and_dies_with_their_membership() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let other = sign_in("dev-editor").await;
    let project = project(&owner).await;
    // A second owner makes a full-access token: content, content types, publishing.
    join(&owner, &other, &project, "owner").await;
    let secret = token(
        &other,
        &project,
        &json!({"name": "Agent", "kind": "full-access", "expires_in_days": 1}),
    )
    .await;
    let schema = |slug: &str| {
        json!({"slug": slug, "definition": {"name": "Notes", "title_field": "title",
            "fields": [{"id": "title", "label": "Title", "type": "text", "required": true}]}})
    };
    let made = as_token(
        &secret,
        Method::POST,
        &format!("{}/content-types", base(&project)),
        Some(&schema("notes")),
    )
    .await;
    assert_eq!(
        made.status,
        StatusCode::CREATED,
        "an owner's token changes content types: {}",
        made.body
    );

    // Demoted to editor: the token's next request is held to what an editor may do.
    let them = identity_of(&other).await;
    let demoted = send(
        &owner,
        Method::PUT,
        &format!("/api/v1/projects/{project}/members/{them}"),
        Some(&json!({"role": "editor"})),
    )
    .await;
    assert!(demoted.status.is_success(), "{}", demoted.body);
    let after = as_token(
        &secret,
        Method::POST,
        &format!("{}/content-types", base(&project)),
        Some(&schema("pages")),
    )
    .await;
    assert_eq!(
        after.status,
        StatusCode::FORBIDDEN,
        "a demoted owner's token: {}",
        after.body
    );
    let read = as_token(
        &secret,
        Method::GET,
        &format!("{}/collections/notes/entries", base(&project)),
        None,
    )
    .await;
    assert_eq!(
        read.status,
        StatusCode::OK,
        "it still does what an editor may: {}",
        read.body
    );

    // Removed: refused at once, and revoked, with an audit row saying why.
    let removed = send(
        &owner,
        Method::DELETE,
        &format!("/api/v1/projects/{project}/members/{them}"),
        None,
    )
    .await;
    assert!(removed.status.is_success(), "{}", removed.body);
    let gone = as_token(
        &secret,
        Method::GET,
        &format!("{}/collections/notes/entries", base(&project)),
        None,
    )
    .await;
    assert_eq!(
        gone.status,
        StatusCode::UNAUTHORIZED,
        "a removed member's token: {}",
        gone.body
    );
    let audit = send(
        &owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/audit?action=token.revoked"),
        None,
    )
    .await;
    assert_eq!(audit.status, StatusCode::OK, "{}", audit.body);
    let rows = audit.body["data"].as_array().expect("events");
    assert_eq!(rows.len(), 1, "{}", audit.body);
    assert_eq!(
        rows[0]["detail"]["reason"], "creator_removed",
        "{}",
        audit.body
    );
    let listed = send(
        &owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/tokens"),
        None,
    )
    .await;
    assert!(
        listed.body["data"]
            .as_array()
            .expect("tokens")
            .iter()
            .all(|token| token["state"] == "revoked"),
        "{}",
        listed.body
    );
}
