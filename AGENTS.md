# Colophon CMS: agent instructions

**Current state (2026-11-06).** `main` carries migrations through 0054 and decisions
through D-146. The content-cards port era is finished — its unique work landed on this
lineage as D-083 to D-087 (migrations 0031–0034); the archived branch stays at
`origin/lineage/content-cards` for reference.

Every roadmap milestone is built; what remains before v0.1 is the human-run proof-gate
work: the rehearsed usability sessions and one independent install (P6's gate, D-132),
the 24-hour soak, and the first public demo (P3.8). Where each milestone stands (full
detail: `docs/ROADMAP.md` and the build log):

- **P0–P2 done:** foundation; never-lose-an-edit (insert-only revisions, `If-Match` saves,
  idempotent writes, the F01/F02 crash tests, `colophon-sim` differentially checked against
  Postgres — replay a failing seed with `cargo run -p colophon-sim -- --seed N --trace`);
  publish-exactly-once (changesets bound to their hash, numbered releases on a
  base generation, the leased outbox, the publishing simulator scenario and crash loop).
- **P3 built:** generation-keyed Valkey cache with a Postgres fallback and ETags,
  tombstoned unpublish, CDN purge jobs, probe receipts per location, the reference site,
  the Studio's design system and non-technical path. Step 3.8 (site live, recorded public
  demo) is the last open piece.
- **R built:** roles as data, the role editor, field lists and `own` conditions, project
  API tokens as their own identity with an exact kill switch, the append-only audit log.
- **M built:** every field type, references that never dangle, objects/lists/page-builder
  blocks, single types, patterns, unique fields, defaults, conditional fields, shared
  components, schema migrations (plan → dry run → per-item outcomes, crash-resumable), and
  entry deletion as dependency-checked tombstones.
- **P4 built:** rollback as a new generation, pinned reads, the media pipeline (upload
  intents, quarantine, derivatives, staged deletion).
- **E and DX built:** locales and localization groups, scheduled publishing with IANA
  zones, comments/tasks bound to pinned revisions, review stages, live lists
  (pg_notify → SSE); the bounded query document, OpenAPI emitted from the route table,
  `@colophon-cms/sdk`, GraphQL, Strapi/Sanity import.
- **S built:** the multi-instance stack and failover proof (`scripts/cluster.sh`,
  `scripts/failover.sh`), Valkey-shared rate limits, `scripts/load.sh … stress`,
  `colophon-bench` micro-benchmarks.
- **V built:** `0.1.0-alpha.1` across every binary, the `@colophon-cms/cli` npm launcher,
  the install script and the Homebrew template; publishing waits on the maintainer
  (npm org, `NPM_TOKEN`, licence, tag — module 19).
- **P5 done:** the MCP adapter (stdio and Streamable HTTP) with no self-approval enforced,
  budget reservations in Postgres, bounded bulk manifests, the eval suite gated at
  `UNAUTHORIZED: 0`, the L08 noisy-agent run committed.
- **P6 built:** WAL archiving and encrypted base backups, the six-stage restore with a
  recovery epoch, `backup.sh daily`, bounded-table retention sweeps, asset versioning, the
  upgrade rehearsal. Remaining gate items are the human-run sessions above.
- **P7 built:** site users and audience roles, magic links through the D-145 mail seam
  (Resend first; `COLOPHON_DEV_MAIL` on the dev stack), RLS-gated delivery, The Draft's
  members area end to end; built-in password accounts with an external provider optional
  (D-141, D-146). Five Astro demo sites (D-133) rebuild themselves on publish with live
  delivery updates (D-138, D-139).

Running locally: `scripts/migrate.sh` migrates the shared `colophon` dev database, then
`COLOPHON_STUDIO_DIR=apps/studio/build target/release/colophon-server` — Studio at
`http://localhost:58000/studio/`, sign in as `dev-owner`. Rebuild the Studio bundle with
`npm run build -w @colophon/studio` and the running server picks
it up from disk; the server binary needs a rebuild only for Rust changes. After touching `sqlx`
queries, re-prepare with `cargo sqlx prepare --workspace --all-targets` (plain `prepare` wipes
`.sqlx`).

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
- Typed errors (`thiserror`) in library crates; `anyhow` only in the binaries. No `unwrap`/`expect` on external
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
  build log at https://404human.dev/colophon/ (source: `akira-shirase-oss/site/minimal-portfolio`).

## Verification commands

`scripts/check.sh` runs exactly what CI runs (D-014): the toolchain check, `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, `cargo doc` with
`-D warnings`, `scripts/check-architecture.sh` (no I/O crates in `colophon-domain`), `cargo deny check`, ShellCheck,
and, when the local stack is running, CI's integration half: `scripts/stack.sh check`, the `#[ignore]`d tests that need
the stack (`cargo test --workspace -- --ignored --test-threads=1`; they stop and restart services), `scripts/testbed.sh`
(svelte-check and Playwright), `scripts/studio.sh` (the Studio against the real server) and the `scripts/load.sh`
smoke runs (health, rich-text validation, signed-in requests, drafts, the Studio). Run it before every push. Full load runs:
`scripts/load.sh throughput|steady [--baseline]`; commit the result files they write. Micro-benchmarks:
`scripts/bench.sh` (medians to `perf/results/`) and `scripts/bench.sh compare <base>` (both builds timed on this
machine; never compare numbers across machines). Benchmarks go in `colophon-bench`, not `colophon-domain`, whose
dev-dependencies change serde_json's maps (D-074).

The local stack (`scripts/stack.sh up`, details in `deploy/README.md`) binds every port to `127.0.0.1`; its credentials
are public development values. The database has two logins: `colophon` owns the tables and runs migrations
(`scripts/migrate.sh`); the server uses `colophon_app`, which row-level security applies to. The development accounts are `dev-owner@colophon.test` and `dev-editor@colophon.test`, password `colophon-dev`,
provisioned by `scripts/migrate.sh`. After changing the Keycloak realm in `deploy/keycloak` (the opt-in `oidc`
profile), run `docker compose -f deploy/compose.yaml --profile oidc up -d --force-recreate keycloak`.

Adding a migration whose tables new `sqlx::query!` calls use: `scripts/migrate.sh` compiles those queries before it can
migrate, so apply the migration with sqlx-cli first, then refresh the offline data, and commit `.sqlx/`:

```sh
export DATABASE_URL=postgres://colophon:colophon-dev@127.0.0.1:55432/colophon
cargo sqlx migrate run --source crates/colophon-db/migrations
SQLX_OFFLINE=false cargo sqlx prepare --workspace
```

The crash tests (F01, F02) build only with the fault points:
`cargo test -p colophon-server --features fault-injection --test faults -- --ignored` (`check.sh` and CI run them). Never change the bind address, and never reuse those credentials outside `deploy/`.
