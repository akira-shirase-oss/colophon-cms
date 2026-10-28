//! Delivery jobs against real PostgreSQL (D-046): committing a release creates one job per
//! enabled destination; a claim is a lease that hides the job from other workers until it runs
//! out; a late result from an old lease is refused; two workers never claim the same job; a job
//! past its deadline fails; and the worker's role sees jobs but no drafts. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::time::Duration;

use colophon_db::{
    PgPool, changesets, destinations,
    entries::{self, NewRevision},
    jobs::{self, Finish},
    projects, releases,
    scope::{self, ProjectTransaction},
};
use common::TestDb;
use uuid::Uuid;

const HOUR: Duration = Duration::from_secs(3600);

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

/// A project with `destinations` webhooks and one committed release; alice and the project.
async fn released_by(db: &TestDb, destination_count: usize) -> (Uuid, Uuid) {
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    for n in 0..destination_count {
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
    release(db, alice, project, 0).await;
    (alice, project)
}

/// A project with `destinations` webhooks and one committed release; the project.
async fn released(db: &TestDb, destination_count: usize) -> Uuid {
    released_by(db, destination_count).await.1
}

/// Commits one more release, of a new article, in `project` on `base`.
async fn release(db: &TestDb, alice: Uuid, project: Uuid, base: i64) {
    let mut tx = enter(&db.app, alice, project).await;
    let entry = Uuid::now_v7();
    entries::create(
        &mut tx,
        entry,
        "articles",
        NewRevision {
            id: Uuid::now_v7(),
            content: r#"{"title":"Hi"}"#,
            content_hash: &[1; 32],
            title: "Hi",
            slug: None,
            schema_version: 1,
            references: &[],
        },
    )
    .await
    .expect("entry");
    let pins = changesets::pins(&mut tx, &[entry]).await.expect("pins");
    let changeset = Uuid::now_v7();
    changesets::create(&mut tx, changeset, "One", &pins, &[2; 32])
        .await
        .expect("changeset");
    changesets::submit(&mut tx, changeset)
        .await
        .expect("submitted");
    changesets::lock(&mut tx, changeset).await.expect("lock");
    changesets::review(&mut tx, changeset, &[2; 32], "approve", "")
        .await
        .expect("approved");
    let review = changesets::reviews(&mut tx, changeset, None)
        .await
        .expect("reviews")[0]
        .id;
    releases::commit(&mut tx, base, changeset, &[2; 32], review, HOUR)
        .await
        .expect("released");
    tx.commit().await.expect("commit");
}

fn succeeded() -> Finish<'static> {
    Finish {
        status: "succeeded",
        next_attempt_at: None,
        http_status: Some(200),
        error: None,
        observation: None,
    }
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_lease_hides_a_job_until_it_runs_out_and_a_late_result_is_refused() {
    let db = TestDb::new().await;
    released(&db, 1).await;
    let first = jobs::claim(&db.app, "worker-a", "local", Duration::from_millis(300), 10)
        .await
        .expect("claim");
    assert_eq!(first.len(), 1, "one destination, one job");
    assert_eq!((first[0].attempts, first[0].lease_generation), (1, 1));
    assert!(first[0].idempotency_key.starts_with("colophon:"));
    assert!(
        jobs::claim(&db.app, "worker-b", "local", Duration::from_millis(300), 10)
            .await
            .expect("claim")
            .is_empty(),
        "leased: nobody else may take it"
    );

    tokio::time::sleep(Duration::from_millis(400)).await;
    let again = jobs::claim(&db.app, "worker-b", "local", Duration::from_secs(30), 10)
        .await
        .expect("claim");
    assert_eq!(
        (again.len(), again[0].attempts, again[0].lease_generation),
        (1, 2, 2),
        "the lease ran out: claimed again"
    );
    assert!(
        !jobs::finish(&db.app, &first[0], "worker-a", &succeeded())
            .await
            .expect("finish"),
        "worker-a's lease is gone: its late result is refused"
    );
    assert!(
        jobs::finish(&db.app, &again[0], "worker-b", &succeeded())
            .await
            .expect("finish")
    );
    assert!(
        jobs::claim(&db.app, "worker-c", "local", Duration::from_secs(30), 10)
            .await
            .expect("claim")
            .is_empty(),
        "succeeded jobs are never claimed"
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn two_workers_never_claim_the_same_job() {
    let db = TestDb::new().await;
    released(&db, 4).await;
    let pool = db.app_pool(4).await;
    let (a, b) = tokio::join!(
        jobs::claim(&pool, "worker-a", "local", Duration::from_secs(30), 2),
        jobs::claim(&pool, "worker-b", "local", Duration::from_secs(30), 2)
    );
    let mut ids: Vec<Uuid> = a
        .expect("a")
        .into_iter()
        .chain(b.expect("b"))
        .map(|job| job.id)
        .collect();
    let claimed = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(
        (claimed, ids.len()),
        (4, 4),
        "four jobs, each claimed exactly once"
    );
    pool.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_job_past_its_deadline_fails_and_the_worker_role_sees_no_drafts() {
    let db = TestDb::new().await;
    released(&db, 1).await;
    let owner = db.owner().await;
    sqlx::query("UPDATE jobs SET deadline_at = now() - interval '1 second', created_at = now() - interval '2 hours'")
        .execute(&owner)
        .await
        .expect("aged");
    assert!(
        jobs::claim(&db.app, "worker-a", "local", Duration::from_secs(30), 10)
            .await
            .expect("claim")
            .is_empty(),
        "out of its lifetime: not claimed"
    );
    assert_eq!(jobs::expire(&db.app).await.expect("expire"), 1);
    let (status, error): (String, Option<String>) =
        sqlx::query_as("SELECT status, last_error FROM jobs")
            .fetch_one(&owner)
            .await
            .expect("job");
    assert_eq!(
        (status.as_str(), error.as_deref()),
        ("failed", Some("ran out of time"))
    );
    owner.close().await;

    for statement in [
        "SELECT count(*) FROM revisions",
        "SELECT count(*) FROM entries",
        "SELECT count(*) FROM memberships",
        "SELECT count(*) FROM sessions",
        "SELECT content FROM publications",
        "INSERT INTO destinations (project_id, environment_id, id, kind, name, url, created_by) SELECT project_id, environment_id, gen_random_uuid(), 'webhook', 'x', 'https://x.example', created_by FROM destinations",
    ] {
        let mut tx = db.app.begin().await.expect("begin");
        sqlx::query("SET LOCAL ROLE colophon_worker")
            .execute(&mut *tx)
            .await
            .expect("worker role");
        let error = sqlx::query(statement)
            .execute(&mut *tx)
            .await
            .expect_err(statement);
        assert!(
            format!("{error:?}").contains("permission denied"),
            "{statement}: {error:?}"
        );
    }
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn one_announcement_is_in_flight_per_destination() {
    let db = TestDb::new().await;
    let (alice, project) = released_by(&db, 1).await;
    let first = jobs::claim(&db.app, "worker-a", "local", Duration::from_millis(400), 10)
        .await
        .expect("claim");
    assert_eq!((first.len(), first[0].generation), (1, 1));

    // Release 2 commits while release 1 is still being sent: its announcement waits.
    release(&db, alice, project, 1).await;
    assert!(
        jobs::claim(&db.app, "worker-b", "local", Duration::from_secs(30), 10)
            .await
            .expect("claim")
            .is_empty(),
        "the destination has an announcement in flight under a live lease"
    );

    // Once the lease runs out both are claimable; the older one is never sent after the newer.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let both = jobs::claim(&db.app, "worker-b", "local", Duration::from_secs(30), 10)
        .await
        .expect("claim");
    let older = both
        .iter()
        .find(|job| job.generation == 1)
        .expect("release 1 again");
    let newer = both
        .iter()
        .find(|job| job.generation == 2)
        .expect("release 2");
    assert_eq!(
        jobs::prepare(&db.app, older).await.expect("prepare"),
        None,
        "superseded by release 2"
    );
    assert!(
        jobs::prepare(&db.app, newer)
            .await
            .expect("prepare")
            .is_some()
    );
    db.drop().await;
}
