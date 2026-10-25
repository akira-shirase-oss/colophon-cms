# Colophon CMS: agent instructions

**Status (2026-10-25): milestone P0 in progress.** Step 0.1 is done: the pinned toolchain, a five-crate Cargo
workspace and CI. Step 0.2 is done: the local stack in `deploy/` (D-015 to D-017). Step 0.3 is done: the error envelope,
request IDs, `/livez`, `/readyz`, `colophon doctor`, `apps/testbed`, and the k6 harness in `perf/` (D-018 to D-020).
Step 0.4 is done: `docs/formats/rich-text-v1.md`, `fixtures/rich-text/v1`, `packages/editor` and the Studio's editor spike
(D-021, D-022). Next is step 0.5: the Rust validator that refuses every invalid fixture with its exact code and pointer.
See `docs/ROADMAP.md`.

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
(svelte-check and Playwright) and a 200,000-request `scripts/load.sh` smoke run. Run it before every push. Full load runs:
`scripts/load.sh throughput|steady [--baseline]`; commit the result files they write.

The local stack (`scripts/stack.sh up`, details in `deploy/README.md`) binds every port to `127.0.0.1`; its credentials
are public development values. Never change the bind address, and never reuse those credentials outside `deploy/`.
