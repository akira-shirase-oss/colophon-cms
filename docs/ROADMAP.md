# Roadmap

Eleven milestones, each named after what it proves. Since D-057 the product goal is every feature Strapi and Sanity
offer ([COMPARISON.md](COMPARISON.md)), built at Colophon's bar: the four lettered milestones carry that parity work and
sit between the numbered ones in the order below. Estimates assume ~45 hours/week (D-010) and are guesses, not
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
journey and plain-words test, the testbed's Setup page and `perf/k6/setup.js`; phone-friendly review checked at
phone width since the Studio's completion work) · 3.8 my site goes live, and the first
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
permissions and `own` conditions (done) · R.4 API tokens and the audit log (built: D-063; tokens that act as their own
identity, an exact kill switch, an append-only log; the million-request runs of `perf/k6/tokens.js` wait for a quiet
machine, so not yet done).

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

**Steps:** M.1 seven field types, descriptions and minimum lengths (built: D-064) · M.2 references that never dangle,
checked by the database on save and by every release, with "referenced by" (built: D-065, migration 0024) · M.3
objects, lists and page-builder blocks, references inside them (built: D-066, migration 0025) · M.4 single types
(built: D-067, migration 0026), patterns (built: D-069), uniqueness (built: D-070, migration 0027), defaults (built:
D-071), conditional fields (built: D-072) and shared components (built: D-073, migration 0029) · M.5 migrations with a plan, a dry run and per-entry outcomes; the simulator racing
schema changes and saves. M.5a is built (D-100, migration 0040, `{P}/migrations`): a plan is computed under
the collection's change lock, dry-runs every draft and open publication into per-entry items, is approved and
applied against its hash, blocks schema writes while it runs, and takes resolutions for entries that need
input; M.5b is built too (D-101, migration 0041): the worker drives a `running` plan under an advisory-lock
claim, `fill`/`move` operations transform or a stored `resolution` replaces each item, per-item outcomes
commit durably (crash-resumable; F-05b proves it), and the plan ends `done` or `blocked` with
`republish_required` pinning open publications to republish. M.5c is built too: the Studio's Migrations
section (a list of plans and a plan's page — counters, a filterable item table, a resolution dialog, and
hash-bound approve/apply/cancel gated on `schema.change`), the "Plan a migration" affordance on the type
builder's `needs_migration` refusal, the testbed's `/migrations` page, `perf/k6/migrations.js`, and the
simulator's schema/save races (invariant V: no revision is written against a superseded version, with the
`stamp-seen-version` mutation proving it's checked). What is built has its tests, a testbed page (`/modelling`) and a load scenario
(`perf/k6/modelling.js`) whose million-request runs wait for a quiet machine; the Studio's builder and editor cover
M.1 to M.3, single types and patterns (branch `studio/dashboard`).

## P4: Time travel and media

**Goal:** any release can be viewed and rolled back; uploads are safe.

- `domain`/`delivery`: rollback as a new generation, release history queries, revocation through historical URLs.
- `studio`: release slider, history and compare, restore as a new revision, media picker.
- `media`: upload intents, quarantine, signature and decoder checks, derivatives, reference tracking, cleanup —
  built through staged deletion with tombstones and the retention sweep (D-098, D-099); the CDN purge path for
  deleted bytes is the remaining gap.
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
  first, on the maintainer's request (D-060, branch `e/schedules`): publish requests scheduled in an IANA zone, DST
  edges chosen explicitly, run as their author by the worker with every publish check, published once or blocked
  with the reason, delayed when late, cancellable.
- Review stages (Strapi's review workflows) on top of changesets; assignees; comments on fields and entries; tasks.
  Comments and tasks' backend built in step E1 (D-102, branch `e1-comments-tasks`, migration 0042): comments
  bind immutable revisions — never moving anchors — verbatim forever with a resolve/reopen pair; tasks carry
  title, assignee, status and a done pair; both answer the entries' `content.read`/`content.update` with the
  `own` condition, survive their entry's deletion readable, and write idempotently with audits. Their surfaces
  are built too (branch `e1-surfaces`): a Comments tab on the entry editor binds the version on screen and
  answers or reopens comments on their etags, and a project Tasks page lists, makes and edits them — nobody
  sent as `assignee: null`; the testbed's Collaboration page and `perf/k6/collaboration.js` exercise the loop.
  E2 builds the review stages (D-104, branch `e2-stages`, migration 0043): an environment may name an
  ordered list of them (`GET`/`PUT {P}/review-stages`, `schema.change`-gated, `ETag`'d, idempotent and
  audited), each needs an approval on the changeset's current hash — in any order — for it to publish, a
  request for changes at any stage blocks it, a stage's `different_reviewer` narrows who may approve, and
  an environment with no list keeps the single implicit review exactly as before. Their surfaces are built
  too (branch `e2-surfaces`): the changeset page lists each stage in order with its state and approver, every
  review names the stage it decides, and Settings' Review stages page edits the list under its ETag; the
  testbed's Review stages page and `perf/k6/stages.js` exercise the gated publish.
- Live presence and collaboration (O-017); duplicate, bulk actions, saved views, the history slider.
- Live queries, after Convex: the Studio's lists, drafts, changesets and receipts update live as PostgreSQL commits,
  from one listener per server fanned out to bounded sessions, every page read at one snapshot (COMPARISON.md). E3's
  backend half is built (D-108, branch `e3-live-lists`, migration 0044): commit-time triggers `pg_notify` compact hints
  on `colophon_events`, each API process holds one `LISTEN` connection that fans them out to `{P}/events` SSE streams
  (per-credential/per-project/per-server bounds, resync-not-replay). The Studio's wiring is built too (branch
  `e3-surfaces`): the project shell holds one stream and turns each hint's kind into the invalidations a fresh visit
  would run, so a member's save lands in an owner's list without a reload or a poll — and a save on an entry being
  edited stays a conflict, never a silent overwrite.
- Visual preview: the site renders a draft through a short-lived capability (spec §06), with click-to-edit.

**Proof gate:** two editors editing at once never lose an acknowledged edit; a schedule missed during downtime is
reported as delayed, never on time; locale variants publish independently; comments and tasks follow permissions.

## D: Developer parity

**Goal:** developers get what Strapi's and Sanity's APIs give them, through the same authorization.

- Query API: filters, sorting, pagination, field selection and reference population on drafts (for permitted callers)
  and on published content; Sanity-style perspectives (published, drafts, a release). Queries use only indexes the
  content type declares, and Studio and draft reads get the delivery cache's always-consistent rule (after Convex).
  DX.1–DX.6 are built (D-103, D-105, D-107, D-109): the bounded document (`colophon_domain::query`), the hash-bound
  cursor, `POST {P}/collections/{collection}/entries/query` running it over current drafts with `content.read`
  scoping, `POST /content/v1/{project}/{environment}/{collection}/query` running the same document over the
  published projection — the release `?generation=` pins — with the canonical query echoed and hashed into
  every cache key, `depth` reference population on both perspectives: batched per level, narrowed to the
  caller's `content.read` on drafts, pinned to the page's generation on delivery, an unresolved target
  honestly keeping its stored id, and the information leak closed: a draft query is refused where a filter or
  sort names a field the role can't read on every row its page could hold, while `fields` still narrows freely
  with `hidden_fields` disclosing the rest. Still to come: the perspectives the drafts and previews take.
- GraphQL over the same queries and permissions; the TypeScript SDK with generated types. The `OpenAPI` 3.1
  document for REST is built (DX.7, D-110): emitted from the one route table the router is built from, served
  at `GET /openapi.json` unauthenticated with `ETag`/304, committed byte-for-byte as `docs/api/openapi.json`
  (80 paths, 104 operations, 110 schemas). The TypeScript SDK is built (DX.8, D-112): `@colophon-cms/sdk` in
  `packages/sdk` — `src/api.types.ts` generated from the committed document by `openapi-typescript` and
  drift-checked the same way, plus a hand-written `Client` (typed methods over the core surface, the error
  envelope parsed into `ApiError`, `If-Match`/`Idempotency-Key` required by the types, bounded reads,
  generation-pinned delivery via `atGeneration`); no second auth surface — a credential is attached, never
  minted. GraphQL is built (DX.10, D-115): `POST {P}/collections/{collection}/graphql` builds an `async-graphql`
  dynamic schema per request from the project's content types — `entries(where, orderBy, first, after)` and
  `entry(id)`, a node type per collection the role may read — and every call's arguments and selection compile
  to the same canonical document `entries/query` runs, validated and executed by the shared `run_query`
  pipeline in one transaction (`applied.query`/`queryHash` byte-equal to REST's echo), refusals surfacing as
  GraphQL errors carrying the document's own `code`/`pointer` in `extensions`. Read-only: mutations stay REST,
  delivery stays `/content/v1`.
- Webhooks by event (entry, release, media, schema), import from Strapi and Sanity exports with a dry run, export.
  Import parity is built (DX.9, D-114): `POST …/collections/{collection}/import` reads native JSONL and JSON
  arrays, marked Strapi envelopes, Strapi v4 `{id, attributes}` and v5 `documentId` documents, and Sanity NDJSON —
  `colophon_domain::transfer::map` normalizes each against the declared fields — and `?dry_run=true` plans the
  same pipeline under savepoints in a transaction that never commits, so the plan writes nothing, needs no
  `Idempotency-Key`, and answers a per-item outcome (`created`/`updated`/`skipped`, or `would_create`/
  `would_update`/`skipped`/`invalid`, with `source`, `unmapped` metadata and refusals pointer-prefixed by the
  item's place). `?type=` filters a mixed Sanity export; a source UUID updates a live entry's draft or names a new
  one's id. Still bounded at 512 and one transaction; resumable staging is P5's. Webhooks stay to come.
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
  **Built:** the benchmarks (`crates/colophon-bench`), `scripts/bench.sh` and the pull-request check that times base
  and head on one runner (D-074); the profiles are still to do.

**Built (S.1, D-075):** `scripts/cluster.sh` (three servers and two workers behind HAProxy), `CLUSTER=1
scripts/load.sh`, and `scripts/failover.sh kill|drain`, whose first runs passed at 100 writes a second; the readiness
probe got a connection of its own after the first drain run found every server going unready together. **Built (S.2,
D-076):** rate limits per address and per credential, shared in Valkey and applied by each server without it. **Built
(S.3, D-077):** `scripts/load.sh <endpoint> stress`, the knee and failure mode of any scenario; each step stores the
failed requests the servers logged, so a full pool shedding load (`shed load`) is told apart from errors. **Built
(D-078):** parsed content types cached by their definition's hash, after the benchmarks found every entry request
parsing its type again (281 µs to 6.5 µs for a 100-field type). Next: stress results for every scenario and the one-,
two- and three-server comparison, on a quiet machine.

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

**Status (2026-10-28, branch `v/versions-distribution`).** What ran is listed in modules 08 and 19, "Tests and
evidence".

- Built and tested: the version is `0.1.0-alpha.1`, one for every crate and the npm packages (module 19,
  "Versions"); `colophon --version` and `colophon-server --version` print version, commit and build date
  (`crates/colophon-version`); `GET /version` reports the release and `api: ["v1"]` (module 04); the CLI checks it
  before every command that talks to a server and says in plain words whether it is too old or too new (module 08,
  "Versions"), which covers the gate's last item for the CLI with canned servers in both directions.
  `@colophon-cms/cli` and its five platform packages, assembled from release archives (module 08, "Install"); the
  install script; the Homebrew formula template; `cargo install --locked --git` (run here from a local clone).
- Written, not yet run in CI: `.github/workflows/release.yml` (five targets, `SHA256SUMS`, CycloneDX SBOMs, signed
  build provenance, a draft release, npm publishing behind an approval; a manual run is a dry run that publishes
  nothing) and `ci.yml`'s `distribution` job. Its first dry run is the first real test of the cross-builds (musl
  with aws-lc, Windows, macOS x64 from arm64).
- Waits on the maintainer: create the npm organization `colophon-cms`; create the `npm` environment with required
  reviewers and `NPM_TOKEN` (an npm token that can publish to `@colophon-cms`) in its secrets; confirm the licence
  (O-014) and add `LICENSE`; add `CHANGELOG.md` with a `## 0.1.0-alpha.1` section; run the workflow by hand once as a
  dry run; then push the tag `v0.1.0-alpha.1`, check the draft release and publish it, and approve the npm job. For
  Homebrew, create the tap `akira-shirase-oss/homebrew-colophon` and commit each release's `colophon.rb` as
  `Formula/colophon.rb`.
- Not built yet: crates.io publishing (`cargo install colophon-cli`, after O-014 and publishing the crates it
  depends on); the SDK (it doesn't exist yet); packaging the MCP server (`crates/colophon-mcp`, built in P5, D-068) for npm,
  Cargo and Homebrew, and its checks; container images for the server
  and worker; the server archive without the Studio's files; CI installs of the published packages, the formula and
  `cargo install` on clean machines, which need a published release.

## P5: Safe automation (≈ spec M4)

**Goal:** scripts and agents can do permitted work at bounded scale and never execute an unauthorized action.

- `auth`: machine credentials, device-authorization login for the CLI, kill switch.
- `automation`: typed tools, bulk manifests with per-item outcomes, budgets reserved in Postgres, the MCP adapter;
  optionally the AI writing assistant (off by default).
- `cli`: the full JSON contract, `--jsonl` streams, `content import` (native JSONL) and `export`.
- Evaluation suite with repeated trials.

**Proof gate:** zero unauthorized actions across the evaluation set; X12 (an agent interrupted halfway through a bulk
edit resumes without duplicates or overwrites); L08 (a noisy agent is throttled; editors keep their latency).

**Steps:** the MCP adapter over local stdio (built: D-068, closing O-013 with a thin JSON-RPC layer;
`crates/colophon-mcp`, MCP 2025-11-25, eighteen typed tools over `/api/v1` with a project API token: read and query
content, releases, migration plans, comments and tasks; write drafts from the revision read, comments and tasks,
make a publish request and ask for its review; nothing approves, publishes, resolves, reassigns or drives a
migration — D-106; tool discovery follows the token's permissions; unit tests and a stack test over stdio) ·
the first piece of the enforcement itself (built: D-111 — `POST {P}/changesets/{id}/reviews` refuses
`agent_self_review` whenever an API token would approve a changeset a token submitted, on the implicit review and
every named stage alike, so no credential can greenlight a proposal with no person in the loop; a token may still
ask for changes, approve a person's changeset under `content.review`, and publish a person-approved changeset under
`content.publish`; covered by `tests/tokens.rs`) ·
the budget substrate (built: P5.2, D-113, migration 0045 — `api_token_limits` ceilings per token and the
`budget_reservations` ledger: the accepting transaction locks the limits row, counts `held`/`settled`/`uncertain`
allowance and writes `held` or refuses `budget_exceeded` (403); documents reserve at `entries::create`/`save`/
`create_localization`, a publish reserves one `jobs` slot per outbox dispatch, `jobs::finish` settles or releases it
with the outcome, superseded pending jobs' reservations release with the supersede, and the worker's sweep closes
reservations whose outcomes are on record and flags stale held rows `uncertain` as `budget.discrepancy` audit
events; limits and usage are managed and read only on the people-facing `POST`/`PATCH`/`GET …/tokens/{token}`
routes; `requests_per_minute` is consumed at request time by D-118's quota); ·
bulk manifests and jobs (built: P5.3, D-116, migration 0046 — `{P}/bulk` keyed chunks of exact entry ids and
base revisions, at most 100 items and 1 MiB a chunk, executed one transaction per item by the worker as the
original submitter: outcomes `applied`/`conflict`/`refused`/`skipped`, a resumable `v1:`-sealed item cursor,
per-item `documents` reservations with the `jobs` slot on accept, revocation and membership removal stopping
work mid-run; covered by `tests/bulk.rs`) ·
the evaluation suite (built: D-117 — `crates/colophon-server/tests/evals.rs`, one ignored, serial stack test
with a scoped API token — or a real `colophon-mcp` stdio session — as the adversary: the six §27 abuse cases
plus the token-privilege and MCP refusal surfaces, every call classified `refused`/`permitted`/`unauthorized`
against typed-refusal oracles and owner-read end state, `COLOPHON_EVAL_TRIALS` repetitions and the
`UNAUTHORIZED: 0` gate; `scripts/check.sh` runs it at three trials, `COLOPHON_EVAL_RESULTS` stores the
artifact) ·
the request-time quota (built: D-118 — `requests_per_minute` consumed as a rolling window in the
admission limiter, keyed `colophon:quota:{project}:{token}`, checked where the token's scoped
transaction opens: 429 `rate_limited` with `Retry-After`, `token.rate_limited` audited once per
throttle episode, members untouched, Valkey-outage semantics the shared limiter's own,
`COLOPHON_QUOTA_WINDOW_MS` shortening the window for tests; covered by `tests/quotas.rs` and
`perf/k6/quotas.js`) ·
the CLI's machine contract and the MCP entry point inside it (built: D-119 — every command's `--json` is
one versioned `{"version", "command", "data"|"error"}` envelope carrying API refusals whole, the seven list
commands stream one envelope per item on `--jsonl` following `next_cursor` within a 1,000-page bound, and
`colophon mcp` links the `colophon-mcp` library into the binary: same configuration, protocol, eighteen safe
tools and refusal boundary, proved end to end in `tests/cli.rs` including a live `initialize`/`tools/list`
session and a 403 arriving `missing_permission`-whole on stdout) ·
L08's load evidence (built: D-120 — `perf/k6/noisy-agent.js`, one run in two sequential
phases: editorial traffic alone, then the same traffic at the same rate beside a
60-a-minute token attempting ~600/s and an editor in a second project, every request
tagged by arm so `handleSummary()` computes the spec's 2x-p95 comparison from the numbers
the run just measured into the stored `l08` object, which `scripts/load.sh` carries into
the result file and fails the run on; the stored run
`perf/results/2026-11-04T154012Z-noisy-agent-steady-apple-m2-pro-12c.json` shows the agent
99.7% refused with `Retry-After`, editorial p95 x1.83 of its baseline and project-B's
x1.86 of its own) ·
remote MCP (built: D-121 — `POST|DELETE {P}/mcp` on the API speaks MCP's Streamable HTTP transport, the same
`colophon-mcp` session logic as stdio over a shared `Backend`; a project API token is the only credential —
a person's is 403 `api_token_required`, another project's 404 — sessions are minted on `initialize`, bounded
per token (429), project and server (503), expire idle and cancel their in-flight requests on `DELETE` or
expiry; a tool's call is dispatched in-process through the server's own router under the same token, so
dispatch is never authorization and the D-106 boundary is unchanged, proved in `tests/mcp.rs` by the
lifecycle, the credential refusals, the transport's `Origin`/`Accept`/protocol-version door checks and
byte-identical parity with stdio) ·
provider `tokens`/`cost` budgets (built: D-122, migration 0047 — `api_token_limits` gains
`tokens_per_day` (1…1,000,000,000) and `cost_per_day_micros` (1…100,000,000,000 micros of the
accounting unit, a fixed-point integer), `Kind::Tokens`/`Cost` wire to their columns, and
`reserve` admits under the ceiling or refuses `budget_exceeded` with the day counted as
`coalesce(settled, reserved)` — open reservations at their allowance, settled outcomes at what
they consumed; the ceilings ride the same `POST`/`PATCH`/`GET …/tokens/{token}` surface and the
usage read gains `tokens_last_day`/`cost_last_day`; covered by `tests/budgets.rs` in
`colophon-db` and `colophon-server`; what still waits on the assistant is a call site that
reserves them) ·
the CLI's import and export (built: D-123 — `content import` sends the file the API accepts verbatim,
checked only the way the API will read it (every JSONL line or array element parses, the file isn't
empty, at most 512 items, a `--file` within the largest request body); `--dry-run` plans each item and
sends no key, an applied import sends one `cli-<uuid>` or `--idempotency-key`'s; `content export`
streams `GET …/export` a line at a time — the file itself on text, one envelope a document on `--jsonl`,
the bounded `{"collection", "entries"}` on `--json`, no `--cursor` since the endpoint doesn't page;
proved end to end in `tests/cli.rs`, the file checks unit-tested in `src/content.rs`) ·
not built: nothing — the milestone is closed, the optional assistant aside. The proof gate is met — X12's resume proof is in
`tests/bulk.rs`, the eval gate's stored run is `perf/results/2026-11-04T141109.623685Z-evals.json`, and
L08's is `perf/results/2026-11-04T154012Z-noisy-agent-steady-apple-m2-pro-12c.json`.

## P6: Operate it (≈ spec M5)

**Goal:** someone other than the maintainer can install, upgrade, back up, restore and diagnose Colophon.

- `operations`: encrypted base backups and WAL archiving, the restore procedure (new epoch, deletion ledger replay,
  reconciliation before dispatch), the upgrade runbook, budgets, runbooks, observability. Built so far: the
  deletion ledger itself (D-124 — append-only `deletion_ledger` written inside every governed delete's transaction;
  `admin ledger export`/`replay` as the off-host copy and the restore primitive; stack-proven in
  `colophon-db/tests/ledger.rs`). Still ahead: the backups, the restore procedure that drives the replay, and the
  rest.
- `verification`: the full fault and load matrix, a 24-hour soak, the benchmark report, the requirements matrix.
  Built so far: the L11–L13 dependency-failure runs (D-127) — `perf/k6/{cache-down,postgres-trouble,purge-trouble}.js`
  inject real faults through the receiver's `/control` (container pauses on Postgres/Valkey, a SIGKILLed and
  respawned worker, a 503-ing purge sink with `Retry-After`, a frozen site generation) while traffic arms measure
  baseline/margin/outage/recovery windows; each exported summary carries its `l11`/`l12`/`l13` verdict, gated by
  `scripts/load.sh`, with stored evidence under `perf/results/`.
- `docs`: complete guides; every example runs in CI.
- `oss`: release process, SBOM, signed artifacts, security policy.
- Usability test with five editors; one independent install.
- built: backups' capture half (D-125) — `archive_mode=on`, `wal_level=replica`, `archive_timeout=60s`, WAL segments
  into the `postgres-wal-archive` volume through `deploy/postgres/archive-wal.sh` (the local stand-in for object
  storage; the stock image has no S3 tooling), `scripts/backup.sh` capturing the cluster by `pg_basebackup` as the
  REPLICATION-only `colophon_backup` role, gpg-symmetric encryption from a passphrase file, `manifest.json` recording
  commit/Postgres/migrations/key-reference/ledger mark, and `verify` re-running `pg_verifybackup` on the decrypted
  bytes — the drill's stored run is `perf/results/2026-11-04T173702Z-backup-drill-apple-m2-pro-12c.json` (capture
  252 s, verify 138 s) ·
  not built: the restore procedure itself (new epoch, ledger replay, reconciliation), scheduling, retention jobs,
  asset versioning, upgrade runbook, observability, the drills and installs of the proof gate.

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
