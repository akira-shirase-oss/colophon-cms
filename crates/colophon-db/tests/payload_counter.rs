//! The installation's pending payload as a running total (D-216, migration 0078): every write
//! path keeps it equal to the SUM it replaces; admissions at the bound never overshoot it, with
//! many projects racing; a planted error is reported, never repaired; a rolled-back admission
//! leaves nothing behind; the worker's fold keeps the log short and never waits on an
//! admission; and workers never wait on admissions at all. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::{sync::Arc, time::Duration};

use colophon_db::{
    DbError, PgPool, assets, bulk, changesets, destinations,
    entries::{self, NewRevision},
    jobs::{self, Finish},
    projects, releases,
    scope::{self, ProjectTransaction},
    work,
};
use common::TestDb;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

const HOUR: Duration = Duration::from_secs(3600);

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

/// The running total equals the SUM it stands for.
async fn exact(db: &TestDb, step: &str) {
    let (counted, actual, drift) = work::reconcile_payload(&db.app).await.expect("reconcile");
    assert_eq!(drift, 0, "{step}: counted {counted}, actual {actual}");
}

/// A project of `alice`'s with `hooks` webhook destinations.
async fn project(db: &TestDb, alice: Uuid, slug: &str, hooks: usize) -> Uuid {
    let project = projects::create(&db.app, alice, slug, slug)
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    for n in 0..hooks {
        destinations::create(
            &mut tx,
            "webhook",
            &format!("hook {n}"),
            &format!("https://hooks.example.com/{slug}/{n}"),
            None,
        )
        .await
        .expect("destination");
    }
    tx.commit().await.expect("commit");
    project
}

/// One draft entry; its id and revision.
async fn entry(tx: &mut ProjectTransaction) -> (Uuid, Uuid) {
    let (entry, revision) = (Uuid::now_v7(), Uuid::now_v7());
    entries::create(
        tx,
        entry,
        "articles",
        NewRevision {
            id: revision,
            content: r#"{"title":"Hi"}"#,
            content_hash: &[1; 32],
            title: "Hi",
            slug: None,
            schema_version: 1,
            references: &[],
            assets: &[],
            unique: &[],
            visibility: None,
        },
        None,
    )
    .await
    .expect("entry");
    (entry, revision)
}

/// Commits a release of a new article on `base`: its outbox supersedes older announcements
/// that haven't started, then admits and enqueues one job per destination.
async fn release(db: &TestDb, alice: Uuid, project: Uuid, base: i64) {
    let mut tx = enter(&db.app, alice, project).await;
    let (entry, _) = entry(&mut tx).await;
    let pins = changesets::pins(&mut tx, &[entry]).await.expect("pins");
    let changeset = Uuid::now_v7();
    changesets::create(&mut tx, changeset, "One", &pins, &[2; 32], None)
        .await
        .expect("changeset");
    changesets::submit(&mut tx, changeset)
        .await
        .expect("submitted");
    changesets::lock(&mut tx, changeset).await.expect("lock");
    changesets::review(&mut tx, changeset, &[2; 32], "approve", "", None)
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

/// An open bulk job carrying `size` bytes of operation, made through the real admission path.
async fn bulk_job(tx: &mut ProjectTransaction, size: usize) -> Result<bulk::Job, DbError> {
    bulk::create(
        tx,
        &bulk::NewJob {
            id: Uuid::now_v7(),
            kind: "entries",
            operation: json!({"collection": "articles", "set": {"title": "x".repeat(size)}}),
            manifest_hash: [3; 32],
            principal: "member",
            expected_items: 0,
            seal: false,
        },
    )
    .await
}

/// An error, as the stress test collects it.
fn debug<E: std::fmt::Debug>(error: E) -> String {
    format!("{error:?}")
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
#[allow(
    clippy::too_many_lines,
    reason = "one walk through every write path, checked after each step"
)]
async fn every_write_path_keeps_the_running_total_exact() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = project(&db, alice, "alpha", 2).await;
    let owner = db.owner().await;
    exact(&db, "a new installation").await;

    // Delivery jobs: a release enqueues them (admission), the next supersedes the ones that
    // haven't started, a worker claims and finishes one, the deadline sweep fails the rest.
    release(&db, alice, project, 0).await;
    exact(&db, "a release's outbox").await;
    release(&db, alice, project, 1).await;
    exact(&db, "a release superseding the last").await;
    let claimed = jobs::claim(&db.app, "worker-a", "local", HOUR, 1)
        .await
        .expect("claim");
    assert_eq!(claimed.len(), 1);
    exact(&db, "a claim").await;
    assert!(
        jobs::finish(&db.app, &claimed[0], "worker-a", &succeeded())
            .await
            .expect("finish")
    );
    exact(&db, "a finish").await;
    sqlx::query(
        "UPDATE jobs SET deadline_at = now() - interval '1 second' WHERE status = 'pending'",
    )
    .execute(&owner)
    .await
    .expect("past their deadline");
    assert!(jobs::expire(&db.app).await.expect("sweep") > 0);
    exact(&db, "the deadline sweep").await;

    // Assets: an upload admits its verify job; the worker records a new size, claims the job
    // and the sweep fails it; a replace admits a reverify; a delete request admits a delete.
    let mut tx = enter(&db.app, alice, project).await;
    let asset = Uuid::now_v7();
    assets::insert(
        &mut tx,
        asset,
        "a.png",
        "image/png",
        5_000,
        &format!("quarantine/{asset}"),
        alice,
        Uuid::now_v7(),
        &format!("verify:{asset}"),
        OffsetDateTime::now_utc() + time::Duration::HOUR,
    )
    .await
    .expect("upload");
    tx.commit().await.expect("commit");
    exact(&db, "an upload").await;
    sqlx::query("UPDATE assets SET size_bytes = 7_000 WHERE id = $1")
        .bind(asset)
        .execute(&owner)
        .await
        .expect("a recorded size");
    exact(&db, "a recorded size").await;
    let claimed = assets::claim(&db.app, "worker-a", HOUR, 10)
        .await
        .expect("claim");
    assert_eq!(claimed.len(), 1);
    exact(&db, "an asset job's claim").await;
    sqlx::query("UPDATE asset_jobs SET deadline_at = now() - interval '1 second'")
        .execute(&owner)
        .await
        .expect("past its deadline");
    sqlx::query("UPDATE asset_jobs SET lease_expires_at = now() - interval '1 second' WHERE status = 'running'")
        .execute(&owner)
        .await
        .expect("lease lapsed");
    assets::expire(&db.app).await.expect("sweep");
    exact(&db, "the asset sweep").await;

    // Bulk: create and append through the real paths; the job opening (running) keeps its
    // items, closing (cancelled) carries them out; deleting items and the job.
    let mut tx = enter(&db.app, alice, project).await;
    let job = bulk_job(&mut tx, 200).await.expect("bulk job");
    tx.commit().await.expect("commit");
    exact(&db, "a bulk job").await;
    let mut tx = enter(&db.app, alice, project).await;
    let (item_entry, item_revision) = entry(&mut tx).await;
    bulk::lock(&mut tx, job.id).await.expect("lock");
    bulk::append(
        &mut tx,
        job.id,
        &[bulk::NewItem {
            key: "k1",
            entry_id: item_entry,
            expected_revision_id: item_revision,
            input: json!({"title": "Bulk item"}),
        }],
    )
    .await
    .expect("append");
    tx.commit().await.expect("commit");
    exact(&db, "a bulk append").await;
    sqlx::query("UPDATE bulk_jobs SET state = 'running', started_at = now() WHERE id = $1")
        .bind(job.id)
        .execute(&owner)
        .await
        .expect("running");
    exact(&db, "a bulk job starting").await;
    sqlx::query("UPDATE bulk_jobs SET state = 'cancelled', finished_at = now() WHERE id = $1")
        .bind(job.id)
        .execute(&owner)
        .await
        .expect("cancelled");
    exact(&db, "a bulk job closing with pending items").await;
    let mut tx = enter(&db.app, alice, project).await;
    let open = bulk_job(&mut tx, 300).await.expect("bulk job");
    tx.commit().await.expect("commit");
    sqlx::query("DELETE FROM bulk_jobs WHERE id = $1")
        .bind(open.id)
        .execute(&owner)
        .await
        .expect("an open job deleted");
    exact(&db, "an open bulk job deleted").await;

    // Migration plans: an open plan and an item with a stored resolution; the plan closing.
    let environment: Uuid = sqlx::query_scalar(
        "SELECT id FROM environments WHERE project_id = $1 AND slug = 'production'",
    )
    .bind(project)
    .fetch_one(&owner)
    .await
    .expect("environment");
    let plan = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO migration_plans (project_id, environment_id, id, collection, base_version,
                                      target_definition, target_definition_hash, operations,
                                      plan_hash, created_by, expected_items)
         VALUES ($1, $2, $3, 'articles', 1, '{\"fields\":[]}', $5, '[{\"op\":\"x\"}]', $5, $4, 1)",
    )
    .bind(project)
    .bind(environment)
    .bind(plan)
    .bind(alice)
    .bind(vec![0_u8; 32])
    .execute(&owner)
    .await
    .expect("plan");
    exact(&db, "a migration plan").await;
    sqlx::query(
        "INSERT INTO migration_items (project_id, environment_id, plan_id, entry_id,
                                      expected_revision_id, outcome, resolution)
         VALUES ($1, $2, $3, $4, $5, 'ok', '{\"keep\":true}')",
    )
    .bind(project)
    .bind(environment)
    .bind(plan)
    .bind(item_entry)
    .bind(item_revision)
    .execute(&owner)
    .await
    .expect("item");
    exact(&db, "a plan item with a resolution").await;
    sqlx::query("UPDATE migration_items SET problem = '{\"why\":\"x\"}' WHERE plan_id = $1")
        .bind(plan)
        .execute(&owner)
        .await
        .expect("a problem stored");
    exact(&db, "a plan item's problem").await;
    sqlx::query(
        "UPDATE migration_plans SET state = 'cancelled', finished_at = now() WHERE id = $1",
    )
    .bind(plan)
    .execute(&owner)
    .await
    .expect("cancelled");
    exact(&db, "a plan closing").await;

    // The fold changes where the total lives, never what it is.
    work::fold_payload(&db.app).await.expect("fold");
    exact(&db, "after a fold").await;
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn admissions_racing_at_the_bound_never_pass_it() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    // Sixteen projects, so the race is across the one lock they share.
    let mut projects = Vec::new();
    for n in 0..16 {
        projects.push(project(&db, alice, &format!("p{n}"), 0).await);
    }
    // Fill the installation to about 200 kB under the bound with recorded asset bytes: 51
    // pending verify jobs at the largest recorded size (20 MiB), then one sized to the gap.
    let owner = db.owner().await;
    let environment: Uuid = sqlx::query_scalar(
        "SELECT id FROM environments WHERE project_id = $1 AND slug = 'production'",
    )
    .bind(projects[0])
    .fetch_one(&owner)
    .await
    .expect("environment");
    for n in 0..52 {
        let filled = work::install_payload(&db.app).await.expect("payload");
        let asset = Uuid::now_v7();
        let key = format!("verify:{asset}");
        let size: i64 = if n < 51 {
            20_971_520
        } else {
            work::MAX_PENDING_PAYLOAD - filled - 200_000 - i64::try_from(key.len()).expect("short")
        };
        sqlx::query(
            "INSERT INTO assets (project_id, environment_id, id, state, file_name, declared_type,
                                 size_bytes, quarantine_key, created_by)
             VALUES ($1, $2, $3, 'usable', 'a', 'application/octet-stream', $6, $4, $5)",
        )
        .bind(projects[0])
        .bind(environment)
        .bind(asset)
        .bind(format!("quarantine/{asset}"))
        .bind(alice)
        .bind(size)
        .execute(&owner)
        .await
        .expect("asset");
        sqlx::query(
            "INSERT INTO asset_jobs (project_id, environment_id, id, asset_id, kind, idempotency_key,
                                     deadline_at)
             VALUES ($1, $2, gen_random_uuid(), $3, 'verify', $4, now() + interval '1 hour')",
        )
        .bind(projects[0])
        .bind(environment)
        .bind(asset)
        .bind(key)
        .execute(&owner)
        .await
        .expect("asset job");
    }
    let base = work::install_payload(&db.app).await.expect("payload");
    assert_eq!(work::MAX_PENDING_PAYLOAD - base, 200_000, "200 kB of room");

    // Every project submits a ~60 kB job at once: only as many fit as the room allows.
    let pool = Arc::new(db.app_pool(20).await);
    let mut racers = tokio::task::JoinSet::new();
    for project in projects.clone() {
        let pool = Arc::clone(&pool);
        racers.spawn(async move {
            let mut tx = enter(&pool, alice, project).await;
            match bulk_job(&mut tx, 60_000).await {
                Ok(_) => {
                    tx.commit().await.expect("commit");
                    true
                }
                Err(DbError::PayloadFull { .. }) => false,
                Err(other) => panic!("unexpected: {other:?}"),
            }
        });
    }
    let mut admitted = 0;
    while let Some(result) = racers.join_next().await {
        admitted += usize::from(result.expect("racer"));
    }
    let size: i64 =
        sqlx::query_scalar("SELECT octet_length(operation::text)::bigint FROM bulk_jobs LIMIT 1")
            .fetch_one(&owner)
            .await
            .expect("a job's stored size");
    let room = work::MAX_PENDING_PAYLOAD - base;
    let fits = usize::try_from(room / size).expect("small");
    assert_eq!(
        admitted, fits,
        "exactly as many as fit ({room} bytes of room, {size} each)"
    );
    let total = work::install_payload(&db.app).await.expect("payload");
    assert!(total <= work::MAX_PENDING_PAYLOAD, "{total} over the bound");
    exact(&db, "after the race").await;
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_planted_error_is_reported_and_never_repaired() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = project(&db, alice, "alpha", 0).await;
    let mut tx = enter(&db.app, alice, project).await;
    bulk_job(&mut tx, 100).await.expect("bulk job");
    tx.commit().await.expect("commit");
    exact(&db, "before").await;

    let owner = db.owner().await;
    sqlx::query("UPDATE payload_counter SET bytes = bytes + 1")
        .execute(&owner)
        .await
        .expect("plant +1");
    let (_, _, drift) = work::reconcile_payload(&db.app).await.expect("reconcile");
    assert_eq!(drift, 1, "the planted byte is reported");
    work::fold_payload(&db.app).await.expect("fold");
    let mut tx = enter(&db.app, alice, project).await;
    bulk_job(&mut tx, 100).await.expect("bulk job");
    tx.commit().await.expect("commit");
    let (_, _, drift) = work::reconcile_payload(&db.app).await.expect("reconcile");
    assert_eq!(
        drift, 1,
        "folds and admissions carry it, nothing repairs it"
    );
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_rolled_back_admission_leaves_nothing_behind() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = project(&db, alice, "alpha", 0).await;
    let before = work::install_payload(&db.app).await.expect("payload");

    // Written and folded at its check, then rolled back (a crash before commit looks the same).
    let mut tx = enter(&db.app, alice, project).await;
    bulk_job(&mut tx, 5_000).await.expect("bulk job");
    assert!(work::pending_payload(&mut tx).await.expect("own view") > before);
    tx.transaction.rollback().await.expect("rollback");

    assert_eq!(
        work::install_payload(&db.app).await.expect("payload"),
        before
    );
    let (deltas, _) = work::payload_log(&db.app).await.expect("log");
    assert_eq!(deltas, 0, "no delta survives the rollback");
    exact(&db, "after a rollback").await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_tick_folds_the_log_away_and_never_waits_on_an_admission() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = project(&db, alice, "alpha", 2).await;
    release(&db, alice, project, 0).await;
    release(&db, alice, project, 1).await;
    // Workers' changes are only ever logged.
    for claimed in jobs::claim(&db.app, "worker-a", "local", HOUR, 10)
        .await
        .expect("claim")
    {
        jobs::finish(&db.app, &claimed, "worker-a", &succeeded())
            .await
            .expect("finish");
    }
    let (deltas, oldest) = work::payload_log(&db.app).await.expect("log");
    assert!(deltas > 0, "finishes logged their deltas");
    assert!(oldest >= 0.0);

    // An admission holds the counter (it ran its check and hasn't committed): the tick skips.
    let worker_pool = db.app_pool(2).await;
    let mut held = enter(&db.app, alice, project).await;
    bulk_job(&mut held, 100).await.expect("bulk job");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), work::fold_payload(&worker_pool))
            .await
            .expect("the tick never waits")
            .expect("fold"),
        None,
        "busy: skipped"
    );
    held.commit().await.expect("commit");

    // Nothing admitted now: the tick folds the log back to nothing.
    assert!(work::fold_payload(&db.app).await.expect("fold").is_some());
    let (deltas, _) = work::payload_log(&db.app).await.expect("log");
    assert_eq!(deltas, 0);
    exact(&db, "after the tick").await;
    db.drop().await;
}

/// Whether a worker sweep finishes within a second while an admission of another project has
/// written payload, run its check and not committed.
async fn sweep_beside_an_open_admission(db: &TestDb) -> Result<u64, DbError> {
    let alice = db.administrator("alice").await;
    let worked = project(db, alice, "worked", 1).await;
    let admitting = project(db, alice, "admitting", 0).await;
    release(db, alice, worked, 0).await;
    let owner = db.owner().await;
    sqlx::query("UPDATE jobs SET deadline_at = now() - interval '1 second'")
        .execute(&owner)
        .await
        .expect("past their deadline");
    // This database's application sessions only: the role is shared with other databases.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "ALTER ROLE colophon_app IN DATABASE {} SET lock_timeout = '1s'",
        db.name
    )))
    .execute(&owner)
    .await
    .expect("lock timeout");
    let fresh = db.app_pool(4).await;

    let mut open = enter(&fresh, alice, admitting).await;
    bulk_job(&mut open, 100).await.expect("bulk job");
    let swept = jobs::expire(&fresh).await;
    open.transaction.rollback().await.expect("rollback");
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "ALTER ROLE colophon_app IN DATABASE {} RESET lock_timeout",
        db.name
    )))
    .execute(&owner)
    .await
    .expect("reset");
    owner.close().await;
    fresh.close().await;
    swept
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn workers_never_wait_on_an_admission() {
    let db = TestDb::new().await;
    let swept = sweep_beside_an_open_admission(&db).await;
    assert!(
        matches!(swept, Ok(n) if n > 0),
        "the sweep ran beside the open admission: {swept:?}"
    );
    exact(&db, "after the sweep").await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_counter_updated_in_the_triggers_would_make_workers_wait() {
    // The design D-216 didn't take: every delta updating the counter row itself. A worker's
    // sweep then queues behind any admission that hasn't committed, and here times out. This
    // is the test above's failing twin, kept so the coupling stays demonstrated.
    let db = TestDb::new().await;
    let owner = db.owner().await;
    sqlx::query(
        "CREATE OR REPLACE FUNCTION colophon_payload_note(delta bigint) RETURNS void
             LANGUAGE sql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
         AS $$ UPDATE public.payload_counter SET bytes = bytes + delta WHERE id = 1 AND delta <> 0 $$",
    )
    .execute(&owner)
    .await
    .expect("the other design");
    owner.close().await;
    let swept = sweep_beside_an_open_admission(&db).await;
    assert!(
        matches!(&swept, Err(error) if format!("{error:?}").contains("lock")),
        "the sweep waited on the admission's counter lock: {swept:?}"
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn admissions_sweeps_finishes_and_supersedes_never_deadlock() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let mut projects = Vec::new();
    for n in 0..6 {
        projects.push(project(&db, alice, &format!("p{n}"), 2).await);
    }
    let owner = db.owner().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "ALTER ROLE colophon_app IN DATABASE {} SET lock_timeout = '5s'",
        db.name
    )))
    .execute(&owner)
    .await
    .expect("lock timeout");
    let pool = Arc::new(db.app_pool(24).await);
    let mut work_set = tokio::task::JoinSet::new();
    for (n, project) in projects.iter().copied().enumerate() {
        // Each project: releases (supersede, admit, enqueue) and bulk submits, alternating.
        let pool = Arc::clone(&pool);
        work_set.spawn(async move {
            for round in 0..15_usize {
                if (round + n) % 2 == 0 {
                    let mut tx = enter(&pool, alice, project).await;
                    let (entry, _) = entry(&mut tx).await;
                    let pins = changesets::pins(&mut tx, &[entry]).await.map_err(debug)?;
                    let changeset = Uuid::now_v7();
                    changesets::create(&mut tx, changeset, "One", &pins, &[2; 32], None)
                        .await
                        .map_err(debug)?;
                    changesets::submit(&mut tx, changeset)
                        .await
                        .map_err(debug)?;
                    changesets::lock(&mut tx, changeset).await.map_err(debug)?;
                    changesets::review(&mut tx, changeset, &[2; 32], "approve", "", None)
                        .await
                        .map_err(debug)?;
                    let review = changesets::reviews(&mut tx, changeset, None)
                        .await
                        .map_err(debug)?[0]
                        .id;
                    let (_, base) = releases::generation(&mut tx).await.map_err(debug)?;
                    releases::commit(&mut tx, base, changeset, &[2; 32], review, HOUR)
                        .await
                        .map_err(debug)?;
                    tx.commit().await.map_err(debug)?;
                } else {
                    let mut tx = enter(&pool, alice, project).await;
                    bulk_job(&mut tx, 500).await.map_err(debug)?;
                    tx.commit().await.map_err(debug)?;
                }
            }
            Ok::<(), String>(())
        });
    }
    // Workers: claims, finishes and deadline sweeps, as fast as they go, meanwhile.
    for worker in 0..3 {
        let pool = Arc::clone(&pool);
        let owner = owner.clone();
        work_set.spawn(async move {
            let name = format!("worker-{worker}");
            for _ in 0..40 {
                for claimed in jobs::claim(&pool, &name, "local", HOUR, 5)
                    .await
                    .map_err(debug)?
                {
                    jobs::finish(&pool, &claimed, &name, &succeeded())
                        .await
                        .map_err(debug)?;
                }
                sqlx::query(
                    "UPDATE jobs SET deadline_at = now() - interval '1 second'
                     WHERE status = 'pending' AND random() < 0.3",
                )
                .execute(&owner)
                .await
                .map_err(debug)?;
                jobs::expire(&pool).await.map_err(debug)?;
                work::fold_payload(&pool).await.map_err(debug)?;
            }
            Ok::<(), String>(())
        });
    }
    let mut failures = Vec::new();
    while let Some(result) = work_set.join_next().await {
        if let Err(error) = result.expect("task") {
            failures.push(error);
        }
    }
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "ALTER ROLE colophon_app IN DATABASE {} RESET lock_timeout",
        db.name
    )))
    .execute(&owner)
    .await
    .expect("reset");
    assert!(failures.is_empty(), "deadlocks or lock waits: {failures:?}");
    exact(&db, "after the storm").await;
    owner.close().await;
    db.drop().await;
}
