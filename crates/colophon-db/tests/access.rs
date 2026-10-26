//! Device logins and access tokens against real PostgreSQL, as the application's login: a code
//! is approved once and exchanged once, expired codes and tokens are dead, a revoked token is
//! gone, and only hashes are stored. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::access::{self, Poll};
use common::TestDb;
use sqlx::types::time::OffsetDateTime;

fn in_minutes(minutes: i64) -> OffsetDateTime {
    OffsetDateTime::now_utc() + time::Duration::minutes(minutes)
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_device_login_is_approved_once_and_exchanged_once() {
    let db = TestDb::new().await;
    let alice = db.person("alice").await;
    let device = [1_u8; 32];
    access::start_device_login(
        &db.app,
        &device,
        "BCDF-GHJK",
        "colophon CLI",
        in_minutes(10),
    )
    .await
    .expect("started");
    assert!(
        matches!(
            access::start_device_login(&db.app, &[2; 32], "BCDF-GHJK", "another", in_minutes(10))
                .await,
            Err(colophon_db::DbError::Conflict)
        ),
        "a live code can't be issued twice"
    );

    assert_eq!(
        access::poll_device_login(&db.app, &device)
            .await
            .expect("poll"),
        Poll::Pending
    );
    let shown = access::device_login(&db.app, "BCDF-GHJK")
        .await
        .expect("show")
        .expect("live");
    assert_eq!(
        (shown.client_name.as_str(), shown.approved),
        ("colophon CLI", false)
    );

    assert!(
        access::approve_device_login(&db.app, "BCDF-GHJK", alice)
            .await
            .expect("approve")
    );
    assert!(
        !access::approve_device_login(&db.app, "BCDF-GHJK", alice)
            .await
            .expect("approve"),
        "only once"
    );
    assert_eq!(
        access::poll_device_login(&db.app, &device)
            .await
            .expect("poll"),
        Poll::Approved {
            identity: alice,
            client_name: "colophon CLI".to_owned()
        }
    );
    assert_eq!(
        access::poll_device_login(&db.app, &device)
            .await
            .expect("poll"),
        Poll::Gone,
        "exchanged once"
    );
    assert_eq!(
        access::device_login(&db.app, "BCDF-GHJK")
            .await
            .expect("show"),
        None
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn an_expired_code_can_be_neither_approved_nor_exchanged() {
    let db = TestDb::new().await;
    let alice = db.person("alice").await;
    let device = [3_u8; 32];
    access::start_device_login(
        &db.app,
        &device,
        "CDFG-HJKL",
        "colophon CLI",
        in_minutes(10),
    )
    .await
    .expect("started");
    let owner = db.owner().await;
    sqlx::query("UPDATE device_logins SET created_at = now() - interval '1 hour', expires_at = now() - interval '1 minute'")
        .execute(&owner)
        .await
        .expect("aged");
    owner.close().await;
    assert!(
        !access::approve_device_login(&db.app, "CDFG-HJKL", alice)
            .await
            .expect("approve")
    );
    assert_eq!(
        access::poll_device_login(&db.app, &device)
            .await
            .expect("poll"),
        Poll::Gone
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_token_works_until_revoked_or_expired_and_only_its_hash_is_kept() {
    let db = TestDb::new().await;
    let alice = db.person("alice").await;
    let hash = [7_u8; 32];
    let id = access::create_access_token(&db.app, alice, &hash, "laptop", in_minutes(60))
        .await
        .expect("created");
    let found = access::find_access_token(&db.app, &hash)
        .await
        .expect("find")
        .expect("live");
    assert_eq!((found.id, found.identity_id), (id, alice));
    access::touch_access_token(&db.app, id)
        .await
        .expect("touch");
    access::revoke_access_token(&db.app, id)
        .await
        .expect("revoke");
    assert_eq!(
        access::find_access_token(&db.app, &hash)
            .await
            .expect("find"),
        None
    );

    let other = [8_u8; 32];
    let id = access::create_access_token(&db.app, alice, &other, "old laptop", in_minutes(60))
        .await
        .expect("created");
    let owner = db.owner().await;
    sqlx::query("UPDATE access_tokens SET created_at = now() - interval '2 hours', expires_at = now() - interval '1 hour' WHERE id = $1")
        .bind(id)
        .execute(&owner)
        .await
        .expect("aged");
    let (stored,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM information_schema.columns WHERE table_name = 'access_tokens' AND column_name = 'token'",
    )
    .fetch_one(&owner)
    .await
    .expect("columns");
    assert_eq!(stored, 0, "there is no column that could hold a token");
    owner.close().await;
    assert_eq!(
        access::find_access_token(&db.app, &other)
            .await
            .expect("find"),
        None,
        "expired"
    );

    // The application can't delete tokens: revoked ones stay for the audit trail.
    let error = sqlx::query("DELETE FROM access_tokens")
        .execute(&db.app)
        .await
        .expect_err("no grant");
    assert!(error.to_string().contains("permission denied"), "{error}");
    db.drop().await;
}
