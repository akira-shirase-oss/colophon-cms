//! API tokens and the audit log in the database (migration 0023; D-063, step R.4): the log
//! refuses every update and delete, from the application and from the table owner; revoking a
//! token waits for a request running under it, and none starts after; and a token is invisible
//! outside its project, found only by its secret's hash. Needs the local stack.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "integration tests: a failed setup step is a test failure"
)]

mod common;

use std::time::Duration;

use colophon_db::{
    audit, projects,
    scope::{self, MemberTransaction},
    tokens::{self, Created, NewToken, Revoked},
};
use colophon_domain::policy::TokenKind;
use common::TestDb;
use sqlx::{PgPool, types::time::OffsetDateTime};
use uuid::Uuid;

async fn as_member(pool: &PgPool, identity: Uuid, project: Uuid) -> MemberTransaction {
    scope::begin_as_member(pool, identity, project)
        .await
        .expect("begin")
        .expect("a member")
}

/// A read-only token in `project`, made by `owner`; its id and identity.
async fn read_only(pool: &PgPool, owner: Uuid, project: Uuid, hash: [u8; 32]) -> (Uuid, Uuid) {
    let policy = TokenKind::ReadOnly.policy(Vec::new()).expect("read-only");
    let mut tx = as_member(pool, owner, project).await;
    let token = NewToken {
        id: Uuid::now_v7(),
        identity_id: Uuid::now_v7(),
        name: "Site build",
        description: "",
        kind: TokenKind::ReadOnly,
        policy: &policy,
        hash,
        expires_at: OffsetDateTime::now_utc() + time::Duration::days(30),
    };
    let Created::Done(stored) = tokens::create(&mut tx, &token).await.expect("create") else {
        panic!("a project's first token was refused");
    };
    tx.commit().await.expect("commit");
    (stored.id, stored.identity_id)
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_audit_log_refuses_every_update_and_delete_from_anyone() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = as_member(&db.app, alice, project).await;
    audit::record(&mut tx, "role.created", "desk", r#"{"name":"Desk"}"#)
        .await
        .expect("recorded");
    tx.commit().await.expect("commit");

    // The application may insert and read, never change: no grant for it.
    for statement in [
        "UPDATE audit_events SET subject = 'forged'",
        "DELETE FROM audit_events",
    ] {
        let mut tx = as_member(&db.app, alice, project).await;
        let refused = sqlx::query(statement).execute(&mut *tx.transaction).await;
        assert!(refused.is_err(), "the application ran: {statement}");
    }
    // Nor the table owner, whom grants don't bind: the trigger refuses it.
    let owner = db.owner().await;
    for statement in [
        "UPDATE audit_events SET subject = 'forged'",
        "DELETE FROM audit_events",
        "TRUNCATE audit_events",
    ] {
        let refused = sqlx::query(statement).execute(&owner).await;
        let message = refused.expect_err(statement).to_string();
        assert!(message.contains("append-only"), "{statement}: {message}");
    }
    let mut tx = as_member(&db.app, alice, project).await;
    let events = audit::list(&mut tx, None, 10, None, None)
        .await
        .expect("read");
    assert_eq!(
        events
            .iter()
            .map(|event| (event.subject.as_str(), event.detail.as_str()))
            .collect::<Vec<_>>(),
        [("desk", r#"{"name": "Desk"}"#)]
    );
    drop(tx);
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn revoking_a_token_waits_for_its_request_and_none_starts_after() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let (token, identity) = read_only(&db.app, alice, project, [3; 32]).await;
    let found = tokens::find(&db.app, &[3; 32])
        .await
        .expect("find")
        .expect("found by its hash");
    assert_eq!((found.id, found.project_id), (token, project));

    // A request running under the token.
    let request = scope::begin_as_token(&db.app, identity, project)
        .await
        .expect("begin")
        .expect("an active token");
    // The owner revokes it meanwhile, on another connection: it waits.
    let others = db.app_pool(2).await;
    let revoking = tokio::spawn({
        let others = others.clone();
        async move {
            let mut tx = as_member(&others, alice, project).await;
            let revoked = tokens::revoke(&mut tx, token).await.expect("revoke");
            tx.commit().await.expect("commit");
            revoked
        }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !revoking.is_finished(),
        "revocation committed while a request ran under the token"
    );
    request.commit().await.expect("the request finishes");
    let revoked = tokio::time::timeout(Duration::from_secs(5), revoking)
        .await
        .expect("revocation goes on once the request is done")
        .expect("joined");
    assert!(matches!(revoked, Revoked::Done(_)), "{revoked:?}");
    // From then on, nothing runs under it.
    assert!(
        scope::begin_as_token(&db.app, identity, project)
            .await
            .expect("begin")
            .is_none(),
        "a revoked token opened a transaction"
    );
    others.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_token_is_invisible_outside_its_project() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let bob = db.administrator("bob").await;
    let alpha = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let beta = projects::create(&db.app, bob, "beta", "Beta")
        .await
        .expect("created")
        .id;
    let (_, identity) = read_only(&db.app, alice, alpha, [5; 32]).await;

    // Bob, in his own project, sees no token at all, however he asks.
    let mut tx = as_member(&db.app, bob, beta).await;
    let seen: i64 = sqlx::query_scalar("SELECT count(*) FROM api_tokens")
        .fetch_one(&mut *tx.transaction)
        .await
        .expect("count");
    let permissions: i64 = sqlx::query_scalar("SELECT count(*) FROM api_token_permissions")
        .fetch_one(&mut *tx.transaction)
        .await
        .expect("count");
    assert_eq!((seen, permissions), (0, 0));
    assert_eq!(tokens::list(&mut tx).await.expect("list"), Vec::new());
    drop(tx);
    // The token itself can't open Bob's project.
    assert!(
        scope::begin_as_token(&db.app, identity, beta)
            .await
            .expect("begin")
            .is_none()
    );
    // And it opens its own with exactly its permissions.
    let own = scope::begin_as_token(&db.app, identity, alpha)
        .await
        .expect("begin")
        .expect("its own project");
    assert_eq!(
        own.policy,
        TokenKind::ReadOnly.policy(Vec::new()).expect("read-only")
    );
    drop(own);
    db.drop().await;
}
