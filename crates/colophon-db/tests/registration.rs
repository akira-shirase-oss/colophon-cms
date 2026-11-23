//! Registration (D-235) against real PostgreSQL: under invite-only registration a sign-up is
//! admitted with a valid, unclaimed project invitation, claimed for exactly one new account in
//! the same transaction, or, without one, as the installation's single first-run account while
//! nobody is an administrator and no person has an account; closed registration admits no first
//! sign-in through a provider. Concurrent sign-ups decide each of those exactly once. Needs the
//! local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::sync::Arc;

use colophon_db::{
    identity::{self, Admission, SignedUp},
    members, projects, scope,
};
use common::TestDb;
use time::OffsetDateTime;
use uuid::Uuid;

const PHC: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0$YmFzZTY0aGFzaGJhc2U2NGhhc2hiYXNlNjRoYXNoYmE";

/// People (not system or token identities) with an account.
async fn people(db: &TestDb) -> i64 {
    let owner = db.owner().await;
    let count = sqlx::query_scalar(
        "SELECT count(*) FROM identities WHERE issuer NOT LIKE 'urn:colophon:%'",
    )
    .fetch_one(&owner)
    .await
    .expect("count");
    owner.close().await;
    count
}

/// `count` sign-ups at once, each with its own email, under `invitation`; their outcomes.
async fn race(db: &TestDb, count: usize, invitation: Option<[u8; 32]>) -> Vec<SignedUp> {
    let pool = db.app_pool(u32::try_from(count).expect("small")).await;
    let barrier = Arc::new(tokio::sync::Barrier::new(count));
    let mut racing = tokio::task::JoinSet::new();
    for n in 0..count {
        let (pool, barrier) = (pool.clone(), Arc::clone(&barrier));
        let email = format!("racer-{n}-{}@example.test", Uuid::now_v7().simple());
        racing.spawn(async move {
            barrier.wait().await;
            identity::sign_up(
                &pool,
                &email,
                PHC,
                None,
                Admission::Invite {
                    invitation: invitation.as_ref(),
                },
                "203.0.113.9",
            )
            .await
            .expect("sign-up")
        });
    }
    let outcomes = racing.join_all().await;
    pool.close().await;
    outcomes
}

fn tally(outcomes: &[SignedUp]) -> (usize, usize) {
    (
        outcomes
            .iter()
            .filter(|o| matches!(o, SignedUp::Made { .. }))
            .count(),
        outcomes
            .iter()
            .filter(|o| **o == SignedUp::InvitationRequired)
            .count(),
    )
}

/// An administrator's project with one invitation whose token hash is `token`.
async fn invitation(db: &TestDb, token: [u8; 32]) -> Uuid {
    let admin = db.administrator("operator").await;
    let project = projects::create(
        &db.app,
        admin,
        &format!("p-{}", &Uuid::now_v7().simple().to_string()[20..]),
        "Invited",
    )
    .await
    .expect("created")
    .id;
    let mut tx = scope::begin_as_member(&db.app, admin, project)
        .await
        .expect("begin")
        .expect("a member");
    members::invite(
        &mut tx,
        &token,
        "editor",
        "A guest",
        OffsetDateTime::now_utc() + time::Duration::hours(72),
    )
    .await
    .expect("invited");
    tx.commit().await.expect("commit");
    project
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn invite_only_admits_one_first_run_account_however_many_race() {
    let db = TestDb::new().await;
    assert!(identity::first_run_open(&db.app).await.expect("open?"));
    let outcomes = race(&db, 8, None).await;
    assert_eq!(tally(&outcomes), (1, 7), "{outcomes:?}");
    assert_eq!(people(&db).await, 1);
    assert!(!identity::first_run_open(&db.app).await.expect("open?"));
    // Taken or new, an email gets the same refusal now.
    let first = outcomes
        .iter()
        .find_map(|o| match o {
            SignedUp::Made { identity, .. } => Some(*identity),
            _ => None,
        })
        .expect("one made");
    let email: String = sqlx::query_scalar("SELECT email FROM identities WHERE id = $1")
        .bind(first)
        .fetch_one(&db.app)
        .await
        .expect("its email");
    for email in [email.as_str(), "someone-new@example.test"] {
        assert_eq!(
            identity::sign_up(
                &db.app,
                email,
                PHC,
                None,
                Admission::Invite { invitation: None },
                "203.0.113.9"
            )
            .await
            .expect("sign-up"),
            SignedUp::InvitationRequired,
            "{email}"
        );
    }
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn once_set_up_invite_only_refuses_a_sign_up_without_an_invitation() {
    let db = TestDb::new().await;
    // An administrator alone closes the first-run window (system identities never open it).
    db.administrator("operator").await;
    assert!(!identity::first_run_open(&db.app).await.expect("open?"));
    assert_eq!(tally(&race(&db, 4, None).await), (0, 4));
    // Open registration admits the same sign-up.
    assert!(matches!(
        identity::sign_up(
            &db.app,
            "open@example.test",
            PHC,
            None,
            Admission::Open { invitation: None },
            "203.0.113.9"
        )
        .await
        .expect("sign-up"),
        SignedUp::Made { .. }
    ));
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn one_invitation_makes_exactly_one_account_however_many_race() {
    let db = TestDb::new().await;
    let token = [41_u8; 32];
    let project = invitation(&db, token).await;
    let before = people(&db).await;
    let outcomes = race(&db, 8, Some(token)).await;
    assert_eq!(tally(&outcomes), (1, 7), "{outcomes:?}");
    let owner = db.owner().await;
    let details: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT detail FROM staff_auth_events WHERE event = 'signed_up' ORDER BY at",
    )
    .fetch_all(&owner)
    .await
    .expect("the staff auth log");
    assert_eq!(
        details.last().cloned().flatten().as_deref(),
        Some("invitation"),
        "the account that claimed the invitation is logged as such: {details:?}"
    );
    owner.close().await;
    assert_eq!(people(&db).await, before + 1, "the refused made no account");
    let joined = outcomes
        .iter()
        .find_map(|o| match o {
            SignedUp::Made { identity, .. } => Some(*identity),
            _ => None,
        })
        .expect("one made");
    let owner = db.owner().await;
    let (role, accepted_by): (String, Option<Uuid>) = sqlx::query_as(
        "SELECT m.role, i.accepted_by FROM memberships m, invitations i
         WHERE m.project_id = $1 AND m.identity_id = $2 AND i.token_hash = $3",
    )
    .bind(project)
    .bind(joined)
    .bind(&token[..])
    .fetch_one(&owner)
    .await
    .expect("the membership and the claim");
    assert_eq!((role.as_str(), accepted_by), ("editor", Some(joined)));
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn an_unknown_or_claimed_invitation_is_refused_alike_and_a_taken_email_leaves_it_unclaimed() {
    let db = TestDb::new().await;
    let token = [42_u8; 32];
    invitation(&db, token).await;
    let invite = |token| Admission::Invite {
        invitation: Some(token),
    };
    assert_eq!(
        identity::sign_up(
            &db.app,
            "a@example.test",
            PHC,
            None,
            invite(&[43_u8; 32]),
            "203.0.113.9"
        )
        .await
        .expect("sign-up"),
        SignedUp::InvitationRequired,
        "an unknown token"
    );
    // A taken email with a valid invitation: 409, and the invitation stays for its person.
    identity::sign_up(
        &db.app,
        "taken@example.test",
        PHC,
        None,
        Admission::Open { invitation: None },
        "203.0.113.9",
    )
    .await
    .expect("sign-up");
    assert_eq!(
        identity::sign_up(
            &db.app,
            "taken@example.test",
            PHC,
            None,
            invite(&token),
            "203.0.113.9"
        )
        .await
        .expect("sign-up"),
        SignedUp::Taken
    );
    assert!(matches!(
        identity::sign_up(
            &db.app,
            "b@example.test",
            PHC,
            None,
            invite(&token),
            "203.0.113.9"
        )
        .await
        .expect("sign-up"),
        SignedUp::Made { .. }
    ));
    assert_eq!(
        identity::sign_up(
            &db.app,
            "c@example.test",
            PHC,
            None,
            invite(&token),
            "203.0.113.9"
        )
        .await
        .expect("sign-up"),
        SignedUp::InvitationRequired,
        "a claimed token, refused like an unknown one"
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn closed_registration_admits_no_first_sign_in_through_a_provider() {
    let db = TestDb::new().await;
    let issuer = "http://issuer.test";
    assert_eq!(
        identity::record_existing_sign_in(&db.app, issuer, "stranger", Some("Stranger"))
            .await
            .expect("sign-in"),
        None
    );
    assert_eq!(people(&db).await, 0, "nothing was written");
    let known = db.person("known").await;
    assert_eq!(
        identity::record_existing_sign_in(&db.app, issuer, "known", Some("Known"))
            .await
            .expect("sign-in"),
        Some(known)
    );
    db.drop().await;
}

/// The first-run decision waits for a competing first sign-up that holds the installation's lock
/// and has made its account but not committed, then refuses: decided on the committed state, never
/// on two snapshots that each saw no account. (Without the lock, the sign-up below sees no
/// committed person and makes a second account.)
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_first_run_sign_up_waits_for_a_competing_one_and_is_refused_after_it_commits() {
    let db = TestDb::new().await;
    let mut competing = db.app.begin().await.expect("begin");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('colophon:first-run-account'), 0)")
        .execute(&mut *competing)
        .await
        .expect("the first-run lock");
    sqlx::query(
        "INSERT INTO identities (id, issuer, subject, email, password_hash)
         VALUES ($1, 'password', 'first@example.test', 'first@example.test', $2)",
    )
    .bind(Uuid::now_v7())
    .bind(PHC)
    .execute(&mut *competing)
    .await
    .expect("the competing account, uncommitted");
    // A pool of its own: `db.app` has one connection, which the competing transaction holds, so
    // waiting for it would pass this test with no lock at all.
    let pool = db.app_pool(1).await;
    let waiting = tokio::spawn(async move {
        identity::sign_up(
            &pool,
            "second@example.test",
            PHC,
            None,
            Admission::Invite { invitation: None },
            "203.0.113.9",
        )
        .await
        .expect("sign-up")
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(!waiting.is_finished(), "it waits for the lock's holder");
    competing.commit().await.expect("commit");
    assert_eq!(waiting.await.expect("joined"), SignedUp::InvitationRequired);
    assert_eq!(people(&db).await, 1);
    db.drop().await;
}

/// A sign-up claiming an invitation that a competing claim holds, uncommitted, waits on the
/// invitation's row and finds it claimed once that commits: no second account. (Without the row
/// lock, it reads the invitation as unclaimed and makes an account the claim then overwrites.)
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn an_invitation_claim_waits_for_a_competing_one_and_is_refused_after_it_commits() {
    let db = TestDb::new().await;
    let token = [44_u8; 32];
    invitation(&db, token).await;
    let claimer = db.person("claimer").await;
    let owner = db.owner().await;
    let mut competing = owner.begin().await.expect("begin");
    sqlx::query(
        "UPDATE invitations SET accepted_by = $2, accepted_at = now() WHERE token_hash = $1",
    )
    .bind(&token[..])
    .bind(claimer)
    .execute(&mut *competing)
    .await
    .expect("the competing claim, uncommitted");
    let pool = db.app_pool(1).await;
    let waiting = tokio::spawn(async move {
        identity::sign_up(
            &pool,
            "late@example.test",
            PHC,
            None,
            Admission::Invite {
                invitation: Some(&token),
            },
            "203.0.113.9",
        )
        .await
        .expect("sign-up")
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(!waiting.is_finished(), "it waits on the invitation's row");
    competing.commit().await.expect("commit");
    assert_eq!(waiting.await.expect("joined"), SignedUp::InvitationRequired);
    let made: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM identities WHERE email = 'late@example.test')",
    )
    .fetch_one(&owner)
    .await
    .expect("query");
    assert!(!made, "no account for the late claim");
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn open_registration_claims_a_valid_invitation_and_ignores_an_invalid_one() {
    let db = TestDb::new().await;
    let token = [45_u8; 32];
    let project = invitation(&db, token).await;
    let open = |token| Admission::Open {
        invitation: Some(token),
    };
    let SignedUp::Made {
        joined: Some(joined),
        ..
    } = identity::sign_up(
        &db.app,
        "joins@example.test",
        PHC,
        None,
        open(&token),
        "203.0.113.9",
    )
    .await
    .expect("sign-up")
    else {
        panic!("a valid invitation is claimed");
    };
    assert_eq!((joined.project, joined.role.as_str()), (project, "editor"));
    // Spent now: the next sign-up with it is still admitted, joining nothing.
    assert!(matches!(
        identity::sign_up(
            &db.app,
            "later@example.test",
            PHC,
            None,
            open(&token),
            "203.0.113.9"
        )
        .await
        .expect("sign-up"),
        SignedUp::Made { joined: None, .. }
    ));
    db.drop().await;
}
