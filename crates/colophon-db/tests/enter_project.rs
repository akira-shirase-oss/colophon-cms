//! D-227 (a) against real PostgreSQL, as the application's login: `colophon_enter_project` sets a member's
//! row-level-security context in one round trip and keeps today's guarantee. The project is set only once
//! the membership is proven; the settings it makes outlive the function (it carries a SET clause, whose nest
//! level restores only `search_path`); and an RLS-protected read later in the same transaction sees exactly
//! the project's rows. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{PgPool, projects};
use common::TestDb;
use sqlx::{Postgres, Row as _, Transaction};
use uuid::Uuid;

/// What the function answered: the role and environment, or nothing.
async fn enter(
    tx: &mut Transaction<'static, Postgres>,
    identity: Uuid,
    project: Uuid,
    environment: Option<&str>,
) -> Option<(String, Option<Uuid>)> {
    sqlx::query("SELECT role, environment_id FROM colophon_enter_project($1, $2, $3)")
        .bind(identity)
        .bind(project)
        .bind(environment)
        .fetch_optional(&mut **tx)
        .await
        .expect("call colophon_enter_project")
        .map(|row| (row.get("role"), row.get("environment_id")))
}

/// The two context settings as the transaction sees them now.
async fn settings(tx: &mut Transaction<'static, Postgres>) -> (String, String) {
    let row = sqlx::query(
        "SELECT coalesce(current_setting('app.identity_id', true), '') AS identity,
                coalesce(current_setting('app.project_id', true), '') AS project",
    )
    .fetch_one(&mut **tx)
    .await
    .expect("read the settings");
    (row.get("identity"), row.get("project"))
}

/// The environments RLS lets this transaction see, by project.
async fn visible_environments(tx: &mut Transaction<'static, Postgres>) -> Vec<Uuid> {
    sqlx::query_scalar("SELECT project_id FROM environments ORDER BY project_id")
        .fetch_all(&mut **tx)
        .await
        .expect("an RLS-protected read")
}

async fn begin(pool: &PgPool) -> Transaction<'static, Postgres> {
    colophon_db::begin_tx(pool).await.expect("begin")
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_members_context_outlives_the_call_and_rls_sees_exactly_its_project() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let alpha = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("alpha")
        .id;
    projects::create(&db.app, alice, "beta", "Beta")
        .await
        .expect("beta");
    let mut tx = begin(&db.app).await;
    let (role, environment) = enter(&mut tx, alice, alpha, Some("production"))
        .await
        .expect("alice is a member");
    assert_eq!(role, "owner");
    assert!(environment.is_some(), "alpha's production environment");
    // The function's SET clause restores only search_path: the context it set is still the transaction's.
    assert_eq!(
        settings(&mut tx).await,
        (alice.to_string(), alpha.to_string())
    );
    // And a later statement's RLS sees alpha's rows and no other project's, though alice owns beta too.
    assert_eq!(visible_environments(&mut tx).await, vec![alpha]);
    tx.rollback().await.expect("rollback");
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_non_member_gets_no_row_and_no_project_context() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let eve = db.person("eve").await;
    let alpha = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("alpha")
        .id;
    let mut tx = begin(&db.app).await;
    assert_eq!(
        enter(&mut tx, eve, alpha, Some("production")).await,
        None,
        "eve isn't a member"
    );
    // The project was never set: whatever eve's request does next, it does outside alpha.
    assert_eq!(settings(&mut tx).await, (eve.to_string(), String::new()));
    assert!(
        visible_environments(&mut tx).await.is_empty(),
        "no project's rows are visible"
    );
    tx.rollback().await.expect("rollback");
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_member_of_one_project_naming_another_gets_nothing() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let zed = db.administrator("zed").await;
    projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("alpha");
    let zeta = projects::create(&db.app, zed, "zeta", "Zeta")
        .await
        .expect("zeta")
        .id;
    let mut tx = begin(&db.app).await;
    assert_eq!(
        enter(&mut tx, alice, zeta, Some("production")).await,
        None,
        "alice isn't in zeta"
    );
    assert_eq!(
        settings(&mut tx).await.1,
        String::new(),
        "zeta was never set"
    );
    assert!(
        visible_environments(&mut tx).await.is_empty(),
        "zeta's rows stay invisible"
    );
    tx.rollback().await.expect("rollback");
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn an_environment_is_found_only_in_the_project_entered() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let alpha = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("alpha")
        .id;
    let mut tx = begin(&db.app).await;
    let (_, missing) = enter(&mut tx, alice, alpha, Some("no-such-environment"))
        .await
        .expect("a member");
    assert_eq!(
        missing, None,
        "an environment alpha doesn't have is not found"
    );
    let (_, none) = enter(&mut tx, alice, alpha, None).await.expect("a member");
    assert_eq!(none, None, "no environment asked for, none looked up");
    tx.rollback().await.expect("rollback");
    db.drop().await;
}
