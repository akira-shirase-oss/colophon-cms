//! Roles against real PostgreSQL (D-058). Step R.1: every new project gets the four built-in
//! roles with exactly the domain's permissions, a member's transaction carries their role's
//! policy, and a membership can only name a role the project has. Step R.2: roles owners define,
//! versioned, deletable only when nobody uses them, an owner role nothing can change, and member
//! managers who can't hand out or take away more than they hold. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    PgPool,
    members::{self, Accepted, Change},
    projects,
    roles::{self, Removed, Written},
    scope::{self, MemberTransaction},
};
use colophon_domain::policy::{Action, Permission, Policy, Role, Subject};
use common::TestDb;
use sqlx::types::time::OffsetDateTime;
use uuid::Uuid;

async fn as_member(pool: &PgPool, identity: Uuid, project: Uuid) -> MemberTransaction {
    scope::begin_as_member(pool, identity, project)
        .await
        .expect("begin")
        .expect("a member")
}

fn on(action: Action, collection: &str) -> Permission {
    Permission {
        subject: Subject::Collection(collection.to_owned()),
        ..Permission::everywhere(action)
    }
}

/// Alice's project, with Bob invited and joined as `role`.
async fn alice_and_bob(db: &TestDb, role: &str) -> (Uuid, Uuid, Uuid) {
    let (alice, bob) = (db.administrator("alice").await, db.person("bob").await);
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = as_member(&db.app, alice, project).await;
    let later = OffsetDateTime::now_utc() + time::Duration::hours(72);
    roles::policy_of(&mut tx, role)
        .await
        .expect("read")
        .expect("the role exists");
    members::invite(&mut tx, &[7; 32], role, "Bob", later)
        .await
        .expect("invited");
    tx.commit().await.expect("commit");
    assert!(matches!(
        members::accept(&db.app, bob, &[7; 32])
            .await
            .expect("accept"),
        Accepted::Joined { .. }
    ));
    (alice, bob, project)
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_new_project_has_the_built_in_roles_with_the_domain_s_permissions() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let owner = db.owner().await;
    for role in Role::ALL {
        let stored: Vec<(String, String)> = sqlx::query_as(
            "SELECT action, subject FROM role_permissions
             WHERE project_id = $1 AND role_key = $2 AND fields IS NULL AND condition IS NULL
             ORDER BY action",
        )
        .bind(project)
        .bind(role.as_str())
        .fetch_all(&owner)
        .await
        .expect("permissions");
        let mut expected: Vec<(String, String)> = Policy::builtin(role)
            .permissions()
            .iter()
            .map(|permission| {
                (
                    permission.action.as_str().to_owned(),
                    permission.subject.as_str().to_owned(),
                )
            })
            .collect();
        expected.sort();
        assert_eq!(
            stored, expected,
            "colophon_seed_roles gives {role:?} the domain's permissions"
        );
    }
    let (built_in,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM roles WHERE project_id = $1 AND built_in")
            .bind(project)
            .fetch_one(&owner)
            .await
            .expect("roles");
    assert_eq!(built_in, 4);

    // The creator's transaction carries the owner's policy.
    let tx = scope::begin_in_project(&db.app, alice, project, "production")
        .await
        .expect("begin")
        .expect("a member");
    assert_eq!(tx.policy, Policy::builtin(Role::Owner));
    assert!(tx.policy.allows(Action::RolesManage, None));
    drop(tx);

    // A membership names one of the project's roles, or nothing is written.
    let bob = db.person("bob").await;
    let refused = sqlx::query(
        "INSERT INTO memberships (project_id, identity_id, role) VALUES ($1, $2, 'superuser')",
    )
    .bind(project)
    .bind(bob)
    .execute(&owner)
    .await;
    assert!(refused.is_err(), "an unknown role is refused");
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_role_is_created_changed_at_its_version_and_deleted_once_unused() {
    let db = TestDb::new().await;
    let (alice, bob, project) = alice_and_bob(&db, "editor").await;
    let pages_only = Policy::new(vec![
        on(Action::ContentRead, "pages"),
        on(Action::ContentUpdate, "pages"),
    ]);

    let mut tx = as_member(&db.app, alice, project).await;
    let Written::Done(made) = roles::create(&mut tx, "pages-desk", "Pages desk", "", &pages_only)
        .await
        .expect("create")
    else {
        panic!("created")
    };
    assert_eq!(
        (
            made.version,
            made.built_in,
            made.members,
            made.policy.clone()
        ),
        (1, false, 0, pages_only.clone())
    );
    assert_eq!(
        roles::create(&mut tx, "pages-desk", "Again", "", &pages_only)
            .await
            .expect("create"),
        Written::Exists
    );
    let keys: Vec<String> = roles::list(&mut tx)
        .await
        .expect("list")
        .into_iter()
        .map(|role| role.key)
        .collect();
    assert_eq!(
        keys,
        ["owner", "developer", "editor", "publisher", "pages-desk"],
        "built-in roles first, then in the order they were made"
    );
    tx.commit().await.expect("commit");

    // Bob takes the new role; what it allows is what his next transaction carries.
    let mut tx = as_member(&db.app, alice, project).await;
    assert_eq!(
        members::change_role(&mut tx, bob, "pages-desk")
            .await
            .expect("change"),
        Change::Done
    );
    tx.commit().await.expect("commit");
    assert_eq!(as_member(&db.app, bob, project).await.policy, pages_only);

    // A change names the version it edited; an older one is refused and changes nothing.
    let wider = Policy::new(vec![
        on(Action::ContentRead, "pages"),
        on(Action::ContentUpdate, "pages"),
        on(Action::ContentCreate, "pages"),
    ]);
    let mut tx = as_member(&db.app, alice, project).await;
    let Written::Done(changed) =
        roles::update(&mut tx, "pages-desk", 1, "Pages", "Pages only", &wider)
            .await
            .expect("update")
    else {
        panic!("changed")
    };
    assert_eq!((changed.version, changed.members), (2, 1));
    assert_eq!(
        roles::update(&mut tx, "pages-desk", 1, "Lost", "", &Policy::default())
            .await
            .expect("update"),
        Written::Stale { current: 2 }
    );
    assert_eq!(
        roles::update(&mut tx, "no-such-role", 1, "X", "", &Policy::default())
            .await
            .expect("update"),
        Written::NotFound
    );
    tx.commit().await.expect("commit");
    assert_eq!(
        as_member(&db.app, bob, project).await.policy,
        wider,
        "a changed role holds from the holder's next transaction"
    );

    deleted_once_unused(&db, alice, bob, project).await;
    db.drop().await;
}

/// The rest of the story above: held, the role can't be deleted; once nobody holds it, it can.
async fn deleted_once_unused(db: &TestDb, alice: Uuid, bob: Uuid, project: Uuid) {
    let mut tx = as_member(&db.app, alice, project).await;
    assert_eq!(
        roles::delete(&mut tx, "pages-desk", 1)
            .await
            .expect("delete"),
        Removed::Stale { current: 2 }
    );
    assert_eq!(
        roles::delete(&mut tx, "pages-desk", 2)
            .await
            .expect("delete"),
        Removed::InUse {
            members: 1,
            invitations: 0
        }
    );
    assert_eq!(
        members::change_role(&mut tx, bob, "editor")
            .await
            .expect("change"),
        Change::Done
    );
    assert_eq!(
        roles::delete(&mut tx, "pages-desk", 2)
            .await
            .expect("delete"),
        Removed::Done
    );
    assert_eq!(
        roles::delete(&mut tx, "pages-desk", 2)
            .await
            .expect("delete"),
        Removed::NotFound
    );
    assert_eq!(
        members::change_role(&mut tx, bob, "pages-desk")
            .await
            .expect("change"),
        Change::UnknownRole
    );
    tx.commit().await.expect("commit");
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_owner_role_is_fixed_even_for_a_direct_write() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = as_member(&db.app, alice, project).await;
    assert_eq!(
        roles::update(&mut tx, "owner", 1, "Boss", "", &Policy::default())
            .await
            .expect("update"),
        Written::Fixed
    );
    assert_eq!(
        roles::delete(&mut tx, "owner", 1).await.expect("delete"),
        Removed::Fixed
    );
    // The database refuses it too, as the application's login.
    let stripped =
        sqlx::query("DELETE FROM role_permissions WHERE project_id = $1 AND role_key = 'owner'")
            .bind(project)
            .execute(&mut *tx.transaction)
            .await;
    assert!(
        stripped.is_err(),
        "the owner's permissions can't be deleted"
    );
    drop(tx);
    let mut tx = as_member(&db.app, alice, project).await;
    let renamed =
        sqlx::query("UPDATE roles SET name = 'Boss' WHERE project_id = $1 AND key = 'owner'")
            .bind(project)
            .execute(&mut *tx.transaction)
            .await;
    assert!(renamed.is_err(), "the owner role can't be changed");
    drop(tx);
    assert_eq!(
        as_member(&db.app, alice, project).await.policy,
        Policy::builtin(Role::Owner)
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_pending_invitation_keeps_its_role_and_an_old_one_does_not() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let later = OffsetDateTime::now_utc() + time::Duration::hours(72);
    let mut tx = as_member(&db.app, alice, project).await;
    assert!(matches!(
        roles::create(
            &mut tx,
            "guest",
            "Guest",
            "",
            &Policy::new(vec![Permission::everywhere(Action::ContentRead)])
        )
        .await
        .expect("create"),
        Written::Done(_)
    ));
    roles::policy_of(&mut tx, "guest").await.expect("read");
    let invitation = members::invite(&mut tx, &[3; 32], "guest", "Gail", later)
        .await
        .expect("invited");
    assert_eq!(
        roles::delete(&mut tx, "guest", 1).await.expect("delete"),
        Removed::InUse {
            members: 0,
            invitations: 1
        }
    );
    assert!(
        members::revoke(&mut tx, invitation.id)
            .await
            .expect("revoke")
    );
    assert_eq!(
        roles::delete(&mut tx, "guest", 1).await.expect("delete"),
        Removed::Done,
        "a revoked invitation is history, not a hold on the role"
    );
    tx.commit().await.expect("commit");
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_member_manager_never_hands_out_or_takes_away_more_than_they_hold() {
    let db = TestDb::new().await;
    let (alice, bob, project) = alice_and_bob(&db, "editor").await;
    let carol = db.person("carol").await;
    let mut tx = as_member(&db.app, alice, project).await;
    // Bob manages members, and can write content, but has nothing else.
    let manager = Policy::new(vec![
        Permission::everywhere(Action::MembersManage),
        Permission::everywhere(Action::ContentRead),
        Permission::everywhere(Action::ContentCreate),
        Permission::everywhere(Action::ContentUpdate),
    ]);
    assert!(matches!(
        roles::create(&mut tx, "team-lead", "Team lead", "", &manager)
            .await
            .expect("create"),
        Written::Done(_)
    ));
    assert_eq!(
        members::change_role(&mut tx, bob, "team-lead")
            .await
            .expect("change"),
        Change::Done
    );
    roles::policy_of(&mut tx, "developer").await.expect("read");
    members::invite(
        &mut tx,
        &[5; 32],
        "developer",
        "Carol",
        OffsetDateTime::now_utc() + time::Duration::hours(1),
    )
    .await
    .expect("invited");
    tx.commit().await.expect("commit");
    assert!(matches!(
        members::accept(&db.app, carol, &[5; 32])
            .await
            .expect("accept"),
        Accepted::Joined { .. }
    ));

    let mut tx = as_member(&db.app, bob, project).await;
    assert_eq!(
        members::change_role(&mut tx, carol, "editor")
            .await
            .expect("change"),
        Change::NotPermitted,
        "Carol is a developer, which Bob's role doesn't cover: he can't demote her"
    );
    assert_eq!(
        members::change_role(&mut tx, bob, "owner")
            .await
            .expect("change"),
        Change::NotPermitted,
        "nor promote himself to more than he holds"
    );
    assert_eq!(
        members::remove(&mut tx, alice).await.expect("remove"),
        Change::NotPermitted,
        "nor remove an owner"
    );
    assert_eq!(
        members::remove(&mut tx, carol).await.expect("remove"),
        Change::NotPermitted,
        "nor a developer"
    );
    drop(tx);

    // An editor is within his powers.
    let mut tx = as_member(&db.app, alice, project).await;
    assert_eq!(
        members::change_role(&mut tx, carol, "editor")
            .await
            .expect("change"),
        Change::Done
    );
    tx.commit().await.expect("commit");
    let mut tx = as_member(&db.app, bob, project).await;
    assert_eq!(
        members::change_role(&mut tx, carol, "team-lead")
            .await
            .expect("change"),
        Change::Done,
        "he may give his own role"
    );
    assert_eq!(
        members::remove(&mut tx, carol).await.expect("remove"),
        Change::Done
    );
    tx.commit().await.expect("commit");
    db.drop().await;
}
