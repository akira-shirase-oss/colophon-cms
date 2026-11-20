//! The per-project work bound under concurrency (D-131; D-216 phase 2's gate). These proofs are
//! written against what can be observed: the admission's answer (admitted, or `WorkFull`) and the
//! true count of a project's open work across the four tables `work::admit` counts. They pass
//! today on WA's serialisation and stay as guards when phase 2 replaces WA's count with a
//! counter row taken at the end. The one proof that needs phase 2's internals (its open-deltas
//! fold) skips until those exist. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    time::{Duration, Instant},
};

use colophon_db::{
    DbError, PgPool, bulk, projects,
    scope::{self, ProjectTransaction},
    work,
};
use common::TestDb;
use serde_json::json;
use uuid::Uuid;

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

/// The project's open work as `work::admit` counts it, read by the table owner: every committed
/// row, whoever wrote it.
async fn open_work(owner: &PgPool, project: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM (
             SELECT 1 FROM jobs WHERE project_id = $1 AND status IN ('pending', 'retrying', 'running', 'held')
             UNION ALL
             SELECT 1 FROM asset_jobs WHERE project_id = $1 AND status IN ('pending', 'running', 'retrying')
             UNION ALL
             SELECT 1 FROM bulk_jobs WHERE project_id = $1 AND state IN ('accepting', 'running')
             UNION ALL
             SELECT 1 FROM migration_plans WHERE project_id = $1 AND state IN ('proposed', 'approved', 'running')
         ) open",
    )
    .bind(project)
    .fetch_one(owner)
    .await
    .expect("count")
}

/// A project of alice's whose open work is `open` pending asset jobs (written directly, foreign
/// keys and triggers off: only the count matters here).
async fn project_with_open(db: &TestDb, slug: &str, open: i64) -> (Uuid, Uuid) {
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, slug, slug)
        .await
        .expect("created")
        .id;
    let owner = db.owner().await;
    let mut conn = owner.acquire().await.expect("connection");
    sqlx::query("SET session_replication_role = replica")
        .execute(&mut *conn)
        .await
        .expect("replica");
    sqlx::query(
        "INSERT INTO asset_jobs (project_id, environment_id, id, asset_id, kind, idempotency_key, deadline_at)
         SELECT e.project_id, e.id, gen_random_uuid(), gen_random_uuid(), 'verify',
                'fill:' || e.project_id || ':' || n, now() + interval '1 hour'
         FROM environments e, generate_series(1, $2) n
         WHERE e.project_id = $1 AND e.slug = 'production'",
    )
    .bind(project)
    .bind(open)
    .execute(&mut *conn)
    .await
    .expect("fill");
    sqlx::query("RESET session_replication_role")
        .execute(&mut *conn)
        .await
        .expect("reset");
    drop(conn);
    owner.close().await;
    (alice, project)
}

/// One admission through the real path: a bulk job (one open row); `Ok(true)` admitted and
/// committed, `Ok(false)` refused `WorkFull`.
async fn admit_one(pool: &PgPool, alice: Uuid, project: Uuid) -> Result<bool, String> {
    admit_after(pool, alice, project, None).await
}

/// [`admit_one`], with every racer's transaction open before any of them is admitted: they wait
/// at `start` together, so their counts really do run at the same moment.
async fn admit_after(
    pool: &PgPool,
    alice: Uuid,
    project: Uuid,
    start: Option<&tokio::sync::Barrier>,
) -> Result<bool, String> {
    let mut tx = enter(pool, alice, project).await;
    if let Some(start) = start {
        start.wait().await;
    }
    match bulk::create(
        &mut tx,
        &bulk::NewJob {
            id: Uuid::now_v7(),
            kind: "entries",
            operation: json!({"collection": "articles", "set": {"title": "x"}}),
            manifest_hash: [3; 32],
            principal: "member",
            expected_items: 0,
            seal: false,
        },
    )
    .await
    {
        Ok(_) => {
            tx.commit().await.map_err(|error| format!("{error:?}"))?;
            Ok(true)
        }
        Err(DbError::WorkFull { .. }) => Ok(false),
        Err(other) => Err(format!("{other:?}")),
    }
}

async fn race_at_the_cap(racers: usize) {
    let db = TestDb::new().await;
    let (alice, project) = project_with_open(&db, "alpha", work::MAX_OPEN - 1).await;
    let owner = db.owner().await;
    assert_eq!(open_work(&owner, project).await, work::MAX_OPEN - 1);

    let pool = Arc::new(db.app_pool(u32::try_from(racers).expect("small") + 2).await);
    let start = Arc::new(tokio::sync::Barrier::new(racers));
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..racers {
        let (pool, start) = (Arc::clone(&pool), Arc::clone(&start));
        set.spawn(async move { admit_after(&pool, alice, project, Some(&start)).await });
    }
    let mut admitted = 0;
    while let Some(result) = set.join_next().await {
        admitted += usize::from(
            result
                .expect("task")
                .expect("admitted or WorkFull, nothing else"),
        );
    }
    assert_eq!(
        admitted, 1,
        "{racers} racers for the last slot: exactly one admitted"
    );
    assert_eq!(open_work(&owner, project).await, work::MAX_OPEN);
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn two_admissions_at_the_last_slot_admit_exactly_one() {
    race_at_the_cap(2).await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn sixteen_admissions_at_the_last_slot_admit_exactly_one() {
    race_at_the_cap(16).await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn an_admission_rolled_back_after_its_writes_leaves_the_slot_free() {
    let db = TestDb::new().await;
    let (alice, project) = project_with_open(&db, "alpha", work::MAX_OPEN - 1).await;
    let owner = db.owner().await;

    // Admitted, written, checked: then the transaction ends without a commit (a crash looks the same).
    let mut tx = enter(&db.app, alice, project).await;
    bulk::create(
        &mut tx,
        &bulk::NewJob {
            id: Uuid::now_v7(),
            kind: "entries",
            operation: json!({"collection": "articles"}),
            manifest_hash: [3; 32],
            principal: "member",
            expected_items: 0,
            seal: false,
        },
    )
    .await
    .expect("admitted");
    tx.transaction.rollback().await.expect("rollback");

    assert_eq!(
        open_work(&owner, project).await,
        work::MAX_OPEN - 1,
        "nothing was kept"
    );
    assert_eq!(
        admit_one(&db.app, alice, project).await,
        Ok(true),
        "the slot the rolled-back admission held is free again"
    );
    assert_eq!(
        admit_one(&db.app, alice, project).await,
        Ok(false),
        "and the bound holds after it"
    );
    owner.close().await;
    db.drop().await;
}

/// Admitters and finishers on one project for a few seconds, with a watcher reading the true
/// count from a fresh snapshot as fast as it can: every snapshot is a committed state, so the
/// bound must hold in each; and when everything has stopped, admissions and finishes account
/// for the count exactly.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up (a heavy proof: run it by name)"]
#[allow(
    clippy::too_many_lines,
    reason = "one run: its admitters, finisher and watcher, then the invariants it held"
)]
async fn admitters_and_finishers_never_pass_the_bound_at_any_commit() {
    const ADMITTERS: usize = 8;
    const FINISHERS: usize = 1;
    const RUN: Duration = Duration::from_secs(8);
    let db = TestDb::new().await;
    let start = work::MAX_OPEN - 20;
    let (alice, project) = project_with_open(&db, "alpha", start).await;
    let owner = Arc::new(db.owner().await);
    let pool = Arc::new(
        db.app_pool(u32::try_from(ADMITTERS + 2).expect("small"))
            .await,
    );
    let stop = Arc::new(AtomicBool::new(false));
    let (admitted, refused, finished, highest) = (
        Arc::new(AtomicI64::new(0)),
        Arc::new(AtomicI64::new(0)),
        Arc::new(AtomicI64::new(0)),
        Arc::new(AtomicI64::new(0)),
    );

    let mut set = tokio::task::JoinSet::new();
    for _ in 0..ADMITTERS {
        let (pool, stop, admitted, refused) = (
            Arc::clone(&pool),
            Arc::clone(&stop),
            Arc::clone(&admitted),
            Arc::clone(&refused),
        );
        set.spawn(async move {
            while !stop.load(Ordering::Relaxed) {
                if admit_one(&pool, alice, project).await? {
                    admitted.fetch_add(1, Ordering::Relaxed);
                } else {
                    refused.fetch_add(1, Ordering::Relaxed);
                }
            }
            Ok::<(), String>(())
        });
    }
    for _ in 0..FINISHERS {
        let (owner, stop, finished) =
            (Arc::clone(&owner), Arc::clone(&stop), Arc::clone(&finished));
        set.spawn(async move {
            while !stop.load(Ordering::Relaxed) {
                // A worker finishing one open job of the project.
                let done = sqlx::query(
                    "UPDATE asset_jobs SET status = 'succeeded', finished_at = now()
                     WHERE ctid = (SELECT ctid FROM asset_jobs
                                   WHERE project_id = $1 AND status = 'pending'
                                   LIMIT 1 FOR UPDATE SKIP LOCKED)",
                )
                .bind(project)
                .execute(&*owner)
                .await
                .map_err(|error| format!("{error:?}"))?
                .rows_affected();
                finished.fetch_add(i64::try_from(done).unwrap_or(0), Ordering::Relaxed);
                // Slower than the admitters, so the project sits at its bound and refusals happen.
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
            Ok(())
        });
    }
    {
        let (owner, stop, highest) = (Arc::clone(&owner), Arc::clone(&stop), Arc::clone(&highest));
        set.spawn(async move {
            while !stop.load(Ordering::Relaxed) {
                let now = open_work(&owner, project).await;
                highest.fetch_max(now, Ordering::Relaxed);
            }
            Ok(())
        });
    }
    let began = Instant::now();
    tokio::time::sleep(RUN).await;
    stop.store(true, Ordering::Relaxed);
    while let Some(result) = set.join_next().await {
        result.expect("task").expect("no error but WorkFull");
    }

    let (admitted, refused, finished, highest) = (
        admitted.load(Ordering::Relaxed),
        refused.load(Ordering::Relaxed),
        finished.load(Ordering::Relaxed),
        highest.load(Ordering::Relaxed),
    );
    eprintln!(
        "{:.1} s: {admitted} admitted, {refused} refused, {finished} finished, highest open {highest} of {}",
        began.elapsed().as_secs_f64(),
        work::MAX_OPEN
    );
    // The run pressed on the bound: it was reached, admissions were refused there, and slots freed
    // by finishes were taken again.
    assert_eq!(highest, work::MAX_OPEN, "the bound was reached");
    assert!(refused > 0, "admissions were refused at the bound");
    assert!(
        admitted > 20,
        "freed slots were taken again ({admitted} admitted)"
    );
    assert!(
        highest <= work::MAX_OPEN,
        "a committed state held more than the bound"
    );
    assert_eq!(
        open_work(&owner, project).await,
        start + admitted - finished,
        "at rest, admissions and finishes account for the count exactly"
    );
    drop(owner);
    db.drop().await;
}

/// Phase 2's fold: workers' open-count deltas, inserted while admissions fold them, are folded
/// exactly once, so at rest the counter row equals the true count. Phase 2 enables it by
/// removing the skip below; until its `work_open` table exists there is nothing to compare.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_open_counter_equals_the_true_count_at_rest() {
    let db = TestDb::new().await;
    let owner = db.owner().await;
    let built: bool = sqlx::query_scalar("SELECT to_regclass('work_open') IS NOT NULL")
        .fetch_one(&owner)
        .await
        .expect("catalog");
    if !built {
        eprintln!(
            "pending: D-216 phase 2's work_open counter isn't built yet; its commit removes this skip"
        );
        owner.close().await;
        db.drop().await;
        return;
    }
    let (alice, project) = project_with_open(&db, "alpha", 100).await;
    let pool = db.app_pool(6).await;
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..4 {
        let pool = pool.clone();
        set.spawn(async move {
            for _ in 0..25 {
                admit_one(&pool, alice, project).await.expect("admit");
            }
        });
    }
    let finisher = owner.clone();
    set.spawn(async move {
        for _ in 0..60 {
            sqlx::query(
                "UPDATE asset_jobs SET status = 'succeeded', finished_at = now()
                 WHERE ctid = (SELECT ctid FROM asset_jobs WHERE project_id = $1 AND status = 'pending'
                               LIMIT 1 FOR UPDATE SKIP LOCKED)",
            )
            .bind(project)
            .execute(&finisher)
            .await
            .expect("finish");
        }
    });
    while let Some(result) = set.join_next().await {
        result.expect("task");
    }
    let counted: i64 = sqlx::query_scalar(
        "SELECT w.open + coalesce((SELECT sum(d.delta) FROM open_deltas d WHERE d.project_id = $1), 0)
         FROM work_open w WHERE w.project_id = $1",
    )
    .bind(project)
    .fetch_one(&owner)
    .await
    .expect("counter");
    assert_eq!(counted, open_work(&owner, project).await);
    pool.close().await;
    owner.close().await;
    db.drop().await;
}
