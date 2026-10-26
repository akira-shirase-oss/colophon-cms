# 10 · sim (`crates/colophon-sim`, `tests/crash-loop`)

> The proof harness: deterministic simulation, a real-process crash loop and a consistency checker. Rare races become
> reproducible seeds.

| | |
|---|---|
| Milestones | P1 (skeleton, save invariants), P2 (releases, outbox, worker; first crash loop), P3 (delivery, cache, evidence; consistency checker), P4 (restore and epochs), P6 (nightly scale, recorded demo) |
| Proposed owner | Claude builds the harness; the maintainer defines and reviews the invariants |
| Depends on | domain (all state machines and ports); fault-injecting port implementations; Docker Compose for the crash loop |
| Used by | CI (every pull request and nightly), contributors (seed replay), the README demo |
| Spec | §08, §14, §16, §30 (fault suite); Colophon decision D-005 |

## Why it exists

The bugs that matter most in a publishing system are timing bugs:
- a response lost after the effect happened;
- two workers finishing the same job;
- an old build landing after a new one.

They are rare in normal testing and almost impossible to reproduce from a bug report. A deterministic simulator runs
the real domain logic under thousands of seeded schedules with injected faults, and turns any violation into a seed
that replays exactly. This is Colophon's signature feature (D-005): evidence instead of claims.

## End goal

- CI runs a fixed budget of seeded simulations on every pull request, across save, review, release, delivery, worker and
  restore flows.
- Any invariant violation prints a seed that replays exactly.
- A deliberately broken rule is always caught.
- A real-process crash loop against PostgreSQL passes, and its recording is the README's headline demo.

## Responsibilities

- The scheduler, virtual time and seeded randomness.
- In-memory store models and fault-injecting adapters.
- Workload actors and crash injection.
- Invariant checkers.
- Seed replay and regression seeds.
- The crash loop and the consistency checker.

## Not its job

- Replacing real-database tests. Postgres semantics are *modelled* here and verified separately (see "Honest boundary").
- Load testing ([verification](17-verification.md) owns k6).
- Testing the UI.

## Breakdown

### 1. Deterministic simulator
- **Scheduler:** single-threaded, so it decides the order of every step. All randomness comes from one seeded PRNG,
  so the same seed always produces the same run.
- **Virtual clock:** implements the domain's `Clock` port. Timeouts, leases, schedules, daylight-saving transitions and
  expiry are tested without waiting.
- **Durable store model:** an in-memory implementation of the domain's store ports. Commits are atomic (all or
  nothing), and conflicting commits behave like Postgres row locks and conditional updates.
- **Volatile state** (in-flight requests, worker memory) is discarded when an actor crashes; durable state survives.
- **Simulated external systems:** CDN, object store, site host and webhook receiver. Each can independently:
  - drop a request;
  - drop a response *after* the effect happened;
  - delay, duplicate or reorder messages;
  - return 429 or 5xx;
  - time out.

### 2. Actors and workload

| Actor | Behaviour |
|---|---|
| Editors | Save with expected revisions, collide, retry with idempotency keys |
| Reviewers | Approve or request changes on exact changesets; approvals expire |
| Publishers | Commit releases, roll back, unpublish, schedule |
| Readers | Perform pinned multi-read sequences and live reads |
| Workers | Claim, execute, renew, finalize; crash at any step |
| Agents | Submit bulk manifests and get interrupted |
| Operator | Restores a backup, starting a new epoch and replaying the deletion ledger |

Crash injection can stop any actor before or after any step, including between an external effect and its record.

### 3. Invariant checkers (run after every step and at the end)

| ID | Checked invariant |
|---|---|
| S1 | No acknowledged save is ever lost. |
| S2 | No logical release is applied twice. |
| S3 | The generation increases monotonically within an epoch. |
| S4 | Publication intervals for an entry and locale never overlap. |
| S5 | A pinned read sequence never mixes generations. |
| S6 | A superseded delivery is never promoted. |
| S7 | Only the current lease holder finalizes a job. |
| S8 | No read crosses a project boundary. |
| S9 | An uncertain outcome is never relabelled success without reconciliation evidence. |

These mirror domain invariants D1–D11 ([domain](01-domain.md)) and worker invariants W1–W7 ([worker](05-worker.md)).

### 4. Replay and regression
- `cargo run -p colophon-sim -- --seed <seed> --trace` replays a run and prints every step.
- Failing seeds are saved as regression fixtures and run forever after.
- **Budgets:** pull requests run a fixed seed count within a time budget; nightly runs a much larger sweep.

### 5. Making sure the simulator has teeth
- **Determinism test:** the same seed produces the same trace hash, run after run.
- **Mutation checks:** deliberately break a rule (for example, skip the `expected_revision` comparison, or let a stale
  lease finalize) and require the simulator to find a violating seed within the budget. A simulator that cannot catch
  a planted bug proves nothing.

### 6. Honest boundary and differential tests
- The simulator models Postgres semantics; it does not run Postgres.
- To limit the gap, the same scenario scripts also run against the real `colophon-db` adapter in integration tests
  (differential testing). The results must agree.
- The docs and the README state this boundary plainly.

### 7. Real-process crash loop (`tests/crash-loop`)
- **Setup:** a Compose stack with PostgreSQL, `colophon-server` in the API and worker roles, and a fake CDN and site
  host.
- **Workload:** a client performs saves, publishes and pinned reads, and records every acknowledged operation.
- **Faults:** a controller sends `kill -9` to the API or worker at random intervals and restarts them.
- **Check:** after N iterations, a checker compares the client's acknowledgement log with the database state and the
  delivery evidence.
- The recorded run is the README's headline demo (P3) and is repeated before v0.1.

### 8. Consistency checker
- Runs concurrent publishes while readers perform pinned and live reads.
- Verifies that no reader sees mixed generations and that each reader's observed generations never go backwards within
  an epoch.

## Invariants (of the harness itself)

| ID | Invariant |
|---|---|
| S10 | All nondeterminism (time, randomness, scheduling, faults) comes from the seed. |
| S11 | A failing seed reproduces exactly on any machine with the same commit. |
| S12 | Every planted mutation in the mutation suite is detected. |

## Interfaces

- **Library:** scenario builder, actors, fault profiles, checkers.
- **Binary:** `colophon-sim --seed --seeds --budget --trace --scenario`.
- **Crash loop:** a script plus a checker binary, run in CI on `main` and before releases.

## Failure modes

| Situation | Behaviour |
|---|---|
| Invariant violated | Print the seed, scenario and minimal trace; fail CI; save the seed as a fixture |
| Nondeterminism detected (trace hash differs) | Fail loudly; the harness is broken until fixed |
| Model and real database disagree | Differential test fails; the model or the adapter is fixed before merging |
| Crash loop finds a lost acknowledgement | Release blocker (stop condition) |

## Tests and evidence

- The determinism, mutation and differential tests above.
- CI artifacts: seed counts, durations, violations found, and regression fixtures.
- Crash-loop reports with iteration counts, kill points and checker output.
- Covers fault cases F01–F05, F12-style duplicate handling for delivery events, F13 and F14 in simulated form, plus
  their real counterparts in [verification](17-verification.md).

## Build order

1. **P1:** scheduler, store model, editor actors, S1, S8, determinism test (step 1.4, done: D-039). Built:
   `crates/colophon-sim` (a `SplitMix64` seed, a single-threaded scheduler, the store model, editors that collide,
   retry, crash and reopen like the Studio's saver), the checks S1 (no acknowledged save lost or stored twice), S8 and
   K (no key reused for a different request), four mutations each caught on the first seed, and
   `crates/colophon-db/tests/differential.rs`, which replays runs against Postgres and requires the same answers. The
   virtual clock arrives with leases and schedules (P2), the first thing that needs time.
2. **P2:** releases, outbox, workers, faulty adapters, S2–S4, S6, S7; first crash loop.
3. **P3:** delivery, cache and evidence, S5, S9; the consistency checker; recorded demo.
4. **P4:** operator restore with new epochs; deletion-ledger replay.
5. **P6:** nightly sweeps, mutation suite complete, demo re-recorded for v0.1.

## Open decisions

- The nightly sweep's size, once there is a nightly job. Pull requests run 2,000 seeds × 400 steps (D-039).
- O-011 (hand-written scheduler) is closed by D-039, and O-005 (synchronous ports) by D-028.
