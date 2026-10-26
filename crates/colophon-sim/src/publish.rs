//! The publishing simulation (docs/modules/10-sim.md, P2; D-047): publishers commit releases on
//! the generation they saw, an outbox queues one announcement per destination with each release,
//! workers claim announcements under leases on a virtual clock and crash at any step, and a
//! webhook receiver sits behind a network that loses requests, loses answers, duplicates and
//! refuses. The same rules as the real system come from the domain (`colophon_domain::release`,
//! `colophon_domain::job`); this module models the database and the worker around them.
//!
//! Checked after every step:
//!
//! - **G**, two releases on one base generation never both commit;
//! - **S2**, no logical release is applied twice;
//! - **A**, a retried publish whose first attempt committed is answered with that release, never
//!   refused as if another release had come first;
//! - **S3**, generations go up by exactly one per committed release;
//! - **S4**, an entry's publication intervals never overlap;
//! - **O**, every committed release has an announcement for every destination (the outbox);
//! - **S6**, a destination never receives an older release after a newer one;
//! - **S7**, only the current lease holder records a job's outcome.
//!
//! At the end, after a quiet drain with no faults: **L**, every job is finished, and every
//! destination whose latest announcement succeeded received it.

use std::collections::{BTreeMap, BTreeSet};

use colophon_domain::{
    job::{self, Next, Outcome, Status},
    release::{self, Interval},
};

use crate::{rng::Rng, sim::Violation};

/// The shape of a publishing run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishConfig {
    /// Environments, each with its own entries, destinations and publishers.
    pub environments: u64,
    /// Entries per environment; releases pick from them.
    pub entries: u64,
    /// Destinations per environment.
    pub destinations: u64,
    /// Publishers per environment; they race for the next generation.
    pub publishers: u64,
    /// Workers, shared by every environment.
    pub workers: u64,
    /// Steps in the run, before the quiet drain.
    pub steps: usize,
    /// Virtual milliseconds per step.
    pub step_ms: i64,
    /// How long a claim lasts.
    pub lease_ms: i64,
    /// A sent request arrives within this long, or never; shorter than the lease.
    pub attempt_ms: i64,
    /// How often a publisher acts on a generation it saw earlier, per thousand.
    pub stale_view: u16,
    /// How often a request is lost before it arrives, per thousand.
    pub lose_request: u16,
    /// How often an answer is lost after the request took effect, per thousand.
    pub lose_answer: u16,
    /// How often the network delivers a request twice, per thousand.
    pub duplicate: u16,
    /// How often the receiver answers 503 instead of accepting, per thousand.
    pub refuse: u16,
    /// How often an acting worker crashes, per thousand.
    pub crash: u16,
}

impl Default for PublishConfig {
    fn default() -> Self {
        Self {
            environments: 2,
            entries: 4,
            destinations: 2,
            publishers: 2,
            workers: 3,
            steps: 600,
            step_ms: 200,
            lease_ms: 3_000,
            attempt_ms: 1_000,
            stale_view: 250,
            lose_request: 80,
            lose_answer: 80,
            duplicate: 60,
            refuse: 80,
            crash: 40,
        }
    }
}

/// A rule broken on purpose; the suite requires each to be caught (S12).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PublishMutation {
    /// The system as it should be.
    #[default]
    None,
    /// A release commits whatever generation its publisher saw.
    IgnoreBase,
    /// Publishing doesn't close the entry's open interval.
    KeepOpenInterval,
    /// A worker records an outcome without holding the lease.
    StaleLeaseFinalizes,
    /// Two workers may announce to one destination at once.
    ParallelSends,
    /// A worker skips the check for a newer release before sending.
    NoSupersedeCheck,
    /// Announcements are queued after the release commits, so a crash between loses them.
    OutboxAfterCommit,
    /// A retried publish is applied again instead of replayed.
    PublishForgetsKey,
}

impl PublishMutation {
    /// Every mutation, for the mutation suite.
    pub const ALL: [Self; 7] = [
        Self::IgnoreBase,
        Self::KeepOpenInterval,
        Self::StaleLeaseFinalizes,
        Self::ParallelSends,
        Self::NoSupersedeCheck,
        Self::OutboxAfterCommit,
        Self::PublishForgetsKey,
    ];

    /// The name used on the command line.
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::IgnoreBase => "ignore-base",
            Self::KeepOpenInterval => "keep-open-interval",
            Self::StaleLeaseFinalizes => "stale-lease-finalizes",
            Self::ParallelSends => "parallel-sends",
            Self::NoSupersedeCheck => "no-supersede-check",
            Self::OutboxAfterCommit => "outbox-after-commit",
            Self::PublishForgetsKey => "publish-forgets-key",
        }
    }

    /// The mutation with this name.
    pub fn parse(name: &str) -> Option<Self> {
        std::iter::once(Self::None)
            .chain(Self::ALL)
            .find(|mutation| mutation.name() == name)
    }
}

/// What happened in a publishing run, so a test can prove the faults were exercised.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PublishStats {
    /// Releases committed.
    pub commits: u64,
    /// Publishes refused because another release committed first.
    pub moved: u64,
    /// Retried publishes answered from their first commit.
    pub replayed: u64,
    /// Announcements the receiver accepted (repeats included).
    pub received: u64,
    /// Accepted announcements whose key it had already seen.
    pub duplicates: u64,
    /// Requests lost before arriving.
    pub lost_requests: u64,
    /// Answers lost after the request took effect.
    pub lost_answers: u64,
    /// 503s from the receiver.
    pub refused: u64,
    /// Worker crashes.
    pub crashes: u64,
    /// Claims of a job whose lease had run out.
    pub reclaims: u64,
    /// Jobs superseded, at commit or before sending.
    pub superseded: u64,
    /// Outcomes refused because the worker no longer held the lease.
    pub late_refused: u64,
    /// Attempts given up before sending, too close to the lease's end.
    pub abandoned: u64,
}

/// A finished publishing run.
#[derive(Debug, Clone)]
pub struct PublishReport {
    /// The seed.
    pub seed: u64,
    /// FNV-1a over the trace: equal for equal runs.
    pub trace_hash: u64,
    /// Every step, one line each.
    pub trace: Vec<String>,
    /// Every invariant failure, in order.
    pub violations: Vec<Violation>,
    /// Counts of what happened.
    pub stats: PublishStats,
}

#[derive(Debug, Clone)]
struct Release {
    generation: i64,
    base: i64,
    key: u64,
}

#[derive(Debug, Clone)]
struct Environment {
    generation: i64,
    releases: Vec<Release>,
    history: BTreeMap<u64, Vec<Interval<u64>>>,
    /// A publish key and the generation it committed: what a retry replays.
    keys: BTreeMap<u64, i64>,
    next_revision: u64,
}

#[derive(Debug, Clone)]
struct Job {
    environment: usize,
    destination: u64,
    generation: i64,
    key: String,
    status: Status,
    attempts: u32,
    holder: Option<u64>,
    lease_generation: u32,
    expires_ms: i64,
    next_attempt_ms: i64,
    deadline_ms: i64,
}

#[derive(Debug, Clone)]
enum Phase {
    Check,
    Send,
    Await { until_ms: i64, outcome: Outcome },
    Record { outcome: Outcome },
}

#[derive(Debug, Clone)]
struct Holding {
    job: usize,
    lease_generation: u32,
    phase: Phase,
}

#[derive(Debug, Clone)]
struct Publisher {
    environment: usize,
    /// The generation it last looked at.
    seen: i64,
    /// A publish whose answer was lost, to send again with the same key.
    retry: Option<(u64, i64, Vec<u64>)>,
}

#[derive(Debug, Clone)]
struct InFlight {
    arrives_ms: i64,
    order: u64,
    environment: usize,
    destination: u64,
    generation: i64,
    key: String,
    refused: bool,
}

struct Run<'a> {
    config: &'a PublishConfig,
    mutation: PublishMutation,
    rng: Rng,
    environments: Vec<Environment>,
    publishers: Vec<Publisher>,
    workers: Vec<Option<Holding>>,
    jobs: Vec<Job>,
    network: Vec<InFlight>,
    received: BTreeMap<(usize, u64), Vec<(i64, String)>>,
    keys_counter: u64,
    order: u64,
    now_ms: i64,
    step: usize,
    quiet: bool,
    trace: Vec<String>,
    violations: Vec<Violation>,
    stats: PublishStats,
}

fn fnv1a(lines: &[String]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in lines.iter().flat_map(|line| line.bytes().chain(*b"\n")) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Runs one publishing seed.
pub fn run_publishing(
    seed: u64,
    config: &PublishConfig,
    mutation: PublishMutation,
) -> PublishReport {
    let environments = (0..config.environments)
        .map(|_| Environment {
            generation: 0,
            releases: Vec::new(),
            history: BTreeMap::new(),
            keys: BTreeMap::new(),
            next_revision: 1,
        })
        .collect();
    let publishers = (0..config.environments)
        .flat_map(|environment| {
            (0..config.publishers).map(move |_| Publisher {
                environment: usize::try_from(environment).unwrap_or(0),
                seen: 0,
                retry: None,
            })
        })
        .collect();
    let mut run = Run {
        config,
        mutation,
        rng: Rng::new(seed),
        environments,
        publishers,
        workers: vec![None; usize::try_from(config.workers).unwrap_or(1)],
        jobs: Vec::new(),
        network: Vec::new(),
        received: BTreeMap::new(),
        keys_counter: 0,
        order: 0,
        now_ms: 0,
        step: 0,
        quiet: false,
        trace: Vec::new(),
        violations: Vec::new(),
        stats: PublishStats::default(),
    };
    for step in 0..config.steps {
        run.step = step;
        run.tick();
    }
    // The quiet drain: no faults and no new releases, until every job finishes (bounded).
    run.quiet = true;
    let drain = config.steps.saturating_mul(4).max(2_000);
    for step in config.steps..config.steps + drain {
        run.step = step;
        if run.jobs.iter().all(|job| job.status.is_final())
            && run.network.is_empty()
            && run.workers.iter().all(Option::is_none)
        {
            break;
        }
        run.tick();
    }
    run.check_end();
    PublishReport {
        seed,
        trace_hash: fnv1a(&run.trace),
        trace: run.trace,
        violations: run.violations,
        stats: run.stats,
    }
}

impl Run<'_> {
    fn note(&mut self, line: &str) {
        self.trace
            .push(format!("{:>5} t={:>7} {line}", self.step, self.now_ms));
    }

    fn violation(&mut self, invariant: &'static str, detail: String) {
        self.note(&format!("VIOLATION {invariant}: {detail}"));
        self.violations.push(Violation {
            invariant,
            step: self.step,
            detail,
        });
    }

    fn chance(&mut self, per_mille: u16) -> bool {
        !self.quiet && self.rng.chance(per_mille)
    }

    fn tick(&mut self) {
        self.now_ms += self.config.step_ms;
        self.deliver();
        let actors = self.publishers.len() + self.workers.len();
        let actor = self.rng.below(actors);
        if actor < self.publishers.len() {
            if !self.quiet {
                self.publish(actor);
            }
        } else {
            self.work(actor - self.publishers.len());
        }
        self.check();
    }

    // ---- publishers and the release transaction ----

    fn publish(&mut self, index: usize) {
        let environment = self.publishers[index].environment;
        let (key, base, entries) = if let Some(retry) = self.publishers[index].retry.take() {
            retry
        } else {
            let current = self.environments[environment].generation;
            let seen = if self.chance(self.config.stale_view) {
                self.publishers[index].seen
            } else {
                current
            };
            self.publishers[index].seen = current;
            self.keys_counter += 1;
            let count = 1 + self.rng.below(2);
            // Distinct entries, as the API requires of a changeset.
            let entries: Vec<u64> = (0..count)
                .map(|_| {
                    u64::try_from(
                        self.rng
                            .below(usize::try_from(self.config.entries).unwrap_or(1)),
                    )
                    .unwrap_or(0)
                })
                .collect::<BTreeSet<u64>>()
                .into_iter()
                .collect();
            (self.keys_counter, seen, entries)
        };
        if self.chance(self.config.lose_request) {
            self.stats.lost_requests += 1;
            self.note(&format!(
                "p{index} publish key {key} on {base}: request lost"
            ));
            self.publishers[index].retry = Some((key, base, entries));
            return;
        }
        let answer = self.commit(environment, index, key, base, &entries);
        if answer.is_err()
            && let Some(&committed) = self.environments[environment].keys.get(&key)
        {
            let detail = format!(
                "publish key {key} committed generation {committed}, but its retry was refused"
            );
            self.violation("A", detail);
        }
        if self.chance(self.config.lose_answer) {
            self.stats.lost_answers += 1;
            self.note(&format!(
                "p{index} publish key {key}: answer lost ({answer:?})"
            ));
            self.publishers[index].retry = Some((key, base, entries));
        }
    }

    /// The release transaction: replay, conditional bump, history, outbox, supersession.
    fn commit(
        &mut self,
        environment: usize,
        publisher: usize,
        key: u64,
        base: i64,
        entries: &[u64],
    ) -> Result<i64, i64> {
        if self.mutation != PublishMutation::PublishForgetsKey
            && let Some(&generation) = self.environments[environment].keys.get(&key)
        {
            self.stats.replayed += 1;
            self.note(&format!(
                "p{publisher} publish key {key}: replayed generation {generation}"
            ));
            return Ok(generation);
        }
        let current = self.environments[environment].generation;
        let generation = if self.mutation == PublishMutation::IgnoreBase {
            current + 1
        } else {
            let Ok(generation) = release::next_generation(base, current) else {
                self.stats.moved += 1;
                self.note(&format!(
                    "p{publisher} publish key {key} on {base}: generation moved to {current}"
                ));
                return Err(current);
            };
            generation
        };
        let env = &mut self.environments[environment];
        env.generation = generation;
        env.releases.push(Release {
            generation,
            base,
            key,
        });
        env.keys.insert(key, generation);
        for &entry in entries {
            let revision = env.next_revision;
            env.next_revision += 1;
            let history = env.history.entry(entry).or_default();
            if self.mutation == PublishMutation::KeepOpenInterval {
                history.push(Interval {
                    revision,
                    from: generation,
                    to: None,
                });
            } else {
                *history = release::apply(history, &revision, generation);
            }
        }
        self.stats.commits += 1;
        self.note(&format!("p{publisher} publish key {key} on {base}: committed generation {generation} ({} entries)", entries.len()));
        // Waiting announcements of older releases are obsolete now.
        for job in &mut self.jobs {
            if job.environment == environment
                && job.generation < generation
                && matches!(job.status, Status::Pending | Status::Retrying)
            {
                job.status = Status::Superseded;
                self.stats.superseded += 1;
            }
        }
        let lost_outbox =
            self.mutation == PublishMutation::OutboxAfterCommit && self.rng.chance(300);
        if lost_outbox {
            self.note(&format!(
                "generation {generation}: crashed before queueing its announcements"
            ));
        } else {
            for destination in 0..self.config.destinations {
                self.jobs.push(Job {
                    environment,
                    destination,
                    generation,
                    key: format!("colophon:e{environment}:g{generation}:d{destination}"),
                    status: Status::Pending,
                    attempts: 0,
                    holder: None,
                    lease_generation: 0,
                    expires_ms: 0,
                    next_attempt_ms: self.now_ms,
                    deadline_ms: self.now_ms + job::LIFETIME_SECS * 1_000,
                });
            }
        }
        Ok(generation)
    }

    // ---- workers ----

    fn work(&mut self, worker: usize) {
        if self.chance(self.config.crash) {
            if self.workers[worker].take().is_some() {
                self.stats.crashes += 1;
                self.note(&format!("w{worker} crashed holding a job"));
            }
            return;
        }
        match self.workers[worker].take() {
            None => self.claim(worker),
            Some(holding) => self.advance(worker, holding),
        }
    }

    fn claim(&mut self, worker: usize) {
        let now = self.now_ms;
        // One announcement in flight per destination: a destination with a live lease is busy.
        let busy: BTreeSet<(usize, u64)> = self
            .jobs
            .iter()
            .filter(|job| job.status == Status::Running && job.expires_ms > now)
            .map(|job| (job.environment, job.destination))
            .collect();
        let due = (0..self.jobs.len()).find(|&index| {
            let job = &self.jobs[index];
            let ready = match job.status {
                Status::Pending | Status::Retrying => job.next_attempt_ms <= now,
                Status::Running => job.expires_ms <= now,
                _ => false,
            };
            ready
                && job.deadline_ms > now
                && (self.mutation == PublishMutation::ParallelSends
                    || !busy.contains(&(job.environment, job.destination)))
        });
        let Some(index) = due else { return };
        let job = &mut self.jobs[index];
        if job.status == Status::Running {
            self.stats.reclaims += 1;
        }
        job.status = Status::Running;
        job.holder = Some(u64::try_from(worker).unwrap_or(0));
        job.lease_generation += 1;
        job.attempts += 1;
        job.expires_ms = now + self.config.lease_ms;
        let (generation, destination, lease_generation) =
            (job.generation, job.destination, job.lease_generation);
        self.note(&format!(
            "w{worker} claimed g{generation}->d{destination} (lease {lease_generation})"
        ));
        self.workers[worker] = Some(Holding {
            job: index,
            lease_generation,
            phase: Phase::Check,
        });
    }

    fn advance(&mut self, worker: usize, mut holding: Holding) {
        let index = holding.job;
        match holding.phase.clone() {
            Phase::Check => {
                let job = &self.jobs[index];
                let newer = self.jobs.iter().any(|other| {
                    other.environment == job.environment
                        && other.destination == job.destination
                        && other.generation > job.generation
                        && other.status != Status::Superseded
                });
                if newer && self.mutation != PublishMutation::NoSupersedeCheck {
                    self.record(worker, &holding, Status::Superseded, None);
                    return;
                }
                holding.phase = Phase::Send;
            }
            Phase::Send => {
                // Never send so late that the lease could run out with the request still on its
                // way: a later holder must not race this one.
                if self.now_ms + self.config.attempt_ms > self.jobs[index].expires_ms {
                    self.stats.abandoned += 1;
                    self.note(&format!(
                        "w{worker} gave up g{}: too close to its lease's end",
                        self.jobs[index].generation
                    ));
                    return;
                }
                let outcome = self.send(index);
                holding.phase = Phase::Await {
                    until_ms: self.now_ms + self.config.attempt_ms,
                    outcome,
                };
            }
            Phase::Await { until_ms, outcome } => {
                if self.now_ms >= until_ms {
                    holding.phase = Phase::Record { outcome };
                }
            }
            Phase::Record { outcome } => {
                let job = &self.jobs[index];
                let entropy = self.rng.next_u64();
                let next = job::after_attempt(
                    &outcome,
                    job.attempts,
                    self.now_ms,
                    job.deadline_ms,
                    entropy,
                );
                let (status, retry_at) = match next {
                    Next::Succeeded => (Status::Succeeded, None),
                    Next::Failed => (Status::Failed, None),
                    Next::Retry { after_ms } => (
                        Status::Retrying,
                        Some(self.now_ms + i64::try_from(after_ms).unwrap_or(i64::MAX)),
                    ),
                };
                self.record(worker, &holding, status, retry_at);
                return;
            }
        }
        self.workers[worker] = Some(holding);
    }

    /// Sends the announcement over the network; the outcome the worker will see.
    fn send(&mut self, index: usize) -> Outcome {
        let job = self.jobs[index].clone();
        if self.chance(self.config.lose_request) {
            self.stats.lost_requests += 1;
            return Outcome::Retryable {
                reason: "timed out".to_owned(),
                retry_after_ms: None,
            };
        }
        let refused = self.chance(self.config.refuse);
        let copies = if self.chance(self.config.duplicate) {
            2
        } else {
            1
        };
        for _ in 0..copies {
            let delay = i64::try_from(
                self.rng
                    .below(usize::try_from(self.config.attempt_ms).unwrap_or(1)),
            )
            .unwrap_or(0);
            self.order += 1;
            self.network.push(InFlight {
                arrives_ms: self.now_ms + delay,
                order: self.order,
                environment: job.environment,
                destination: job.destination,
                generation: job.generation,
                key: job.key.clone(),
                refused,
            });
        }
        if refused {
            self.stats.refused += 1;
            return job::classify(503, None);
        }
        if self.chance(self.config.lose_answer) {
            self.stats.lost_answers += 1;
            return Outcome::Retryable {
                reason: "timed out".to_owned(),
                retry_after_ms: None,
            };
        }
        job::classify(204, None)
    }

    /// The conditional update: lands only for the current holder of the current lease.
    fn record(&mut self, worker: usize, holding: &Holding, status: Status, retry_at: Option<i64>) {
        let me = u64::try_from(worker).unwrap_or(0);
        let job = &self.jobs[holding.job];
        let current = job.status == Status::Running
            && job.holder == Some(me)
            && job.lease_generation == holding.lease_generation;
        if !current && self.mutation != PublishMutation::StaleLeaseFinalizes {
            self.stats.late_refused += 1;
            self.note(&format!(
                "w{worker} late result for g{} refused",
                job.generation
            ));
            return;
        }
        if !current {
            let detail = format!(
                "w{worker} recorded g{} {} without holding its lease",
                job.generation,
                status.as_str()
            );
            self.violation("S7", detail);
        }
        let job = &mut self.jobs[holding.job];
        job.status = status;
        job.holder = None;
        if let Some(at) = retry_at {
            job.next_attempt_ms = at;
        }
        if status == Status::Superseded {
            self.stats.superseded += 1;
        }
        let (generation, destination) = (job.generation, job.destination);
        self.note(&format!(
            "w{worker} recorded g{generation}->d{destination} {}",
            status.as_str()
        ));
    }

    // ---- the receiver ----

    fn deliver(&mut self) {
        let now = self.now_ms;
        let mut arriving: Vec<InFlight> = Vec::new();
        self.network.retain(|message| {
            if message.arrives_ms <= now {
                arriving.push(message.clone());
                false
            } else {
                true
            }
        });
        arriving.sort_by_key(|message| (message.arrives_ms, message.order));
        for message in arriving {
            if message.refused {
                continue;
            }
            let seen = self
                .received
                .entry((message.environment, message.destination))
                .or_default();
            let newest = seen.iter().map(|(generation, _)| *generation).max();
            let repeat = seen.iter().any(|(_, key)| *key == message.key);
            seen.push((message.generation, message.key.clone()));
            self.stats.received += 1;
            if repeat {
                self.stats.duplicates += 1;
            }
            self.note(&format!(
                "d{} received g{} ({})",
                message.destination,
                message.generation,
                if repeat { "repeat" } else { "new" }
            ));
            if let Some(newest) = newest
                && newest > message.generation
            {
                let detail = format!(
                    "destination {} of environment {} received g{} after g{newest}",
                    message.destination, message.environment, message.generation
                );
                self.violation("S6", detail);
            }
        }
    }

    // ---- invariants ----

    fn check(&mut self) {
        let mut found: Vec<(&'static str, String)> = Vec::new();
        let queued: BTreeSet<(usize, i64, u64)> = self
            .jobs
            .iter()
            .map(|job| (job.environment, job.generation, job.destination))
            .collect();
        for (index, env) in self.environments.iter().enumerate() {
            let mut bases: Vec<i64> = env.releases.iter().map(|release| release.base).collect();
            bases.sort_unstable();
            if bases.windows(2).any(|pair| pair[0] == pair[1]) {
                found.push((
                    "G",
                    format!("environment {index}: two releases committed on one base"),
                ));
            }
            let mut keys: Vec<u64> = env.releases.iter().map(|release| release.key).collect();
            keys.sort_unstable();
            if keys.windows(2).any(|pair| pair[0] == pair[1]) {
                found.push((
                    "S2",
                    format!("environment {index}: a publish was applied twice"),
                ));
            }
            let contiguous = env
                .releases
                .iter()
                .enumerate()
                .all(|(n, release)| release.generation == i64::try_from(n).unwrap_or(-1) + 1);
            if !contiguous || env.generation != i64::try_from(env.releases.len()).unwrap_or(-1) {
                found.push((
                    "S3",
                    format!(
                        "environment {index}: generations aren't 1..{}",
                        env.releases.len()
                    ),
                ));
            }
            for (entry, history) in &env.history {
                if !release::well_formed(history) {
                    found.push((
                        "S4",
                        format!(
                            "environment {index}, entry {entry}: overlapping publication intervals"
                        ),
                    ));
                }
            }
            for release in &env.releases {
                for destination in 0..self.config.destinations {
                    if !queued.contains(&(index, release.generation, destination)) {
                        found.push(("O", format!("environment {index}: release {} has no announcement for destination {destination}", release.generation)));
                    }
                }
            }
        }
        for (invariant, detail) in found {
            if !self
                .violations
                .iter()
                .any(|seen| seen.invariant == invariant && seen.detail == detail)
            {
                self.violation(invariant, detail);
            }
        }
    }

    fn check_end(&mut self) {
        let unfinished: Vec<String> = self
            .jobs
            .iter()
            .filter(|job| !job.status.is_final())
            .map(|job| {
                format!(
                    "e{} g{} d{} {}",
                    job.environment,
                    job.generation,
                    job.destination,
                    job.status.as_str()
                )
            })
            .collect();
        if !unfinished.is_empty() {
            self.violation(
                "L",
                format!(
                    "jobs still unfinished after the drain: {}",
                    unfinished.join(", ")
                ),
            );
        }
        let mut missing = Vec::new();
        for (index, _) in self.environments.iter().enumerate() {
            for destination in 0..self.config.destinations {
                let latest = self
                    .jobs
                    .iter()
                    .filter(|job| job.environment == index && job.destination == destination)
                    .max_by_key(|job| job.generation);
                if let Some(job) = latest
                    && job.status == Status::Succeeded
                    && !self
                        .received
                        .get(&(index, destination))
                        .is_some_and(|seen| seen.iter().any(|(_, key)| *key == job.key))
                {
                    missing.push(format!("e{index} d{destination} g{}", job.generation));
                }
            }
        }
        if !missing.is_empty() {
            self.violation(
                "L",
                format!("succeeded but never received: {}", missing.join(", ")),
            );
        }
    }
}
