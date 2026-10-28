# 08 · cli (`crates/colophon-cli` → binary `colophon`)

> The terminal as a first-class interface. Every normal operation is scriptable, with stable JSON and honest exit
> codes, and never needs a database password.

| | |
|---|---|
| Milestones | P1 (login, content create/list), P2 (release and jobs commands), P3 (doctor, integration test), P5 (full JSON contract, import, export), P6 (init, migrate plan/apply, admin) |
| Proposed owner | Claude |
| Depends on | `clap` 4 (derive), the shared HTTP client, request/response types (open), OS keyring |
| Used by | developers, scripts, agents (directly or through the [automation](09-automation.md) adapter) |
| Spec | §09, §11 |

## Why it exists

Developers and agents operate from terminals, and both need behaviour they can rely on in scripts. That means the same
authority as the Studio, machine-readable output, no hidden prompts, and exit codes that say what kind of failure
happened. The spec lists two slightly different command sets (§09 and §11). This document is the single merged
contract (D-008).

## End goal

Every normal operation is available through `colophon`, with stable JSON output and exit codes. It calls the same API
as the Studio, never needs a production database password, and never guesses the environment for a destructive
command. Snapshot and parity tests guard the contract.

## Responsibilities

- The command surface and its output contracts.
- Exit codes.
- Profiles and credentials.
- Idempotency keys for retries.
- Dry runs.
- Waiting on asynchronous operations.
- Operator (admin) mode.

## Not its job

- Business rules or authorization. The API enforces them.
- Direct database access, except in the separate operator mode.
- Running production infrastructure.

## Breakdown

### Commands (merged contract, D-008)

```text
colophon init my-site --template blog
colophon dev
colophon login --profile staging
colophon doctor --env staging --json
colophon schema validate
colophon schema diff --env staging --json
colophon migrate plan --env staging --out plan.json
colophon migrate apply --env staging --plan plan.json
colophon content create posts --file article.json --json
colophon content get posts SLUG --env production [--generation N] --json
colophon content list posts --limit 20 --json
colophon content import posts --file posts.jsonl --dry-run
colophon release create --from changeset.json --json
colophon release validate RELEASE_ID --json
colophon release publish RELEASE_ID --env production --dry-run
colophon release publish RELEASE_ID --env production --approval ID
colophon release status RELEASE_ID --json
colophon release history --env production --json
colophon jobs inspect JOB_ID --json
colophon integration test DESTINATION_ID --json
colophon export --env production --out export-directory
```

- `content get --generation` and `release history` are Colophon additions for time travel.
- Built in step 1.6 (D-041): `colophon login [--server] [--profile] [--name]` (device sign-in approved in the Studio;
  credentials in `credentials.json`, mode 0600, under `COLOPHON_CONFIG_DIR`, `XDG_CONFIG_HOME/colophon` or
  `~/.config/colophon`), `colophon logout` (revokes the token), `colophon whoami`, `colophon content create
  COLLECTION --project ID|SLUG (--data JSON | --file PATH) [--idempotency-key KEY]` and `colophon content list
  COLLECTION --project … [--limit N]`. `--profile` holds the server; `--project` stands in for profile defaults until
  profiles carry a project.
- `module enable` from spec §11 is dropped, because v0.1 has no optional modules.
- `colophon dev` may orchestrate local dependencies but must never discover or use production credentials silently.

### Operator mode
- `colophon admin …` covers bootstrap and infrastructure migrations.
- Built: `colophon admin migrate` and, in step 1.8, `colophon admin bootstrap --subject S [--issuer I]
  [--json]` (D-043): the first administrator, once. Both read `COLOPHON_MIGRATION_DATABASE_URL`; the issuer defaults
  to `COLOPHON_OIDC_ISSUER`. `colophon whoami` prints your subject, so an operator can sign in once and then bootstrap
  themselves.
- It is the only path with direct infrastructure access, and it requires a distinct mode.
- Normal commands never need a production database password.

### Versions (D-062, built in milestone V)
- `colophon --version` (and `-V`) prints the version, the git commit and the build date:
  `colophon 0.1.0-alpha.1 (commit 50558a7…, built 2026-10-28)`. `colophon-server --version` prints the same line.
  The stamp comes from the binary's build script (`crates/colophon-version`; module [19](19-oss.md), "Versions"); a
  build without git and without `COLOPHON_COMMIT` says `commit unknown` rather than inventing one.
- Before it does anything else, every command that talks to a server (`login`, `whoami`, `content create`,
  `content list`) reads the server's `GET /version` ([api](04-api.md)) without sending a token, and stops with a
  plain explanation when this colophon doesn't fit that server. The rule (`src/compat.rs`), in order:
  1. the server serves the API version this colophon speaks (`v1`); otherwise this colophon is too old (the server
     only serves newer API versions) or too new (only older ones);
  2. before 1.0, both are on the same minor line (any 0.1.x with any 0.1.x, pre-releases included), because until
     1.0 a minor release may change the API (SemVer item 4); a server on a later line means this colophon is too old,
     an earlier line too new. From 1.0 on, `/api/vN` is the contract and a newer server that still serves `v1` fits;
  3. the server is at least `OLDEST_SERVER` (now 0.1.0-alpha.1), the oldest with every route this colophon calls;
     raise it in the change that makes a command call something older servers lack.
- A server that answers `/version` with the 404 envelope predates version reports, so this colophon is too new for
  it. An answer that isn't a version report at all exits 6 ("is it a Colophon server?"); no answer exits 5.
- The message names both versions and says what to do, for example: `colophon 0.1.0 is too old for the server at
  https://cms.example, which runs 0.2.0: before 1.0 each minor release may change the API, so colophon and the server
  must share a minor version, and the server is on a later one (0.1 here, 0.2 there). Install colophon 0.2.0 to match
  it (…/releases).`
- `logout` skips the check on purpose: signing out must work even against a server this colophon can't otherwise use.
- `doctor` doesn't check versions yet (it reads `/readyz` only).

### Output (spec §09, §11)
- Default output is for humans.
- `--json` writes exactly one versioned JSON result to stdout.
- `--jsonl` streams one documented event per line.
- Logs, progress and diagnostics go to stderr.

### Exit codes (spec §11)

| Code | Meaning |
|---|---|
| 0 | Success (including `202 accepted`, reported as `accepted`) |
| 2 | Usage or validation error, including a colophon too old or too new for the server (retrying can't help) |
| 3 | Unauthorized or denied |
| 4 | Conflict |
| 5 | Transient failure or dependency unavailable |
| 6 | Internal failure |

### Behaviour rules
- **Asynchronous work:** a `202` exits 0 with status `accepted`. `--wait --timeout` reports the eventual state or a
  wait timeout, without cancelling accepted work.
- **Dry runs:** `--dry-run` validates scope and presents a plan with no external side effects. It is not a reservation;
  execution rechecks revisions, schema hashes, permissions and approval.
- **Retries:** one idempotency key per logical operation, reused on retry after transport loss, never regenerated per
  attempt.
- **Environments:** destructive and publishing commands require an explicit `--env`. Production is never inferred from
  the current directory.
- **Credentials:** they come from a protected profile, the OS keyring, or explicit environment injection. Token values
  never appear in process arguments or output.
- **Agent mode** is non-interactive and never waits for a hidden confirmation.

## Invariants

| ID | Invariant |
|---|---|
| C1 | `--json` prints exactly one versioned result on stdout; nothing else goes to stdout. |
| C2 | Exit codes map one-to-one onto error classes. |
| C3 | Agent mode never prompts. |
| C4 | A retried logical operation reuses its idempotency key. |
| C5 | Destructive and publishing commands never run without an explicit environment. |
| C6 | Secrets never appear in argv, stdout or stderr. |

## Interfaces

- Calls `/api/v1` with a machine credential or a device-login session.
- Profiles are stored in the user config directory, with tokens in the OS keyring.

## Failure modes

| Situation | Behaviour |
|---|---|
| Network lost mid-request | Retry with the same idempotency key; report the recorded result |
| Server returns 409 | Exit 4 with the conflict description in the JSON result |
| Server unavailable | Exit 5; `retryable: true` in the result |
| Missing `--env` on publish | Exit 2 before any request is sent |
| `--wait` times out | Exit 5 with the last observed state; the accepted work continues |

## Tests and evidence

- Versions (milestone V): the compatibility rule's unit tests (`src/compat.rs`: both directions of every reason,
  pre-releases, crossing 1.0, odd API lists, loose report parsing, the exact message); `tests/compat.rs` runs the
  binary against canned servers (a later line, an earlier line, no `/version`, a dropped API version, a reply that
  isn't a report, a fitting server that is then used, `whoami` and `content list` checking without sending the token,
  an unreachable server); `tests/exit_codes.rs` checks that `--version` prints the version, commit and date.
- `insta` snapshots of every `--json` result shape.
- An exit-code matrix test for every error class.
- Contract tests against a running server.
- The parity gate with the Studio and SDK ([api](04-api.md)).
- A test that no token appears in a `ps`-visible argument or output.

## Build order

1. **P1:** `login` (a device sign-in approved in the Studio), `whoami`, `logout`, `content create`, `content list`,
   `--json`, exit codes (step 1.6, done).
2. **P2:** `release validate|publish|status`, `jobs inspect`.
3. **P3:** `doctor`, `integration test`.
4. **P5:** scoped machine credentials, `--jsonl`, `content import`, `export`, `content get --generation`,
   `release history`.
5. **P6:** `init`, `migrate plan|apply`, `admin`.

## Open decisions

- Shared request/response types crate vs a Rust client generated from OpenAPI.
- A short alias for `colophon`.
- How agent mode is selected: a flag, an environment variable, or both.
