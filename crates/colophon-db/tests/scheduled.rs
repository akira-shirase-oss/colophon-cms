//! Scheduled publishes against real PostgreSQL, as the application's login: a schedule pins
//! the changeset, its hash, the approval and who asked; claiming runs once and only when due;
//! cancelling is a one-way door; a crashed claim is recorded, not retried. Needs the stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    PgPool, changesets, projects,
    scheduled::{self, Cancelled, ScheduledOutcome},
    scope::{self, ProjectTransaction},
};
use common::TestDb;
use sqlx::types::time::OffsetDateTime;
use uuid::Uuid;

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

/// A submitted, approved changeset in the transaction's environment: id, hash, review id.
async fn approved(tx: &mut ProjectTransaction, title: &str) -> (Uuid, [u8; 32], Uuid) {
    let id = Uuid::now_v7();
    let hash = [9_u8; 32];
    changesets::create(tx, id, title, &[], &hash)
        .await
        .expect("created");
    changesets::submit(tx, id).await.expect("submitted");
    changesets::review(tx, id, &hash, "approve", "looks right")
        .await
        .expect("approved");
    let reviews = changesets::reviews(tx, id, Some(&hash))
        .await
        .expect("reviews");
    (id, hash, reviews.first().expect("the approval").id)
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_schedule_claims_once_runs_once_and_ends_in_evidence() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "newsroom", "Newsroom")
        .await
        .expect("project")
        .id;

    // A schedule for a moment ahead: not due yet, so the claim loop leaves it alone.
    let (set, hash, review) = {
        let mut tx = enter(&db.app, alice, project).await;
        let approved = approved(&mut tx, "Launch").await;
        tx.commit().await.expect("commit");
        approved
    };
    let schedule = {
        let mut tx = enter(&db.app, alice, project).await;
        let row = scheduled::schedule(
            &mut tx,
            Uuid::now_v7(),
            set,
            &hash,
            review,
            0,
            OffsetDateTime::now_utc() + time::Duration::hours(1),
        )
        .await
        .expect("scheduled");
        tx.commit().await.expect("commit");
        row
    };
    assert_eq!(schedule.status, "pending");
    assert!(
        scheduled::claim_due(&db.app, 10)
            .await
            .expect("claim")
            .is_empty(),
        "a schedule that is not due is not claimed"
    );

    // Backdating run_at makes it due; the claim takes it exactly once.
    let owner = db.owner().await;
    sqlx::query("UPDATE scheduled_publishes SET run_at = now() - interval '1 second'")
        .execute(&owner)
        .await
        .expect("backdated");
    let due = scheduled::claim_due(&db.app, 10).await.expect("claim");
    assert_eq!(due.len(), 1);
    assert_eq!(due.first().expect("one").id, schedule.id);
    assert!(
        scheduled::claim_due(&db.app, 10)
            .await
            .expect("reclaim")
            .is_empty(),
        "a claimed schedule is never claimed again"
    );

    // The pins the commit replays under survive the claim.
    let due = due.into_iter().next().expect("one");
    assert_eq!(due.changeset_id, set);
    assert_eq!(due.changeset_hash, hash);
    assert_eq!(due.requested_by, alice);
    assert_eq!(due.base_generation, 0);

    // Finishing records the release it landed as.
    scheduled::finish(&db.app, schedule.id, 7)
        .await
        .expect("finished");
    let mut tx = enter(&db.app, alice, project).await;
    let ended = scheduled::for_changeset(&mut tx, set)
        .await
        .expect("listed");
    tx.commit().await.expect("commit");
    assert_eq!(
        ended.first().expect("one").outcome,
        Some(ScheduledOutcome::Committed(7))
    );
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_schedule_cancels_only_while_pending_and_fails_with_why() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "newsroom", "Newsroom")
        .await
        .expect("project")
        .id;
    let (set, hash, review) = {
        let mut tx = enter(&db.app, alice, project).await;
        let approved = approved(&mut tx, "Launch").await;
        tx.commit().await.expect("commit");
        approved
    };
    let mut tx = enter(&db.app, alice, project).await;
    let schedule = scheduled::schedule(
        &mut tx,
        Uuid::now_v7(),
        set,
        &hash,
        review,
        0,
        OffsetDateTime::now_utc() + time::Duration::hours(1),
    )
    .await
    .expect("scheduled");

    // Cancelling a pending schedule works once; the second time it is too late.
    assert_eq!(
        scheduled::cancel(&mut tx, schedule.id)
            .await
            .expect("cancel"),
        Cancelled::Cancelled
    );
    assert_eq!(
        scheduled::cancel(&mut tx, schedule.id)
            .await
            .expect("again"),
        Cancelled::TooLate
    );
    assert_eq!(
        scheduled::cancel(&mut tx, Uuid::now_v7())
            .await
            .expect("missing"),
        Cancelled::NotFound
    );
    tx.commit().await.expect("commit");

    // A claimed schedule is past cancelling; failing it records the refusal's message.
    let mut tx = enter(&db.app, alice, project).await;
    let second = scheduled::schedule(
        &mut tx,
        Uuid::now_v7(),
        set,
        &hash,
        review,
        0,
        OffsetDateTime::now_utc() + time::Duration::hours(1),
    )
    .await
    .expect("scheduled");
    tx.commit().await.expect("commit");
    let owner = db.owner().await;
    sqlx::query("UPDATE scheduled_publishes SET run_at = now() - interval '1 second'")
        .execute(&owner)
        .await
        .expect("backdated");
    assert_eq!(
        scheduled::claim_due(&db.app, 10)
            .await
            .expect("claim")
            .len(),
        1
    );
    let mut tx = enter(&db.app, alice, project).await;
    assert_eq!(
        scheduled::cancel(&mut tx, second.id).await.expect("cancel"),
        Cancelled::TooLate,
        "a claimed schedule is past cancelling"
    );
    tx.commit().await.expect("commit");
    scheduled::fail(&db.app, second.id, "the environment moved to generation 4")
        .await
        .expect("failed");
    let mut tx = enter(&db.app, alice, project).await;
    let ended = scheduled::for_changeset(&mut tx, set)
        .await
        .expect("listed");
    tx.commit().await.expect("commit");
    assert_eq!(
        ended
            .iter()
            .find(|row| row.id == second.id)
            .expect("the failed one")
            .outcome,
        Some(ScheduledOutcome::Failed(
            "the environment moved to generation 4".to_owned()
        ))
    );
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_stale_claim_is_recorded_and_another_project_sees_nothing() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "newsroom", "Newsroom")
        .await
        .expect("project")
        .id;
    let (set, hash, review) = {
        let mut tx = enter(&db.app, alice, project).await;
        let approved = approved(&mut tx, "Launch").await;
        tx.commit().await.expect("commit");
        approved
    };
    let mut tx = enter(&db.app, alice, project).await;
    let schedule = scheduled::schedule(
        &mut tx,
        Uuid::now_v7(),
        set,
        &hash,
        review,
        0,
        OffsetDateTime::now_utc() - time::Duration::seconds(5),
    )
    .await
    .expect("scheduled");
    tx.commit().await.expect("commit");

    // Claimed, then abandoned as if the server died: expiry records it, never reruns it.
    assert_eq!(
        scheduled::claim_due(&db.app, 10)
            .await
            .expect("claim")
            .len(),
        1
    );
    let owner = db.owner().await;
    sqlx::query(
        "UPDATE scheduled_publishes SET claimed_at = now() - interval '1 hour'
         WHERE status = 'claimed'",
    )
    .execute(&owner)
    .await
    .expect("aged");
    assert_eq!(
        scheduled::expire(&db.app, std::time::Duration::from_secs(300))
            .await
            .expect("expired"),
        1
    );
    assert!(
        scheduled::claim_due(&db.app, 10)
            .await
            .expect("claim")
            .is_empty(),
        "an expired claim never runs"
    );
    let mut tx = enter(&db.app, alice, project).await;
    let ended = scheduled::for_changeset(&mut tx, set)
        .await
        .expect("listed");
    tx.commit().await.expect("commit");
    assert!(matches!(
        ended.first().expect("one").outcome,
        Some(ScheduledOutcome::Failed(_))
    ));

    // A member of a different project can't see the schedule at all.
    let bob = db.administrator("bob").await;
    let other = projects::create(&db.app, bob, "elsewhere", "Elsewhere")
        .await
        .expect("project")
        .id;
    let mut tx = enter(&db.app, bob, other).await;
    assert!(
        scheduled::for_changeset(&mut tx, set)
            .await
            .expect("listed")
            .is_empty(),
        "another project's schedules do not exist to it"
    );
    assert_eq!(
        scheduled::cancel(&mut tx, schedule.id)
            .await
            .expect("cancel"),
        Cancelled::NotFound
    );
    tx.commit().await.expect("commit");
    owner.close().await;
    db.drop().await;
}
