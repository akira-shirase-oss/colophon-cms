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
        limits: None,
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

/// The cap is exact under a race: with one slot left, twenty concurrent creators queue on the
/// owner-role lock and exactly one gets the slot — each counts in a statement that starts after
/// it holds the lock, so it sees the token the creator before it committed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn twenty_creators_racing_for_the_last_slot_make_exactly_one_token() {
    let db = TestDb::new().await;
    let owner = db.administrator("owner").await;
    let project = projects::create(&db.app, owner, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let filled = usize::try_from(tokens::MAX_TOKENS - 1).expect("a count");
    for n in 0..filled {
        let mut hash = [0_u8; 32];
        hash[..8].copy_from_slice(&u64::try_from(n).expect("n").to_be_bytes());
        read_only(&db.app, owner, project, hash).await;
    }
    // Twenty connections, so the racers really run at once (the test pool has one).
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .min_connections(20)
        .connect(&format!(
            "postgres://colophon_app:colophon-dev@{}/{}",
            common::host(),
            db.name
        ))
        .await
        .expect("a wider pool");
    // Every racer opens its transaction, then all start creating at once.
    let start = std::sync::Arc::new(tokio::sync::Barrier::new(20));
    let racers: Vec<_> = (0..20_u8)
        .map(|n| {
            let pool = pool.clone();
            let start = std::sync::Arc::clone(&start);
            tokio::spawn(async move {
                let policy = TokenKind::ReadOnly.policy(Vec::new()).expect("read-only");
                let mut tx = as_member(&pool, owner, project).await;
                start.wait().await;
                let mut hash = [0xff_u8; 32];
                hash[0] = n;
                let token = NewToken {
                    id: Uuid::now_v7(),
                    identity_id: Uuid::now_v7(),
                    name: "Racer",
                    description: "",
                    kind: TokenKind::ReadOnly,
                    policy: &policy,
                    hash,
                    expires_at: OffsetDateTime::now_utc() + time::Duration::days(30),
                    limits: None,
                };
                let made = tokens::create(&mut tx, &token).await.expect("create");
                tx.commit().await.expect("commit");
                matches!(made, Created::Done(_))
            })
        })
        .collect();
    let mut made = 0;
    for racer in racers {
        if racer.await.expect("joined") {
            made += 1;
        }
    }
    assert_eq!(made, 1, "exactly one racer takes the last slot");
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM api_tokens WHERE project_id = $1 AND revoked_at IS NULL",
    )
    .bind(project)
    .fetch_one(&db.owner().await)
    .await
    .expect("count");
    assert_eq!(active, tokens::MAX_TOKENS);
    pool.close().await;
    db.drop().await;
}

/// A token does what both it and its creator's role, as it is now, allow (D-228, migration 0081): a
/// demotion narrows it at its next request, and once its creator is no longer a member it opens
/// nothing, revoked or not; removing them revokes it, with nothing else of the project's touched.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
#[expect(
    clippy::too_many_lines,
    reason = "one story: a token through its creator's promotion, demotion and removal"
)]
async fn a_token_is_held_to_its_creator_s_role_as_it_is_now() {
    use colophon_db::members::{self, Change};
    use colophon_domain::policy::Action;

    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let bob = db.person("bob").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    // Bob joins as a second owner and makes a full-access token; Alice makes a read-only one.
    let mut tx = as_member(&db.app, alice, project).await;
    members::invite(
        &mut tx,
        &[7; 32],
        "owner",
        "Bob",
        OffsetDateTime::now_utc() + time::Duration::hours(1),
    )
    .await
    .expect("invited");
    tx.commit().await.expect("commit");
    members::accept(&db.app, bob, &[7; 32])
        .await
        .expect("accept");
    let full = TokenKind::FullAccess
        .policy(Vec::new())
        .expect("full access");
    let mut tx = as_member(&db.app, bob, project).await;
    let made = NewToken {
        id: Uuid::now_v7(),
        identity_id: Uuid::now_v7(),
        name: "Agent",
        description: "",
        kind: TokenKind::FullAccess,
        policy: &full,
        hash: [8; 32],
        expires_at: OffsetDateTime::now_utc() + time::Duration::days(30),
        limits: None,
    };
    let Created::Done(bobs) = tokens::create(&mut tx, &made).await.expect("create") else {
        panic!("refused");
    };
    tx.commit().await.expect("commit");
    let (alices, alices_identity) = read_only(&db.app, alice, project, [9; 32]).await;
    let policy = async |identity| {
        scope::begin_as_token(&db.app, identity, project)
            .await
            .expect("begin")
            .map(|tx| tx.policy.clone())
    };

    // An owner still: everything the token holds.
    let as_owner = policy(bobs.identity_id).await.expect("a live token");
    assert_eq!(as_owner, full);

    // Demoted to editor: the token reads and writes content, and publishes and changes content
    // types no longer — at once, with nothing cached.
    let mut tx = as_member(&db.app, alice, project).await;
    assert_eq!(
        members::change_role(&mut tx, bob, "editor")
            .await
            .expect("demote"),
        Change::Done
    );
    tx.commit().await.expect("commit");
    let as_editor = policy(bobs.identity_id).await.expect("still a member");
    assert!(as_editor.allows(Action::ContentUpdate, Some("articles")));
    assert!(!as_editor.allows(Action::ContentPublish, Some("articles")));
    assert!(!as_editor.allows(Action::SchemaChange, None));
    assert!(full.covers(&as_editor), "never more than the token holds");

    // Removed: the token opens nothing even before it is revoked (a membership that ends another
    // way, such as a restore's ledger replay)...
    let mut tx = as_member(&db.app, alice, project).await;
    assert_eq!(
        members::remove(&mut tx, bob).await.expect("remove"),
        Change::Done
    );
    tx.commit().await.expect("commit");
    assert!(
        scope::begin_as_token(&db.app, bobs.identity_id, project)
            .await
            .expect("begin")
            .is_none(),
        "a removed member's token opened a transaction"
    );
    // ...and revoking a removed member's tokens (the API does it in the removal's transaction)
    // takes exactly Bob's.
    let mut tx = as_member(&db.app, alice, project).await;
    let revoked = tokens::revoke_created_by(&mut tx, bob)
        .await
        .expect("revoke");
    assert_eq!(
        revoked.iter().map(|token| token.id).collect::<Vec<_>>(),
        [bobs.id]
    );
    assert!(revoked[0].revoked_at.is_some());
    tx.commit().await.expect("commit");
    assert!(
        policy(alices_identity).await.is_some(),
        "Alice's token is untouched"
    );
    let mut tx = as_member(&db.app, alice, project).await;
    let again = tokens::revoke_created_by(&mut tx, bob)
        .await
        .expect("again");
    assert_eq!(again.len(), 0, "nothing left to revoke");
    assert!(
        tokens::get(&mut tx, alices)
            .await
            .expect("get")
            .is_some_and(|t| t.revoked_at.is_none())
    );
    drop(tx);
    db.drop().await;
}
