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

/// The address the tests' own sign-ins come from.
const HERE: &str = "203.0.113.1";

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
    match identity::sign_up(
        pool,
        email,
        &hash,
        Some("Bob"),
        identity::Admission::Open { invitation: None },
        "203.0.113.9",
    )
    .await
    .expect("sign-up")
    {
        SignedUp::Made { identity, .. } => identity,
        SignedUp::Taken => panic!("{email} was not taken before"),
        SignedUp::InvitationRequired => panic!("open registration needs no invitation"),
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
        identity::password_sign_in(&db.app, &common::gate(), "bob@example.com", HERE, password)
            .await
            .expect("sign-in"),
        PasswordSignIn::SignedIn(bob)
    );
    let again = passwords::hashed("another password").await.expect("hashed");
    assert_eq!(
        identity::sign_up(
            &db.app,
            "bob@example.com",
            &again,
            None,
            identity::Admission::Open { invitation: None },
            "203.0.113.9"
        )
        .await
        .expect("sign-up"),
        SignedUp::Taken
    );

    // A wrong password and an email with no account answer alike.
    for email in ["bob@example.com", "carol@example.com"] {
        assert_eq!(
            identity::password_sign_in(&db.app, &common::gate(), email, HERE, "wrong password")
                .await
                .expect("sign-in"),
            PasswordSignIn::Refused,
            "{email} should refuse"
        );
    }

    // The stored row holds the PHC string, never the password.
    let row = sqlx::query!(
        "SELECT email, password_hash FROM identities WHERE id = $1",
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
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn enough_misses_pause_the_guessing_address_and_never_the_account() {
    let db = TestDb::new().await;
    let password = "the real password";
    made(&db.app, "dave@example.com", password).await;
    let guesser = "198.51.100.9";

    // Every miss refuses alike, the LOCK_AFTER-th included.
    for miss in 1..=LOCK_AFTER {
        assert_eq!(
            identity::password_sign_in(
                &db.app,
                &common::gate(),
                "dave@example.com",
                guesser,
                "a guess"
            )
            .await
            .expect("sign-in"),
            PasswordSignIn::Refused,
            "miss {miss}"
        );
    }
    // Paused for the guesser: the right password from there answers like a wrong one.
    assert_eq!(
        identity::password_sign_in(
            &db.app,
            &common::gate(),
            "dave@example.com",
            guesser,
            password
        )
        .await
        .expect("sign-in"),
        PasswordSignIn::Refused
    );
    // The owner, elsewhere, signs in — and the guesser's pause is untouched by it.
    assert!(matches!(
        identity::password_sign_in(&db.app, &common::gate(), "dave@example.com", HERE, password)
            .await
            .expect("sign-in"),
        PasswordSignIn::SignedIn(_)
    ));
    let paused = sqlx::query_scalar!(
        r#"SELECT locked_until > now() AS "paused!" FROM sign_in_throttle
           WHERE realm = 'staff' AND email = 'dave@example.com' AND address = $1"#,
        guesser
    )
    .fetch_one(&db.app)
    .await
    .expect("the guesser's key");
    assert!(paused, "only the owner's own key was forgotten");
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

/// Concurrent first attempts can't clear the pause one of them set (D-162):
/// with no row yet for the key, every attempt used to pass the pause check at once, and the
/// one counted after the [`LOCK_AFTER`]th took the pause for "a pause that ran out" and reset
/// it. Here the table is held so that every attempt has checked before any has counted.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn concurrent_first_misses_keep_the_pause_they_reach() {
    let db = TestDb::new().await;
    let password = "the real password";
    made(&db.app, "erin@example.com", password).await;
    let guesser = "198.51.100.10";
    let attempts = usize::try_from(LOCK_AFTER).expect("small") + 4;
    let pool = db.app_pool(u32::try_from(attempts).expect("small")).await;
    let gate = colophon_db::passwords::PasswordGate::new(attempts);

    // Counting writes wait behind this SHARE lock; the pause checks (row locks) don't.
    let owner = db.owner().await;
    let mut hold = owner.begin().await.expect("begin");
    sqlx::query("LOCK TABLE sign_in_throttle IN SHARE MODE")
        .execute(&mut *hold)
        .await
        .expect("lock");
    let misses: Vec<_> = (0..attempts)
        .map(|_| {
            let (pool, gate) = (pool.clone(), gate.clone());
            tokio::spawn(async move {
                identity::password_sign_in(&pool, &gate, "erin@example.com", guesser, "a guess")
                    .await
                    .expect("sign-in")
            })
        })
        .collect();
    // Every attempt is waiting on a lock: past its check, or (fixed) behind the account's.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let waiting: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_locks WHERE NOT granted")
            .fetch_one(&owner)
            .await
            .expect("locks");
        if waiting >= i64::try_from(attempts).expect("small") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "only {waiting} attempts waiting"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    hold.commit().await.expect("release");
    for miss in misses {
        assert_eq!(miss.await.expect("joined"), PasswordSignIn::Refused);
    }

    // More misses than LOCK_AFTER, so the guesser is paused — the right password included.
    assert_eq!(
        identity::password_sign_in(
            &db.app,
            &common::gate(),
            "erin@example.com",
            guesser,
            password
        )
        .await
        .expect("sign-in"),
        PasswordSignIn::Refused,
        "the pause the eighth miss set was cleared by the misses after it"
    );
    pool.close().await;
    owner.close().await;
    db.drop().await;
}

/// Sign-in keys hold emails and client addresses, so each realm's keys are visible only where
/// that realm signs in (D-166): staff keys outside any project, a project's audience keys in
/// that project's context, nobody else's anywhere.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn sign_in_keys_are_visible_only_in_their_own_realm() {
    let db = TestDb::new().await;
    let owner = db.owner().await;
    let (mine, theirs) = (Uuid::now_v7(), Uuid::now_v7());
    for realm in [
        "staff".to_owned(),
        format!("audience:{mine}"),
        format!("audience:{theirs}"),
    ] {
        sqlx::query(
            "INSERT INTO sign_in_throttle (realm, email, address, failures)
             VALUES ($1, 'a@b.c', '198.51.100.1', 1)",
        )
        .bind(realm)
        .execute(&owner)
        .await
        .expect("a key");
    }
    let visible = |project: Option<Uuid>| {
        let pool = db.app.clone();
        async move {
            let mut tx = scope::begin(
                &pool,
                scope::Scope {
                    identity: None,
                    project,
                },
            )
            .await
            .expect("begin");
            let realms: Vec<String> =
                sqlx::query_scalar("SELECT realm FROM sign_in_throttle ORDER BY realm")
                    .fetch_all(&mut *tx)
                    .await
                    .expect("read");
            tx.commit().await.expect("commit");
            realms
        }
    };
    assert_eq!(visible(None).await, ["staff"]);
    assert_eq!(visible(Some(mine)).await, [format!("audience:{mine}")]);
    // Nor can a project's context write another realm's key.
    let mut tx = scope::begin(
        &db.app,
        scope::Scope {
            identity: None,
            project: Some(mine),
        },
    )
    .await
    .expect("begin");
    let written = sqlx::query(
        "INSERT INTO sign_in_throttle (realm, email, address, failures)
         VALUES ('staff', 'b@b.c', '198.51.100.2', 1)",
    )
    .execute(&mut *tx)
    .await;
    assert!(written.is_err(), "a project context wrote a staff key");
    drop(tx);
    owner.close().await;
    db.drop().await;
}

/// A paused key is refused before it takes a password slot (D-213): only work that will hash
/// takes one. The gate's only slot is held elsewhere and nothing may wait, so an attempt that
/// asked for a slot first would be shed as busy instead of answering as a paused key does.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_paused_key_is_refused_without_taking_a_slot() {
    let db = TestDb::new().await;
    made(&db.app, "fay@example.com", "the real password").await;
    let guesser = "198.51.100.11";
    for _ in 1..=LOCK_AFTER {
        identity::password_sign_in(
            &db.app,
            &common::gate(),
            "fay@example.com",
            guesser,
            "a guess",
        )
        .await
        .expect("sign-in");
    }
    let gate = passwords::PasswordGate::with_waiting(1, 0, std::time::Duration::from_millis(50));
    let held = gate.slot().await.expect("the only slot");
    assert_eq!(gate.available(), 0);
    assert_eq!(
        identity::password_sign_in(
            &db.app,
            &gate,
            "fay@example.com",
            guesser,
            "the real password"
        )
        .await
        .expect("a paused key answers without waiting for a slot"),
        PasswordSignIn::Refused
    );
    assert_eq!(gate.available(), 0, "the paused attempt took no slot");
    drop(held);
    assert_eq!(gate.available(), 1);
    db.drop().await;
}

/// Records the summary of every statement sqlx runs on this thread.
#[derive(Clone, Default)]
struct Statements(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Statements {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _context: tracing_subscriber::layer::Context<'_, S>,
    ) {
        struct Summary(String);
        impl tracing::field::Visit for Summary {
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "summary" {
                    value.clone_into(&mut self.0);
                }
            }
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "summary" {
                    self.0 = format!("{value:?}");
                }
            }
        }
        if event.metadata().target() == "sqlx::query" {
            let mut summary = Summary(String::new());
            event.record(&mut summary);
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(summary.0);
        }
    }
}

/// A sign-in's round trips (D-250): on success three statements — the pause check, the count with
/// the account's read, the session with its log line — and no BEGIN or COMMIT, from a new address
/// as from a trusted one; a miss two. Each statement, on a connection that has run it before, is
/// one round trip, so this is the sign-in's whole wait on the database.
#[tokio::test(flavor = "current_thread")]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_sign_in_waits_on_the_database_three_times() {
    use tracing_subscriber::layer::SubscriberExt as _;
    let db = TestDb::new().await;
    let password = "correct horse battery staple";
    made(&db.app, "rita@example.com", password).await;
    let statements = Statements::default();
    let _recording =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(statements.clone()));
    let session = |seed: u8| identity::NewSession {
        token_hash: Box::leak(Box::new([seed; 32])),
        csrf_hash: Box::leak(Box::new([seed.wrapping_add(1); 32])),
        expires_at: OffsetDateTime::now_utc() + time::Duration::HOUR,
    };
    let taken = |statements: &Statements| {
        std::mem::take(
            &mut *statements
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    };
    for (address, seed) in [
        ("198.51.100.1", 1),
        ("198.51.100.1", 3),
        ("198.51.100.2", 5),
    ] {
        taken(&statements);
        let signed = identity::password_sign_in_session(
            &db.app,
            &common::gate(),
            "rita@example.com",
            address,
            password,
            session(seed),
        )
        .await
        .expect("sign-in");
        assert!(matches!(signed, identity::SessionSignIn::SignedIn { .. }));
        let ran = taken(&statements);
        assert_eq!(ran.len(), 3, "{address}: {ran:#?}");
        assert!(
            !ran.iter()
                .any(|sql| sql.starts_with("BEGIN") || sql.starts_with("COMMIT")),
            "{ran:#?}"
        );
    }
    taken(&statements);
    let missed = identity::password_sign_in_session(
        &db.app,
        &common::gate(),
        "rita@example.com",
        "198.51.100.3",
        "wrong",
        session(7),
    )
    .await
    .expect("sign-in");
    assert_eq!(missed, identity::SessionSignIn::Refused);
    assert_eq!(taken(&statements).len(), 2);
}
