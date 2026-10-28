# Roadmap

Thirteen milestones, each named after what it proves. Since D-057 the product goal is every feature Strapi and Sanity
offer ([COMPARISON.md](COMPARISON.md)), built at Colophon's bar: the four lettered milestones R, M, E and D carry that
parity work, and since D-062 S and V carry scale and distribution; they sit between the numbered ones in the order
below. Estimates assume ~45 hours/week (D-010) and are guesses, not
measurements. A milestone is done when its **proof gate** passes with stored evidence, not when its screens exist.

| Order | Milestone | Proof gate | Estimate |
|---|---|---|---|
| P0 | Foundation | Editor JSON round-trips; Rust rejects invalid documents; CI is green | 1–2 weeks |
| P1 | Never lose an edit | F01, F02, F04; cross-project denial; first simulator runs | 3–5 weeks |
| P2 | Publish exactly once | F03, F05; concurrent releases; simulator covers the outbox; first crash loop | 3–4 weeks |
| P3 | Prove it reached the site | F06–F08, L01–L04, mixed-release checker; **first public demo** | 3–4 weeks |
| R | Roles and permissions | Every action × role × field × condition decided by one evaluator; no permission check bypassed in the API, CLI or Studio | 2–3 weeks |
| M | Content modelling parity | Relations, components, lists, blocks and single types round-trip and stay valid through schema changes | 4–6 weeks |
| P4 | Time travel and media | Rollback, pinned reads, revocation, upload fault tests, the media library | 3–4 weeks |
| E | Editorial parity | Localization, scheduling, review stages, comments, tasks and live collaboration without a lost edit | 5–7 weeks |
| D | Developer parity | Query API, GraphQL, SDK, API tokens, import from Strapi and Sanity, extension points | 4–5 weeks |
| S | Scale and performance | Several servers and workers behind a load balancer survive kills and drains; every scenario has a stored stress result; Rust benchmarks gate CI (D-062) | 2–3 weeks |
| V | Versions and distribution | One version across server, CLI, SDK and MCP; the CLI installs from npm, Cargo, Homebrew and a script on a clean machine (D-062) | 1–2 weeks |
| P5 | Safe automation | Zero unauthorized actions in the eval set; interrupted bulk job resumes (X12) | 3–4 weeks |
| P6 | Operate it | F09–F14, L05–L13, 24-hour soak, restore drill; **v0.1 release** | 3–4 weeks |

Total: about **10–13 months** to v0.1 at the pace these estimates assumed, with the first public demo at the end of P3.

---

## P0: Foundation (≈ spec M0)

**Goal:** a repository where every later change lands on pinned, tested ground.

- Toolchain: fix `PATH` so rustup's pinned toolchain is used (Homebrew 1.96.0 ignores `rust-toolchain.toml`); commit
  `rust-toolchain.toml`, `Cargo.lock`, the frontend lockfile and container digests.
- Cargo workspace with `colophon-domain`, `colophon-db`, `colophon-api`, `colophon-cli`, `colophon-server`; shared
  `[workspace.dependencies]` and `[workspace.lints]`.
- Frontend workspace with `apps/studio`, `packages/ui`, `packages/content-renderer`.
- `deploy/compose.yaml`: PostgreSQL 18, Valkey (D-015), Garage (D-016, replacing MinIO), Keycloak; started and checked by `scripts/stack.sh` (D-017).
- API error envelope, request IDs, `/livez` and `/readyz`.
- The testing rule (D-018) in place: `apps/testbed` with Playwright, and the k6 harness in `perf/` with committed
  results and per-machine baselines. Every later step adds its own checks and scenarios.
- Tiptap spike: custom blocks, paste, image placeholder, undo/redo, JSON serialization; Rust validator for the allowed
  node set and depth ≤ 16; shared JSON fixtures used by both Rust and TypeScript.
- CI: format, lint (`-D warnings`), tests, `cargo deny`, frontend type-check and tests.
- Settle open decisions O-001 to O-007.

**Proof gate:** valid fixtures round-trip through editor → JSON → Rust validator → renderer; invalid fixtures are
rejected with field paths; CI is green from a clean clone.

**Done (October 2026).** **Steps as built:** 0.1 toolchain and workspace · 0.2 local stack · 0.3 errors, health and the testing rule · 0.4 the
rich-text format and editor · 0.5 the Rust validator · 0.6 the renderer · 0.7 Material 3 colours and the first screen.

## P1: Never lose an edit (≈ spec M1)

**Goal:** an editor and the CLI create the same validated draft; conflicts are safe; projects are isolated.

- `domain`: IDs, scopes, schema model and validation, entries, immutable revisions, locales, slug rules, save rule.
- `db`: core tables, composite foreign keys, slug claims, insert-only revisions, expected-revision save, idempotency
  records, database roles with row-level security.
- `auth`: OIDC login (Keycloak), sessions, CSRF/Origin checks, owner bootstrap, roles, invitations, cross-project denial.
- `api`: projects, environments, schemas, entries, revisions; `If-Match`, `Idempotency-Key`, cursor pagination; the
  generated OpenAPI document (D-027).
- `cli`: `login`, `content create`, `content list`, `--json`, exit codes.
- `studio`: content list, editor with autosave and the conflict screen.
- `sim`: scheduler, virtual clock, in-memory store, save invariants, determinism test.

**Proof gate:** F01 (kill before commit: no false acknowledgement), F02 (kill after commit: idempotent retry returns
the same result), F04 (two editors on one base revision: one gets 409, nothing lost); a second project is denied with
colliding slugs and IDs; the spec's first slice works end to end (spec §37).

**Steps:** 1.1 sign-in and projects that can't see each other (done: D-031 to D-033) · 1.2 a draft that can't be lost
(done: entries, revisions, the expected-revision save, idempotency, F01 and F02; D-034 to D-036) · 1.3 conflicts
(done: the Studio's editor, autosave and conflict screen, F04 end to end; D-037, D-038) · 1.4 the first simulator run (done: D-039; it found a double-save case in the Studio, now
fixed) · 1.5 content types you define (done: D-040; `articles` is now an ordinary type) · 1.6 the CLI signs in
and creates the same record as the Studio (done: D-041) · 1.7 roles and invitations (done: D-042; one policy
table, members, single-use invitation links) · 1.8 the first owner (done: D-043; `colophon admin bootstrap`, and only
administrators create projects).

**Proof gate, re-checked after 1.8:** F01 and F02 pass against the real binary (`faults.rs`) and in the simulator;
F04 passes in two browsers, in the database and in the simulator; a second project is denied by ID on every route; the
spec's first slice works end to end (the Studio saves a validated draft, the CLI creates the same record, a conflict
returns safely, another project is denied); roles are enforced by one policy table, invitations work once, and the
first owner is bootstrapped once, from the operator's terminal. The gate is met, with one check moved: entry slugs
don't exist yet, so "colliding slugs" moves to P2/P3, where slugs arrive with publishing. Carried forward from the P1
list, not built: slug rules and claims (with publishing), locales, environments beyond `production`, and the generated
OpenAPI document (D-027).

## P2: Publish exactly once (≈ spec M2)

**Goal:** reviewed changes become one atomic release, and external work happens exactly as intended despite crashes.

- `domain`: changesets, reviews and approvals bound to hashes, releases (epoch + generation), publication history,
  job and delivery state machines.
- `db`: release commit transaction, publication-history intervals, outbox and inbox tables, lease columns.
- `worker`: dispatcher, leases, attempt generations, deadlines, retries with jitter, supersession.
- `delivery`: `/content/v1` from published projections (no cache yet), pinned reads.
- `api`/`cli`/`studio`: review and publish flows, `release validate|publish|status`, `jobs inspect`.
- `sdk`: generated client and pinned-read helper.
- `sim`: releases, outbox, worker crashes, duplicate and reordered messages. First real-process crash loop.

**Proof gate:** F03 (kill the worker after an external call, before recording it: reconcile, no duplicated effect
claimed impossible), F05 (an old delivery after a newer release is blocked and recorded as superseded); two
concurrent releases on one base generation cannot both win.

**Proof gate, checked after 2.3:** F03 passes (the worker killed after announcing; the next claim announces again
with the same key and the job succeeds after two attempts), F05 passes (a release waiting when a newer one commits is
superseded and never sent), and two releases on one base generation can't both commit (`releases.rs`, in the database
and end to end). Step 2.4 adds the simulator's release, outbox and worker model, which found that two
announcements could be in flight to one destination at once (fixed: one at a time), and the first real-process crash
loop. P2 is done.

**Steps:** 2.1 approving the exact change (done: D-044; changesets pin exact revisions and their content-type
versions, approvals bind to the changeset's hash, and any later edit makes them stale) · 2.2 one release, one
number (done: D-045; a release takes the next generation only from the publisher's base, publication history is
non-overlapping intervals, `/content/v1` reads a published copy as a role that can't read drafts, and two releases on
one base can't both commit) · 2.3 work that survives a crash (done: D-046; the outbox, the worker's leases, retries and
supersession, F03 and F05) · 2.4 killing it on purpose (done: D-047; the simulator covers releases, the outbox and
leased workers, the first real-process crash loop, and one announcement in flight per destination).

## P3: Prove it reached the site (≈ spec M3)

**Goal:** public reads are cached safely, invalidated correctly, and every release has evidence.

- `delivery`: generation-keyed cache with request coalescing and bounded fallback, Cloudflare adapter, cache tags,
  targeted purge, unpublish with tombstones and origin revocation, evidence probes.
- `worker`: purge and probe jobs, the site build-and-promote adapter.
- `web` + `templates/blog`: the reference blog, prerendered from a generation-pinned manifest, with a generation marker
  and a `/colophon` page; deployed as the maintainer's site.
- `studio`: publishing labels and the delivery-receipt timeline.
- `docs`: site skeleton with a quickstart.
- `sim`: delivery, cache and evidence faults. Consistency checker for mixed releases.

**Proof gate:** F06 (a new matching article invalidates listings), F07 (unpublished content is denied at the origin;
remaining exposure is visible), F08 (cache loss loses nothing), L01–L04; the mixed-release checker passes. **First
public demo:** recorded crash loop plus a live site with receipts.

**Steps:** 3.1 a cache that can't serve the wrong thing (D-048; a generation-keyed Valkey cache for
`/content/v1` with the generation always resolved from Postgres, single-flight fills, a Postgres fallback when the
cache is down, ETags with 304s, CDN cache policies and tags; F06, F08, L04) · 3.2 taking it back (D-049, D-050; unpublish with
tombstones, origin revocation through historical URLs, purge jobs through a fake CDN; F07) · 3.3 a receipt for every
publish (D-051; probes, evidence scoped to the locations checked, the Studio's receipt timeline) · 3.4 my own site, running
on Colophon (D-052; the reference blog with a generation marker and the build-and-promote adapter) · 3.5 the Studio, designed
(D-053, D-056; the design system, entry states, title search, revision comparison, review diffs, the sign-in theme) · 3.6
content modelling in the Studio (slugs, a content-type builder whose changes are checked against stored content; D-054,
D-055) · 3.7 the non-technical path (D-059: plain words from one glossary, one-click publish, a first-run checklist,
teaching empty states, developer settings out of the way, phone-friendly review; with sign-up and a setup code for a
fresh installation's first administrator, D-061; built: the journey from sign-up to a first release, its Playwright
journey and plain-words test, the testbed's Setup page and `perf/k6/setup.js`; phone-friendly review not yet
checked at phone width) · 3.8 my site goes live, and the first
public demo (docs skeleton, simulator cache and delivery faults, the mixed-release checker, a
recorded crash loop).

## R: Roles and permissions (D-058)

**Goal:** owners decide exactly who may do what, down to a field, and every path enforces it the same way.

- `domain`: permissions as data (action, subject, fields, condition) and one pure evaluator; the four D-042 roles as
  seeded defaults with unchanged powers.
- `db`: roles and permissions as data, memberships naming a role by key (migration 0017), API tokens with hashes and expiry, an
  append-only audit log of role, membership and token changes; idempotency records stored for locality and cheap
  retention (time-ordered keys or daily partitions), since saves slow down 3.5x as that table grows (COMPARISON.md).
- `api`: one authorization call in every handler; field-level refusals on writes and field-level projections on reads;
  `own` conditions; read-only, full-access and custom API tokens.
- `studio`: Settings → Roles with a permission matrix (collections × actions, fields, "only their own"), the role
  picker on members and invitations, actions hidden when not permitted (the server still decides).
- `sim`: random roles and permissions against the evaluator; the API's decisions match it.

**Proof gate:** the evaluator's decision table is exhaustive for the built-in roles and property-tested for random
roles; every route has a denial test; field and condition tests for create, update, read and publish; a token can do
exactly what its permission set allows; a load scenario with custom roles shows the evaluator costs no regression.

**Steps:** R.1 roles as data, behaviour unchanged (done) · R.2 custom roles and the Studio's role editor (done) · R.3 field-level
permissions and `own` conditions (done) · R.4 API tokens and the audit log.

## M: Content modelling parity

**Goal:** the shapes Strapi and Sanity model (relations, reusable groups, lists, page builders, singletons) are content
types here, validated in Rust and kept valid as they change.

- Field types: references (one and many, with the other side listed), components/objects (nested fields), bounded
  lists of any type, block lists (Strapi's dynamic zones, Sanity's arrays of objects), email, URL, date, time, JSON,
  Markdown, colour; single types; field descriptions, defaults, groups and tabs; validation by pattern, uniqueness,
  length and value, and conditional fields.
- Migrations with a plan, a dry run, approval and per-entry outcomes for the changes D-055 refuses.
- The Studio's builder covers every type; the editor renders nested and repeated fields; references pick from search.

**Proof gate:** every type round-trips byte for byte; references never dangle (database-enforced); a migration that
fails halfway resumes without duplicates; the simulator covers schema changes racing saves.

## P4: Time travel and media

**Goal:** any release can be viewed and rolled back; uploads are safe.

- `domain`/`delivery`: rollback as a new generation, release history queries, revocation through historical URLs.
- `studio`: release slider, history and compare, restore as a new revision, media picker.
- `media`: upload intents, quarantine, signature and decoder checks, derivatives, reference tracking, cleanup.
- Preview capabilities on a separate origin; sitemaps and feeds from one generation; scheduled publishing with time
  zones and daylight-saving handling.

**Proof gate:** rollback and pinned-read tests; revoked content stays unavailable through historical generations;
malicious upload fixtures are rejected; L14 (large media workload keeps editing responsive); X07 (daylight-saving and
expired-approval scheduling).

## E: Editorial parity

**Goal:** everything an editorial team does in Strapi or Sanity, with nothing lost and every change attributed.

- Localization: locale variants with their own slugs, drafts and publication history (spec §06), the locale switcher,
  fallbacks as explicit policy.
- Scheduled publishing and unpublishing, with time zones and missed schedules shown as delayed (spec §06, §14). Built
  first, on the maintainer's request, and done (D-060, migration 0019): publish requests scheduled in an IANA zone,
  DST edges chosen explicitly, run as their author by the worker with every publish check, published once or blocked
  with the reason, delayed when late, cancellable.
- Review stages (Strapi's review workflows) on top of changesets; assignees; comments on fields and entries; tasks.
- Live presence and collaboration (O-017); duplicate, bulk actions, saved views, the history slider.
- Live queries, after Convex: the Studio's lists, drafts, changesets and receipts update as PostgreSQL commits, from one
  listener per server fanned out to bounded sessions, every page read at one snapshot (COMPARISON.md).
- Visual preview: the site renders a draft through a short-lived capability (spec §06), with click-to-edit.

**Proof gate:** two editors editing at once never lose an acknowledged edit; a schedule missed during downtime is
reported as delayed, never on time; locale variants publish independently; comments and tasks follow permissions.

## D: Developer parity

**Goal:** developers get what Strapi's and Sanity's APIs give them, through the same authorization.

- Query API: filters, sorting, pagination, field selection and reference population on drafts (for permitted callers)
  and on published content; Sanity-style perspectives (published, drafts, a release). Queries use only indexes the
  content type declares, and Studio and draft reads get the delivery cache's always-consistent rule (after Convex).
- GraphQL over the same queries and permissions; OpenAPI for REST; the TypeScript SDK with generated types.
- Webhooks by event (entry, release, media, schema), import from Strapi and Sanity exports with a dry run, export.
- Extension points instead of plugins (D-057): custom field types as data, Studio panels in sandboxed iframes.

**Proof gate:** every query is permission-filtered (no field or entry leaks across roles); GraphQL and REST answer the
same; an import of the Strapi and Sanity reference datasets round-trips; load scenarios for queries and GraphQL.

## S: Scale and performance (D-062)

**Goal:** Colophon runs as several servers and workers behind a load balancer, and every scenario has a known
breaking point and fails safely past it.

- `deploy`: a multi-instance stack (HAProxy in front of three servers, two workers) beside the single-server one.
- `server`: rate limits per principal and per address, shared across instances in Valkey and failing safe without it;
  readiness that drains before shutdown; a connection budget summed across instances.
- `perf`: a stress mode for every k6 scenario (raise the rate until the latency gate breaks; record the highest
  sustained rate and the failure mode); the same runs on one, two and three servers.
- Rust: Criterion benchmarks for hot paths with a CI regression check; profiles of each milestone's heaviest scenario.

**Proof gate:** a server killed under load behind the balancer fails only in-flight requests and idempotent retries
succeed; a draining server finishes what it accepted; rate limits hold across instances and when Valkey is down;
every scenario has a stored stress result with its knee and failure mode; throughput on three servers is measured
against one, with the bottleneck named.

## V: Versions and distribution (D-062)

**Goal:** anyone can install the CLI, SDK and MCP server without building Rust, and every piece says whether it fits
the server it talks to.

- One semantic version per release for the server, worker, CLI, SDK and MCP server; `/api/v1` is the contract.
- The server reports its version and API range; the CLI, SDK and MCP server explain an incompatibility in plain words.
- Release workflow: binaries for macOS (arm64, x64), Linux (x64, arm64) and Windows (x64) with checksums and signed
  provenance; `@colophon-cms/cli` on npm (a launcher with per-platform optional dependencies), `cargo install`, a
  Homebrew tap, an install script, and container images for the server and worker.
- `@colophon-cms/sdk` and the MCP server published from the same release.

**Proof gate:** a release dry run builds every artifact; `npx @colophon-cms/cli --version`, `cargo install`, the
Homebrew formula and the install script each install the same version on a clean machine in CI; an old CLI against a
new server (and the reverse) gets the plain-words answer, tested.

## P5: Safe automation (≈ spec M4)

**Goal:** scripts and agents can do permitted work at bounded scale and never execute an unauthorized action.

- `auth`: machine credentials, device-authorization login for the CLI, kill switch.
- `automation`: typed tools, bulk manifests with per-item outcomes, budgets reserved in Postgres, the MCP adapter;
  optionally the AI writing assistant (off by default).
- `cli`: the full JSON contract, `--jsonl` streams, `content import` (native JSONL) and `export`.
- Evaluation suite with repeated trials.

**Proof gate:** zero unauthorized actions across the evaluation set; X12 (an agent interrupted halfway through a bulk
edit resumes without duplicates or overwrites); L08 (a noisy agent is throttled; editors keep their latency).

## P6: Operate it (≈ spec M5)

**Goal:** someone other than the maintainer can install, upgrade, back up, restore and diagnose Colophon.

- `operations`: encrypted base backups and WAL archiving, the restore procedure (new epoch, deletion ledger replay,
  reconciliation before dispatch), the upgrade runbook, budgets, runbooks, observability.
- `verification`: the full fault and load matrix, a 24-hour soak, the benchmark report, the requirements matrix.
- `docs`: complete guides; every example runs in CI.
- `oss`: release process, SBOM, signed artifacts, security policy.
- Usability test with five editors; one independent install.

**Proof gate:** F09–F14, L05–L13, the 24-hour soak, a restore drill on clean infrastructure, the usability gate, one
independent install. **v0.1 release.**

---

## After v0.1

A business template with forms, and possibly a commerce showcase ("100 buyers, 1 item, exactly one wins", spec X01).
Localization, the schema designer and Strapi/Sanity import moved into milestones E, M and D (D-057). See
[modules/parked.md](modules/parked.md).

## Mapping to the spec's milestones

| Spec | Colophon |
|---|---|
| M0 contracts and editor spike | P0 |
| M1 core authoring | P1 |
| M2 release engine | P2 |
| M3 cache and evidence | P3 (time-travel UI and media pulled into P4) |
| M4 safe AI workflows | P5 (reframed as automation) |
| M5 integrated operation | P6 |
| M6 business and commerce | Parked (D-006) |
| M7 complete product acceptance | Partly in P6; the rest after v0.1 |
