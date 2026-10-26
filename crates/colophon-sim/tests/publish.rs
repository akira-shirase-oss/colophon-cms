//! The publishing simulation's own guarantees (docs/modules/10-sim.md; D-047): runs are
//! deterministic, a sweep holds every invariant while actually exercising every fault, and every
//! planted bug is caught within the sweep.

use colophon_sim::{PublishConfig, PublishMutation, run_publishing};

/// Seeds per sweep in a test build; the CI sweep runs the release binary on more.
const SWEEP: u64 = 200;

#[test]
fn the_same_seed_gives_the_same_publishing_run() {
    let config = PublishConfig::default();
    let (first, again) = (
        run_publishing(7, &config, PublishMutation::None),
        run_publishing(7, &config, PublishMutation::None),
    );
    assert_eq!(first.trace, again.trace);
    assert_eq!(first.trace_hash, again.trace_hash);
    let distinct: std::collections::BTreeSet<u64> = (1..=20)
        .map(|seed| run_publishing(seed, &config, PublishMutation::None).trace_hash)
        .collect();
    assert_eq!(distinct.len(), 20, "different seeds explore different schedules");
}

#[test]
fn a_publishing_sweep_holds_every_invariant_and_exercises_every_fault() {
    let config = PublishConfig::default();
    let mut totals = colophon_sim::PublishStats::default();
    for seed in 1..=SWEEP {
        let report = run_publishing(seed, &config, PublishMutation::None);
        assert!(
            report.violations.is_empty(),
            "seed {seed}: {:?}\nreplay: cargo run -p colophon-sim -- --scenario publishing --seed {seed} --trace",
            report.violations.first()
        );
        let stats = report.stats;
        totals.commits += stats.commits;
        totals.moved += stats.moved;
        totals.replayed += stats.replayed;
        totals.received += stats.received;
        totals.duplicates += stats.duplicates;
        totals.lost_requests += stats.lost_requests;
        totals.lost_answers += stats.lost_answers;
        totals.refused += stats.refused;
        totals.crashes += stats.crashes;
        totals.reclaims += stats.reclaims;
        totals.superseded += stats.superseded;
        totals.late_refused += stats.late_refused;
        totals.abandoned += stats.abandoned;
    }
    // A sweep that never raced, crashed or lost an answer would prove nothing.
    for (what, count) in [
        ("committed releases", totals.commits),
        ("publishes refused on a moved base", totals.moved),
        ("replayed publishes", totals.replayed),
        ("announcements received", totals.received),
        ("repeated announcements", totals.duplicates),
        ("lost requests", totals.lost_requests),
        ("lost answers", totals.lost_answers),
        ("503s", totals.refused),
        ("worker crashes", totals.crashes),
        ("reclaimed leases", totals.reclaims),
        ("superseded announcements", totals.superseded),
        ("late results refused", totals.late_refused),
    ] {
        assert!(count > 0, "the sweep never produced any {what}");
    }
}

#[test]
fn every_planted_publishing_bug_is_caught() {
    let config = PublishConfig::default();
    for mutation in PublishMutation::ALL {
        let caught = (1..=SWEEP).find_map(|seed| {
            let report = run_publishing(seed, &config, mutation);
            report.violations.first().map(|violation| (seed, violation.invariant))
        });
        assert!(caught.is_some(), "{} was never caught in {SWEEP} seeds", mutation.name());
        if let Some((seed, invariant)) = caught {
            println!("{}: caught by {invariant} at seed {seed}", mutation.name());
        }
    }
}
