//! Email-and-password accounts against real PostgreSQL, as the application's login: a sign-up
//! stores only the Argon2id hash, the same email can sign up once, a wrong password and a
//! missing account get the same refusal, enough misses pause the account for the right
//! password too, and a person invited to a project can sign up and join with the link.
//! Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    PgPool,
    identity::{self, LOCK_AFTER, PasswordSignIn, SignedUp},
    members::{self, Accepted},
    passwords, projects,
    scope::{self, MemberTransaction},
};
use common::TestDb;
use sqlx::types::time::OffsetDateTime;
use uuid::Uuid;

async fn as_member(pool: &PgPool, identity: Uuid, project: Uuid) -> MemberTransaction {
    scope::begin_as_member(pool, identity, project)
        .await
        .expect("begin")
        .expect("a member")
}

async fn made(pool: &PgPool, email: &str, password: &str) -> Uuid {
    let hash = passwords::hashed(password).await.expect("hashed");
    match identity::sign_up(pool, email, &hash, Some("Bob"))
        .await
        .expect("sign-up")
    {
        SignedUp::Made(id) => id,
        SignedUp::Taken => panic!("{email} was not taken before"),
    }
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn an_account_signs_up_once_signs_in_and_never_shows_its_password() {
    let db = TestDb::new().await;
    let password = "correct horse battery staple";
    let bob = made(&db.app, "bob@example.com", password).await;

    // The right password signs in; the same email can't sign up twice.
    assert_eq!(
        identity::password_sign_in(&db.app, "bob@example.com", password)
            .await
            .expect("sign-in"),
        PasswordSignIn::SignedIn(bob)
    );
    let again = passwords::hashed("another password").await.expect("hashed");
    assert_eq!(
        identity::sign_up(&db.app, "bob@example.com", &again, None)
            .await
            .expect("sign-up"),
        SignedUp::Taken
    );

    // A wrong password and an email with no account answer alike.
    for email in ["bob@example.com", "carol@example.com"] {
        assert_eq!(
            identity::password_sign_in(&db.app, email, "wrong password")
                .await
                .expect("sign-in"),
            PasswordSignIn::Refused,
            "{email} should refuse"
        );
    }

    // The stored row holds the PHC string, never the password; the miss above counted once.
    let row = sqlx::query!(
        "SELECT email, password_hash, failed_sign_ins FROM identities WHERE id = $1",
        bob
    )
    .fetch_one(&db.app)
    .await
    .expect("the identity row");
    assert_eq!(row.email.as_deref(), Some("bob@example.com"));
    assert!(
        row.password_hash
            .as_deref()
            .is_some_and(|phc| phc.starts_with("$argon2id$") && !phc.contains(password))
    );
    assert_eq!(row.failed_sign_ins, 1);
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn enough_misses_pause_the_account_even_for_the_right_password() {
    let db = TestDb::new().await;
    let password = "the real password";
    made(&db.app, "dave@example.com", password).await;

    for miss in 1..LOCK_AFTER {
        assert_eq!(
            identity::password_sign_in(&db.app, "dave@example.com", "a guess")
                .await
                .expect("sign-in"),
            PasswordSignIn::Refused,
            "miss {miss} should refuse, not lock"
        );
    }
    let locked = identity::password_sign_in(&db.app, "dave@example.com", "a guess")
        .await
        .expect("sign-in");
    assert!(
        matches!(locked, PasswordSignIn::Locked(until) if until > OffsetDateTime::now_utc()),
        "miss {LOCK_AFTER} should lock: {locked:?}"
    );
    // While paused, the right password gets the same answer — no way to try more.
    let still = identity::password_sign_in(&db.app, "dave@example.com", password)
        .await
        .expect("sign-in");
    assert!(matches!(still, PasswordSignIn::Locked(_)), "{still:?}");
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn an_invited_person_creates_their_account_and_joins() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "herald", "Herald")
        .await
        .expect("created")
        .id;
    let mut tx = as_member(&db.app, alice, project).await;
    members::invite(
        &mut tx,
        &[7; 32],
        "editor",
        "Erin",
        OffsetDateTime::now_utc() + time::Duration::hours(72),
    )
    .await
    .expect("invited");
    tx.commit().await.expect("commit");

    // Erin has no account yet; she creates one and joins with the link, in that order.
    let erin = made(&db.app, "erin@example.com", "a fine password").await;
    assert_eq!(
        members::accept(&db.app, erin, &[7; 32])
            .await
            .expect("accept"),
        Accepted::Joined {
            project_id: project,
            role: "editor".to_owned(),
        }
    );
    db.drop().await;
}
