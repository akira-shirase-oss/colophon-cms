# 17 · verification (`tests/`)

> The fault, load, security and contract matrices, plus the release evidence bundle. Proof is the product's identity,
> so it has its own module.

| | |
|---|---|
| Milestones | All (each milestone's proof gate lives here) |
| Proposed owner | Shared: the maintainer owns the requirement matrix; Claude builds the harnesses |
| Depends on | every module; `proptest`, `insta`, the local stack's PostgreSQL, `cargo-nextest`, Vitest, Playwright, k6, Criterion (micro-benchmarks, D-074) |
| Used by | CI, release decisions, the README and docs (published evidence) |
| Spec | §26–§31, §36 |

## Why it exists

Colophon's claims (crash-proof, time travel, verified delivery) are only as good as the tests behind them. The spec
defines a full matrix of fault, load and workflow cases. This module turns them into runnable suites with stored
results, and defines which failures block a release whatever the latency charts say.

## End goal

A machine-readable requirements matrix links each requirement to its tests, the last result and the raw artifacts.
CI gates block on correctness failures, and every release ships an evidence bundle. Test count is diagnostic, never a
quality claim (spec §30).

## Responsibilities

- The test layers and CI tiers.
- The F, L and X case suites.
- Security negative tests.
- Benchmarks and reporting rules.
- The requirements matrix.
- Stop conditions.

## Not its job

- The deterministic simulator itself ([sim](10-sim.md)), although its results feed the matrix.
- Production monitoring ([operations](18-operations.md)).

## Breakdown

### Test layers (spec §30)

| Layer | What it covers |
|---|---|
| Domain unit + property tests | Legal and illegal transitions with a fake clock; idempotency, authorization and generation invariants over random sequences |
| Database integration | Real PostgreSQL 18 and the cache in containers, with migrations |
| HTTP in-process | `oneshot` tests for every route and error |
| Contract | CLI and SDK examples against the same API; OpenAPI compatibility |
| Browser | Studio keyboard flows, conflicts, upload, preview, receipts (Playwright) |
| Simulation | Seeded runs, mutation checks, differential tests ([sim](10-sim.md)) |
| Crash loop | Real processes killed at random against Postgres |
| Security negatives | Spec §27 families (below) |
| Fuzzing | URL parsers, schema validation, rich-text decoding, pagination cursors, state transitions |
| Load | k6 arrival-rate scenarios with thresholds and a non-zero exit on failure |

### Fault-injection suite (spec §30)

| ID | Injected fault | Required invariant |
|---|---|---|
| F01 | Kill server before content commit | No success acknowledgment for an uncommitted revision |
| F02 | Kill after commit, before response | Idempotent retry returns the same logical operation and durable result |
| F03 | Kill worker after purge/send, before recording | Reconcile or safely retry; no duplicated business effect claimed impossible. In place since step 2.3: `tests/faults.rs` |
| F04 | Two editors save the same base revision | One conflicts; neither silently loses an accepted edit |
| F05 | Old delivery runs after a newer release | Generation check blocks regression and records superseded work. In place since step 2.3: `tests/worker.rs` |
| F06 | Collection filter gains a new matching article | Listing cache invalidates even though the old response did not reference the new entry |
| F07 | Entry unpublished while old caches exist | Origin denies revoked content; purge/evidence tracked; remaining exposure visible |
| F08 | Cache eviction/failover/restart | No loss of content, jobs, sessions or hard budget accounting |
| F09 | Outbox/disk budget exhausted | New durable work fails before acceptance, or enters an explicit documented degraded state |
| F10 | Migration interrupted or schema edited concurrently | Checkpointed recovery or conflict; no mixed undocumented schema |
| F11 | Model produces invalid or hostile output | No unsafe action; reviewer sees a rejected suggestion with safe diagnostics |
| F12 | Duplicate/reordered external events | Authoritative reconciliation and exactly one logical effect (the spec's payment case is parked; applied to delivery and webhook receipts) |
| F13 | Restore backup containing an old outbox | New recovery epoch; reconcile external effects before replay; no blind resend |
| F14 | Deleted project appears in a restored backup | Deletion ledger reapplied before read APIs and caches are enabled |

**In place (step 1.2):** F01 and F02 run against the real binary in
`crates/colophon-server/tests/faults.rs`. A build with the `fault-injection` feature exits at a named point
(`COLOPHON_FAULT=entry.save:before-commit`, `entry.save:after-commit`, `entry.create:after-commit`); release builds
don't contain the points (D-036). F04's database half (two concurrent saves on one base: one wins, one is stale, the
loser's revision is never stored) is in `crates/colophon-db/tests/entries.rs`, and the testbed shows it in two browser
tabs. **Since step 1.3** F04 also runs end to end in the Studio (`apps/studio/tests/server/`): two browsers, the
later save opens the conflict screen, and neither choice loses an accepted edit; the browser also re-sends a save whose
answer was dropped and gets the replayed result (F02 from the client's side). **Since step 1.4** the simulator
(`crates/colophon-sim`) runs F01, F02 and F04 under thousands of seeded schedules with lost requests, lost answers and
crashed clients, and `crates/colophon-db/tests/differential.rs` checks its model against Postgres.

### Load-test cases (spec §29)

| ID | Scenario | Expected evidence |
|---|---|---|
| L01 | One-user smoke, 5 min | Correct bodies, authorization, revisions and release IDs; no load before basic correctness |
| L02 | Warm read baseline, 30 min | Meets the warm-origin gate; record hits, pools, CPU, RSS and plans |
| L03 | Cold cache / 100% unique keys | Meets the cold-origin gate; no uncontrolled database amplification |
| L04 | Hot-key stampede: 1,000 callers at expiry | Bounded regeneration and coalescing; no permission-crossing reuse |
| L05 | Mixed reads, writes and publishing | Each operation class meets its target; content stays consistent |
| L06 | Spike from 1× to 5× load for 2 min | Controlled 429/503, bounded queues; back to baseline within 2 min after load ends |
| L07 | 24-hour soak at 50% baseline mixed load | Correct final state; quotas and retention enforced; memory, WAL and disk growth explained |
| L08 | One noisy agent plus active editors | Agent throttled; editorial p95 ≤ 2× mixed-workload baseline; no cross-project starvation |
| L09 | 10,000-record draft import | Resumable chunks, per-item outcomes, stable memory; no accidental publication |
| L10 | CDN warm traffic + repeated publishes | Per-region latency and observed generation; detect stale refills and mixed-release pages |
| L11 | Cache unavailable for 5 min | Rate-limited fallback, no lost accepted work, bounded origin pressure, correct recovery |
| L12 | PostgreSQL slow/unavailable, worker kill | Writes fail honestly; no fake commit; jobs resume; cache policy stays explicit |
| L13 | CDN purge 429/5xx and delayed site rebuild | Backoff and pending/uncertain delivery; no claim of global success |
| L14 | Large media/decoder workload | Worker limits hold; content editing stays responsive |
| L15 | Commerce concurrency and provider faults | **Parked** with commerce (D-006) |
| L16 | A server killed (SIGKILL) or drained (SIGTERM) under writes behind the balancer (milestone S, D-075) | Every acknowledged write stored exactly once; a drain costs no request, even a first try; idempotent retries succeed. `scripts/failover.sh kill\|drain` against `scripts/cluster.sh`, results in `perf/results/` |
| L17 | Stress: each scenario's steady mode at rates rising 25% a step until it breaks (milestone S, D-077) | The knee (highest rate that held), the failure mode (`shed load`, `errors`, `fell behind`, `latency`), every step with the machine's load average and the failed requests the servers logged, by code; `scripts/load.sh <endpoint> stress`, on one server or `CLUSTER=1`. `shed load` means every failed request was a 503 `unavailable` after the pool's acquire timeout (D-033): overload refused as designed. The first modelling run (2026-10-28, `errors` at 921/s) predates the per-step counts; rerun with them on the same day, its failures were all `unavailable` |

### Workflow cases that apply (spec §31)

| ID | Scenario | Status |
|---|---|---|
| X06 | Locale fallback to an unpublished draft | Applies: denied; output holds only eligible published content |
| X07 | Scheduled publication crosses a daylight-saving change or approval expires | Applies |
| X08 | Slow older site build finishes after a new generation | Applies |
| X09 | Clone production into staging | Applies to content: no production sessions, credentials or unintended scheduled dispatch |
| X10 | 10,000 asset references across a template upgrade | Applies |
| X11 | Cache and identity provider fail while editors are active | Applies |
| X12 | Agent interrupted after half a bulk edit | Applies |
| X13 | Paid download or preview after revocation | Preview part applies; paid downloads are parked |
| X14 | Restore a backup predating a deletion and payment | Deletion part applies; payment part is parked |
| X01–X05 | Commerce, subscriptions, leads | Parked or later (D-006) |

### Security test families (spec §27)
- **Identity and policy:**
  - revoked session and expired token;
  - wrong project or environment;
  - hidden field;
  - unauthorized publish or export;
  - stale approval.
- **Browser safety:**
  - stored markup payloads and hostile links;
  - preview-origin isolation;
  - CSRF and unauthorized cross-origin calls;
  - unsafe redirects.
- **Network and asset abuse:**
  - metadata-address access, DNS rebinding, IPv6 and mapped-address bypass, redirects to private hosts;
  - decompression and pixel bombs;
  - misleading file types.
- **Cache isolation:**
  - the same slug across projects;
  - a private response is never reused publicly;
  - locale and query-key collisions;
  - a stale fill after publish;
  - removal of prior public versions.
- **Agent abuse:**
  - prompt injection in content;
  - tool-argument manipulation;
  - cost overruns;
  - a mass-deletion attempt;
  - an altered approved changeset;
  - an unsafe import.
- **Supply chain:**
  - a tampered artifact or unexpected publisher;
  - a compromised test fixture;
  - untrusted pull-request access to secrets;
  - license-policy failure.

Scanner reports never replace code review and adversarial permission testing.

### Performance (spec §28)
- **Reference profile:**

  | Component | Size |
  |---|---|
  | API | 4 vCPU / 8 GiB |
  | Worker | 2 vCPU / 4 GiB |
  | PostgreSQL | 4 vCPU / 16 GiB, SSD with recorded IOPS |
  | Cache | 1 GiB budget |
  | Load generator | A separate host |

- **Recorded with every run:** CPU model, region/RTT, OS, exact versions, build flags, pool limits, CDN plan, cache rules
  and storage configuration.
- **Dataset D1:**
  - three isolated projects;
  - 100,000 entries across ten collections per project;
  - three revisions per entry, typically 6 KiB of content;
  - 10,000 asset metadata records;
  - realistic reference, locale and slug distributions.

  A separate corpus holds maximum-size and invalid records. Seeds are fixed and the generator is exported.
- **Targets:** listed in [END_GOAL.md](../END_GOAL.md#quality-bars). Cached, uncached, authenticated, write and
  publication paths are reported separately.
- The development machine (including the 32 GB Ubuntu Dell named in the spec) is for functional tests. It is not an
  independent load generator plus benchmark host.

### Reporting rules (spec §29)
- Warm up for 5 minutes, then run three independent measured repetitions per baseline scenario.
- Report per-run distributions; never average percentiles.
- Show sample counts and windows for release-level p99.
- **Separate:**
  - successful latency;
  - rejected requests;
  - expected fault errors;
  - unexpected errors.
- Overload rejections stay visible.
- A generator that misses its offered rate (dropped iterations) invalidates the run.
- **Every report includes:**
  - commit, fixtures and command;
  - environment;
  - raw results and traces;
  - thresholds, exceptions and conclusions.

### Micro-benchmarks (milestone S, D-074)

`crates/colophon-bench` times, with Criterion, the CPU work a request does between its waits: one function, one
input, one machine. It answers "did this change make that work slower?", never "what does a server sustain?"; only
the load runs above answer that, and laptop numbers stay laptop numbers (D-018).

- **What is timed** (`benches/hot_paths.rs`; each benchmark names the request that runs it, picked from the code):

  | Group | Benchmarks | Input |
  |---|---|---|
  | `rich_text` | `validate_blog_post` | the blog-post fixture, 2 KB of canonical rich text |
  | `entry` | `validate_article`, `validate_page`, `parse_body_page`, `sha256_page` | an article; the page `perf/k6/modelling.js` saves (an object, lists, a pattern, blocks, references) |
  | `content_type` | `load_articles`, `load_page`, `load_big`, `canonical_json_big`, `expand_components_page` | the stored definition every entry request parses: articles, the page type, a 100-field type with 8 patterns |
  | `policy` | `new_builtin_owner`, `new_custom_500`, `fields_custom_500`, `allows_custom_500` | the role every signed-in request rebuilds: the owner, and a custom role of 500 permissions |
  | `changeset` | `canonical_100` | the hash input of a 100-item changeset |
  | `delivery` | `cache_key_and_tags` | the key and tags of a `/content/v1` entry read |
  | `auth` | `session_token_hash`, `csrf_check` | 43-character session and CSRF tokens |
  | `near_limit` | `rich_text_validate`, `validate_article`, `parse_body`, `sha256` | a 238 KiB body (the limit is 256 KiB) |

- **Inputs** are built in code or read from `fixtures/`, with no network and no database. The crate's tests keep them
  valid, and `check.sh` and CI's rust job run each benchmark once, unoptimized (`cargo test -p colophon-bench
  --benches`), so they keep building. `colophon-api`'s helpers are crate-private; its per-request SHA-256 is the same
  aws-lc-rs call as `colophon_auth::tokens::hash`, which `entry/sha256_page` and `near_limit/sha256` time.
- **Why a crate of their own:** inside `colophon-domain` the benchmarks would be built with its dev-dependencies, whose
  serde_json keeps object keys in insertion order (`preserve_order`, an IndexMap); the server's keeps a BTreeMap, and
  every validation walks those maps.
- **Running:** `scripts/bench.sh` runs them all in the bench profile (the release defaults, which the server ships
  with) and writes the medians, with their 95% intervals, to `perf/results/<date>-bench-<machine>.json`, with the
  commit, machine, compiler and the load average before and after. `cargo bench -p colophon-bench -- policy/` runs
  one group.
- **The regression check:** `scripts/bench.sh compare <base>` builds this checkout, and `<base>` in a temporary
  worktree with a target directory of its own (in a shared one, Cargo gave the base's release builds of the workspace
  crates the same names as this checkout's, and later builds here took them as fresh), then runs both on this machine
  in three rounds (1 s of warm-up and 3 s of measurement per benchmark), alternating which build goes first. A benchmark regresses only when this checkout is
  slower than the base **in every round** by more than **15%** and more than **25 ns**; the report gives each
  benchmark's median of the rounds and the change. CI's `benchmarks` job runs it on pull requests only, against the
  pull request's base, and puts the report in the job summary and an artifact.
- **Why compare on one machine, in one job:** GitHub's runners differ about 2.4x by CPU model alone (the health
  smoke did 44,243 requests a second on an EPYC 9V45 and 18,613 on a 7763; `ci.yml`), so a stored number from another
  job compares machines, not code. Within one job both builds share the CPU, the kernel and the neighbours.
  Alternating the order spreads drift over both; requiring every round to be slower makes one disturbed round
  harmless; the nanosecond floor keeps code-layout luck on a 40 ns benchmark from failing a pull request. Pushes to main run nothing: there is no second build there to compare with.
- **What it can't see:** a slowdown under 15%, one that shows in only some rounds, and anything outside these
  functions (I/O, locks, the database). A benchmark whose input changes must be renamed, or base and head time
  different work.
- **The threshold, measured:** an A/A run built one commit twice and timed it against itself on the maintainer's
  M2 Pro while other work kept the load average near 9
  (`perf/results/2026-10-28T120430Z-bench-compare-aa-apple-m2-pro-12c.json`). Single rounds disagreed by up to
  +57.7% (`near_limit/sha256`) and −46.7% (`policy/new_custom_500`); 7 of the 66 round pairs differed by more than
  15% (median 1.8%, 90th percentile 15.2%), so a one-round check at 15% would have failed about one benchmark in ten
  with nothing changed. Over three rounds no benchmark was slower in all of them, and every median of rounds stayed
  within ±5.1%. 15% is three times that, and well below the slowdowns worth stopping a pull request for, like the
  6.7x one below. A full comparison's rounds took 10.5 minutes there. GitHub's runners haven't run it yet (nothing
  has been pushed from this branch): if the first pull requests show false alarms, raise `THRESHOLD_PCT` or `ROUNDS`
  openly (V2), and make the job a required check only after about ten pull requests have passed it without one.

#### First results (2026-10-28)

`perf/results/2026-10-28T123240Z-bench-apple-m2-pro-12c.json`: commit `d0b0af7`, Apple M2 Pro, Rust 1.99.1,
Criterion's medians after 3 s of warm-up and 5 s of measurement, load average 10 to 14 from other work on the machine.
Micro-benchmarks on a laptop: they rank costs and catch slowdowns, they are not what a server sustains.

| Benchmark | Median | |
|---|---:|---|
| `entry/validate_article` | 7.15 µs | a 2 KB article; `rich_text/validate_blog_post` alone is 6.82 µs |
| `entry/validate_page` | 5.35 µs | the modelling page |
| `entry/parse_body_page` | 1.82 µs | its request body into a `Value` |
| `content_type/load_articles` | 1.96 µs | |
| `content_type/load_page` | 22.0 µs | four times the page's own validation |
| `content_type/load_big` | 303 µs | 100 fields, 8 patterns |
| `policy/new_custom_500` | 15.8 µs | 96 to 102 µs before `d0b0af7` |
| `policy/fields_custom_500` | 2.02 µs | |
| `changeset/canonical_100` | 8.53 µs | |
| `delivery/cache_key_and_tags` | 1.16 µs | |
| `auth/session_token_hash` | 40.1 ns | |
| `near_limit/parse_body` | 1.42 ms | a 238 KiB body into a `Value` |
| `near_limit/validate_article` | 932 µs | |
| `near_limit/sha256` | 99.3 µs | |

What they showed:

1. **Every entry request parses its content type again, patterns included.** `entries::content_type` reads the
   stored definition as text and parses it into a `Value`, then a `ContentType`, on every read and save. For the
   modelling page that is 22 µs, four times its validation; for the 100-field type, 303 µs. A profile of
   `ContentType::parse` on that type (macOS `sample`) put 49% of it in `regex::RegexBuilder::build`: each text
   pattern (D-069) is compiled again per request, then dropped. Fixed by D-078: a bounded cache of parsed types in
   `colophon-api`, keyed by the SHA-256 of the definition text read. A hit is `content_type/cached_*`: 0.12 µs for
   articles, 0.59 µs for the page and 6.5 µs for the 100-field type, against 1.89, 20.4 and 281 µs parsing, mostly
   the hash itself (`perf/results/2026-10-28T135828Z-bench-apple-m2-pro-12c.json`, same machine, load average 12 to
   8).
2. **Rebuilding a role was quadratic in its permissions.** `Policy::new` looked for each permission among those
   merged so far, and every signed-in request rebuilds its role. Fixed in `d0b0af7` (sort, then merge neighbours):
   96.1 µs to 14.4 µs for 500 permissions, same machine, three rounds
   (`perf/results/2026-10-28T121716Z-bench-compare-policy-apple-m2-pro-12c.json`).
3. **The largest bodies cost most to parse, not to validate.** A 238 KiB rich-text body takes 1.42 ms to become a
   `Value` (a map for every object in it) and 932 µs to validate. The 1 MiB body limit bounds both; no change.

### Stop conditions (spec §30)
Any of the following blocks a release regardless of performance:
- unexplained data loss;
- cross-project exposure;
- invalid authorization;
- duplicate effects;
- uncertainty relabelled as success.

### CI tiers (spec §34)

| Tier | Runs |
|---|---|
| Every pull request | Format, lint (`-D warnings`), type checks, unit and property tests, API/schema compatibility, relevant integration and browser tests, simulator seed budget, `cargo deny`, secret scan, micro-benchmarks against the base on one runner (D-074) |
| Main branch | Broader fault suites, crash loop, performance smoke, larger simulator sweep |
| Release | Full load matrix, 24-hour soak, restore and upgrade drill, dependency and licence inventory, SBOM, signed artifacts, verified docs |

### Requirements matrix
- `tests/MATRIX.md` (human-readable) generated from `tests/matrix.yaml` (machine-readable).
- It maps every requirement (spec §36 IDs plus Colophon invariants) to its tests, last result, commit and artifacts.
- Unsupported configurations are listed explicitly, never silently skipped in the pass rate.

## Invariants

| ID | Invariant |
|---|---|
| V1 | No release ships with a stop condition open. |
| V2 | Targets are revised openly, never quietly lowered to turn a failing run green. |
| V3 | Every published number has its command, environment and raw data. |

## Interfaces

- Test suites under `tests/{contracts,faults,security,load,crash-loop,fixtures}`.
- CI workflows; the evidence bundle attached to each release.

## Failure modes

| Situation | Behaviour |
|---|---|
| Flaky test | Quarantined with an issue and an owner; never silently retried into green |
| Load generator saturated | Run invalid; re-run on a larger generator |
| Stop condition found | Release blocked; fix plus regression test before retrying |

## Tests and evidence

This module *is* the evidence. Its own check is that the matrix has no requirement without a test, and no test result
without an artifact.

## Build order

1. **P0:** CI pull-request tier.
2. **P1–P2:** F01–F05; the first real-process crash loop (`tests/crash_loop.rs`, step 2.4).
3. **P3:** F06–F08, L01–L04.
4. **P4:** L14, X07, X13 (preview).
5. **P5:** F11, L08, X12.
6. **S:** micro-benchmarks and their pull-request check (built, D-074); stress runs; a profile of each
   milestone's heaviest scenario.
7. **P6:** F09–F14, L05–L13, the 24-hour soak, the release tier, the complete matrix.

## Open decisions

- Where load tests run (cloud hosts sized to the reference profile).
- The matrix file format, and whether results are published on the docs site.
