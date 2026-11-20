//! Admissions a second with N concurrent submitters (D-216's measurement), opening bulk jobs
//! through the real admission path and committing, for a fixed time, over a backlog of open
//! work the old per-admission SUM had to scan. Two modes: across projects (each submitter its
//! own project: only installation-wide locks are shared) and within one project (every
//! submitter in the same project: its own lock, WA, is shared too). Prints one line per N. A
//! measurement, not a check: run it by name on a quiet machine, on each commit being
//! compared, and record the machine and commit with the numbers.
#![expect(
    clippy::expect_used,
    reason = "a measurement harness: any setup failure ends the run"
)]

mod common;

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use colophon_db::{PgPool, bulk, projects, scope};
use common::TestDb;
use serde_json::json;
use uuid::Uuid;

/// How long each N runs.
const WINDOW: Duration = Duration::from_secs(10);
/// Open bulk jobs left in the backlog, spread over their own projects.
const BACKLOG_PROJECTS: usize = 20;
const BACKLOG_PER_PROJECT: usize = 250;
/// Admissions one project takes before its submitters move on to a fresh one, under the
/// per-project bound on open work (`work::MAX_OPEN`, 10,000), so no window ever meets it.
const PER_PROJECT: usize = 9_000;
/// The concurrency levels measured.
const LEVELS: [usize; 4] = [1, 8, 32, 64];

async fn submit(pool: &PgPool, identity: Uuid, project: Uuid, size: usize) {
    let mut tx = scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member");
    bulk::create(
        &mut tx,
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
    .expect("admitted");
    tx.commit().await.expect("commit");
}

#[tokio::test]
#[ignore = "a measurement: run by name on a quiet machine (needs the local stack)"]
async fn admissions_a_second_by_concurrent_submitters() {
    measure(Mode::AcrossProjects).await;
}

#[tokio::test]
#[ignore = "a measurement: run by name on a quiet machine (needs the local stack)"]
async fn admissions_a_second_by_concurrent_submitters_in_one_project() {
    measure(Mode::OneProject).await;
}

#[derive(Clone, Copy)]
enum Mode {
    /// Each submitter its own project.
    AcrossProjects,
    /// Every submitter in the same project, moving together to a fresh one every
    /// [`PER_PROJECT`] admissions.
    OneProject,
}

async fn fresh_projects(db: &TestDb, alice: Uuid, name: &str, count: usize) -> Arc<Vec<Uuid>> {
    let mut made = Vec::with_capacity(count);
    for n in 0..count {
        made.push(
            projects::create(&db.app, alice, &format!("{name}{n}"), "Submitter")
                .await
                .expect("created")
                .id,
        );
    }
    Arc::new(made)
}

#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "counts and percentiles of a few hundred thousand samples, printed"
)]
async fn measure(mode: Mode) {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let pool = Arc::new(db.app_pool(70).await);

    // The backlog: open work the bound counts.
    for p in 0..BACKLOG_PROJECTS {
        let project = projects::create(&db.app, alice, &format!("backlog{p}"), "Backlog")
            .await
            .expect("created")
            .id;
        for _ in 0..BACKLOG_PER_PROJECT {
            submit(&db.app, alice, project, 1_000).await;
        }
    }

    for concurrency in LEVELS {
        // Fresh projects per level, so no project carries one level's open work into the next.
        // Across projects: one each, and a submitter past PER_PROJECT moves to its next one.
        // In one project: a shared run of projects, all submitters in the same one at a time.
        let (projects, lanes) = match mode {
            Mode::AcrossProjects => (
                fresh_projects(&db, alice, &format!("n{concurrency}s"), concurrency * 4).await,
                concurrency,
            ),
            Mode::OneProject => (
                fresh_projects(&db, alice, &format!("n{concurrency}one"), 16).await,
                1,
            ),
        };
        let admitted = Arc::new(AtomicUsize::new(0));
        let mut tasks = tokio::task::JoinSet::new();
        let started = Instant::now();
        for submitter in 0..concurrency {
            let pool = Arc::clone(&pool);
            let projects = Arc::clone(&projects);
            let admitted = Arc::clone(&admitted);
            tasks.spawn(async move {
                let mut latencies = Vec::new();
                let mut mine = 0_usize;
                while started.elapsed() < WINDOW {
                    let turn = match mode {
                        Mode::AcrossProjects => mine / PER_PROJECT,
                        Mode::OneProject => admitted.load(Ordering::Relaxed) / PER_PROJECT,
                    };
                    let project = projects[(turn * lanes + submitter % lanes) % projects.len()];
                    let begun = Instant::now();
                    submit(&pool, alice, project, 200).await;
                    latencies.push(begun.elapsed());
                    mine += 1;
                    admitted.fetch_add(1, Ordering::Relaxed);
                }
                latencies
            });
        }
        let mut latencies = Vec::new();
        while let Some(done) = tasks.join_next().await {
            latencies.extend(done.expect("submitter"));
        }
        let elapsed = started.elapsed().as_secs_f64();
        latencies.sort_unstable();
        let at = |q: f64| {
            let index = ((latencies.len() as f64 - 1.0) * q).round() as usize;
            latencies[index].as_secs_f64() * 1_000.0
        };
        println!(
            "N={concurrency:>2}  admissions={:>6}  per_second={:>8.1}  p50_ms={:>7.2}  p99_ms={:>7.2}",
            latencies.len(),
            latencies.len() as f64 / elapsed,
            at(0.5),
            at(0.99)
        );
    }
    db.drop().await;
}
