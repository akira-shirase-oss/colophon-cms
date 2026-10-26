//! Administrators and the one-time bootstrap against real PostgreSQL (D-043): the bootstrap binds
//! the first administrator to an issuer and subject, works once (even when two run at the same
//! moment), and is permanent; only administrators can create projects, and the application login
//! can make nobody an administrator. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    administrators::{self, Bootstrapped, Standing},
    identity, projects,
};
use common::TestDb;

const ISSUER: &str = "http://issuer.test";

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn bootstrap_makes_one_administrator_once() {
    let db = TestDb::new().await;
    let owner = db.owner().await;
    let someone = db.person("someone").await;
    assert_eq!(
        administrators::standing(&db.app, someone)
            .await
            .expect("standing"),
        Standing::NotBootstrapped
    );

    // Before the person has ever signed in: the identity is created, and their first sign-in
    // finds it and fills in their name.
    let Bootstrapped::Done { identity: first } =
        administrators::bootstrap(&owner, ISSUER, "operator")
            .await
            .expect("bootstrap")
    else {
        panic!("the first bootstrap succeeds");
    };
    let signed_in = identity::record_sign_in(&db.app, ISSUER, "operator", Some("Operator"))
        .await
        .expect("sign-in");
    assert_eq!(signed_in, first, "the same identity");
    let profile = identity::profile(&db.app, first)
        .await
        .expect("profile")
        .expect("found");
    assert!(profile.identity.administrator);
    assert_eq!(profile.identity.display_name.as_deref(), Some("Operator"));

    assert_eq!(
        administrators::bootstrap(&owner, ISSUER, "operator")
            .await
            .expect("again"),
        Bootstrapped::AlreadyDone { identity: first },
        "running it again for the same person changes nothing"
    );
    assert_eq!(
        administrators::bootstrap(&owner, ISSUER, "someone")
            .await
            .expect("someone else"),
        Bootstrapped::Refused,
        "nor can it make anyone else an administrator"
    );
    assert_eq!(
        administrators::standing(&db.app, someone)
            .await
            .expect("standing"),
        Standing::Member
    );
    assert_eq!(
        administrators::standing(&db.app, first)
            .await
            .expect("standing"),
        Standing::Administrator
    );

    // Permanent, even for the table owner.
    for statement in [
        "DELETE FROM installation",
        "UPDATE installation SET bootstrapped_at = now()",
        "TRUNCATE installation",
    ] {
        let error = sqlx::query(statement)
            .execute(&owner)
            .await
            .expect_err(statement);
        assert!(
            format!("{error:?}").contains("permanent"),
            "{statement}: {error:?}"
        );
    }
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn two_bootstraps_at_once_make_one_administrator() {
    let db = TestDb::new().await;
    let (first, second) = (db.owner().await, db.owner().await);
    let (a, b) = tokio::join!(
        administrators::bootstrap(&first, ISSUER, "ann"),
        administrators::bootstrap(&second, ISSUER, "ben")
    );
    let outcomes = [a.expect("ann"), b.expect("ben")];
    let done = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, Bootstrapped::Done { .. }))
        .count();
    let refused = outcomes
        .iter()
        .filter(|outcome| **outcome == Bootstrapped::Refused)
        .count();
    assert_eq!((done, refused), (1, 1), "{outcomes:?}");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM administrators")
        .fetch_one(&first)
        .await
        .expect("count");
    assert_eq!(count, 1);
    first.close().await;
    second.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn only_administrators_create_projects_and_requests_make_no_administrators() {
    let db = TestDb::new().await;
    let admin = db.administrator("admin").await;
    let member = db.person("member").await;
    projects::create(&db.app, admin, "alpha", "Alpha")
        .await
        .expect("an administrator creates a project");

    // Below the API: the database refuses a non-administrator's project.
    let refused = projects::create(&db.app, member, "beta", "Beta").await;
    assert!(
        format!("{refused:?}").contains("row-level security"),
        "{refused:?}"
    );

    // The application login can't make anyone an administrator, or bootstrap.
    for statement in [
        "INSERT INTO administrators (identity_id) VALUES ($1)",
        "INSERT INTO installation (bootstrapped_by) VALUES ($1)",
    ] {
        let error = sqlx::query(statement)
            .bind(member)
            .execute(&db.app)
            .await
            .expect_err(statement);
        assert!(
            format!("{error:?}").contains("permission denied"),
            "{statement}: {error:?}"
        );
    }
    assert_eq!(
        administrators::standing(&db.app, member)
            .await
            .expect("standing"),
        Standing::NotBootstrapped,
        "an administrator added by hand isn't a bootstrap"
    );
    db.drop().await;
}
