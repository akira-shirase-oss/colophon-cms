//! Admissions a second with N concurrent submitters (D-216's measurement): each submitter is
//! its own project, opening a bulk job through the real admission path and committing, for a
//! fixed time, over a backlog of open work the old per-admission SUM had to scan. Prints one
//! line per N. A measurement, not a check: run it by name on a quiet machine, on each commit
//! being compared, and record the machine and commit with the numbers.
#![expect(
    clippy::expect_used,
    reason = "a measurement harness: any setup failure ends the run"
)]

mod common;

use std::{
    sync::Arc,
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
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "counts and percentiles of a few hundred thousand samples, printed"
)]
async fn admissions_a_second_by_concurrent_submitters() {
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

    let mut submitters = Vec::new();
    for n in 0..64 {
        submitters.push(
            projects::create(&db.app, alice, &format!("s{n}"), "Submitter")
                .await
                .expect("created")
                .id,
        );
    }

    for concurrency in [1_usize, 8, 32, 64] {
        let mut tasks = tokio::task::JoinSet::new();
        let started = Instant::now();
        for project in submitters.iter().copied().take(concurrency) {
            let pool = Arc::clone(&pool);
            tasks.spawn(async move {
                let mut latencies = Vec::new();
                while started.elapsed() < WINDOW {
                    let begun = Instant::now();
                    submit(&pool, alice, project, 200).await;
                    latencies.push(begun.elapsed());
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
