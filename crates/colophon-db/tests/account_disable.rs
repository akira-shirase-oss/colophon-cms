//! Disabling an account (D-235, migration 0096) against real PostgreSQL: one transaction
//! revokes every session it holds and every API token it made and logs it; a disabled account
//! signs in like a wrong password and gets no session; only an enabled administrator may do it,
//! never to their own account; and two administrators disabling each other at once leave one.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    DbError, PgPool,
    identity::{self, PasswordSignIn, SetDisabled},
    passwords, projects,
};
use common::{TestDb, gate};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

const EMAIL: &str = "member@example.test";

/// A live session for `who`; the token hash it was made with.
async fn session(pool: &PgPool, who: Uuid) -> Result<[u8; 32], DbError> {
    let hash = *Uuid::now_v7().as_bytes();
    let token = [hash, hash].concat().try_into().expect("32 bytes");
    identity::create_session(
        pool,
        who,
        &token,
        &[3; 32],
        OffsetDateTime::now_utc() + Duration::hours(1),
    )
    .await?;
    Ok(token)
}

/// An API token `creator` made in `project`, written as the owner would; its id.
async fn token(owner: &PgPool, project: Uuid, creator: Uuid) -> Uuid {
    let (id, identity) = (Uuid::now_v7(), Uuid::now_v7());
    sqlx::query(
        "INSERT INTO identities (id, issuer, subject) VALUES ($1, 'urn:colophon:api-token', $2::text)",
    )
    .bind(identity)
    .bind(id)
    .execute(owner)
    .await
    .expect("the token's identity");
    sqlx::query(
        "INSERT INTO api_tokens (id, project_id, identity_id, name, kind, token_hash, created_by, expires_at)
         VALUES ($1, $2, $3, 'deploys', 'read-only', $4, $5, now() + interval '30 days')",
    )
    .bind(id)
    .bind(project)
    .bind(identity)
    .bind(Uuid::now_v7().as_bytes().repeat(2))
    .bind(creator)
    .execute(owner)
    .await
    .expect("the token");
    id
}

async fn events(owner: &PgPool, who: Uuid) -> Vec<(String, Option<Uuid>)> {
    sqlx::query_as(
        "SELECT event, actor_id FROM staff_auth_events WHERE identity_id = $1 ORDER BY at, id",
    )
    .bind(who)
    .fetch_all(owner)
    .await
    .expect("events")
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn disabling_revokes_every_session_and_token_at_once_and_enabling_lets_it_back() {
    let db = TestDb::new().await;
    let owner = db.owner().await;
    let admin = db.administrator("admin").await;
    let member = identity::ensure_password_account(
        &owner,
        EMAIL,
        &passwords::hashed("colophon-dev").await.expect("hash"),
        None,
    )
    .await
    .expect("the member");
    let project = projects::create(&db.app, admin, "site", "Site")
        .await
        .expect("project")
        .id;
    let (first, second) = (
        session(&db.app, member).await.expect("a session"),
        session(&db.app, member).await.expect("another"),
    );
    let made = token(&owner, project, member).await;
    let kept = token(&owner, project, admin).await;

    let outcome = identity::set_disabled(&db.app, admin, member, true)
        .await
        .expect("disable");
    assert_eq!(outcome, SetDisabled::Done);
    for hash in [first, second] {
        assert!(
            identity::find_session(&db.app, &hash)
                .await
                .expect("find")
                .is_none(),
            "a session outlived the disable"
        );
    }
    let revoked: Vec<(Uuid, Option<Uuid>)> = sqlx::query_as(
        "SELECT id, revoked_by FROM api_tokens WHERE revoked_at IS NOT NULL ORDER BY id",
    )
    .fetch_all(&owner)
    .await
    .expect("tokens");
    assert_eq!(
        revoked,
        [(made, Some(admin))],
        "only the member's token, revoked by the admin"
    );
    assert_ne!(made, kept);
    assert!(matches!(
        session(&db.app, member).await,
        Err(DbError::AccountDisabled)
    ));
    assert_eq!(
        identity::password_sign_in(&db.app, &gate(), EMAIL, "198.51.100.7", "colophon-dev")
            .await
            .expect("sign-in"),
        PasswordSignIn::Refused,
        "the right password answers like a wrong one"
    );
    assert_eq!(
        identity::set_disabled(&db.app, admin, member, true)
            .await
            .expect("again"),
        SetDisabled::Unchanged
    );

    assert_eq!(
        identity::set_disabled(&db.app, admin, member, false)
            .await
            .expect("enable"),
        SetDisabled::Done
    );
    assert!(session(&db.app, member).await.is_ok());
    assert!(matches!(
        identity::password_sign_in(&db.app, &gate(), EMAIL, "198.51.100.7", "colophon-dev").await,
        Ok(PasswordSignIn::SignedIn(id)) if id == member
    ));
    assert_eq!(
        events(&owner, member).await,
        [
            ("account_disabled".to_owned(), Some(admin)),
            ("account_enabled".to_owned(), Some(admin))
        ]
    );
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn only_an_enabled_administrator_disables_and_never_their_own_account() {
    let db = TestDb::new().await;
    let owner = db.owner().await;
    let admin = db.administrator("admin").await;
    let member = db.person("member").await;
    let other = db.person("other").await;
    let project = projects::create(&db.app, admin, "site", "Site")
        .await
        .expect("project")
        .id;
    let made = token(&owner, project, admin).await;
    let token_identity: Uuid =
        sqlx::query_scalar("SELECT identity_id FROM api_tokens WHERE id = $1")
            .bind(made)
            .fetch_one(&owner)
            .await
            .expect("identity");

    for (actor, target, want) in [
        (member, other, SetDisabled::NotAdministrator),
        (admin, admin, SetDisabled::OwnAccount),
        (admin, Uuid::now_v7(), SetDisabled::NotFound),
        (admin, token_identity, SetDisabled::NotFound),
    ] {
        assert_eq!(
            identity::set_disabled(&db.app, actor, target, true)
                .await
                .expect("asked"),
            want
        );
    }
    let disabled: i64 =
        sqlx::query_scalar("SELECT count(*) FROM identities WHERE disabled_at IS NOT NULL")
            .fetch_one(&owner)
            .await
            .expect("count");
    assert_eq!(disabled, 0, "a refusal changed nothing");
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn two_administrators_disabling_each_other_at_once_leave_one_enabled() {
    let db = TestDb::new().await;
    let owner = db.owner().await;
    let pool = db.app_pool(4).await;
    for round in 0..10 {
        let a = db.administrator(&format!("a{round}")).await;
        let b = db.administrator(&format!("b{round}")).await;
        let (ab, ba) = tokio::join!(
            identity::set_disabled(&pool, a, b, true),
            identity::set_disabled(&pool, b, a, true),
        );
        let mut outcomes = [ab.expect("a"), ba.expect("b")];
        outcomes.sort_by_key(|o| *o == SetDisabled::Done);
        assert_eq!(
            outcomes,
            [SetDisabled::NotAdministrator, SetDisabled::Done],
            "round {round}"
        );
    }
    let enabled: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM administrators a JOIN identities i ON i.id = a.identity_id WHERE i.disabled_at IS NULL",
    )
    .fetch_one(&owner)
    .await
    .expect("count");
    assert_eq!(enabled, 10, "one of each pair stays enabled");
    pool.close().await;
    owner.close().await;
    db.drop().await;
}
