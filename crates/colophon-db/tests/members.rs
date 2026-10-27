//! Members and invitations against real PostgreSQL, as the application's login: a project never
//! loses its last owner (even when two owners demote each other at once), an owner demoted while
//! their request runs can no longer manage members, an invitation joins once, an expired or revoked one is dead, and an existing member can't use one up. Needs the
//! local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    PgPool,
    members::{self, Accepted, Change},
    projects,
    scope::{self, MemberTransaction},
};
use colophon_domain::policy::{Policy, Role};
use common::TestDb;
use sqlx::types::time::OffsetDateTime;
use uuid::Uuid;

async fn as_member(pool: &PgPool, identity: Uuid, project: Uuid) -> MemberTransaction {
    scope::begin_as_member(pool, identity, project)
        .await
        .expect("begin")
        .expect("a member")
}

fn later() -> OffsetDateTime {
    OffsetDateTime::now_utc() + time::Duration::hours(72)
}

/// Alice's project, with Bob invited and joined as `role`.
async fn alice_and_bob(db: &TestDb, role: &str) -> (Uuid, Uuid, Uuid) {
    let (alice, bob) = (db.administrator("alice").await, db.person("bob").await);
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = as_member(&db.app, alice, project).await;
    members::invite(&mut tx, &[9; 32], role, "Bob", later())
        .await
        .expect("invited");
    tx.commit().await.expect("commit");
    let joined = members::accept(&db.app, bob, &[9; 32])
        .await
        .expect("accept");
    assert_eq!(
        joined,
        Accepted::Joined {
            project_id: project,
            role: role.to_owned()
        }
    );
    (alice, bob, project)
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn an_invitation_joins_once_and_shows_its_project_to_its_holder() {
    let db = TestDb::new().await;
    let (alice, carol) = (db.administrator("alice").await, db.person("carol").await);
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = as_member(&db.app, alice, project).await;
    let invitation = members::invite(&mut tx, &[1; 32], "editor", "Carol", later())
        .await
        .expect("invited");
    assert_eq!(members::invitations(&mut tx).await.expect("list").len(), 1);
    tx.commit().await.expect("commit");

    let offer = members::offer(&db.app, carol, &[1; 32])
        .await
        .expect("offer")
        .expect("pending");
    assert_eq!(
        (offer.project_name.as_str(), offer.role.as_str()),
        ("Alpha", "editor")
    );
    assert_eq!(offer.invited_by.as_deref(), Some("alice"));
    assert_eq!(
        members::offer(&db.app, carol, &[2; 32])
            .await
            .expect("offer"),
        None,
        "a wrong token sees nothing"
    );

    assert_eq!(
        members::accept(&db.app, carol, &[1; 32])
            .await
            .expect("accept"),
        Accepted::Joined {
            project_id: project,
            role: "editor".to_owned()
        }
    );
    assert_eq!(
        members::accept(&db.app, carol, &[1; 32])
            .await
            .expect("accept"),
        Accepted::Gone,
        "used once"
    );
    let mut tx = as_member(&db.app, carol, project).await;
    assert_eq!(tx.role, "editor");
    let names: Vec<String> = members::list(&mut tx)
        .await
        .expect("list")
        .into_iter()
        .filter_map(|m| m.display_name)
        .collect();
    assert_eq!(names, ["alice", "carol"], "owners first");
    drop(tx);

    // An existing member can't use an invitation up: it stays for whoever it was meant for.
    let mut tx = as_member(&db.app, alice, project).await;
    members::invite(&mut tx, &[3; 32], "owner", "Someone else", later())
        .await
        .expect("invited");
    tx.commit().await.expect("commit");
    assert_eq!(
        members::accept(&db.app, carol, &[3; 32])
            .await
            .expect("accept"),
        Accepted::AlreadyMember
    );
    let mut tx = as_member(&db.app, alice, project).await;
    assert_eq!(
        members::invitations(&mut tx).await.expect("list").len(),
        1,
        "still pending"
    );
    assert!(members::revoke(&mut tx, invitation.id).await.is_ok());
    drop(tx);
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn revoked_and_expired_invitations_are_dead() {
    let db = TestDb::new().await;
    let (alice, carol) = (db.administrator("alice").await, db.person("carol").await);
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = as_member(&db.app, alice, project).await;
    let revoked = members::invite(&mut tx, &[4; 32], "editor", "Revoked", later())
        .await
        .expect("invited");
    members::invite(&mut tx, &[5; 32], "editor", "Expired", later())
        .await
        .expect("invited");
    assert!(members::revoke(&mut tx, revoked.id).await.expect("revoke"));
    assert!(
        !members::revoke(&mut tx, revoked.id).await.expect("revoke"),
        "only once"
    );
    tx.commit().await.expect("commit");
    let owner = db.owner().await;
    sqlx::query("UPDATE invitations SET created_at = now() - interval '4 days', expires_at = now() - interval '1 day' WHERE label = 'Expired'")
        .execute(&owner)
        .await
        .expect("aged");
    owner.close().await;
    for token in [[4; 32], [5; 32]] {
        assert_eq!(
            members::offer(&db.app, carol, &token).await.expect("offer"),
            None
        );
        assert_eq!(
            members::accept(&db.app, carol, &token)
                .await
                .expect("accept"),
            Accepted::Gone
        );
    }
    assert!(
        scope::begin_as_member(&db.app, carol, project)
            .await
            .expect("begin")
            .is_none(),
        "carol never joined"
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_last_owner_can_be_neither_demoted_nor_removed() {
    let db = TestDb::new().await;
    let (alice, bob, project) = alice_and_bob(&db, "editor").await;
    let mut tx = as_member(&db.app, alice, project).await;
    assert_eq!(
        members::change_role(&mut tx, alice, "editor")
            .await
            .expect("change"),
        Change::LastOwner
    );
    assert_eq!(
        members::remove(&mut tx, alice).await.expect("remove"),
        Change::LastOwner
    );
    assert_eq!(
        members::change_role(&mut tx, Uuid::now_v7(), "editor")
            .await
            .expect("change"),
        Change::NotFound
    );
    assert_eq!(
        members::change_role(&mut tx, bob, "owner")
            .await
            .expect("change"),
        Change::Done
    );
    assert_eq!(
        members::change_role(&mut tx, alice, "editor")
            .await
            .expect("change"),
        Change::Done,
        "two owners now"
    );
    assert_eq!(
        members::change_role(&mut tx, bob, "editor")
            .await
            .expect("change"),
        Change::NotOwner,
        "having stepped down, Alice manages no one"
    );
    tx.commit().await.expect("commit");

    // Below the API: even a direct update can't leave the project without an owner.
    let mut tx = as_member(&db.app, bob, project).await;
    sqlx::query("UPDATE memberships SET role = 'editor' WHERE project_id = $1")
        .bind(project)
        .execute(&mut *tx.transaction)
        .await
        .expect("the update runs; the trigger checks at commit");
    let error = tx.commit().await.expect_err("refused at commit");
    assert!(
        format!("{error:?}").contains("at least one owner"),
        "{error:?}"
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn two_owners_demoting_each_other_at_once_leave_one_owner() {
    let db = TestDb::new().await;
    let (alice, bob, project) = alice_and_bob(&db, "owner").await;
    let pool = db.app_pool(2).await;
    let mut first = as_member(&pool, alice, project).await;
    let mut second = as_member(&pool, bob, project).await;
    assert_eq!(
        members::change_role(&mut first, bob, "editor")
            .await
            .expect("change"),
        Change::Done
    );
    // Bob demoting Alice waits for Alice's transaction, then sees he is the only owner left.
    let waiting = tokio::spawn(async move {
        let outcome = members::change_role(&mut second, alice, "editor")
            .await
            .expect("change");
        drop(second);
        outcome
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(
        !waiting.is_finished(),
        "the second change waits for the first"
    );
    first.commit().await.expect("commit");
    // Bob is no longer an owner, so his demotion of Alice must not leave the project ownerless.
    let outcome = waiting.await.expect("task");
    assert_ne!(outcome, Change::Done, "the project would have had no owner");
    let mut tx = as_member(&pool, alice, project).await;
    let owners: Vec<Uuid> = members::list(&mut tx)
        .await
        .expect("list")
        .into_iter()
        .filter(|member| member.role == "owner")
        .map(|member| member.identity_id)
        .collect();
    assert_eq!(owners, [alice]);
    drop(tx);
    pool.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn an_owner_demoted_mid_request_can_no_longer_manage_members() {
    let db = TestDb::new().await;
    let (alice, bob, project) = alice_and_bob(&db, "owner").await;
    let carol = db.person("carol").await;
    let mut tx = as_member(&db.app, alice, project).await;
    members::invite(&mut tx, &[8; 32], "editor", "Carol", later())
        .await
        .expect("invited");
    tx.commit().await.expect("commit");
    members::accept(&db.app, carol, &[8; 32])
        .await
        .expect("accept");

    let pool = db.app_pool(3).await;
    // Bob's request has begun and read his role as owner...
    let mut bobs = as_member(&pool, bob, project).await;
    assert_eq!(bobs.role, "owner");
    // ...when Alice demotes him.
    let mut alices = as_member(&pool, alice, project).await;
    assert_eq!(
        members::change_role(&mut alices, bob, "editor")
            .await
            .expect("change"),
        Change::Done
    );
    let waiting = tokio::spawn(async move {
        let promoted = members::change_role(&mut bobs, carol, "owner")
            .await
            .expect("change");
        let removed = members::remove(&mut bobs, carol).await.expect("remove");
        let policy = members::current_policy(&mut bobs).await.expect("policy");
        drop(bobs);
        (promoted, removed, policy)
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(
        !waiting.is_finished(),
        "Bob's change waits for Alice's to commit"
    );
    alices.commit().await.expect("commit");
    let (promoted, removed, policy) = waiting.await.expect("task");
    assert_eq!(
        (promoted, removed, policy),
        (
            Change::NotOwner,
            Change::NotOwner,
            Some(Policy::builtin(Role::Editor))
        ),
        "a stale owner role grants nothing, and the fresh policy is the editor's"
    );
    let mut tx = as_member(&pool, alice, project).await;
    let carols: Vec<String> = members::list(&mut tx)
        .await
        .expect("list")
        .into_iter()
        .filter(|member| member.identity_id == carol)
        .map(|member| member.role)
        .collect();
    assert_eq!(carols, ["editor"], "Carol was neither promoted nor removed");
    drop(tx);
    pool.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn another_project_s_owner_cannot_revoke_or_see_these_invitations() {
    let db = TestDb::new().await;
    let (alice, mallory) = (
        db.administrator("alice").await,
        db.administrator("mallory").await,
    );
    let alpha = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let beta = projects::create(&db.app, mallory, "beta", "Beta")
        .await
        .expect("created")
        .id;
    let mut tx = as_member(&db.app, alice, alpha).await;
    let invitation = members::invite(&mut tx, &[6; 32], "owner", "Alice's friend", later())
        .await
        .expect("invited");
    tx.commit().await.expect("commit");
    let mut tx = as_member(&db.app, mallory, beta).await;
    assert!(
        !members::revoke(&mut tx, invitation.id)
            .await
            .expect("revoke"),
        "not in her project"
    );
    assert_eq!(
        members::invitations(&mut tx).await.expect("list"),
        Vec::new()
    );
    drop(tx);
    db.drop().await;
}
