<script>
	import Status from '#lib/components/Status.svelte';
</script>

The bugs that matter most in a publishing system are timing bugs: an answer lost after the effect happened, two workers finishing the same job, an old build landing after a new one. They are rare in normal testing and nearly impossible to reproduce from a bug report. `crates/colophon-sim` runs Colophon's real rules under thousands of seeded schedules with faults injected, and turns any violation into a number that replays it exactly.

## How it works

- **One seed decides everything.** A seeded `SplitMix64` generator and a single-threaded scheduler pick every step, so the same seed gives the same run, step for step.
- **The real rules.** The domain crate is synchronous and does no I/O, so the simulator drives the same functions the server uses, with a virtual clock and an in-memory model of the store.
- **Actors that behave like the real clients**: editors that save like the Studio (expected revisions, idempotency keys, a device copy), publishers racing on the release they saw, workers that claim under leases and crash at any step, and a receiver behind a network that loses requests and answers, duplicates and refuses.
- **Invariants after every step.** A run stops at the first broken one and prints its seed.
- **No dependencies.** A few hundred lines anyone can read, rather than a framework, because this is the one crate whose determinism matters most.

## Replaying a failure

```sh runnable
cargo run -p colophon-sim -- --seed 42 --trace
```

`--trace` prints every step. A sweep runs many seeds and stops at the first failure:

```sh runnable
cargo run --release -p colophon-sim -- --seeds 2000
cargo run --release -p colophon-sim -- --scenario publishing --seeds 1000
```

The first is the editing scenario (saves, conflicts, retries, crashes); the second covers releases, the outbox and leased workers. CI and `scripts/check.sh` run exactly these two sweeps on every push. Exit codes: 0 when every run held, 1 when one failed (with the seed to replay), 2 for a usage error.

## It has teeth

A simulator that never fails proves nothing, so each scenario has **planted bugs** that must be caught. `--mutation NAME` turns one on:

```sh runnable
cargo run -p colophon-sim -- --seeds 200 --mutation skip-base-check
cargo run -p colophon-sim -- --scenario publishing --seeds 200 --mutation parallel-sends
```

With a planted bug, the sweep should fail: it exits 1 and prints the seed that caught it. Editing has four (`skip-base-check`, `forget-keys`, `leak-across-projects`, `new-key-after-crash`); publishing has eight (`ignore-base`, `keep-open-interval`, `stale-lease-finalizes`, `parallel-sends`, `no-supersede-check`, `outbox-after-commit`, `publish-forgets-key`, `sends-late`). The crate's tests fail if any of them goes uncaught (each within 200 seeds), and a differential test replays runs against real PostgreSQL to keep the model honest.

## What it found

- **A double save in the Studio.** After a reload, the Studio resent a kept edit with a new idempotency key, so a save that had already committed could be stored twice. The device copy now keeps the request in flight and resends it unchanged (D-038, D-039).
- **Two announcements in flight to one destination.** While one worker was still sending release N, another could claim and send N+1, and a slow network could deliver them out of order. A destination now has at most one announcement in flight (D-047).

## The honest boundary

The simulator models PostgreSQL and the network; it doesn't run them. That is what the real tests are for: the crash tests kill the real binary at named points, and the crash loop kills the whole server with `SIGKILL` at random moments against the real stack ([Crash tests](/operations/crash-tests/)).

Still to come: delivery, cache and evidence faults in the simulator, and a checker that no reader saw a mixed release <Status planned="P3" />; restore and recovery epochs <Status planned="P6" />.

## Design

[Module 10, sim](/design/modules/10-sim/); decisions D-005, D-039 and D-047 in the [decisions](/design/decisions/). On GitHub: [docs/modules/10-sim.md](https://github.com/akira-shirase-oss/colophon-cms/blob/main/docs/modules/10-sim.md).
