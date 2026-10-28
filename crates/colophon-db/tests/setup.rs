//! Setup codes against real PostgreSQL (D-061, amending D-043): only a hash is stored and the
//! application login can't read it from the table; a code makes one administrator, once, and
//! expires; attempts are rate-limited per person and in all; and once anyone is an administrator
//! (by a code or by `colophon admin bootstrap`), nothing is issued, attempted or claimed. Needs
//! the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::time::Duration;

use colophon_db::{
    administrators::{self, Standing},
    setup::{self, Attempt, Claimed},
};
use common::TestDb;

const HOUR: Duration = Duration::from_hours(1);

/// Stands in for a code's SHA-256: this layer only stores and compares 32-byte hashes, so any
/// distinct 32 bytes per code do (the real hashing is tested in colophon-auth and the server).
fn hash(code: &str) -> [u8; 32] {
    let mut out = [0_u8; 32];
    let mut state: u64 = 0xcbf2_9ce4_8422_2325;
    for (index, slot) in out.iter_mut().enumerate() {
        for byte in code.bytes().chain([u8::try_from(index).unwrap_or(0)]) {
            state = (state ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
        *slot = state.to_le_bytes()[0];
    }
    out
}

/// The live code with this hash, as the server finds it.
async fn live(db: &TestDb, code_hash: &[u8; 32]) -> Option<uuid::Uuid> {
    setup::live_codes(&db.app)
        .await
        .expect("live codes")
        .into_iter()
        .find(|code| &code.hash == code_hash)
        .map(|code| code.id)
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_code_is_stored_as_its_hash_and_makes_one_administrator_once() {
    let db = TestDb::new().await;
    let owner = db.owner().await;
    let first = db.person("first").await;
    let second = db.person("second").await;
    let code = hash("CODE-ONE");

    let expires = setup::issue(&db.app, &code, HOUR)
        .await
        .expect("issue")
        .expect("issued while nobody is an administrator");
    let left = expires - time::OffsetDateTime::now_utc();
    assert!(
        left > time::Duration::minutes(59) && left <= time::Duration::minutes(60),
        "{left}"
    );
    // Only the hash is stored.
    let stored: Vec<Vec<u8>> = sqlx::query_scalar("SELECT code_hash FROM setup_codes")
        .fetch_all(&owner)
        .await
        .expect("owner reads");
    assert_eq!(stored, vec![code.to_vec()]);

    // A wrong hash, an unknown id: nothing happens.
    let id = live(&db, &code).await.expect("live");
    assert_eq!(
        setup::claim(&db.app, first, id, &hash("CODE-TWO"))
            .await
            .expect("claim"),
        Claimed::Invalid
    );
    assert_eq!(
        setup::claim(&db.app, first, uuid::Uuid::now_v7(), &code)
            .await
            .expect("claim"),
        Claimed::Invalid
    );
    assert_eq!(
        administrators::standing(&db.app, first)
            .await
            .expect("standing"),
        Standing::NotBootstrapped
    );

    // The right one, once.
    assert_eq!(
        setup::claim(&db.app, first, id, &code)
            .await
            .expect("claim"),
        Claimed::Claimed
    );
    assert_eq!(
        administrators::standing(&db.app, first)
            .await
            .expect("standing"),
        Standing::Administrator
    );
    assert_eq!(
        setup::claim(&db.app, second, id, &code)
            .await
            .expect("again"),
        Claimed::AlreadySetUp,
        "a used code makes nobody else an administrator"
    );
    assert_eq!(
        administrators::standing(&db.app, second)
            .await
            .expect("standing"),
        Standing::Member
    );
    let (used_by, used): (Option<uuid::Uuid>, bool) =
        sqlx::query_as("SELECT used_by, used_at IS NOT NULL FROM setup_codes")
            .fetch_one(&owner)
            .await
            .expect("owner reads");
    assert_eq!((used_by, used), (Some(first), true));
    // The bootstrap's record is the same as `colophon admin bootstrap` makes, so it can't run again
    // for anyone else either.
    let by: uuid::Uuid = sqlx::query_scalar("SELECT bootstrapped_by FROM installation")
        .fetch_one(&owner)
        .await
        .expect("installation");
    assert_eq!(by, first);
    assert_eq!(
        administrators::bootstrap(&owner, "http://issuer.test", "second")
            .await
            .expect("bootstrap"),
        administrators::Bootstrapped::Refused
    );
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_application_login_reaches_codes_only_through_the_functions() {
    let db = TestDb::new().await;
    setup::issue(&db.app, &hash("CODE-HIDDEN"), HOUR)
        .await
        .expect("issue")
        .expect("issued");
    let denied = sqlx::query("SELECT * FROM setup_codes")
        .fetch_all(&db.app)
        .await
        .expect_err("the application login can't read the table");
    assert!(denied.to_string().contains("permission denied"), "{denied}");
    for table in [
        "setup_codes",
        "setup_attempts",
        "installation",
        "administrators",
    ] {
        let refused = sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {table}")))
            .execute(&db.app)
            .await
            .expect_err("nor change it");
        assert!(
            refused.to_string().contains("permission denied"),
            "{table}: {refused}"
        );
    }
    let person = db.person("person").await;
    let refused = sqlx::query("INSERT INTO installation (bootstrapped_by) VALUES ($1)")
        .bind(person)
        .execute(&db.app)
        .await
        .expect_err("nor bootstrap without a code");
    assert!(
        refused.to_string().contains("permission denied"),
        "{refused}"
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn an_expired_code_is_dead() {
    let db = TestDb::new().await;
    let owner = db.owner().await;
    let person = db.person("person").await;
    let code = hash("CODE-EXPIRES");
    setup::issue(&db.app, &code, HOUR)
        .await
        .expect("issue")
        .expect("issued");
    let id = live(&db, &code).await.expect("live");
    sqlx::query(
        "UPDATE setup_codes SET issued_at = now() - interval '2 hours', expires_at = now() - interval '1 second'",
    )
    .execute(&owner)
    .await
    .expect("age the code");
    assert_eq!(live(&db, &code).await, None, "an expired code isn't live");
    assert_eq!(
        setup::claim(&db.app, person, id, &code)
            .await
            .expect("claim"),
        Claimed::Invalid
    );
    assert_eq!(
        administrators::standing(&db.app, person)
            .await
            .expect("standing"),
        Standing::NotBootstrapped
    );
    // Lifetimes are bounded: a minute to a day.
    for lifetime in [Duration::from_secs(30), Duration::from_hours(25)] {
        setup::issue(&db.app, &hash("CODE-BOUNDS"), lifetime)
            .await
            .expect_err("refused");
    }
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn at_most_ten_codes_live_and_none_once_an_administrator_exists() {
    let db = TestDb::new().await;
    let owner = db.owner().await;
    for index in 0..10 {
        assert!(
            setup::issue(&db.app, &hash(&format!("CODE-{index}")), HOUR)
                .await
                .expect("issue")
                .is_some(),
            "code {index}"
        );
    }
    assert_eq!(
        setup::issue(&db.app, &hash("CODE-ELEVEN"), HOUR)
            .await
            .expect("issue"),
        None,
        "the eleventh waits for one to expire"
    );
    assert_eq!(setup::live_codes(&db.app).await.expect("live").len(), 10);
    assert!(!setup::has_administrator(&db.app).await.expect("set up?"));

    // The terminal's bootstrap closes the web's way in.
    administrators::bootstrap(&owner, "http://issuer.test", "operator")
        .await
        .expect("bootstrap");
    assert!(setup::has_administrator(&db.app).await.expect("set up?"));
    assert!(
        setup::live_codes(&db.app).await.expect("live").is_empty(),
        "no code is live once an administrator exists"
    );
    assert_eq!(
        setup::issue(&db.app, &hash("CODE-LATE"), HOUR)
            .await
            .expect("issue"),
        None
    );
    let person = db.person("late").await;
    assert_eq!(
        setup::attempt(&db.app, person).await.expect("attempt"),
        Attempt::AlreadySetUp
    );
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM setup_codes LIMIT 1")
        .fetch_one(&owner)
        .await
        .expect("a code");
    let code: Vec<u8> = sqlx::query_scalar("SELECT code_hash FROM setup_codes WHERE id = $1")
        .bind(id)
        .fetch_one(&owner)
        .await
        .expect("its hash");
    let code = <[u8; 32]>::try_from(code.as_slice()).expect("32 bytes");
    assert_eq!(
        setup::claim(&db.app, person, id, &code)
            .await
            .expect("claim"),
        Claimed::AlreadySetUp,
        "even an unused, unexpired code makes nobody an administrator now"
    );
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM setup_attempts")
        .fetch_one(&owner)
        .await
        .expect("count");
    assert_eq!(attempts, 0, "nothing is recorded once it's set up");
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn attempts_are_limited_per_person_and_in_all() {
    let db = TestDb::new().await;
    let owner = db.owner().await;
    let guesser = db.person("guesser").await;
    for index in 0..5 {
        assert_eq!(
            setup::attempt(&db.app, guesser).await.expect("attempt"),
            Attempt::Allowed,
            "attempt {index}"
        );
    }
    let Attempt::Limited { retry_after } = setup::attempt(&db.app, guesser).await.expect("sixth")
    else {
        panic!("the sixth attempt in fifteen minutes is refused");
    };
    assert!((890..=900).contains(&retry_after), "{retry_after}");
    // Someone else still may.
    let other = db.person("other").await;
    assert_eq!(
        setup::attempt(&db.app, other).await.expect("attempt"),
        Attempt::Allowed
    );
    // Twenty in all, from anyone: three more people take fourteen more places.
    for person in 0..3 {
        let someone = db.person(&format!("crowd-{person}")).await;
        for _ in 0..5 {
            if setup::attempt(&db.app, someone).await.expect("attempt") != Attempt::Allowed {
                break;
            }
        }
    }
    let recorded: i64 = sqlx::query_scalar("SELECT count(*) FROM setup_attempts")
        .fetch_one(&owner)
        .await
        .expect("count");
    assert_eq!(recorded, 20, "refused attempts aren't recorded");
    let newcomer = db.person("newcomer").await;
    assert!(
        matches!(
            setup::attempt(&db.app, newcomer).await.expect("attempt"),
            Attempt::Limited { .. }
        ),
        "the twenty-first attempt from anyone waits"
    );
    // Once the window has passed, there's room again.
    sqlx::query("UPDATE setup_attempts SET attempted_at = now() - interval '16 minutes'")
        .execute(&owner)
        .await
        .expect("age the attempts");
    assert_eq!(
        setup::attempt(&db.app, guesser).await.expect("attempt"),
        Attempt::Allowed
    );
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM setup_attempts")
        .fetch_one(&owner)
        .await
        .expect("count");
    assert_eq!(left, 1, "attempts outside the window are cleared");
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn two_codes_claimed_at_the_same_moment_make_one_administrator() {
    let db = TestDb::new().await;
    let pool = db.app_pool(4).await;
    let (a, b) = (db.person("a").await, db.person("b").await);
    let (code_a, code_b) = (hash("CODE-A"), hash("CODE-B"));
    for code in [&code_a, &code_b] {
        setup::issue(&db.app, code, HOUR)
            .await
            .expect("issue")
            .expect("issued");
    }
    let (id_a, id_b) = (
        live(&db, &code_a).await.expect("a"),
        live(&db, &code_b).await.expect("b"),
    );
    let (first, second) = tokio::join!(
        setup::claim(&pool, a, id_a, &code_a),
        setup::claim(&pool, b, id_b, &code_b)
    );
    let mut outcomes = [first.expect("a claims"), second.expect("b claims")];
    outcomes.sort_by_key(|outcome| *outcome != Claimed::Claimed);
    assert_eq!(outcomes, [Claimed::Claimed, Claimed::AlreadySetUp]);
    let administrators: i64 = sqlx::query_scalar("SELECT count(*) FROM administrators")
        .fetch_one(&db.owner().await)
        .await
        .expect("count");
    assert_eq!(administrators, 1);
    pool.close().await;
    db.drop().await;
}
