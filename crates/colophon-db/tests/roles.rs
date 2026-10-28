//! Roles as data against real PostgreSQL (D-058, step R.1): every new project gets the four
//! built-in roles with exactly the domain's permissions, a member's transaction carries their
//! role's policy, and a membership can only name a role the project has. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{projects, scope};
use colophon_domain::policy::{Action, Policy, Role};
use common::TestDb;

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
