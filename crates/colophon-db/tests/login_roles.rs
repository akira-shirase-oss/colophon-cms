//! `colophon admin init-roles` against real PostgreSQL (D-173): on a cluster that already has
//! the roles it changes nothing — the development login still signs in with its own password —
//! and a role created with one of its SCRAM verifiers signs in with the password, and not with
//! another. Needs the local stack. Roles are cluster-wide, so the test never alters the shared
//! ones; the throwaway login it creates has a name of its own and is dropped.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    PgPool,
    login_roles::{self, RoleAction},
};
use sqlx::AssertSqlSafe;
use uuid::Uuid;

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn init_on_a_cluster_that_has_the_roles_changes_nothing() {
    let host = common::host();
    let owner = PgPool::connect(&format!("postgres://colophon:colophon-dev@{host}/postgres"))
        .await
        .expect("the stack's database answers");
    let outcomes = login_roles::init(&owner, "a-generated-password-0123456789", None)
        .await
        .expect("init runs");
    assert_eq!(outcomes.len(), login_roles::ROLES.len());
    assert!(
        outcomes
            .iter()
            .all(|outcome| outcome.action == RoleAction::Existing),
        "{outcomes:?}"
    );
    // Its password was left alone: the development login still signs in with its own.
    let app = PgPool::connect(&format!(
        "postgres://colophon_app:colophon-dev@{host}/postgres"
    ))
    .await
    .expect("colophon_app keeps its password");
    let member: bool = sqlx::query_scalar(
        "SELECT pg_has_role('colophon_app', 'colophon_delivery', 'SET') \
         AND NOT pg_has_role('colophon_app', 'colophon_delivery', 'USAGE')",
    )
    .fetch_one(&app)
    .await
    .expect("membership query");
    assert!(member, "the grants hold: SET without inheritance");
    // The development password is refused before anything is sent.
    let refused = login_roles::init(&owner, "colophon-dev", None).await;
    assert!(
        matches!(refused, Err(login_roles::InitError::Password { .. })),
        "{refused:?}"
    );
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_verifier_signs_in_with_its_password_and_no_other() {
    let host = common::host();
    let owner = PgPool::connect(&format!("postgres://colophon:colophon-dev@{host}/postgres"))
        .await
        .expect("the stack's database answers");
    let name = format!("colophon_scram_{}", Uuid::now_v7().simple());
    let password = format!("pw-{}", Uuid::now_v7().simple());
    let verifier = login_roles::scram_verifier(&password, b"sixteen byte salt");
    // The name is a UUID and the verifier is base64: neither can end its token or literal.
    sqlx::query(AssertSqlSafe(format!(
        "CREATE ROLE {name} LOGIN PASSWORD '{verifier}'"
    )))
    .execute(&owner)
    .await
    .expect("create the throwaway login");
    let right = PgPool::connect(&format!("postgres://{name}:{password}@{host}/postgres")).await;
    let wrong = PgPool::connect(&format!("postgres://{name}:not-{password}@{host}/postgres")).await;
    let stored: Option<String> =
        sqlx::query_scalar("SELECT rolpassword FROM pg_authid WHERE rolname = $1")
            .bind(&name)
            .fetch_one(&owner)
            .await
            .expect("the stored verifier");
    if let Ok(pool) = &right {
        pool.close().await;
    }
    sqlx::query(AssertSqlSafe(format!("DROP ROLE {name}")))
        .execute(&owner)
        .await
        .expect("drop the throwaway login");
    assert!(right.is_ok(), "the password signs in: {right:?}");
    assert!(wrong.is_err(), "another password doesn't");
    assert_eq!(
        stored.as_deref(),
        Some(verifier.as_str()),
        "stored as given"
    );
}
