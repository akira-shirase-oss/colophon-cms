//! Scheduled publishing against real PostgreSQL (D-060, case X07): a schedule publishes once, as
//! one release, when its time comes and not before; a late start is labelled delayed; an edit, an
//! expired approval, or a scheduler who may no longer publish blocks it with the reason and
//! publishes nothing; a cancelled schedule never runs; and two workers running the same schedule
//! at once publish it once. The clock is passed in, as the spec asks: nothing waits for real
//! time. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    PgPool, changesets,
    entries::{self, NewRevision},
    members::{self, Accepted, Change},
    projects, releases,
    schedules::{self, Cancelled, Created, Due, NewSchedule, Ran},
    scope::{self, ProjectTransaction},
};
use common::TestDb;
use sqlx::types::time::OffsetDateTime;
use time::Duration;
use uuid::Uuid;

const JOBS: std::time::Duration = std::time::Duration::from_secs(3600);

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

struct Ready {
    alice: Uuid,
    project: Uuid,
    entry: Uuid,
    revision: Uuid,
    changeset: Uuid,
    review: Uuid,
    approved_at: OffsetDateTime,
}

/// Alice's project with one article pinned into a changeset she approved.
async fn ready(db: &TestDb) -> Ready {
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    let (entry, revision) = (Uuid::now_v7(), Uuid::now_v7());
    entries::create(
        &mut tx,
        entry,
        "articles",
        NewRevision {
            id: revision,
            content: r#"{"title":"Hello"}"#,
            content_hash: &[3; 32],
            title: "Hello",
            slug: None,
            schema_version: 1,
        },
    )
    .await
    .expect("created");
    let pins = changesets::pins(&mut tx, &[entry]).await.expect("pins");
    let changeset = Uuid::now_v7();
    changesets::create(&mut tx, changeset, "Spring menu", &pins, &[5; 32])
        .await
        .expect("created");
    changesets::submit(&mut tx, changeset)
        .await
        .expect("submitted");
    changesets::lock(&mut tx, changeset).await.expect("lock");
    changesets::review(&mut tx, changeset, &[5; 32], "approve", "")
        .await
        .expect("approved");
    let approval = changesets::reviews(&mut tx, changeset, None)
        .await
        .expect("reviews")
        .remove(0);
    tx.commit().await.expect("commit");
    Ready {
        alice,
        project,
        entry,
        revision,
        changeset,
        review: approval.id,
        approved_at: approval.created_at,
    }
}

/// Schedules `ready`'s changeset as `who`, for an hour after its approval.
async fn schedule(db: &TestDb, ready: &Ready, who: Uuid) -> (Uuid, OffsetDateTime) {
    let (id, run_at) = (Uuid::now_v7(), ready.approved_at + Duration::hours(1));
    let mut tx = enter(&db.app, who, ready.project).await;
    let created = schedules::create(
        &mut tx,
        &NewSchedule {
            id,
            changeset: ready.changeset,
            hash: &[5; 32],
            approval: ready.review,
            local_time: "2026-11-03T09:00:00",
            time_zone: "Europe/London",
            choice: None,
            run_at,
        },
    )
    .await
    .expect("create");
    assert!(matches!(created, Created::Done(_)));
    tx.commit().await.expect("commit");
    (id, run_at)
}

async fn generation(db: &TestDb, who: Uuid, project: Uuid) -> i64 {
    let mut tx = enter(&db.app, who, project).await;
    releases::generation(&mut tx).await.expect("generation").1
}

async fn the_one_due(db: &TestDb, at: OffsetDateTime) -> Due {
    let due = schedules::due(&db.app, at, 10).await.expect("due");
    assert_eq!(due.len(), 1, "exactly one schedule is due at {at}");
    due[0]
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_schedule_publishes_once_when_its_time_comes() {
    let db = TestDb::new().await;
    let ready = ready(&db).await;
    let (id, run_at) = schedule(&db, &ready, ready.alice).await;

    // Only one pending schedule per changeset.
    let mut tx = enter(&db.app, ready.alice, ready.project).await;
    let again = schedules::create(
        &mut tx,
        &NewSchedule {
            id: Uuid::now_v7(),
            changeset: ready.changeset,
            hash: &[5; 32],
            approval: ready.review,
            local_time: "2026-11-03T10:00:00",
            time_zone: "Europe/London",
            choice: None,
            run_at: run_at + Duration::hours(1),
        },
    )
    .await
    .expect("create");
    assert_eq!(again, Created::AlreadyScheduled);
    drop(tx);

    assert_eq!(
        schedules::due(&db.app, run_at - Duration::seconds(1), 10)
            .await
            .expect("due"),
        Vec::new(),
        "not before its time"
    );
    let due = the_one_due(&db, run_at).await;
    assert_eq!(
        schedules::run(&db.app, &due, run_at + Duration::seconds(5), JOBS)
            .await
            .expect("run"),
        Ran::Published {
            generation: 1,
            delayed: false
        }
    );
    assert_eq!(
        schedules::run(&db.app, &due, run_at + Duration::seconds(6), JOBS)
            .await
            .expect("run"),
        Ran::Gone,
        "a schedule runs once"
    );
    assert_eq!(
        schedules::due(&db.app, run_at, 10).await.expect("due"),
        Vec::new()
    );
    let mut tx = enter(&db.app, ready.alice, ready.project).await;
    let ran = schedules::get(&mut tx, id)
        .await
        .expect("get")
        .expect("kept");
    assert_eq!((ran.state.as_str(), ran.generation), ("published", Some(1)));
    assert!(!ran.delayed());
    assert_eq!(ran.title, "Spring menu");
    let release = releases::of_changeset(&mut tx, ready.changeset)
        .await
        .expect("release");
    assert!(release.is_some(), "the release is the changeset's");
    assert!(matches!(
        schedules::cancel(&mut tx, id).await.expect("cancel"),
        Cancelled::Finished(state) if state == "published"
    ));
    drop(tx);
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_late_start_is_labelled_delayed_and_a_cancelled_schedule_never_runs() {
    let db = TestDb::new().await;
    let ready = ready(&db).await;
    let (id, run_at) = schedule(&db, &ready, ready.alice).await;

    // Cancelled before its time: it never runs.
    let mut tx = enter(&db.app, ready.alice, ready.project).await;
    let Cancelled::Done(cancelled) = schedules::cancel(&mut tx, id).await.expect("cancel") else {
        panic!("cancelled")
    };
    assert_eq!(cancelled.state, "cancelled");
    tx.commit().await.expect("commit");
    assert_eq!(
        schedules::due(&db.app, run_at + Duration::hours(1), 10)
            .await
            .expect("due"),
        Vec::new()
    );

    // Scheduled again, and run two minutes late (the platform was down): delayed, not on time.
    let (id, run_at) = schedule(&db, &ready, ready.alice).await;
    let due = the_one_due(&db, run_at + Duration::minutes(2)).await;
    assert_eq!(
        schedules::run(&db.app, &due, run_at + Duration::minutes(2), JOBS)
            .await
            .expect("run"),
        Ran::Published {
            generation: 1,
            delayed: true
        }
    );
    let mut tx = enter(&db.app, ready.alice, ready.project).await;
    assert!(
        schedules::get(&mut tx, id)
            .await
            .expect("get")
            .expect("kept")
            .delayed()
    );
    drop(tx);
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn an_edit_or_an_expired_approval_blocks_it_and_nothing_is_published() {
    let db = TestDb::new().await;
    let ready = ready(&db).await;
    let (id, run_at) = schedule(&db, &ready, ready.alice).await;

    // The approval lasts seven days: run eight days on, it has expired.
    let late = ready.approved_at + Duration::days(8);
    let due = the_one_due(&db, late).await;
    let Ran::Blocked { reason } = schedules::run(&db.app, &due, late, JOBS)
        .await
        .expect("run")
    else {
        panic!("blocked")
    };
    assert!(reason.contains("approval expired"), "{reason}");
    assert_eq!(generation(&db, ready.alice, ready.project).await, 0);

    // Scheduled again, then the article is edited: the edit blocks it, rather than publishing
    // something nobody approved or silently publishing the old version.
    let mut tx = enter(&db.app, ready.alice, ready.project).await;
    let Cancelled::Finished(state) = schedules::cancel(&mut tx, id).await.expect("cancel") else {
        panic!("already finished")
    };
    assert_eq!(state, "blocked");
    drop(tx);
    let (_, run_at_again) = schedule(&db, &ready, ready.alice).await;
    assert_eq!(run_at_again, run_at);
    let mut tx = enter(&db.app, ready.alice, ready.project).await;
    entries::save(
        &mut tx,
        ready.entry,
        ready.revision,
        NewRevision {
            id: Uuid::now_v7(),
            content: r#"{"title":"Hello again"}"#,
            content_hash: &[4; 32],
            title: "Hello again",
            slug: None,
            schema_version: 1,
        },
    )
    .await
    .expect("saved");
    tx.commit().await.expect("commit");
    let due = the_one_due(&db, run_at).await;
    let Ran::Blocked { reason } = schedules::run(&db.app, &due, run_at, JOBS)
        .await
        .expect("run")
    else {
        panic!("blocked")
    };
    assert!(reason.contains("edited after it was approved"), "{reason}");
    assert_eq!(generation(&db, ready.alice, ready.project).await, 0);
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn it_runs_as_its_scheduler_whose_role_as_it_is_then_decides() {
    let db = TestDb::new().await;
    let ready = ready(&db).await;
    // Bob joins as a publisher and schedules it.
    let bob = db.person("bob").await;
    let mut tx = scope::begin_as_member(&db.app, ready.alice, ready.project)
        .await
        .expect("begin")
        .expect("a member");
    members::offering(&mut tx, "publisher")
        .await
        .expect("offer");
    members::invite(
        &mut tx,
        &[8; 32],
        "publisher",
        "Bob",
        OffsetDateTime::now_utc() + Duration::hours(1),
    )
    .await
    .expect("invited");
    tx.commit().await.expect("commit");
    assert!(matches!(
        members::accept(&db.app, bob, &[8; 32])
            .await
            .expect("accept"),
        Accepted::Joined { .. }
    ));
    let (_, run_at) = schedule(&db, &ready, bob).await;

    // Before it runs, Bob becomes an editor, who can't publish: it is blocked, as him.
    let mut tx = scope::begin_as_member(&db.app, ready.alice, ready.project)
        .await
        .expect("begin")
        .expect("a member");
    assert_eq!(
        members::change_role(&mut tx, bob, "editor")
            .await
            .expect("change"),
        Change::Done
    );
    tx.commit().await.expect("commit");
    let due = the_one_due(&db, run_at).await;
    assert_eq!(due.scheduled_by, bob);
    let Ran::Blocked { reason } = schedules::run(&db.app, &due, run_at, JOBS)
        .await
        .expect("run")
    else {
        panic!("blocked")
    };
    assert!(reason.contains("may no longer publish"), "{reason}");

    // Scheduled by him again (as a publisher once more), then he leaves the project.
    let mut tx = scope::begin_as_member(&db.app, ready.alice, ready.project)
        .await
        .expect("begin")
        .expect("a member");
    members::change_role(&mut tx, bob, "publisher")
        .await
        .expect("change");
    tx.commit().await.expect("commit");
    schedule(&db, &ready, bob).await;
    let mut tx = scope::begin_as_member(&db.app, bob, ready.project)
        .await
        .expect("begin")
        .expect("a member");
    assert_eq!(
        members::remove(&mut tx, bob).await.expect("leave"),
        Change::Done
    );
    tx.commit().await.expect("commit");
    let due = the_one_due(&db, run_at).await;
    let Ran::Blocked { reason } = schedules::run(&db.app, &due, run_at, JOBS)
        .await
        .expect("run")
    else {
        panic!("blocked")
    };
    assert!(reason.contains("no longer a member"), "{reason}");
    assert_eq!(generation(&db, ready.alice, ready.project).await, 0);
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn two_workers_running_one_schedule_at_once_publish_it_once() {
    let db = TestDb::new().await;
    let ready = ready(&db).await;
    let (_, run_at) = schedule(&db, &ready, ready.alice).await;
    let due = the_one_due(&db, run_at).await;
    let (one, other) = (db.app_pool(2).await, db.app_pool(2).await);
    let (first, second) = tokio::join!(
        schedules::run(&one, &due, run_at, JOBS),
        schedules::run(&other, &due, run_at, JOBS)
    );
    let mut outcomes = [first.expect("run"), second.expect("run")];
    outcomes.sort_by_key(|ran| matches!(ran, Ran::Gone));
    assert_eq!(
        outcomes,
        [
            Ran::Published {
                generation: 1,
                delayed: false
            },
            Ran::Gone
        ]
    );
    assert_eq!(generation(&db, ready.alice, ready.project).await, 1);
    one.close().await;
    other.close().await;
    db.drop().await;
}
