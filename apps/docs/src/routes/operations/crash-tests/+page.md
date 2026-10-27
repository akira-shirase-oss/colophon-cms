<script>
	import Status from '#lib/components/Status.svelte';
</script>

Colophon's first promise is that killing it at any moment loses nothing and doubles nothing. Three kinds of test check that, from precise to brutal: named fault points in the real binary, a crash loop that kills the whole server at random, and a simulator that explores thousands of interleavings on a virtual clock. All of them except the simulator need the [local stack](/operations/local-stack/) and the migrations:

```sh runnable
scripts/stack.sh up
scripts/migrate.sh
```

## Named fault points

Killing a process from outside at the exact moment between a commit and its answer is racy. So a build with the `fault-injection` Cargo feature exits, with code 86 and no response, at a named point when `COLOPHON_FAULT` names it. A normal build doesn't contain the points at all (D-036).

| Point | Where |
|---|---|
| `entry.create:before-commit`, `entry.create:after-commit` | creating an entry, either side of its commit |
| `entry.save:before-commit`, `entry.save:after-commit` | saving a draft, either side of its commit |
| `worker:after-dispatch` | the worker, after a delivery was sent and before its outcome is recorded |

`crates/colophon-server/tests/faults.rs` starts the real server with one of them, sends the request, sees the process die, starts it again and checks the result:

```sh runnable
cargo test -p colophon-server --features fault-injection --test faults -- --ignored --test-threads=1
```

- **F01**, killed before a save commits: no success was acknowledged, nothing was stored, and a retry with the same key works.
- **F02**, killed after the commit, before the answer: the save or create is stored once, and the retry with the same key returns the same result instead of writing again or failing.
- **F03**, the worker killed after announcing a release, before recording it: the lease runs out, another claim announces it again with the same idempotency key, and the job succeeds after two attempts. Nothing claims the first announcement didn't happen.

## The crash loop

`crates/colophon-server/tests/crash_loop.rs` publishes articles in two projects, each with a webhook, while the whole server (API and worker together) is killed with `SIGKILL` at random moments and started again. Its worker runs with short leases (1.5 s), attempts (0.5 s) and polls (50 ms) so killed work comes back within the test. Requests that got no answer are sent again with the same keys, as a real client would. Then a checker compares what the client was told with the database, the public API and what the receiver got:

- every acknowledged publish is a release, at the generation the client was told;
- each project's generations run 1 to N, each publish request published once;
- no entry's publication history overlaps;
- the receiver never got an older release after a newer one, and got the latest;
- every announcement finished delivered or superseded;
- delivery at each generation serves exactly what was published by then.

```sh runnable
cargo test -p colophon-server --test crash_loop -- --ignored
```

It kills 12 times by default. `COLOPHON_CRASH_LOOP_KILLS` sets more, and `COLOPHON_CRASH_LOOP_SEED` (default 2400) replays a run's kill timing; each run prints its seed. The server's own timing still varies, so a seed repeats the kills, not every interleaving. CI runs it with every other test that needs the stack:

```sh runnable
cargo test --workspace -- --ignored --test-threads=1
```

## The simulator

The [deterministic simulator](/concepts/simulator/) explores far more interleavings than real processes can, on a virtual clock, with lost requests, lost answers and crashes, and replays any failure from its seed. CI runs both scenarios on every push:

```sh runnable
cargo run --release -p colophon-sim -- --seeds 2000
cargo run --release -p colophon-sim -- --scenario publishing --seeds 1000
```

A failing seed exits 1 and prints the command that replays it, with `--seed N` and `--trace`.

## Every check CI runs

`scripts/check.sh` runs what CI runs, in the same order. With the local stack up it also runs the integration half on a fresh `colophon_check` database: every ignored test above, the fault tests, the testbed and the Studio in Chromium, and short load runs. Contributors run it before every push:

```sh runnable
scripts/check.sh
```

`QUICK=1` skips the integration half.

## The fault matrix

The full list is in [module 17, verification](/design/modules/17-verification/). Where each case stands:

| Case | What happens | Expected | Evidence | Status |
|---|---|---|---|---|
| F01 | Server killed before a content commit | No acknowledgement of an uncommitted version | `tests/faults.rs`, the simulator | <Status done="1.2" /> |
| F02 | Killed after commit, before the response | The retry returns the same result | `tests/faults.rs`, the simulator | <Status done="1.2" /> |
| F03 | Worker killed after a send, before recording it | Safe retry with the same key; no duplicate claimed impossible | `tests/faults.rs` | <Status done="2.3" /> |
| F04 | Two editors save the same base version | One conflicts; no accepted edit lost | `colophon-db` `tests/entries.rs`, the Studio's `editing.spec.ts`, the simulator | <Status done="1.3" /> |
| F05 | An old delivery runs after a newer release | Blocked, recorded as superseded | `tests/worker.rs`, the publishing simulation | <Status done="2.3" /> |
| F06 | A collection gains a new matching article | Cached lists show it | `tests/cache.rs` | <Status done="3.1" /> |
| F07 | An entry is unpublished while old caches exist | The origin denies it; purges tracked; remaining exposure visible | `tests/cache.rs`, `tests/purges.rs` | <Status done="3.2" /> |
| F08 | Cache eviction, failover or restart | Nothing lost | `tests/cache.rs` | <Status done="3.1" /> |
| F09 | Outbox or disk budget exhausted | New work fails before it is accepted | | <Status planned="P6" /> |
| F10 | Migration interrupted, or a schema edited concurrently | Checkpointed recovery or a conflict | | <Status planned="P6" /> |
| F11 | A model produces invalid or hostile output | No unsafe action; the reviewer sees a rejected suggestion | | <Status planned="P6" /> |
| F12 | Duplicate or reordered external events | Exactly one logical effect | | <Status planned="P6" /> |
| F13 | A backup restored with an old outbox | A new recovery epoch; reconcile before any resend | | <Status planned="P6" /> |
| F14 | A deleted project reappears in a restored backup | The deletion ledger is reapplied first | | <Status planned="P6" /> |

The `tests/` paths are under `crates/colophon-server/` unless named otherwise. F10 has a partial answer today: each migration runs in one transaction, so an interrupted one leaves nothing behind ([Upgrades and migrations](/operations/upgrades/)); the checkpointed recovery the case asks for is P6.

## Not built yet

- The simulator's delivery and cache faults, and the checker that no reader ever saw a mixed release, which P3's proof gate needs. <Status planned="3.8" />
- A recorded crash-loop demo, part of the first public demo. <Status planned="3.8" />
