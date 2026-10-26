# 05 · worker (`crates/colophon-worker`)

> Durable background work. Every committed intent is eventually done, superseded, or shown as failed or uncertain,
> never silently lost and never doubled behind a false success.

| | |
|---|---|
| Milestones | P2 (dispatcher, leases, retries, one destination), P3 (CDN purge, probes, site adapter), P4 (scheduled releases, image pool), P5 (AI and bulk pools) |
| Proposed owner | Claude builds; publishing transitions and provider adapters get a manual review |
| Depends on | domain (job, delivery and schedule state machines, `DestinationAdapter`), db (outbox, leases), `tokio`, `tokio-util` |
| Used by | colophon-server `--role worker` |
| Spec | §14, §16, §25, §33 |

## Built so far

Step 2.3 (D-046): `crates/colophon-worker`, run by `colophon-server --role worker` or `all`.

- **The outbox:** committing a release inserts one job per enabled destination in the same transaction, and
  supersedes that destination's older jobs that haven't started (migration 0009).
- **Claims and leases:** `colophon_db::jobs::claim` fails jobs past their one-hour lifetime, then claims due jobs and
  running ones whose lease ran out, with `FOR UPDATE SKIP LOCKED`, raising the lease generation each time; `finish`
  lands only for the current owner and lease generation. All of it runs as `colophon_worker`.
- **Attempts:** a supersession check, then a `release.committed` POST with `Idempotency-Key` and
  `Colophon-Generation`, a 10-second deadline, no redirects and no transaction open; the outcome follows
  `colophon_domain::job` (2xx succeeds; 408, 425, 429, 500, 502, 503 and 504 retry with full-jitter backoff up to five
  attempts; the rest fail).
- **Addresses:** https and public only (`colophon_domain::destination`), checked on save and before every attempt;
  the worker's resolver keeps only public addresses. A development server on loopback may use private ones.
- **The loop:** it claims as soon as a slot is free (eight by default) and sleeps only when there's nothing to do;
  on shutdown it stops claiming and lets attempts finish within the cleanup deadline.
- **Not yet:** per-destination and per-project limits, signed announcements, wake-ups instead of polling,
  reconciliation beyond repeating with the same key, admission budgets (F09), scheduled releases.

## Why it exists

External work (purging a CDN, building a site, processing an image, calling a webhook) can't happen inside a database
transaction. The request might succeed while its response is lost, or the worker might die halfway. The worker turns
committed intents into external effects with leases, deadlines, retries and reconciliation, so a crash at any moment
leaves a state that can be explained and repaired.

## End goal

Workers find all pending work from Postgres alone. They execute it within bounded pools and deadlines, record
evidence, reconcile uncertain outcomes before repeating non-idempotent work, and survive `kill -9` at any point.
Proven by F03, F05, F09, the crash loop and the simulator.

## Responsibilities

- Claiming and leasing jobs.
- Executing through destination adapters.
- Retries with backoff.
- Reconciling uncertain outcomes.
- Superseding obsolete work.
- Running scheduled releases.
- Cancellation.
- Admission control and concurrency pools.
- Graceful shutdown.

## Not its job

- Deciding authorization or publication state. Workers execute committed jobs; they can't invent either (spec §03).
- Holding database locks across external calls.
- Serving HTTP.

## Breakdown

### Claiming and leases (spec §16)
- Claim bounded batches with `FOR UPDATE SKIP LOCKED`.
- Record lease owner, lease expiry, attempt generation, deadline, next attempt time, status, and a stable business
  idempotency key.
- Renew the lease only while work is progressing and within the job deadline.
- **Finalize only through a conditional update** on the current lease owner and attempt generation. A late result from
  an old lease is rejected.

### Attempts and retries (spec §16)
- Each external attempt has a 10-second deadline by default.
- At most five attempts, with capped exponential backoff and full jitter, within a one-hour job lifetime.
- A bounded `Retry-After` is respected.
- Transient network errors, selected 5xx responses and throttling are retried. Authentication and invalid-configuration
  errors are terminal until corrected.
- Adapters may be stricter.

### Pools and fairness (spec §16, §33)

| Pool | Limit |
|---|---|
| Publishing / delivery | 8 slots total, at most 2 per destination, one active publication stream per project and environment |
| Image processing | 4 jobs |
| AI | 2 jobs |

Each pool is a separate semaphore, so expensive work can never starve editorial saves or publishing.

### Reconciliation (spec §16)
- An external action may succeed before its response is lost. Uncertain outcomes are reconciled using release IDs,
  provider operation IDs or read-back evidence **before** repeating non-idempotent work.
- Reconciliation runs on startup and periodically.
- An outcome that can't be confirmed stays `uncertain` with a repair action.

### Supersession
- Work for generation N is marked superseded once N+1 is desired.
- An obsolete "make release X current" is never dispatched after release Y.
- Superseded history is kept for inspection.

### Scheduled releases (spec §06, §14)
- A due-time scanner picks up schedules stored with their UTC instant (time-zone and daylight-saving logic lives in
  [domain](01-domain.md)).
- At execution, authority, approval, revision hashes and dependencies are revalidated. Edited content blocks the release.
- A missed schedule after downtime is labelled *delayed*, never on time.

### Cancellation and revocation
- Cancellation stops new attempts. It does not reverse remote side effects that already happened.
- For safety-critical actions, cancellation and permission revocation are rechecked before each next external attempt.

### Admission (spec §33)
- Capacity is reserved before work is accepted: 10,000 pending jobs per project and 1 GiB of pending payload metadata
  per install.
- Large bodies live in object storage, referenced from the job.
- When capacity is exhausted, new work is refused with a clear error **before** it is accepted (F09).

### Destination adapters (spec §25)
- The contract is `validate_configuration`, `plan`, `execute`, `inspect`, `reconcile`, `revoke`.
- Output separates *request accepted*, *provider completed* and *observed result*.
- Missing read-back produces `uncertain`, never invented confirmation.
- **First adapters:** the direct public content API (no external call), then the Cloudflare purge and probe adapter,
  then the site build-and-promote adapter (P3).

### Lifecycle (rust-mastery chapter 07)
- Every spawned task is tracked with `TaskTracker` and stops on a `CancellationToken`.
- Shutdown: readiness off → stop claiming → drain in-flight attempts within a deadline → cancel.
- Job panics and `JoinError`s are observed and recorded.

## Invariants

| ID | Invariant |
|---|---|
| W1 | No database transaction or lock is held across an external call. |
| W2 | Only the current lease holder and attempt generation can finalize a job (domain D9). |
| W3 | All pending work is discoverable from Postgres; nothing accepted lives only in memory or the cache. |
| W4 | Every external call carries a stable business idempotency key; retries reuse it. |
| W5 | An older generation is never promoted after a newer one is desired (domain D10). |
| W6 | Uncertain outcomes are reconciled before non-idempotent work is repeated, and are never relabelled success. |
| W7 | Work that cannot be retained is refused before acceptance. |

## Interfaces

- Runs inside `colophon-server --role worker` (or `--role all`).
- Reads and writes jobs, outbox, deliveries and evidence through [db](02-db.md).
- Calls destination adapters through the domain's `DestinationAdapter` port.
- Job state, sanitized evidence and repair actions are visible through the API and `colophon jobs inspect`.

## Failure modes

| Situation | Behaviour |
|---|---|
| Worker killed after an external call, before recording it (F03) | Lease expires; another worker reclaims; reconcile before any repeat |
| Old delivery runs after a newer release (F05) | Generation check blocks it; recorded as superseded |
| Outbox or disk budget exhausted (F09) | New durable work fails before acceptance, or enters a documented degraded state |
| Cache unavailable | Wake-ups fall back to polling; no work is lost |
| Postgres unavailable | Stop claiming; nothing is acknowledged from memory; resume from Postgres afterwards |
| Provider 429 / 5xx | Backoff with jitter within the attempt budget, then `failed` or `uncertain` with a repair action |
| Credentials expired | Terminal until corrected; visible in receipts |

## Tests and evidence

- F03, F05 and F09 against real Postgres.
- Lease-expiry races: a late finalize from an old lease is rejected.
- Supersession under out-of-order completion.
- Scheduled releases across daylight-saving changes and expired approvals, with a fake clock (X07).
- The simulator's worker scenarios: drop, delay, duplicate, reorder ([sim](10-sim.md)).
- The real-process crash loop.
- **In place since step 2.3:** `crates/colophon-server/tests/worker.rs` (announced once with a stable key; a 503
  retried with the same key; a 400 final; an older release waiting when a newer commits is superseded and never sent:
  F05); F03 in `tests/faults.rs` (the worker killed after announcing; another claims the job after the lease runs out
  and announces again with the same key); `crates/colophon-db/tests/jobs.rs` (a lease hides a job until it runs out; a
  late result is refused; two workers never claim one job; a job past its deadline fails; the worker's role can't
  read drafts); `perf/k6/publishing.js` (whole publishes while the worker delivers to a local receiver).
- Lifecycle tests: drain within the deadline, cancellation observed.

## Build order

1. **P2:** dispatcher, leases, attempt generations, retries, supersession, a webhook destination (step 2.3, done; the
   direct content API needs no destination: it reads the published projection).
2. **P3:** Cloudflare purge and probe jobs; the site build-and-promote adapter.
3. **P4:** scheduled releases; the image pool for [media](07-media.md).
4. **P5:** the AI and bulk-job pools for [automation](09-automation.md).

## Open decisions

- **O-010:** wake-up mechanism: polling only, Postgres `LISTEN/NOTIFY`, or cache notifications.
- Retry implementation: `backon` or a hand-written loop with jitter and a deadline (both are rust-mastery defaults).
- Periodic reconciliation interval for delivery jobs.
