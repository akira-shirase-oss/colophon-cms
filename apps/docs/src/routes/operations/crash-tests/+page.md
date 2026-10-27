<script>
	import Status from '#lib/components/Status.svelte';
</script>

Colophon's first promise is that killing it at any moment loses nothing and doubles nothing. Three kinds of test check that, from precise to brutal. All of them need the [local stack](/operations/local-stack/) and the migrations (`scripts/migrate.sh`), except the simulator, which needs nothing.

## Named fault points

Killing a process from outside at the exact moment between a commit and its answer is racy. So a build with the `fault-injection` Cargo feature exits at a named point when `COLOPHON_FAULT` names it (for example `entry.save:after-commit`); a normal build has no way to exit on request. `crates/colophon-server/tests/faults.rs` kills the real server there, restarts it, and checks the result:

```sh runnable
cargo test -p colophon-server --features fault-injection --test faults -- --ignored --test-threads=1
```

- **F01**, killed before a save commits: no success was acknowledged, nothing was stored, and a retry with the same key works.
- **F02**, killed after the commit, before the answer: the save is stored once, and the retry returns the same result.
- **F03**, the worker killed after announcing a release, before recording it: the lease runs out, another claim announces it again with the same idempotency key, and the job succeeds after two attempts.

## The crash loop

`crates/colophon-server/tests/crash_loop.rs` publishes articles in two projects, each with a webhook, while the whole server (API and worker together) is killed with `SIGKILL` at random moments and started again. Requests that got no answer are resent with the same keys, as a real client would. Then a checker compares what the client was told with the database, the public API and what the receiver got: every acknowledged publish is a release at the generation the client was told; each project's generations run 1 to N with each changeset published once; no publication history overlaps; the receiver never got an older release after a newer one and got the latest; every announcement finished delivered or superseded; delivery at each generation serves exactly what was published by then.

```sh runnable
cargo test -p colophon-server --test crash_loop -- --ignored
```

It kills 12 times by default. `COLOPHON_CRASH_LOOP_KILLS` sets more, and `COLOPHON_CRASH_LOOP_SEED` replays the kill timing of a run (each run prints its seed). CI runs it with every other test that needs the stack:

```sh runnable
cargo test --workspace -- --ignored --test-threads=1
```

## The simulator

The [deterministic simulator](/concepts/simulator/) explores far more interleavings than real processes can, on a virtual clock, and replays any failure from its seed:

```sh runnable
cargo run --release -p colophon-sim -- --seeds 2000
cargo run --release -p colophon-sim -- --scenario publishing --seeds 1000
```

## The fault matrix

The full list is in [module 17, verification](/design/modules/17-verification/). Where each case stands:

| Case | What happens | Expected | Status |
|---|---|---|---|
| F01 | Server killed before a content commit | No acknowledgement of an uncommitted revision | <Status implemented /> |
| F02 | Killed after commit, before the response | The retry returns the same result | <Status implemented /> |
| F03 | Worker killed after a send, before recording it | Safe retry with the same key; no duplicate claimed impossible | <Status implemented /> |
| F04 | Two editors save the same base revision | One conflicts; no accepted edit lost | <Status implemented /> |
| F05 | An old delivery runs after a newer release | Blocked, recorded as superseded | <Status implemented /> |
| F06 | A collection gains a new matching article | Cached lists show it | <Status implemented /> |
| F07 | An entry is unpublished while old caches exist | The origin denies it; purges tracked; remaining exposure visible | <Status implemented /> |
| F08 | Cache eviction, failover or restart | Nothing lost | <Status implemented /> |
| F09 | Outbox or disk budget exhausted | New work fails before it is accepted | <Status planned="P6" /> |
| F10 | Migration interrupted, or a schema edited concurrently | Checkpointed recovery or a conflict | <Status planned="P6" /> |
| F11 | A model produces invalid or hostile output | No unsafe action; the reviewer sees a rejected suggestion | <Status planned="P6" /> |
| F12 | Duplicate or reordered external events | Exactly one logical effect | <Status planned="P6" /> |
| F13 | A backup restored with an old outbox | A new recovery epoch; reconcile before any resend | <Status planned="P6" /> |
| F14 | A deleted project reappears in a restored backup | The deletion ledger is reapplied first | <Status planned="P6" /> |

Not yet: the simulator's delivery and cache faults, and the checker that no reader ever saw a mixed release, which P3's proof gate needs <Status planned="P3" />; a recorded crash-loop demo <Status planned="P3" />.
