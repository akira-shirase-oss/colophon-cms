//! Budget reservations against real PostgreSQL (migration 0045; D-113, invariant AU3): a
//! token's write reserves its document under the limits lock, so two racing requests of one
//! token can't both see room; the outcome's commit settles it; a publish reserves one slot per
//! job it dispatches, the job's outcome closes it, and the reconciler frees what died and
//! flags what is stuck as `budget.discrepancy`. A person or an unconstrained token leaves no
//! rows at all. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::time::Duration;

use colophon_db::{
    DbError, PgPool, audit, budgets, changesets, destinations,
    entries::{self, NewRevision},
    jobs::{self, Finish},
    projects, releases,
    scope::{self, MemberTransaction, ProjectTransaction},
    tokens::{self, Created, NewToken},
};
use colophon_domain::{
    budget::{Kind, Limits},
    policy::TokenKind,
};
use common::TestDb;
use sqlx::types::time::OffsetDateTime;
use uuid::Uuid;

const HOUR: Duration = Duration::from_secs(3600);

async fn as_member(pool: &PgPool, identity: Uuid, project: Uuid) -> MemberTransaction {
    scope::begin_as_member(pool, identity, project)
        .await
        .expect("begin")
        .expect("a member")
}

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

async fn as_token(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project_as_token(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("an active token")
}

/// A full-access token in `project` made by `owner`, with `limits`; its id and identity.
async fn limited_token(
    pool: &PgPool,
    owner: Uuid,
    project: Uuid,
    hash: [u8; 32],
    limits: &Limits,
) -> (Uuid, Uuid) {
    let policy = TokenKind::FullAccess
        .policy(Vec::new())
        .expect("full access");
    let mut tx = as_member(pool, owner, project).await;
    let token = NewToken {
        id: Uuid::now_v7(),
        identity_id: Uuid::now_v7(),
        name: "Site build",
        description: "",
        kind: TokenKind::FullAccess,
        policy: &policy,
        hash,
        expires_at: OffsetDateTime::now_utc() + time::Duration::days(30),
        limits: Some(limits),
    };
    let Created::Done(stored) = tokens::create(&mut tx, &token).await.expect("create") else {
        panic!("a project's first token was refused");
    };
    assert_eq!(stored.limits, Some(*limits), "the limits row round-trips");
    tx.commit().await.expect("commit");
    (stored.id, stored.identity_id)
}

/// A fresh project; alice (an administrator, its owner) and the project.
async fn project(db: &TestDb) -> (Uuid, Uuid) {
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    (alice, project)
}

fn document() -> NewRevision<'static> {
    NewRevision {
        id: Uuid::now_v7(),
        content: r#"{"title":"Hi"}"#,
        content_hash: &[1; 32],
        title: "Hi",
        slug: None,
        schema_version: 1,
        references: &[],
        assets: &[],
        unique: &[],
        visibility: None,
    }
}

/// Writes one article under `tx`; the caller commits.
async fn write(tx: &mut ProjectTransaction) -> Result<Uuid, DbError> {
    let entry = Uuid::now_v7();
    entries::create(tx, entry, "articles", document(), None).await?;
    Ok(entry)
}

/// Whether `error` is the budget refusing `kind` work.
fn refused(error: &DbError, kind: Kind) -> bool {
    matches!(error, DbError::BudgetExceeded { kind: found, .. } if *found == kind)
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_token_writes_until_its_documents_limit_then_is_refused() {
    let db = TestDb::new().await;
    let (alice, project) = project(&db).await;
    let (_, identity) = limited_token(
        &db.app,
        alice,
        project,
        [7; 32],
        &Limits {
            documents_per_day: Some(2),
            ..Limits::default()
        },
    )
    .await;

    for _ in 0..2 {
        let mut tx = as_token(&db.app, identity, project).await;
        write(&mut tx).await.expect("within the budget");
        tx.commit().await.expect("commit");
    }
    // The third write is refused by the reservation itself, so nothing it wrote stands.
    let mut tx = as_token(&db.app, identity, project).await;
    let error = write(&mut tx).await.expect_err("over the budget");
    assert!(refused(&error, Kind::Documents), "{error:?}");
    drop(tx);

    // The ledger and the usage count agree: two settled documents, nothing open.
    let mut tx = as_member(&db.app, alice, project).await;
    let usage = budgets::usage(&mut tx, identity).await.expect("usage");
    assert_eq!(
        (usage.documents_last_day, usage.jobs_open),
        (2, 0),
        "what the token spent"
    );
    let ledger = budgets::of_principal(&mut tx, identity)
        .await
        .expect("reservations");
    assert_eq!(ledger.len(), 2);
    assert!(
        ledger
            .iter()
            .all(|row| row.kind == "documents" && row.state == "settled" && row.settled == Some(1)),
        "every reservation settled with the write: {ledger:?}"
    );
    drop(tx);
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: admit, settle below the estimate, release, refuse"
)]
async fn provider_budgets_admit_settle_release_and_refuse() {
    let db = TestDb::new().await;
    let (alice, project) = project(&db).await;
    let (_, identity) = limited_token(
        &db.app,
        alice,
        project,
        [11; 32],
        &Limits {
            tokens_per_day: Some(1_000),
            cost_per_day_micros: Some(500_000),
            ..Limits::default()
        },
    )
    .await;

    // Under the tokens ceiling: 400 reserved and held; the day count is the allowance.
    let mut tx = as_token(&db.app, identity, project).await;
    let first = budgets::reserve(&mut tx, Kind::Tokens, 400)
        .await
        .expect("under the ceiling")
        .expect("a held row's id");
    tx.commit().await.expect("commit");
    let mut tx = as_member(&db.app, alice, project).await;
    let usage = budgets::usage(&mut tx, identity).await.expect("usage");
    assert_eq!(usage.tokens_last_day, 400, "the held allowance counts");
    tx.commit().await.expect("commit");

    // The outcome settles what the provider actually billed — less than the allowance — and
    // the day count drops to the truth: 250 spent, 750 of headroom back.
    let mut tx = as_token(&db.app, identity, project).await;
    budgets::settle(&mut tx, Some(first), 250)
        .await
        .expect("settled");
    let second = budgets::reserve(&mut tx, Kind::Tokens, 700)
        .await
        .expect("250 + 700 fits under 1,000")
        .expect("held");
    budgets::settle(&mut tx, Some(second), 700)
        .await
        .expect("settled");
    let error = budgets::reserve(&mut tx, Kind::Tokens, 51)
        .await
        .expect_err("950 spent, 51 over the ceiling");
    assert!(refused(&error, Kind::Tokens), "{error:?}");
    let third = budgets::reserve(&mut tx, Kind::Tokens, 50)
        .await
        .expect("exactly the ceiling fits")
        .expect("held");
    tx.commit().await.expect("commit");

    // A released reservation never happened: its allowance is free again. The cost ceiling
    // refuses what the freed amount would have overflowed, and a settle below the estimate
    // leaves room for the rest.
    let mut tx = as_token(&db.app, identity, project).await;
    let cost = budgets::reserve(&mut tx, Kind::Cost, 300_000)
        .await
        .expect("cost under its ceiling")
        .expect("held");
    budgets::release(&mut tx, cost).await.expect("released");
    let cost = budgets::reserve(&mut tx, Kind::Cost, 500_000)
        .await
        .expect("the released allowance was freed")
        .expect("held");
    budgets::settle(&mut tx, Some(cost), 400_000)
        .await
        .expect("billed less than the estimate");
    let error = budgets::reserve(&mut tx, Kind::Cost, 100_001)
        .await
        .expect_err("400,000 spent, 100,001 over the ceiling");
    assert!(refused(&error, Kind::Cost), "{error:?}");
    budgets::reserve(&mut tx, Kind::Cost, 100_000)
        .await
        .expect("exactly the ceiling fits");
    tx.commit().await.expect("commit");

    // The ledger reads both kinds back, settled at what the outcomes consumed; the tokens
    // reservation still held counts its allowance, not a made-up outcome.
    let mut tx = as_member(&db.app, alice, project).await;
    let ledger = budgets::of_principal(&mut tx, identity)
        .await
        .expect("reservations");
    let by_kind = |kind: &str| -> Vec<_> { ledger.iter().filter(|row| row.kind == kind).collect() };
    assert_eq!(by_kind("tokens").len(), 3, "{ledger:?}");
    assert_eq!(by_kind("cost").len(), 3, "{ledger:?}");
    assert!(
        by_kind("tokens")
            .iter()
            .any(|row| row.id == third && row.state == "held"),
        "the third tokens reservation is still in flight"
    );
    let usage = budgets::usage(&mut tx, identity).await.expect("usage");
    assert_eq!(
        (usage.tokens_last_day, usage.cost_last_day),
        (1_000, 500_000),
        "settled at the truth, held at the allowance, released at nothing: {usage:?}"
    );
    drop(tx);

    // A token whose limits row names only another dimension still holds provider
    // reservations — at any size — and a token with no limits row leaves none at all.
    let (_, documents_only) = limited_token(
        &db.app,
        alice,
        project,
        [12; 32],
        &Limits {
            documents_per_day: Some(5),
            ..Limits::default()
        },
    )
    .await;
    let mut tx = as_token(&db.app, documents_only, project).await;
    budgets::reserve(&mut tx, Kind::Tokens, i64::MAX / 2)
        .await
        .expect("no provider ceiling is unconstrained")
        .expect("a row: a limits row exists, so the ledger still records");
    tx.commit().await.expect("commit");

    let policy = TokenKind::FullAccess
        .policy(Vec::new())
        .expect("full access");
    let mut tx = as_member(&db.app, alice, project).await;
    let token = NewToken {
        id: Uuid::now_v7(),
        identity_id: Uuid::now_v7(),
        name: "No ceilings",
        description: "",
        kind: TokenKind::FullAccess,
        policy: &policy,
        hash: [13; 32],
        expires_at: OffsetDateTime::now_utc() + time::Duration::days(30),
        limits: None,
    };
    let Created::Done(stored) = tokens::create(&mut tx, &token).await.expect("create") else {
        panic!("a token with room was refused");
    };
    tx.commit().await.expect("commit");
    let mut tx = as_token(&db.app, stored.identity_id, project).await;
    for kind in [Kind::Tokens, Kind::Cost] {
        assert!(
            matches!(
                budgets::reserve(&mut tx, kind, i64::MAX / 2).await,
                Ok(None)
            ),
            "no limits row: {kind} is unbounded and leaves no row"
        );
    }
    tx.commit().await.expect("commit");
    let mut tx = as_member(&db.app, alice, project).await;
    assert!(
        budgets::of_principal(&mut tx, stored.identity_id)
            .await
            .expect("reservations")
            .is_empty(),
        "an unconstrained token wrote ledger rows"
    );
    drop(tx);
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn two_racing_writes_of_one_token_cant_both_fit() {
    let db = TestDb::new().await;
    let (alice, project) = project(&db).await;
    let (_, identity) = limited_token(
        &db.app,
        alice,
        project,
        [9; 32],
        &Limits {
            documents_per_day: Some(1),
            ..Limits::default()
        },
    )
    .await;
    let pool = db.app_pool(2).await;
    let write_once = || async {
        let mut tx = as_token(&pool, identity, project).await;
        let written = write(&mut tx).await;
        if written.is_ok() {
            tx.commit().await.expect("commit");
        }
        written
    };
    let results = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(write_once(), write_once())
    })
    .await
    .expect("the limits lock can't deadlock");
    let landed = [&results.0, &results.1]
        .into_iter()
        .filter(|r| r.is_ok())
        .count();
    let errors: Vec<&DbError> = [&results.0, &results.1]
        .into_iter()
        .filter_map(|result| result.as_ref().err())
        .collect();
    assert_eq!(
        (landed, errors.len()),
        (1, 1),
        "exactly one of the racing writes fit: {results:?}"
    );
    assert!(refused(errors[0], Kind::Documents), "{:?}", errors[0]);
    pool.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_member_or_an_unconstrained_token_leaves_no_rows() {
    let db = TestDb::new().await;
    let (alice, project) = project(&db).await;
    let policy = TokenKind::FullAccess
        .policy(Vec::new())
        .expect("full access");
    let mut tx = as_member(&db.app, alice, project).await;
    let token = NewToken {
        id: Uuid::now_v7(),
        identity_id: Uuid::now_v7(),
        name: "Wide open",
        description: "",
        kind: TokenKind::FullAccess,
        policy: &policy,
        hash: [4; 32],
        expires_at: OffsetDateTime::now_utc() + time::Duration::days(30),
        limits: None,
    };
    let Created::Done(stored) = tokens::create(&mut tx, &token).await.expect("create") else {
        panic!("a project's first token was refused");
    };
    assert_eq!(stored.limits, None);
    tx.commit().await.expect("commit");

    // Both write past what would fit any small limit; neither leaves a reservation.
    for _ in 0..3 {
        let mut tx = as_token(&db.app, stored.identity_id, project).await;
        write(&mut tx).await.expect("unconstrained token");
        tx.commit().await.expect("commit");
        let mut tx = enter(&db.app, alice, project).await;
        write(&mut tx).await.expect("a member");
        tx.commit().await.expect("commit");
    }
    let mut tx = as_member(&db.app, alice, project).await;
    for principal in [alice, stored.identity_id] {
        assert!(
            budgets::of_principal(&mut tx, principal)
                .await
                .expect("reservations")
                .is_empty(),
            "an unconstrained principal wrote ledger rows"
        );
        let usage = budgets::usage(&mut tx, principal).await.expect("usage");
        assert_eq!(usage.documents_last_day, 0);
    }
    drop(tx);
    db.drop().await;
}

/// An approved changeset of one new article in `project`, made by alice; its id, hash and
/// approving review. The release's commit happens in the caller's transaction so the caller's
/// identity is the jobs' principal.
async fn approved(db: &TestDb, alice: Uuid, project: Uuid) -> (Uuid, [u8; 32], Uuid) {
    let mut tx = enter(&db.app, alice, project).await;
    let entry = write(&mut tx).await.expect("entry");
    let pins = changesets::pins(&mut tx, &[entry]).await.expect("pins");
    let changeset = Uuid::now_v7();
    let hash = [2; 32];
    changesets::create(&mut tx, changeset, "One", &pins, &hash, None)
        .await
        .expect("changeset");
    changesets::submit(&mut tx, changeset)
        .await
        .expect("submitted");
    changesets::lock(&mut tx, changeset).await.expect("lock");
    changesets::review(&mut tx, changeset, &hash, "approve", "", None)
        .await
        .expect("approved");
    let review = changesets::reviews(&mut tx, changeset, None)
        .await
        .expect("reviews")[0]
        .id;
    tx.commit().await.expect("commit");
    (changeset, hash, review)
}

/// `count` webhooks in `project`.
async fn hooks(db: &TestDb, alice: Uuid, project: Uuid, count: usize) {
    let mut tx = enter(&db.app, alice, project).await;
    for n in 0..count {
        destinations::create(
            &mut tx,
            "webhook",
            &format!("hook {n}"),
            &format!("https://hooks.example.com/{n}"),
            None,
        )
        .await
        .expect("destination");
    }
    tx.commit().await.expect("commit");
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_release_reserves_a_slot_per_job_and_the_outcome_closes_it() {
    let db = TestDb::new().await;
    let (alice, project) = project(&db).await;
    hooks(&db, alice, project, 3).await;
    let (_, identity) = limited_token(
        &db.app,
        alice,
        project,
        [6; 32],
        &Limits {
            max_concurrent_jobs: Some(2),
            ..Limits::default()
        },
    )
    .await;

    // A release whose dispatch would take three slots is refused entirely: it commits nothing.
    let (changeset, hash, review) = approved(&db, alice, project).await;
    let mut tx = as_token(&db.app, identity, project).await;
    let error = releases::commit(&mut tx, 0, changeset, &hash, review, HOUR)
        .await
        .expect_err("three destinations, two slots");
    assert!(refused(&error, Kind::Jobs), "{error:?}");
    drop(tx);
    let mut tx = as_member(&db.app, alice, project).await;
    assert!(
        budgets::of_principal(&mut tx, identity)
            .await
            .expect("reservations")
            .is_empty(),
        "a refused dispatch reserved nothing"
    );
    drop(tx);

    // One destination disabled, the dispatch fits its two slots.
    let mut tx = enter(&db.app, alice, project).await;
    let listed = destinations::list(&mut tx).await.expect("destinations");
    destinations::set_enabled(&mut tx, listed[0].id, false)
        .await
        .expect("disabled");
    tx.commit().await.expect("commit");

    // With two free slots the dispatch is reserved before it commits (AU3): one `held` row
    // per job, named by it.
    let mut tx = as_token(&db.app, identity, project).await;
    releases::commit(&mut tx, 0, changeset, &hash, review, HOUR)
        .await
        .expect("released");
    tx.commit().await.expect("commit");
    let mut tx = as_member(&db.app, alice, project).await;
    let ledger = budgets::of_principal(&mut tx, identity)
        .await
        .expect("reservations");
    assert_eq!(ledger.len(), 2, "one reservation per destination");
    assert!(
        ledger
            .iter()
            .all(|row| row.kind == "jobs" && row.state == "held" && row.job_id.is_some()),
        "{ledger:?}"
    );
    let usage = budgets::usage(&mut tx, identity).await.expect("usage");
    assert_eq!(usage.jobs_open, 2);
    drop(tx);

    // A release while both slots are held by running jobs is refused — the worker claims
    // them first, since a pending job would only be superseded, which frees its slot.
    let claimed = jobs::claim(&db.app, "worker-a", "local", Duration::from_secs(30), 10)
        .await
        .expect("claim");
    assert_eq!(claimed.len(), 2);
    let (changeset, hash, review) = approved(&db, alice, project).await;
    let mut tx = as_token(&db.app, identity, project).await;
    let error = releases::commit(&mut tx, 1, changeset, &hash, review, HOUR)
        .await
        .expect_err("both slots held");
    assert!(refused(&error, Kind::Jobs), "{error:?}");
    drop(tx);

    // …until the jobs' outcomes land: each `finish` settles its reservation in the same commit.
    for job in &claimed {
        let done = Finish {
            status: "succeeded",
            next_attempt_at: None,
            http_status: Some(200),
            error: None,
            observation: None,
        };
        assert!(
            jobs::finish(&db.app, job, "worker-a", &done)
                .await
                .expect("finish")
        );
    }
    let mut tx = as_member(&db.app, alice, project).await;
    let ledger = budgets::of_principal(&mut tx, identity)
        .await
        .expect("reservations");
    assert!(
        ledger.iter().all(|row| row.state == "settled"),
        "both slots settled: {ledger:?}"
    );
    let usage = budgets::usage(&mut tx, identity).await.expect("usage");
    assert_eq!(usage.jobs_open, 0);
    drop(tx);

    // The slots are free: the next release dispatches again.
    let mut tx = as_token(&db.app, identity, project).await;
    releases::commit(&mut tx, 1, changeset, &hash, review, HOUR)
        .await
        .expect("released again");
    tx.commit().await.expect("commit");
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn superseding_a_release_frees_its_jobs_slots() {
    let db = TestDb::new().await;
    let (alice, project) = project(&db).await;
    hooks(&db, alice, project, 1).await;
    let (_, identity) = limited_token(
        &db.app,
        alice,
        project,
        [8; 32],
        &Limits {
            max_concurrent_jobs: Some(1),
            ..Limits::default()
        },
    )
    .await;

    // Release 1 dispatches and its job's slot is held. Release 2 commits before the worker
    // claims it: the superseded job's reservation is released inside the same commit, so the
    // one-slot budget still fits the new dispatch.
    let (changeset, hash, review) = approved(&db, alice, project).await;
    let mut tx = as_token(&db.app, identity, project).await;
    releases::commit(&mut tx, 0, changeset, &hash, review, HOUR)
        .await
        .expect("release 1");
    tx.commit().await.expect("commit");
    let (changeset, hash, review) = approved(&db, alice, project).await;
    let mut tx = as_token(&db.app, identity, project).await;
    releases::commit(&mut tx, 1, changeset, &hash, review, HOUR)
        .await
        .expect("release 2 supersedes release 1's pending job");
    tx.commit().await.expect("commit");

    let mut tx = as_member(&db.app, alice, project).await;
    let ledger = budgets::of_principal(&mut tx, identity)
        .await
        .expect("reservations");
    let states: Vec<&str> = ledger.iter().map(|row| row.state.as_str()).collect();
    assert_eq!(states, ["held", "released"], "newest first: {ledger:?}");
    let usage = budgets::usage(&mut tx, identity).await.expect("usage");
    assert_eq!(usage.jobs_open, 1, "only release 2's job holds a slot");
    drop(tx);
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_reconciler_closes_the_dead_and_flags_the_stuck() {
    let db = TestDb::new().await;
    let (alice, project) = project(&db).await;
    hooks(&db, alice, project, 1).await;
    let (_, identity) = limited_token(
        &db.app,
        alice,
        project,
        [5; 32],
        &Limits {
            max_concurrent_jobs: Some(4),
            ..Limits::default()
        },
    )
    .await;
    let (changeset, hash, review) = approved(&db, alice, project).await;
    let mut tx = as_token(&db.app, identity, project).await;
    releases::commit(&mut tx, 0, changeset, &hash, review, HOUR)
        .await
        .expect("released");
    tx.commit().await.expect("commit");

    // Two anomalies, as the table owner sees them: a job whose outcome is on record but whose
    // reservation was never closed, and a held reservation of no job at all, older than any
    // request could be.
    let owner = db.owner().await;
    let environment: Uuid = sqlx::query_scalar("SELECT id FROM environments WHERE project_id = $1")
        .bind(project)
        .fetch_one(&owner)
        .await
        .expect("environment");
    sqlx::query("UPDATE jobs SET status = 'failed', finished_at = now()")
        .execute(&owner)
        .await
        .expect("an outcome on record");
    let lone = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO budget_reservations
             (project_id, environment_id, id, principal_id, kind, reserved, state, created_at)
         VALUES ($1, $2, $3, $4, 'documents', 1, 'held', now() - interval '1 hour')",
    )
    .bind(project)
    .bind(environment)
    .bind(lone)
    .bind(identity)
    .execute(&owner)
    .await
    .expect("a stale held reservation");

    let done = budgets::reconcile(&db.app).await.expect("reconciled");
    assert_eq!(
        (done.released, done.uncertain),
        (1, 1),
        "the dead job's reservation closed, the orphan flagged"
    );
    // The flagged row is announced, attributed to the system identity, and keeps counting:
    // `uncertain` is not free.
    let mut tx = as_member(&db.app, alice, project).await;
    let usage = budgets::usage(&mut tx, identity).await.expect("usage");
    assert_eq!(
        (usage.jobs_open, usage.documents_last_day),
        (0, 1),
        "the orphan still counts, the closed job doesn't"
    );
    let events = audit::list(&mut tx, None, 10, None, None)
        .await
        .expect("audit");
    let discrepancy = events
        .iter()
        .find(|event| event.action == "budget.discrepancy")
        .expect("a discrepancy was announced");
    assert_eq!(discrepancy.actor_id, budgets::WORKER_IDENTITY);
    assert_eq!(discrepancy.subject, lone.to_string());
    drop(tx);

    // The flagged row is retained, still counted open, until it is resolved — a second sweep
    // finds nothing new to flag.
    let ledger = budgets::of_principal(&mut as_member(&db.app, alice, project).await, identity)
        .await
        .expect("reservations");
    assert!(
        ledger
            .iter()
            .any(|row| row.id == lone && row.state == "uncertain"),
        "{ledger:?}"
    );
    let again = budgets::reconcile(&db.app).await.expect("reconciled");
    assert_eq!((again.released, again.uncertain), (0, 0));
    owner.close().await;
    db.drop().await;
}
