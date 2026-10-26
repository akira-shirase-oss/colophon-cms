//! The simulator's own guarantees (docs/modules/10-sim.md, S10 to S12): runs are deterministic,
//! a sweep of seeds holds every invariant while actually exercising the faults, every planted
//! bug is caught, and every regression seed keeps passing.

use colophon_sim::{Config, Mutation, run};

/// Seeds per sweep in a test build; the CI sweep runs the release binary on more.
const SWEEP: u64 = 300;

#[test]
fn the_same_seed_gives_the_same_run() {
    let config = Config::default();
    let (first, again) = (
        run(42, &config, Mutation::None),
        run(42, &config, Mutation::None),
    );
    assert_eq!(first.trace, again.trace);
    assert_eq!(first.trace_hash, again.trace_hash);
    assert_eq!(first.log, again.log);

    let distinct: std::collections::BTreeSet<u64> = (1..=20)
        .map(|seed| run(seed, &config, Mutation::None).trace_hash)
        .collect();
    assert_eq!(
        distinct.len(),
        20,
        "different seeds explore different schedules"
    );
}

#[test]
fn a_sweep_holds_every_invariant_and_exercises_every_fault() {
    let config = Config::default();
    let mut totals = colophon_sim::Stats::default();
    for seed in 1..=SWEEP {
        let report = run(seed, &config, Mutation::None);
        assert!(
            report.violations.is_empty(),
            "seed {seed}: {:?}\nreplay: cargo run -p colophon-sim -- --seed {seed} --trace",
            report.violations.first()
        );
        let stats = report.stats;
        totals.acknowledged += stats.acknowledged;
        totals.replayed += stats.replayed;
        totals.stale += stats.stale;
        totals.lost_requests += stats.lost_requests;
        totals.lost_answers += stats.lost_answers;
        totals.crashes += stats.crashes;
        totals.probes += stats.probes;
    }
    // A sweep that never lost an answer or hit a conflict would prove nothing.
    for (what, count) in [
        ("acknowledged saves", totals.acknowledged),
        ("replayed answers", totals.replayed),
        ("stale saves", totals.stale),
        ("lost requests", totals.lost_requests),
        ("lost answers", totals.lost_answers),
        ("crashes", totals.crashes),
        ("cross-project probes", totals.probes),
    ] {
        assert!(count >= 100, "only {count} {what} in {SWEEP} seeds");
    }
}

#[test]
fn every_planted_bug_is_caught() {
    let config = Config::default();
    for mutation in Mutation::ALL {
        let caught = (1..=200).find(|seed| !run(*seed, &config, mutation).violations.is_empty());
        assert!(caught.is_some(), "{} survived 200 seeds", mutation.name());
    }
}

#[test]
fn each_planted_bug_breaks_the_invariant_it_should() {
    let config = Config::default();
    let first = |mutation| {
        (1..=200)
            .find_map(|seed| run(seed, &config, mutation).violations.into_iter().next())
            .map(|violation| (violation.invariant, violation.detail))
    };
    let expect = |mutation, invariant: &str, words: &str| {
        let found = first(mutation);
        assert!(
            found
                .as_ref()
                .is_some_and(|(id, detail)| *id == invariant && detail.contains(words)),
            "{mutation:?}: {found:?}"
        );
    };
    expect(Mutation::SkipBaseCheck, "S1", "overwritten");
    expect(Mutation::ForgetKeys, "S1", "stored twice");
    expect(Mutation::LeakAcrossProjects, "S8", "another project");
    expect(Mutation::NewKeyAfterCrash, "S1", "stored twice");
}

#[test]
fn regression_seeds_still_pass() {
    let list = include_str!("../regressions.txt");
    for line in list
        .lines()
        .filter(|line| !line.trim_start().starts_with('#') && !line.trim().is_empty())
    {
        let mut fields = line.split_whitespace();
        let seed: u64 = fields
            .next()
            .and_then(|f| f.parse().ok())
            .unwrap_or_default();
        let steps: usize = fields.next().and_then(|f| f.parse().ok()).unwrap_or(400);
        let config = Config {
            steps,
            ..Config::default()
        };
        let report = run(seed, &config, Mutation::None);
        assert!(
            report.violations.is_empty(),
            "regression seed {seed}: {:?}",
            report.violations
        );
    }
}
