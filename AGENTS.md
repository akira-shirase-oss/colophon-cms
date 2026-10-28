# Colophon CMS: agent instructions

**Status (2026-10-27): milestone P3 in progress.** P0 is complete: steps 0.1 to 0.7 built the pinned toolchain and
workspace, the local stack in `deploy/`, the error envelope and health checks, `apps/testbed` and the k6 harness in
`perf/`, the rich-text format with its editor, validator and renderer, and `packages/ui` with the Studio's first screen
(D-013 to D-030). Step 1.1 is done: OpenID Connect sign-in through Keycloak (`colophon-auth`), server-side sessions with
CSRF and `Origin` checks, projects isolated by row-level security (`colophon-db`, migration 0001), the testbed's Session
page, and the signed-in load scenario `perf/k6/session.js` (D-031 to D-033). Step 1.2 is done: entries with
insert-only revisions in the built-in `articles` collection, saves with `If-Match`, idempotent writes, the crash tests
F01 and F02, the testbed's Entries page and `perf/k6/entries.js` (D-034 to D-036). Step 1.3 is done: the server
serves the Studio at `/studio`; the Studio edits real drafts with autosave, a device copy of unsaved edits, and a
conflict screen (`apps/studio/tests/server/`, `scripts/studio.sh`, `perf/k6/studio.js`; D-037, D-038). Step 1.4 is
done: `crates/colophon-sim`, a deterministic simulator with mutation checks and a differential test against Postgres
(D-039); replay a failing seed with `cargo run -p colophon-sim -- --seed N --trace`. Step 1.5 is done: content
types (`colophon_domain::schema`, migration 0003, `{P}/content-types`, the Studio's type-driven editor,
`perf/k6/content-types.js`; D-040). Step 1.6 is done: `colophon login`/`logout`/`whoami`/`content create`/
`content list`, device sign-in approved at `/studio/device`, bearer tokens on every route (migration 0004,
`crates/colophon-server/tests/cli.rs`, `perf/k6/cli.js`; D-041). Step 1.7 is done: roles enforced by one policy
table (`colophon_domain::policy`), members and single-use invitation links (migration 0005, `colophon_db::members`,
the Studio's Members and `/studio/invite` pages, `perf/k6/members.js`; D-042). Step 1.8 is done: `colophon admin
bootstrap` makes the first administrator once, and only administrators create projects (migration 0006,
`colophon_db::administrators`, `perf/k6/projects.js`; D-043). Tests create projects as `dev-owner`, which
`scripts/migrate.sh` bootstraps; a Keycloak started before 1.8 must be recreated (deploy/README.md). P1's proof gate
is met; see `docs/ROADMAP.md` for what carries forward. P2 has begun: step 2.1 is done, changesets that pin exact
revisions and reviews bound to their hash (`colophon_domain::changeset`, migration 0007, `{P}/changesets`, the
Studio's changeset pages, `perf/k6/changesets.js`; D-044). Step 2.2 is done: publishing commits a release on the
publisher's base generation, publication history is non-overlapping intervals, and `/content/v1` reads a published
copy as `colophon_delivery` (migration 0008, `colophon_db::{releases, delivery}`, `perf/k6/delivery.js`; D-045; rerun
`scripts/stack.sh up` on an older stack to create the role). Step 2.3 is done: a release commits its delivery jobs
(the outbox), and `crates/colophon-worker` (`--role worker|all`) claims them under leases as `colophon_worker` and
posts webhooks with a stable idempotency key to public addresses only (migration 0009, F03 in `tests/faults.rs`,
F05 in `tests/worker.rs`, `perf/k6/publishing.js` with `perf/receiver/`; D-046). Step 2.4 is done: the simulator's
publishing scenario (`colophon-sim --scenario publishing`), one announcement in flight per destination (migration
0010), and the real-process crash loop (`tests/crash_loop.rs`; D-047). P2 is done. P3, proving it reached the site: step 3.1 is done, a
generation-keyed Valkey cache for `/content/v1` with the generation always read from Postgres, single-flight fills, a
Postgres fallback and ETags (migration 0011; D-048). Step 3.2 is done: unpublish with tombstones and origin revocation,
and CDN purge jobs (migrations 0012, 0013; D-049, D-050). Step 3.3 is done: probes and a receipt for every publish,
evidence scoped to the locations checked (migration 0014; D-051). Step 3.4 is done: the reference site in `apps/web`,
built from one release and promoted atomically (D-052, `perf/k6/site.js`). Step 3.5 is done: title search, entry states,
revision comparison and review diffs, and the design system: every shadcn-svelte component in `packages/ui` on
Colophon's tokens (D-053, D-056), shown at `/studio/design`. Step 3.6 is done: slugs, and a content-type builder whose
changes are checked against stored content (migration 0016; D-054, D-055). Step 3.7 is done but for a check at phone
width: plain words from one glossary (`apps/studio/src/lib/words.ts`), one-click publish, a first-run checklist, and
sign-up with a one-time setup code for a fresh installation (migration 0020, `perf/k6/setup.js`; D-059, D-061).
Milestone R, roles and permissions as data (D-058): R.1 (the four built-in roles as data, behaviour unchanged;
migration 0017), R.2 (custom roles and the Studio's role editor; migration 0018, `perf/k6/roles.js`) and R.3 (field
lists and `own` conditions; migrations 0021, 0022) are done. Scheduled publishing, from milestone E, is done (migration
0019, the worker's scheduler, `perf/k6/schedules.js`; D-060). R.4 is built, its full load runs pending: project API
tokens that act as their own identity, kept away from access control, with an exact kill switch, and an append-only
audit log (migration 0023, `colophon_db::{tokens, audit}`, `scope::begin_as_token`, `perf/k6/tokens.js`; D-063). Milestone M has begun: field types (M.1, D-064), references that never dangle (M.2, migrations 0024; D-065)
and objects, lists and page-builder blocks (M.3, migration 0025; D-066) are built, their full load runs pending; single types (M.4, migration 0026; D-067), text patterns (M.4, D-069), unique fields (M.4, migration 0027; D-070), defaults (M.4, D-071), conditional fields (M.4, D-072) and shared components (M.4, migration 0029; D-073) too. Milestone V (D-062)
is built: one version (`0.1.0-alpha.1`, `colophon --version`, `GET /version`, the CLI's compatibility check), a
release workflow written but not yet run on GitHub, the `@colophon-cms/cli` npm launcher, an install script and a
Homebrew formula template; publishing waits on the maintainer (npm organization, `NPM_TOKEN`, a licence, a tag; see
module 19). Next: step 3.8 (the site goes live, the first public demo) and the rest of M.4. D-062 adds milestones S (several servers behind a load balancer, stress
runs, Rust benchmarks; S.1, the multi-instance stack and failover proof, is built: `scripts/cluster.sh`, `scripts/failover.sh`, D-075) and V (versions, and the CLI, SDK and MCP server packaged for npm, Cargo and Homebrew).

## Read first

1. `docs/END_GOAL.md`: what done means and what is in or out of scope.
2. `docs/ARCHITECTURE.md`: sources of truth, request paths, repository layout; `docs/SYSTEM_DESIGN.md` for flows,
   consistency, capacity and failure handling.
3. The module doc for the area you are touching, under `docs/modules/`.
4. `docs/DECISIONS.md`: Colophon's deliberate deviations from the master specification.
5. `docs/spec/master-spec.txt`: the full master specification (grep it; the `.docx` beside it is the original).

When documents conflict, the order of authority is: `DECISIONS.md` > module docs > `ARCHITECTURE.md` /
`END_GOAL.md` > master spec. If you change a design, update the module doc in the same change, and record any new
deviation from the spec in `DECISIONS.md`.

## What this project optimizes for

**Proof over breadth.** The maintainer's goal is an open-source and portfolio project that is genuinely useful and not
"AI jargon". A feature counts only when its failure behaviour is tested and its evidence is stored. Never trade a
failing invariant for a new feature, and never relabel uncertainty as success to make a dashboard look clean.

## Non-negotiable invariants (summary; full lists live in the module docs)

1. **PostgreSQL is the only authority** for content, revisions, releases, permissions, jobs and budgets. Redis and the
   CDN are disposable accelerators; losing them can never lose data or bypass a permission check.
2. **No database transaction spans an external call** (model, CDN, object store, identity provider). Commit an intent
   to the outbox, execute, store evidence, reconcile uncertainty.
3. **Truthful states.** `202 accepted` is not "published"; an accepted purge is not "served"; unknown stays `uncertain`.
4. **Bounded everything.** Every queue, channel, body, page, cache, retry loop and pool has a limit and a deadline.
5. **One API, no bypass.** Studio, CLI, SDK and agents call the same Rust services with the same authorization.
6. **Agents propose; the platform enforces.** Draft-only by default, exact-change approvals, budgets reserved in
   Postgres, no self-approval.
7. **Release generations only move forward.** Rollback is a new generation; a stale worker can never promote an older one.

## Rust rules

- On the maintainer's machine, use the `rust-mastery` skill: read `~/Desktop/Learning/rust-mastery/AGENT_PLAYBOOK.md`
  and the chapters it routes to (07 and 12 are mandatory for async and service code).
- The toolchain is pinned to Rust 1.99.1 (`rust-toolchain.toml`, D-013). On the maintainer's machine Homebrew
  `cargo`/`rustc` 1.96.0 come first on `PATH` and **ignore the pin**, so prefix every Cargo command with rustup's
  proxies: `export PATH="$(brew --prefix rustup)/bin:$PATH"`. `scripts/check-toolchain.sh` fails if you forget.
- `colophon-domain` must not depend on `tokio`, `axum`, `sqlx`, `reqwest` or any I/O crate. The build enforces the
  architecture.
- Typed errors (`thiserror`) in library crates; `anyhow` only in the two binaries. No `unwrap`/`expect` on external
  input. Every I/O await has a deadline. No secrets, tokens, prompts or article bodies in logs.

## Frontend rules

- SvelteKit + TypeScript (strict). The Studio is a client-side app served as static assets by the Rust server on the
  same origin. SvelteKit never queries PostgreSQL or holds CMS authority.
- Rich text is stored as validated Tiptap JSON, never HTML: the `colophon/rich-text` v1 format
  (`docs/formats/rich-text-v1.md`). Rust revalidates every mutation. Change the format only with new fixtures, and keep
  every existing valid fixture valid.

## Ownership (D-019)

- Claude builds every crate and app, including `colophon-domain` and `colophon-db`; the maintainer reviews.
- Database migrations, authentication changes, publishing transitions and provider adapters are listed in the step's
  build-log entry for a line-by-line review (spec §34).

## Testing rule (D-018, from the maintainer)

Every feature ships with all three, in the same branch:

1. **Regression tests** in CI on every push: Rust unit tests, in-process API tests, and black-box tests against the
   real binary and the local stack. Every fixed bug gets a test that fails without the fix.
2. **Testbed coverage:** `apps/testbed` (separate from the Studio) exercises the feature through the real HTTP API,
   with Playwright tests in CI.
3. **A load scenario** in `perf/k6/` with thresholds. Full runs send at least 1,000,000 requests; results go in
   `perf/results/` with the machine and commit; misses and regressions fail. Never present laptop numbers as
   production capacity.

## Working rules

- Work in small slices. Each issue states the user outcome, the data invariant, the failure case, the permission, and
  the acceptance test (spec §34).
- Report exactly what ran and what passed. Never call something verified, benchmarked or secure without an executed run
  and stored evidence.
- The repository is `github.com/akira-shirase-oss/colophon-cms` (remote `git@github-learning:akira-shirase-oss/colophon-cms.git`).
  Work happens on a branch and is merged into `main` when a piece is complete; the journey is recorded in the
  build log at https://404human.dev/colophon/ (source: `akira-shirase-oss/minimal-portfolio`).

## Verification commands

`scripts/check.sh` runs exactly what CI runs (D-014): the toolchain check, `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, `cargo doc` with
`-D warnings`, `scripts/check-architecture.sh` (no I/O crates in `colophon-domain`), `cargo deny check`, ShellCheck,
and, when the local stack is running, CI's integration half: `scripts/stack.sh check`, the `#[ignore]`d tests that need
the stack (`cargo test --workspace -- --ignored --test-threads=1`; they stop and restart services), `scripts/testbed.sh`
(svelte-check and Playwright), `scripts/studio.sh` (the Studio against the real server) and the `scripts/load.sh`
smoke runs (health, rich-text validation, signed-in requests, drafts, the Studio). Run it before every push. Full load runs:
`scripts/load.sh throughput|steady [--baseline]`; commit the result files they write.

The local stack (`scripts/stack.sh up`, details in `deploy/README.md`) binds every port to `127.0.0.1`; its credentials
are public development values. The database has two logins: `colophon` owns the tables and runs migrations
(`scripts/migrate.sh`); the server uses `colophon_app`, which row-level security applies to. After changing the Keycloak
realm in `deploy/keycloak`, run `docker compose -f deploy/compose.yaml up -d --force-recreate keycloak`.

Adding a migration whose tables new `sqlx::query!` calls use: `scripts/migrate.sh` compiles those queries before it can
migrate, so apply the migration with sqlx-cli first, then refresh the offline data, and commit `.sqlx/`:

```sh
export DATABASE_URL=postgres://colophon:colophon-dev@127.0.0.1:55432/colophon
cargo sqlx migrate run --source crates/colophon-db/migrations
SQLX_OFFLINE=false cargo sqlx prepare --workspace
```

The crash tests (F01, F02) build only with the fault points:
`cargo test -p colophon-server --features fault-injection --test faults -- --ignored` (`check.sh` and CI run them). Never change the bind address, and never reuse those credentials outside `deploy/`.
