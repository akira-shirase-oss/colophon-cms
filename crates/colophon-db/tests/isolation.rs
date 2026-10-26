//! Row-level security and the identity tables against real PostgreSQL. Each run creates a fresh
//! database, migrates it as the owner, and works in it as `colophon_app`, the application's
//! login, so the policies are in force exactly as in production. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    DbError, PgPool, identity, projects,
    scope::{self, Scope},
};
use common::TestDb;
use sqlx::{AssertSqlSafe, types::time::OffsetDateTime};
use uuid::Uuid;

/// Runs a fixed counting query (written in this file, never from input) inside `within`.
async fn count(pool: &PgPool, within: Scope, sql: &str) -> i64 {
    let mut transaction = scope::begin(pool, within).await.expect("begin");
    let (n,): (i64,) = sqlx::query_as(AssertSqlSafe(sql.to_owned()))
        .fetch_one(&mut *transaction)
        .await
        .expect("query");
    transaction.commit().await.expect("commit");
    n
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_application_login_cannot_bypass_row_level_security() {
    let db = TestDb::new().await;
    let (user, bypass, superuser): (String, bool, bool) = sqlx::query_as(
        "SELECT current_user::text, rolbypassrls, rolsuper FROM pg_roles WHERE rolname = current_user",
    )
    .fetch_one(&db.app)
    .await
    .expect("role query");
    assert_eq!(
        (user.as_str(), bypass, superuser),
        ("colophon_app", false, false)
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_project_is_invisible_to_everyone_but_its_members() {
    let db = TestDb::new().await;
    let (alice, bob) = (db.person("alice").await, db.person("bob").await);
    let alpha = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created");

    assert_eq!(projects::list(&db.app, alice).await.expect("list").len(), 1);
    assert_eq!(
        projects::get(&db.app, alice, alpha.id)
            .await
            .expect("get")
            .map(|project| project.role),
        Some("owner".to_owned())
    );
    assert_eq!(
        projects::list(&db.app, bob).await.expect("list"),
        Vec::new()
    );
    assert_eq!(
        projects::get(&db.app, bob, alpha.id).await.expect("get"),
        None,
        "not a member looks the same as not found"
    );

    // Below the API: even a query that forgets to filter sees nothing it shouldn't.
    for table in ["projects", "memberships", "environments"] {
        let sql = format!("SELECT count(*) FROM {table}");
        assert_eq!(
            count(&db.app, Scope::default(), &sql).await,
            0,
            "no context, no {table}"
        );
    }
    let bob_only = Scope {
        identity: Some(bob),
        project: None,
    };
    assert_eq!(
        count(&db.app, bob_only, "SELECT count(*) FROM projects").await,
        0
    );
    let alice_only = Scope {
        identity: Some(alice),
        project: None,
    };
    assert_eq!(
        count(&db.app, alice_only, "SELECT count(*) FROM projects").await,
        1
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_context_ends_with_its_transaction_on_a_reused_connection() {
    let db = TestDb::new().await;
    let alice = db.person("alice").await;
    let alpha = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created");
    let in_alpha = Scope {
        identity: None,
        project: Some(alpha.id),
    };

    assert_eq!(
        count(&db.app, in_alpha, "SELECT count(*) FROM environments").await,
        1
    );
    // The same single connection with no context: nothing is visible after a commit...
    assert_eq!(
        count(
            &db.app,
            Scope::default(),
            "SELECT count(*) FROM environments"
        )
        .await,
        0
    );
    // ...or after a rollback.
    let mut transaction = scope::begin(&db.app, in_alpha).await.expect("begin");
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM environments")
        .fetch_one(&mut *transaction)
        .await
        .expect("query");
    assert_eq!(n, 1);
    transaction.rollback().await.expect("rollback");
    assert_eq!(
        count(
            &db.app,
            Scope::default(),
            "SELECT count(*) FROM environments"
        )
        .await,
        0
    );
    let (leftover,): (String,) =
        sqlx::query_as("SELECT coalesce(current_setting('app.project_id', true), '')")
            .fetch_one(&db.app)
            .await
            .expect("setting");
    assert_eq!(
        leftover, "",
        "no project context survives on the connection"
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn nobody_can_write_into_a_project_from_another_ones_context() {
    let db = TestDb::new().await;
    let (alice, bob) = (db.person("alice").await, db.person("bob").await);
    let alpha = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created");
    let beta = projects::create(&db.app, bob, "beta", "Beta")
        .await
        .expect("created");

    // Bob, working in his own project, tries to make himself an owner of Alice's.
    let mut transaction = scope::begin(
        &db.app,
        Scope {
            identity: Some(bob),
            project: Some(beta.id),
        },
    )
    .await
    .expect("begin");
    let error = sqlx::query(
        "INSERT INTO memberships (project_id, identity_id, role) VALUES ($1, $2, 'owner')",
    )
    .bind(alpha.id)
    .bind(bob)
    .execute(&mut *transaction)
    .await
    .expect_err("row-level security refuses it");
    assert!(error.to_string().contains("row-level security"), "{error}");
    transaction.rollback().await.expect("rollback");

    // And the application role can't change or remove projects at all: it has no grant.
    for sql in [
        "DELETE FROM projects WHERE id = $1",
        "UPDATE projects SET name = 'x' WHERE id = $1",
    ] {
        let mut transaction = scope::begin(
            &db.app,
            Scope {
                identity: Some(alice),
                project: Some(alpha.id),
            },
        )
        .await
        .expect("begin");
        let error = sqlx::query(sql)
            .bind(alpha.id)
            .execute(&mut *transaction)
            .await
            .expect_err("no grant");
        assert!(
            error.to_string().contains("permission denied"),
            "{sql}: {error}"
        );
        transaction.rollback().await.expect("rollback");
    }
    let slugs: Vec<String> = projects::list(&db.app, bob)
        .await
        .expect("list")
        .into_iter()
        .map(|project| project.slug)
        .collect();
    assert_eq!(slugs, ["beta"]);
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_taken_slug_is_a_conflict() {
    let db = TestDb::new().await;
    let (alice, bob) = (db.person("alice").await, db.person("bob").await);
    projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created");
    let error = projects::create(&db.app, bob, "alpha", "Another")
        .await
        .expect_err("taken");
    assert!(matches!(error, DbError::Conflict), "{error:?}");
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn sessions_end_when_revoked_or_expired_and_only_hashes_are_kept() {
    let db = TestDb::new().await;
    let alice = db.person("alice").await;
    let in_an_hour = OffsetDateTime::now_utc() + time::Duration::hours(1);
    let token = [7_u8; 32];
    let id = identity::create_session(&db.app, alice, &token, &[9_u8; 32], in_an_hour)
        .await
        .expect("created");
    let found = identity::find_session(&db.app, &token)
        .await
        .expect("find")
        .expect("a live session");
    assert_eq!((found.id, found.identity_id), (id, alice));
    identity::touch_session(&db.app, id).await.expect("touch");
    identity::revoke_session(&db.app, id).await.expect("revoke");
    assert_eq!(
        identity::find_session(&db.app, &token).await.expect("find"),
        None
    );

    // Expired sessions aren't found either. The CHECK forbids creating one already expired, so
    // the owner ages it, as time would.
    let token = [8_u8; 32];
    let id = identity::create_session(&db.app, alice, &token, &[9_u8; 32], in_an_hour)
        .await
        .expect("created");
    let owner = db.owner().await;
    sqlx::query(
        "UPDATE sessions SET created_at = now() - interval '2 hours', expires_at = now() - interval '1 hour' WHERE id = $1",
    )
    .bind(id)
    .execute(&owner)
    .await
    .expect("age it");
    owner.close().await;
    assert_eq!(
        identity::find_session(&db.app, &token).await.expect("find"),
        None
    );

    // The application role can't rewrite a token hash: its column grants don't allow it.
    let error = sqlx::query("UPDATE sessions SET token_hash = $1")
        .bind(&[1_u8; 32][..])
        .execute(&db.app)
        .await
        .expect_err("no grant");
    assert!(error.to_string().contains("permission denied"), "{error}");
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_sign_in_attempt_can_be_finished_once() {
    let db = TestDb::new().await;
    let state = [3_u8; 32];
    let attempt = identity::SignInAttempt {
        nonce: "n".repeat(32),
        pkce_verifier: "v".repeat(64),
        return_to: "/studio".to_owned(),
    };
    let soon = OffsetDateTime::now_utc() + time::Duration::minutes(10);
    identity::start_sign_in(&db.app, &state, &attempt, soon)
        .await
        .expect("started");
    assert_eq!(
        identity::finish_sign_in(&db.app, &state)
            .await
            .expect("finish"),
        Some(attempt.clone())
    );
    assert_eq!(
        identity::finish_sign_in(&db.app, &state)
            .await
            .expect("finish"),
        None,
        "a state works once"
    );

    // A return path to another site is refused by the database itself.
    let elsewhere = identity::SignInAttempt {
        return_to: "//evil.example".to_owned(),
        ..attempt
    };
    assert!(
        identity::start_sign_in(&db.app, &[4_u8; 32], &elsewhere, soon)
            .await
            .is_err()
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_profile_names_the_person_and_only_their_projects() {
    let db = TestDb::new().await;
    let (alice, bob) = (db.person("alice").await, db.person("bob").await);
    projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created");
    projects::create(&db.app, bob, "beta", "Beta")
        .await
        .expect("created");

    let profile = identity::profile(&db.app, alice)
        .await
        .expect("profile")
        .expect("alice exists");
    assert_eq!(profile.identity.id, alice);
    assert_eq!(profile.identity.display_name.as_deref(), Some("alice"));
    let slugs: Vec<&str> = profile.projects.iter().map(|p| p.slug.as_str()).collect();
    assert_eq!(slugs, ["alpha"], "bob's project is not alice's");

    assert_eq!(
        identity::profile(&db.app, Uuid::now_v7())
            .await
            .expect("profile"),
        None
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn touching_a_session_writes_at_most_once_per_interval() {
    let db = TestDb::new().await;
    let alice = db.person("alice").await;
    let in_an_hour = OffsetDateTime::now_utc() + time::Duration::hours(1);
    let token = [5_u8; 32];
    let id = identity::create_session(&db.app, alice, &token, &[9_u8; 32], in_an_hour)
        .await
        .expect("created");
    let last_seen = |db: &PgPool| {
        let db = db.clone();
        async move {
            identity::find_session(&db, &token)
                .await
                .expect("find")
                .expect("live")
                .last_seen_at
        }
    };

    // Fresh: a touch changes nothing.
    let fresh = last_seen(&db.app).await;
    identity::touch_session(&db.app, id).await.expect("touch");
    assert_eq!(last_seen(&db.app).await, fresh);

    // Older than the interval: a touch records the use.
    let owner = db.owner().await;
    sqlx::query("UPDATE sessions SET last_seen_at = now() - interval '2 minutes' WHERE id = $1")
        .bind(id)
        .execute(&owner)
        .await
        .expect("aged");
    owner.close().await;
    let aged = last_seen(&db.app).await;
    assert!(OffsetDateTime::now_utc() - aged >= identity::TOUCH_INTERVAL);
    identity::touch_session(&db.app, id).await.expect("touch");
    assert!(last_seen(&db.app).await - aged >= identity::TOUCH_INTERVAL);
    db.drop().await;
}
