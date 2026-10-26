//! The first real-process crash loop (docs/modules/10-sim.md, section 7; step 2.4, D-047). A client
//! publishes articles in two projects, each with a webhook destination, while the server, API and
//! worker together, is killed with SIGKILL at random moments and started again; a request that
//! got no answer is sent again with the same keys, as a real client would. Afterwards a checker
//! compares what the client was told with the database, the public delivery API and what the
//! receiver got:
//!
//! - every acknowledged publish is a release, at the generation the client was told;
//! - each project's generations are 1..N, each changeset published once;
//! - no entry's publication history overlaps;
//! - the receiver never got an older release after a newer one, and got the latest;
//! - every announcement finished delivered or superseded;
//! - delivery at generation g serves exactly g entries (one new entry per release).
//!
//! `COLOPHON_CRASH_LOOP_KILLS` sets the number of kills (default 12); `COLOPHON_CRASH_LOOP_SEED`
//! the seed for kill timing (the run prints it). Needs the stack and `scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use std::time::{Duration, Instant};

use common::{
    ORIGIN, Receiver, Server, Session, app, article, call, destination, entries, finished_job, key,
    project, sign_in,
};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// Short leases and attempts, so a killed worker's jobs are claimed again within the test.
const WORKER: [(&str, &str); 3] = [
    ("COLOPHON_WORKER_LEASE_MS", "1500"),
    ("COLOPHON_WORKER_ATTEMPT_MS", "500"),
    ("COLOPHON_WORKER_POLL_MS", "50"),
];

/// `SplitMix64`, for kill timing that a seed can repeat (the server's own timing still varies).
struct Seeded(u64);

impl Seeded {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// One publish in progress: what's been done, with the keys to redo the rest after a kill.
#[derive(Debug, Clone)]
struct Cycle {
    entry_key: String,
    entry: Option<String>,
    set_key: String,
    set: Option<(String, String)>,
    submitted: bool,
    approved: bool,
    publish_key: String,
    base: i64,
}

#[derive(Debug)]
struct ProjectState {
    id: String,
    generation: i64,
    cycle: Option<Cycle>,
    /// Acknowledged releases: generation and changeset path.
    acknowledged: Vec<(i64, String)>,
}

#[derive(Debug, Default)]
struct Counts {
    kills: u64,
    /// Requests cut off by a kill, whatever step they were.
    cut: u64,
    /// Publishes answered from their first commit when sent again.
    replayed: u64,
}

fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
}

/// Advances `state`'s publish by one request; false if the request got no answer (a kill).
#[expect(
    clippy::too_many_lines,
    reason = "one publish, request by request: each branch is one step of it"
)]
async fn step(session: &Session, state: &mut ProjectState, counts: &mut Counts) -> bool {
    let cycle = state.cycle.get_or_insert_with(|| Cycle {
        entry_key: key(),
        entry: None,
        set_key: key(),
        set: None,
        submitted: false,
        approved: false,
        publish_key: key(),
        base: state.generation,
    });
    let project = state.id.clone();
    if cycle.entry.is_none() {
        let title = format!("Crash loop {}", state.acknowledged.len() + 1);
        let Ok(reply) = call(
            session,
            Method::POST,
            &entries(&project),
            Some(&article(&title, "text")),
            &[("idempotency-key", &cycle.entry_key)],
        )
        .await
        else {
            return false;
        };
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
        cycle.entry = reply.body["data"]["id"].as_str().map(str::to_owned);
        return true;
    }
    if cycle.set.is_none() {
        let body = json!({"title": "crash loop", "entries": [cycle.entry]});
        let Ok(reply) = call(
            session,
            Method::POST,
            &format!("{}/changesets", base(&project)),
            Some(&body),
            &[("idempotency-key", &cycle.set_key)],
        )
        .await
        else {
            return false;
        };
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
        let id = reply.body["data"]["id"].as_str().expect("an id");
        let hash = reply.body["data"]["hash"].as_str().expect("a hash");
        cycle.set = Some((
            format!("{}/changesets/{id}", base(&project)),
            format!("\"{hash}\""),
        ));
        return true;
    }
    let (path, if_match) = cycle.set.clone().expect("a changeset");
    if !cycle.submitted {
        let Ok(reply) = call(
            session,
            Method::POST,
            &format!("{path}/submit"),
            None,
            &[("if-match", &if_match)],
        )
        .await
        else {
            return false;
        };
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        cycle.submitted = true;
        return true;
    }
    if !cycle.approved {
        let body = json!({"decision": "approve"});
        let Ok(reply) = call(
            session,
            Method::POST,
            &format!("{path}/reviews"),
            Some(&body),
            &[("if-match", &if_match)],
        )
        .await
        else {
            return false;
        };
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        cycle.approved = true;
        return true;
    }
    let body = json!({"base_generation": cycle.base});
    let Ok(reply) = call(
        session,
        Method::POST,
        &format!("{path}/publish"),
        Some(&body),
        &[
            ("if-match", &if_match),
            ("idempotency-key", &cycle.publish_key),
        ],
    )
    .await
    else {
        counts.cut += 1;
        return false;
    };
    assert_eq!(
        reply.status,
        StatusCode::CREATED,
        "a publish, first time or resent: {}",
        reply.body
    );
    if reply.replayed() {
        counts.replayed += 1;
    }
    let generation = reply.body["data"]["generation"]
        .as_i64()
        .expect("a generation");
    assert_eq!(
        generation,
        cycle.base + 1,
        "nobody else publishes in this project"
    );
    state.generation = generation;
    state.acknowledged.push((generation, path));
    state.cycle = None;
    true
}

/// Publishes round-robin until `deadline`; a request cut off by the kill leaves its cycle to resume.
async fn publish_until(
    session: &Session,
    states: &mut [ProjectState],
    counts: &mut Counts,
    deadline: Instant,
) {
    let mut turn = 0;
    while Instant::now() < deadline {
        let index = turn % states.len();
        turn += 1;
        let remaining = deadline.saturating_duration_since(Instant::now());
        let state = &mut states[index];
        if tokio::time::timeout(remaining, step(session, state, counts))
            .await
            .is_err()
        {
            counts.cut += 1;
            return;
        }
    }
}

async fn finish_cycles(session: &Session, states: &mut [ProjectState], counts: &mut Counts) {
    for state in states.iter_mut() {
        while state.cycle.is_some() {
            assert!(
                step(session, state, counts).await,
                "the last server keeps running"
            );
        }
    }
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh; slow: kills and restarts the server repeatedly"]
#[expect(
    clippy::too_many_lines,
    reason = "the loop and its checker, in the order they run"
)]
async fn killing_the_server_and_worker_loses_and_doubles_nothing() {
    let kills: u64 = std::env::var("COLOPHON_CRASH_LOOP_KILLS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    let seed: u64 = std::env::var("COLOPHON_CRASH_LOOP_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2_400);
    println!("crash loop: {kills} kills, seed {seed}");
    let mut rng = Seeded(seed);
    let receiver = Receiver::start(&[]).await;
    let mut counts = Counts::default();

    let server = Server::start_with(&WORKER).await;
    let owner = sign_in("dev-owner").await;
    let mut states = Vec::new();
    for _ in 0..2 {
        let id = project(&owner).await;
        destination(&owner, &id, &receiver.url).await;
        states.push(ProjectState {
            id,
            generation: 0,
            cycle: None,
            acknowledged: Vec::new(),
        });
    }
    let mut server = Some(server);
    for _ in 0..kills {
        let alive = Duration::from_millis(300 + rng.next() % 1_700);
        publish_until(&owner, &mut states, &mut counts, Instant::now() + alive).await;
        drop(server.take()); // SIGKILL: no response sent, no destructor run, in-flight work cut off
        counts.kills += 1;
        server = Some(Server::start_with(&WORKER).await);
    }
    finish_cycles(&owner, &mut states, &mut counts).await;

    // Let the worker drain: every project's latest release must be delivered.
    for state in &states {
        let job = finished_job(&owner, &state.id, state.generation, 30).await;
        assert_eq!(
            job["status"], "succeeded",
            "the latest announcement of {}: {job}",
            state.id
        );
    }

    let database = common::owner_database().await;
    let announcements = receiver.seen();
    let mut published = 0;
    for state in &states {
        // Every acknowledged publish is a release at the generation the client was told, each
        // changeset once, generations 1..N. (From the database: the API lists the newest 100.)
        let releases: Vec<(i64, uuid::Uuid)> = sqlx::query_as(
            "SELECT generation, changeset_id FROM releases WHERE project_id = $1::uuid ORDER BY generation",
        )
        .bind(&state.id)
        .fetch_all(&database)
        .await
        .expect("releases");
        let generations: Vec<i64> = releases.iter().map(|(generation, _)| *generation).collect();
        let expected: Vec<i64> = (1..=state.generation).collect();
        assert_eq!(
            generations, expected,
            "{}: generations are 1..{}",
            state.id, state.generation
        );
        assert_eq!(
            state.acknowledged.len(),
            releases.len(),
            "{}: every release was acknowledged once",
            state.id
        );
        for (generation, path) in &state.acknowledged {
            let changeset = path.rsplit('/').next().expect("an id");
            let (_, stored) = releases
                .iter()
                .find(|(g, _)| g == generation)
                .expect("the acknowledged release exists");
            assert_eq!(
                stored.to_string(),
                changeset,
                "{}: generation {generation} is the acknowledged changeset",
                state.id
            );
        }
        let mut changesets: Vec<uuid::Uuid> =
            releases.iter().map(|(_, changeset)| *changeset).collect();
        changesets.sort_unstable();
        changesets.dedup();
        assert_eq!(
            changesets.len(),
            releases.len(),
            "{}: no changeset published twice",
            state.id
        );

        // Publication history never overlaps.
        let rows: Vec<(uuid::Uuid, i64, Option<i64>)> = sqlx::query_as(
            "SELECT entry_id, valid_from, valid_to FROM publications WHERE project_id = $1::uuid ORDER BY entry_id, valid_from",
        )
        .bind(&state.id)
        .fetch_all(&database)
        .await
        .expect("history");
        let mut by_entry: std::collections::BTreeMap<
            uuid::Uuid,
            Vec<colophon_domain::release::Interval<()>>,
        > = std::collections::BTreeMap::new();
        for (entry, from, to) in rows {
            by_entry
                .entry(entry)
                .or_default()
                .push(colophon_domain::release::Interval {
                    revision: (),
                    from,
                    to,
                });
        }
        assert!(
            by_entry
                .values()
                .all(|history| colophon_domain::release::well_formed(history)),
            "{}: overlapping history",
            state.id
        );

        // The receiver never got an older release after a newer one, and got the latest.
        let prefix = format!("colophon:{}:", state.id);
        let order: Vec<i64> = announcements
            .iter()
            .filter(|r| r.key.starts_with(&prefix))
            .filter_map(|r| r.generation.parse().ok())
            .collect();
        assert!(
            order.windows(2).all(|pair| pair[0] <= pair[1]),
            "{}: announcements out of order: {order:?}",
            state.id
        );
        assert_eq!(
            order.last().copied(),
            Some(state.generation),
            "{}: the latest release was announced",
            state.id
        );

        // Every announcement finished delivered or superseded.
        let unfinished: Vec<(String, i64)> = sqlx::query_as(
            "SELECT status, count(*) FROM jobs WHERE project_id = $1::uuid AND status NOT IN ('succeeded', 'superseded') GROUP BY status",
        )
        .bind(&state.id)
        .fetch_all(&database)
        .await
        .expect("jobs");
        assert_eq!(
            unfinished,
            Vec::new(),
            "{}: announcements not delivered or superseded",
            state.id
        );

        // Delivery at generation g serves exactly g entries: one new entry per release.
        for generation in [1, state.generation / 2, state.generation]
            .into_iter()
            .filter(|g| *g >= 1)
        {
            let response = app()
                .get(format!(
                    "{ORIGIN}/content/v1/{}/production/articles?generation={generation}&limit=100",
                    state.id
                ))
                .send()
                .await
                .expect("request");
            let body: Value = response.json().await.expect("JSON");
            let count = body["data"].as_array().map_or(0, Vec::len);
            assert_eq!(
                i64::try_from(count).unwrap_or(-1),
                generation.min(100),
                "{}: entries at generation {generation}",
                state.id
            );
        }
        published += state.generation;
    }
    let mut keys: Vec<&str> = announcements.iter().map(|r| r.key.as_str()).collect();
    let total = keys.len();
    keys.sort_unstable();
    keys.dedup();
    println!(
        "crash loop: {} kills, {published} releases acknowledged and checked, {} requests cut off by a kill, {} publishes answered from their first commit when sent again, {total} announcements received ({} repeats of a key)",
        counts.kills,
        counts.cut,
        counts.replayed,
        total - keys.len()
    );
    database.close().await;
    drop(server);
}
