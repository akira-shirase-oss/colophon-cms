# Testing guide: Colophon CMS

https://github.com/akira-shirase-oss/colophon-cms

Colophon is a Rust + SvelteKit CMS that treats PostgreSQL as the only source of truth and tries to prove what it published. Drafts are insert-only revisions (a save on a stale revision is refused), and a publish is meant to be exactly-once: a changeset pins exact versions, an approval counts only for that hash, and a worker killed mid-delivery is supposed to reconcile instead of repeating. This checkout is pre-release (`0.1.0-alpha.1`); there is no `LICENSE` file yet (Apache-2.0 is proposed).

Written against commit `88ef666` (2026-11-07).

## Prerequisites

- [rustup](https://rustup.rs) — the repo pins **Rust 1.99.1** in `rust-toolchain.toml` (edition 2024). Only rustup's `cargo` honors the pin; Homebrew's `cargo` ignores it.
- **Docker with Compose** — required. The local stack is PostgreSQL 18, Valkey, and Garage (optional Keycloak behind the `oidc` profile). First `scripts/stack.sh up` downloads about 1 GB of images.
- **Node 22.17+** and npm (the workspace `package.json` `engines` field). Use `npm ci` once.
- `curl`, `jq`, and `openssl` for the stack's own checks.
- Optional: [cargo-deny](https://github.com/EmbarkStudios/cargo-deny), [k6](https://k6.io) (`scripts/install-k6.sh` installs the pinned version).

On macOS, put rustup first if `scripts/check-toolchain.sh` complains:

```sh
export PATH="$(brew --prefix rustup)/bin:$PATH"
```

## Setup and run

```sh
git clone https://github.com/akira-shirase-oss/colophon-cms.git
cd colophon-cms

npm ci
scripts/stack.sh up          # Postgres :55432, Valkey :56379, Garage :53900; then a check pass
scripts/migrate.sh           # apply migrations, then create the two dev accounts
npm run build -w @colophon/studio

set -a; . deploy/local.env; set +a
COLOPHON_STUDIO_DIR="$PWD/apps/studio/build" cargo run --release -p colophon-server
```

In another shell, against the same env file:

```sh
set -a; . deploy/local.env; set +a
cargo run --release -p colophon-cli -- doctor
```

`doctor` should exit 0 only when the server is ready.

Then open:

- `http://localhost:58000/auth/login?return_to=/me` — sign in as **`dev-editor@colophon.test`** / **`colophon-dev`** (the README says `dev-editor`; the accounts `scripts/migrate.sh` actually creates are the `@colophon.test` emails).
- `http://localhost:58000/studio/` — the editor UI.

`dev-owner@colophon.test` / `colophon-dev` is an administrator. These credentials are public development values; never reuse them outside this stack. Everything binds to `127.0.0.1`. Ports, roles, and stack checks: `deploy/README.md`.

Useful alternatives:

```sh
scripts/check.sh             # what CI runs (fmt, clippy, tests, frontend, and integration if the stack is up)
scripts/testbed.sh --dev     # explore the testbed by hand
scripts/studio.sh            # Studio against the real server
scripts/first-run.sh         # empty database on :58400; the server prints a setup code, you become admin
cargo run -p colophon-cli -- login --server http://localhost:58000
```

`QUICK=1 scripts/check.sh` skips the integration half that needs the stack.

**What was verified on a Linux box (2026-11-10, America/Toronto):** Rust 1.99.1 compiles the workspace (`SQLX_OFFLINE=true` is set in `.cargo/config.toml`, so no database is needed to *build*). There is no Docker here, so the stack, migrate, and a live server were not started. Unit tests that don't need Postgres passed. `cargo test --workspace --locked` then failed in `colophon-db` `locales` (see below).

## Happy-path checklist

- [ ] `scripts/stack.sh up` finishes and `scripts/stack.sh check` passes.
- [ ] `scripts/migrate.sh` succeeds; `colophon doctor` exits 0.
- [ ] Sign in as `dev-editor@colophon.test`, land on `/me`, and see your projects.
- [ ] Open `/studio/`, create or edit a draft, and confirm a refresh still shows the saved text.
- [ ] Publish a change and watch the status move toward a receipt (saved → published → delivery / verified), not just a spinner.
- [ ] `colophon login` against `http://localhost:58000`, approve the code in the Studio, then `colophon whoami` prints the same account.
- [ ] Optional: `scripts/testbed.sh` (Playwright) or `scripts/frontend.sh`.

## Try to break it

These are the guarantees the repo actually claims. Prefer the project's own tests when you can; they already inject crashes.

**Crash-safe drafts**

- In the Studio, save a draft, then immediately kill the server (`Ctrl-C` or `kill -9` on the `colophon-server` PID) and start it again with the same env. An acknowledged save should still be there; it should not appear twice.
- Open the same entry in two sessions. Save in A, then save in B without reloading. The stale revision should be *refused*, not silently overwritten.
- Automated (needs the stack, and the fault-injection feature):

```sh
cargo test -p colophon-server --features fault-injection --test faults --locked -- --ignored --test-threads=1
```

The tests in `crates/colophon-server/tests/faults.rs` cover a save killed before or after its commit, a create killed after commit (replayed, not duplicated), and schema edits that must not land half-written.

**Exactly-once publish**

- Approve and publish something, then kill the worker mid-delivery and start the server again (`colophon-server` can run `--role api|worker|all`; the default is both). The same release should reconcile rather than deliver twice.
- The same `faults` test file covers a worker killed after announcing, a delivery repeated under faults remaining one effect at the destination, and `crates/colophon-server/tests/crash_loop.rs` (`killing_the_server_and_worker_loses_and_doubles_nothing`).
- `scripts/check.sh` also runs the deterministic simulator (`colophon-sim`) when you don't set `QUICK=1`.

**Boundaries that should stay closed**

- As `dev-editor`, try to read or write another project's content (including by guessing IDs in the API). It should be denied.
- Confirm a failed or rolled-back publish does not become the live site.

Don't treat a `PoolTimedOut` from `cargo test` without the stack as a product bug — see the note below.

## How to report a bug

Send Vinay (`akira-shirase-oss` / @404humandev) a note, or open a GitHub issue, including:

- Commit SHA (`git rev-parse HEAD`) and `colophon-server --version` or `GET /version` if the server started.
- OS, `rustc --version`, `node --version`, Docker / Compose versions.
- The exact commands you ran, expected vs actual, and relevant logs (redact nothing from the local stack — those credentials are already public — but don't paste real secrets if you pointed it at anything else).
- For draft/publish bugs: the entry/revision IDs, whether the save was acknowledged, and whether you killed the process before or after the response came back.

**Security issues** (cross-project reads/writes, policy bypass, recovered drafts, forged delivery): do **not** open a public issue. Use [GitHub security advisories](https://github.com/akira-shirase-oss/colophon-cms/security/advisories/new). Details are in `SECURITY.md`.

## Setup notes we hit

- No `LICENSE` file (the README says it will be added once the maintainer confirms Apache-2.0).
- This is the only one of these four repos with CI (`.github/workflows/ci.yml`).
- `crates/colophon-db/tests/locales.rs` needs the local Postgres stack but is **not** marked `#[ignore]`. `cargo test --workspace` without `scripts/stack.sh up` compiles, then those three tests fail after ~30s with `PoolTimedOut`. Other `colophon-db` tests are correctly ignored.
- The README login hint says `dev-editor`; provisioned accounts are `dev-editor@colophon.test` and `dev-owner@colophon.test`.
