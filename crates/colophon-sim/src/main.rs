//! `colophon-sim`: run seeds, replay one, or check that a planted bug is caught.
//!
//! ```text
//! colophon-sim --seed 42 [--trace]          one run; --trace prints every step
//! colophon-sim --seeds 1000 [--from 1]      a sweep; stops at the first failing seed
//! ... [--steps 400] [--mutation NAME]       NAME: none, skip-base-check, forget-keys,
//!                                           leak-across-projects, new-key-after-crash
//! ```
//!
//! Exits 0 when every run held its invariants, 1 when one failed (printing the seed to replay),
//! and 2 for a usage error.

use std::process::ExitCode;

use colophon_sim::{Config, Mutation, Report, run};

struct Args {
    from: u64,
    seeds: u64,
    trace: bool,
    config: Config,
    mutation: Mutation,
}

fn number<T: std::str::FromStr>(name: &str, value: Option<String>) -> Result<T, String> {
    value
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| format!("{name} needs a number"))
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args {
        from: 1,
        seeds: 1,
        trace: false,
        config: Config::default(),
        mutation: Mutation::None,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seed" => {
                parsed.from = number("--seed", args.next())?;
                parsed.seeds = 1;
            }
            "--seeds" => parsed.seeds = number("--seeds", args.next())?,
            "--from" => parsed.from = number("--from", args.next())?,
            "--steps" => parsed.config.steps = number("--steps", args.next())?,
            "--trace" => parsed.trace = true,
            "--mutation" => {
                let name = args.next().unwrap_or_default();
                parsed.mutation =
                    Mutation::parse(&name).ok_or_else(|| format!("unknown mutation {name:?}"))?;
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(parsed)
}

fn print_trace(report: &Report) {
    for line in &report.trace {
        println!("{line}");
    }
}

fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(problem) => {
            eprintln!("colophon-sim: {problem}");
            eprintln!(
                "usage: colophon-sim [--seed N | --seeds N [--from N]] [--steps N] [--trace] [--mutation NAME]"
            );
            return ExitCode::from(2);
        }
    };
    let mut totals = colophon_sim::Stats::default();
    for seed in args.from..args.from.saturating_add(args.seeds) {
        let report = run(seed, &args.config, args.mutation);
        if args.trace {
            print_trace(&report);
        }
        if let Some(violation) = report.violations.first() {
            eprintln!(
                "seed {seed} failed {} at step {}: {}",
                violation.invariant, violation.step, violation.detail
            );
            eprintln!(
                "replay: cargo run -p colophon-sim -- --seed {seed} --steps {} --mutation {} --trace",
                args.config.steps,
                args.mutation.name()
            );
            return ExitCode::from(1);
        }
        let stats = report.stats;
        totals.acknowledged += stats.acknowledged;
        totals.replayed += stats.replayed;
        totals.stale += stats.stale;
        totals.lost_requests += stats.lost_requests;
        totals.lost_answers += stats.lost_answers;
        totals.crashes += stats.crashes;
        totals.probes += stats.probes;
        if args.seeds == 1 {
            println!("seed {seed}: trace {:016x}", report.trace_hash);
        }
    }
    println!(
        "{} seed(s) x {} steps held S1, S8 and K (mutation: {}): {} acknowledged saves, {} replayed, {} stale, {} lost requests, {} lost answers, {} crashes, {} probes",
        args.seeds,
        args.config.steps,
        args.mutation.name(),
        totals.acknowledged,
        totals.replayed,
        totals.stale,
        totals.lost_requests,
        totals.lost_answers,
        totals.crashes,
        totals.probes
    );
    ExitCode::SUCCESS
}
