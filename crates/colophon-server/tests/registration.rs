//! Registration (D-235) against the real binary: `closed` refuses every sign-up, `invite` refuses
//! one without a valid project invitation once the installation is set up and admits one that
//! claims an invitation (joining its project), and `open` admits any new email. The race and
//! first-run rules are proven at the database (`colophon-db/tests/registration.rs`) and on a
//! fresh installation (`setup.rs`). Needs the stack and `scripts/migrate.sh` (dev-owner is the
//! administrator).
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{ORIGIN, Reply, Server, app, call, owner_database, project, sign_in, unique_slug};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

fn fresh_email() -> String {
    format!("reg-{}@colophon.test", unique_slug())
}

async fn sign_up(body: &Value) -> Reply {
    let response = app()
        .post(format!("{ORIGIN}/auth/signup"))
        .json(body)
        .send()
        .await
        .expect("request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json::<Value>().await.unwrap_or(Value::Null);
    Reply {
        status,
        headers,
        body,
    }
}

fn reason(reply: &Reply) -> Option<&str> {
    reply.body["error"]["details"]["reason"].as_str()
}

async fn has_account(email: &str) -> bool {
    let db = owner_database().await;
    let found = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM identities WHERE email = $1)")
        .bind(email)
        .fetch_one(&db)
        .await
        .expect("query");
    db.close().await;
    found
}

/// A project of dev-owner's with one editor invitation; `(project, token)`.
async fn invitation() -> (String, String) {
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let invited = call(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/invitations"),
        Some(&json!({"role": "editor", "label": "A newcomer"})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(invited.status, StatusCode::CREATED, "{}", invited.body);
    let link = invited.body["data"]["link"].as_str().expect("a link");
    let (_, token) = link.split_once('#').expect("the token after #");
    (project, token.to_owned())
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn closed_registration_refuses_every_sign_up_and_makes_no_account() {
    let _server = Server::start_with(&[("COLOPHON_REGISTRATION", "closed")]).await;
    let email = fresh_email();
    let refused = sign_up(&json!({"email": email, "password": "colophon-dev"})).await;
    assert_eq!(
        (refused.status, reason(&refused)),
        (StatusCode::FORBIDDEN, Some("registration_closed")),
        "{}",
        refused.body
    );
    assert!(
        refused.headers.get("set-cookie").is_none(),
        "no session for a refused sign-up"
    );
    assert!(!has_account(&email).await, "no account was made");
    // An invitation doesn't open a closed installation either.
    let (_, token) = invitation().await;
    let refused =
        sign_up(&json!({"email": email, "password": "colophon-dev", "invitation": token})).await;
    assert_eq!(
        reason(&refused),
        Some("registration_closed"),
        "{}",
        refused.body
    );
    assert!(!has_account(&email).await);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn invite_only_refuses_a_sign_up_without_a_valid_invitation_alike() {
    let _server = Server::start_with(&[("COLOPHON_REGISTRATION", "invite")]).await;
    // dev-owner is the administrator: the first-run window is closed.
    let taken = "dev-editor@colophon.test";
    sign_in("dev-editor").await;
    let new = fresh_email();
    for body in [
        json!({"email": new, "password": "colophon-dev"}),
        json!({"email": new, "password": "colophon-dev", "invitation": "not-a-real-token"}),
        json!({"email": taken, "password": "colophon-dev"}),
    ] {
        let refused = sign_up(&body).await;
        assert_eq!(
            (refused.status, reason(&refused)),
            (StatusCode::FORBIDDEN, Some("invitation_required")),
            "{body}: {}",
            refused.body
        );
        assert!(refused.headers.get("set-cookie").is_none());
    }
    assert!(!has_account(&new).await, "no account was made");
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn invite_only_admits_a_sign_up_that_claims_its_invitation_once_and_joins() {
    let _server = Server::start_with(&[("COLOPHON_REGISTRATION", "invite")]).await;
    let (project, token) = invitation().await;
    let email = fresh_email();
    let made =
        sign_up(&json!({"email": email, "password": "colophon-dev", "invitation": token})).await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    assert_eq!(
        (
            made.body["data"]["joined"]["project_id"].as_str(),
            made.body["data"]["joined"]["role"].as_str()
        ),
        (Some(project.as_str()), Some("editor")),
        "{}",
        made.body
    );
    // The new account is in the project, with the invitation's role.
    let session = sign_in(email.trim_end_matches("@colophon.test")).await;
    let view = call(
        &session,
        Method::GET,
        &format!("/api/v1/projects/{project}"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(view.body["data"]["role"], "editor", "{}", view.body);
    // The invitation is spent: a second sign-up with it is refused like a made-up one.
    let second = fresh_email();
    let refused =
        sign_up(&json!({"email": second, "password": "colophon-dev", "invitation": token})).await;
    assert_eq!(
        (refused.status, reason(&refused)),
        (StatusCode::FORBIDDEN, Some("invitation_required")),
        "{}",
        refused.body
    );
    assert!(!has_account(&second).await);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn open_registration_admits_a_new_email() {
    let _server = Server::start_with(&[("COLOPHON_REGISTRATION", "open")]).await;
    let email = fresh_email();
    let made = sign_up(&json!({"email": email, "password": "colophon-dev"})).await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    assert_eq!(made.body["data"]["joined"], Value::Null, "{}", made.body);
    assert!(has_account(&email).await);
}
