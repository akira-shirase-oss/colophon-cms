# End goal

## In one sentence

**An open-source Rust + Svelte CMS that can prove what it published and survives being killed at any moment:
useful enough that the maintainer's own site runs on it, and deep enough to stand out as engineering work.**

## Definition of done for v0.1

v0.1 is done when every box below is checked with stored evidence (commit, command, raw output):

- [ ] The maintainer's own site runs on Colophon in production.
- [ ] A recorded demo shows the crash loop: random `kill -9` of the server and worker during saves and publishes,
      followed by a checker proving no accepted edit was lost, no release ran twice, and no reader saw a mixed release.
- [ ] The deterministic simulator runs in CI on every pull request; any failing seed replays exactly; a deliberately
      broken invariant is caught (the simulator is shown to have teeth).
- [ ] All fault cases F01–F14 and load cases L01–L14 pass (L15 is commerce, parked), as defined in
      [verification](modules/17-verification.md).
- [ ] A benchmark report on the reference profile is committed with commands, dataset seed and raw results.
- [ ] A backup-and-restore drill onto clean infrastructure is documented, including a new recovery epoch and the
      deletion ledger replay.
- [ ] One person other than the maintainer installs Colophon from the README alone, and the friction is recorded.
- [ ] Five representative editors attempt draft → preview → review → publish; at least four succeed without help
      (spec §05 usability gate), and the failures are written up.
- [ ] v0.1 is released with signed artifacts, an SBOM, a changelog, the docs site, and one technical write-up per hard
      problem (outbox, release generations, the simulator, cache invalidation).

## The four signature features

### 1. Crash-proof publishing

- **What the user sees:** saves either succeed or clearly fail; a publish either commits as one release or not at
  all; nothing is lost or doubled when a process dies.
- **How it works:** expected-revision saves (stale save → 409), one-transaction release commits that advance a
  generation, a transactional outbox, idempotency keys on every mutation and external call, leased jobs with attempt
  generations. See [domain](modules/01-domain.md), [db](modules/02-db.md), [worker](modules/05-worker.md).
- **How it is proven:** fault cases F01–F05 and F12–F14, the real-process crash loop, the simulator.
- **Milestones:** P1–P2, demo at P3.

### 2. Deterministic simulation testing

- **What the user sees:** a README badge and a replay command; for contributors, a bug report is a seed.
- **How it works:** the domain logic and workers run inside a seeded, single-threaded simulator with a virtual clock and
  fake CDN, storage and network that drop, delay, duplicate and reorder messages. See [sim](modules/10-sim.md).
- **How it is proven:** determinism tests (same seed, same trace hash), mutation checks (a deliberately broken rule must
  be caught), and differential tests that run the same scenarios against real Postgres.
- **Milestones:** P1 skeleton, grows each milestone.

### 3. Time travel

- **What the user sees:** a release slider in the Studio; any page of the site as it was at release N; rollback that is
  just a new release pointing at older content.
- **How it works:** immutable revisions plus publication history stored as non-overlapping generation intervals per
  entry and locale; pinned reads by generation. See [domain](modules/01-domain.md), [delivery](modules/06-delivery.md),
  [studio](modules/11-studio.md).
- **How it is proven:** pinned-read consistency checks, rollback tests, revocation tests (unpublished content stays
  unavailable even through historical URLs).
- **Milestone:** P4.

### 4. Delivery receipts

- **What the user sees:** each release shows *Saved → Published in CMS → Website update pending → Verified at configured
  locations*, or *Needs attention* with a safe next action.
- **How it works:** a delivery state machine per destination, evidence probes that look for the release's generation
  marker, and stored evidence (generation, marker or hash, location, time, errors). See
  [delivery](modules/06-delivery.md), [worker](modules/05-worker.md).
- **How it is proven:** CDN purge failures and delayed rebuilds produce *pending/uncertain*, never false success
  (fault and load cases F07, L13).
- **Milestone:** P3.

## Scope

### Core (v0.1, milestones P0–P6 and R, M, E, D)

**Every feature Strapi and Sanity offer** (D-057; the list and its status are in [COMPARISON.md](COMPARISON.md)),
with **roles and permissions** down to fields and an entry's creator (D-058), built at the bar above: no feature counts
until its failure behaviour is tested and its evidence stored. And all modules in [modules/README.md](modules/README.md)
except the parked list: content model and revisions,
localization in the data model, editorial review, releases and rollback, public delivery with caching and evidence,
media (images), preview links, scheduled publishing, the CLI, the TypeScript SDK, automation (scoped tokens, bulk jobs,
MCP), the Studio, the reference blog, the docs site, verification, operations and open-source hygiene.

### Later (after v0.1)

- A business-site template with lead forms and notifications.
- Import adapters for Payload exports (Strapi and Sanity imports are in milestone D).
- The optional AI writing assistant, if it was not finished in P5.

The localization UI, the schema designer and Strapi/Sanity import moved into the core with D-057.

### Parked (see [modules/parked.md](modules/parked.md))

Commerce, payments, inventory, shipping, tax, refunds, subscriptions and entitlements; hosting adapters for
GitHub/Vercel; custom domains; membership and knowledge-base templates.

### Non-goals (from spec §01, as amended by D-057)

Arbitrary untrusted native plugins (milestone D offers safe extension points instead), active-active multi-region
database writes, multi-seller marketplaces, usage-based billing, autonomous production repair, and hostile-tenant open
registration. GraphQL and real-time collaboration were non-goals; they are in scope since D-057 (milestones D and E).

## Quality bars

### Correctness stop conditions (block any release, whatever the speed)

Unexplained data loss, cross-project exposure, an authorization bypass, a falsely acknowledged save, a duplicated
logical release, or uncertainty relabelled as success (spec §28, §30).

### Performance targets (initial; spec §28; none measured yet)

| Workload on the reference profile | Target |
|---|---|
| Warm public origin reads, 300 req/s for 30 min | p95 ≤ 150 ms, p99 ≤ 350 ms, unexpected failures < 0.1% |
| Cold / cache-bypass origin reads, 100 req/s | p95 ≤ 300 ms, p99 ≤ 750 ms, bounded pools |
| Draft writes 20/s plus 100 reads/s | p95 ≤ 300 ms, p99 ≤ 700 ms, no lost accepted revision |
| Publish 5 releases/s, ≤ 10 entries each | database commit p95 ≤ 500 ms |
| Warm CDN reads, 2,000/s over 3 probe regions | per-region p95 TTFB ≤ 150 ms, p99 ≤ 400 ms, ≥ 95% hit rate |
| Publish to probe visibility (direct edge API) | p95 ≤ 10 s, p99 ≤ 30 s |

Targets may be revised openly after baselines; they are never quietly lowered to turn a failing run green.

### Security gate

Controls mapped to the relevant OWASP ASVS 5.0 Level 2 requirements before any public production use, with negative
tests for cross-project access, XSS, SSRF, uploads, cache leakage and agent escalation (spec §26–27). This is a
self-assessment, not a certification.

## What "done" does not mean

Not production commerce, not hostile multi-tenant hosting, not a certified security posture, and not "globally
verified" delivery: evidence is always scoped to the locations actually probed.
