//! The guards that keep a project its owner can't be talked round by a session setting
//! (D-155, migration 0058): `app.deleting_project` and `app.ledger_replay` are flags any
//! session can set, so the triggers that bend for a project's deletion and a ledger replay now
//! ask who is running the statement — and, for the deferred last-owner check, whether the
//! project row is really gone. Run as `colophon_app`, the application's own login. Needs the
//! local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    projects,
    scope::{self, MemberTransaction},
};
use common::TestDb;
use uuid::Uuid;

async fn staff(db: &TestDb, identity: Uuid, project: Uuid) -> MemberTransaction {
    scope::begin_as_member(&db.app, identity, project)
        .await
        .expect("begin")
        .expect("a member")
}

async fn set(tx: &mut MemberTransaction, setting: &str, value: &str) {
    sqlx::query("SELECT set_config($1, $2, true)")
        .bind(setting)
        .bind(value)
        .execute(&mut *tx.transaction)
        .await
        .expect("set_config");
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn forged_bypass_settings_open_nothing_for_the_application() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "guarded", "Guarded")
        .await
        .expect("created")
        .id;

    // The owner role always holds every permission: claiming to be deleting the project
    // doesn't let the application strip them.
    let mut tx = staff(&db, alice, project).await;
    set(&mut tx, "app.deleting_project", &project.to_string()).await;
    let stripped =
        sqlx::query("DELETE FROM role_permissions WHERE project_id = $1 AND role_key = 'owner'")
            .bind(project)
            .execute(&mut *tx.transaction)
            .await;
    assert!(
        stripped.is_err(),
        "the owner role stays fixed under a forged deletion flag: {stripped:?}"
    );
    drop(tx);

    // A project keeps its last owner: a forged replay flag doesn't remove them…
    let mut tx = staff(&db, alice, project).await;
    set(&mut tx, "app.ledger_replay", "on").await;
    sqlx::query("DELETE FROM memberships WHERE project_id = $1 AND identity_id = $2")
        .bind(project)
        .bind(alice)
        .execute(&mut *tx.transaction)
        .await
        .expect("the delete itself runs; the deferred check decides at commit");
    let committed = tx.transaction.commit().await;
    assert!(
        committed.is_err(),
        "the last owner stays under a forged replay flag"
    );

    // …nor does a forged deletion flag with the context moved away before the commit, so
    // that the project merely looks gone to the caller.
    let mut tx = staff(&db, alice, project).await;
    set(&mut tx, "app.deleting_project", &project.to_string()).await;
    sqlx::query("DELETE FROM memberships WHERE project_id = $1 AND identity_id = $2")
        .bind(project)
        .bind(alice)
        .execute(&mut *tx.transaction)
        .await
        .expect("the delete itself runs");
    set(&mut tx, "app.project_id", &Uuid::now_v7().to_string()).await;
    let committed = tx.transaction.commit().await;
    assert!(
        committed.is_err(),
        "the last owner stays while the project exists, whatever the caller can see"
    );

    // Alice is still the owner, and the owner role still holds its permissions.
    let owner = db.owner().await;
    let owners: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM memberships WHERE project_id = $1 AND role = 'owner'",
    )
    .bind(project)
    .fetch_one(&owner)
    .await
    .expect("count");
    assert_eq!(owners, 1);
    let permissions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM role_permissions WHERE project_id = $1 AND role_key = 'owner'",
    )
    .bind(project)
    .fetch_one(&owner)
    .await
    .expect("count");
    assert!(permissions > 0, "the owner role kept its permissions");
    owner.close().await;
    db.drop().await;
}
