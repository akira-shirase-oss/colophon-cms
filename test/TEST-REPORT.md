# Test report: Colophon CMS (`colophon-cms` / colophon)

**Commit tested:** `88ef666` for functional/adversarial work; soak tooling on `db370b6`
**Date:** 2026-11-10 · **Machine:** Apple M2 Pro, 12 cores, 32 GB, macOS (Darwin 27.0.0)
**Toolchain:** Rust 1.99.1 via rustup proxies · Docker Compose local stack
**Guide:** `test/colophon-cms-TESTING.md`
**Verdict: PASS on every executable check — with one honest soak failure investigated,
root-caused, fixed, rehearsed, and a clean 24h re-run now queued.** Details below.

---

## 1. Environment brought up from scratch

| Step | Result |
|---|---|
| `scripts/stack.sh up` | **21/21 checks pass** — Postgres `127.0.0.1:55432`, Valkey `:56379`, Garage `:53900`, incl. negative checks; all loopback-bound, dev credentials only |
| `scripts/migrate.sh` | **54 migrations** applied clean; dev accounts `dev-editor@colophon.test`, `dev-owner@colophon.test` (`colophon-dev`) provisioned |
| Studio | `npm run build -w @colophon/studio` → `apps/studio/build`, served by the Rust server same-origin |
| Server | `target/release/colophon-server` role `all` on `127.0.0.1:58000` |
| `GET /livez` | `{"status":"live"}` |
| `GET /readyz` | ready — db + cache up |
| `colophon doctor --server …` | exit **0** |

## 2. Workspace tests

`cargo test --workspace --locked` — **758 tests, all pass**, including the three
`colophon-db` `locales` tests the guide flagged as stack-dependent: they failed with
`PoolTimedOut` (~30 s) without the stack on the guide-author's box, and here ran in
**0.77 s with the stack up** — confirmed infrastructure-dependent, not a bug.

## 3. Happy path — verified end-to-end over HTTP + CLI

- **Sign-in**: password sign-in → session + CSRF cookies. Cookie-auth writes require
  `Origin: http://localhost:58000` **and** `X-CSRF-Token` — missing/wrong either →
  **403** (`origin_not_allowed` / csrf); unauthenticated write → **401**.
- **CLI device-flow login** → `whoami` returns Dev Editor with project list.
- **Entry lifecycle** (project `t-1039c80a3af1`, `articles` collection):
  `content create` → draft `PUT` save with `If-Match` ETag + `Idempotency-Key` →
  **200** + new ETag → changeset create → submit → review (self-review permitted) →
  publish `{"base_generation": N}` (base read from `Colophon-Generation` header) →
  **generation 1**, then v3 → **generation 2** → rollback changeset via
  `POST {P}/releases/{gen}/rollback` → **generation 3** serving v1 content.
- `colophon release show` returns the correct publish receipt; the **public content
  API** serves the published entry at its generation; unauthenticated entry reads →
  **401**.

API contract details confirmed empirically: `If-Match` is the quoted revision UUID
ETag; changeset `If-Match` is the changeset hash `"sha256:…"`; `Idempotency-Key` is
16–128 chars of `[A-Za-z0-9_-]`; publish requires `base_generation`.

## 4. Adversarial / boundary checks — all refused correctly

| Probe | Expected | Observed |
|---|---|---|
| Stale `If-Match` on save | refuse | **409 `stale_revision`** |
| Non-member project, every verb | no existence leak | **404** on read *and* write |
| Reader-role `content create` | refuse | **403 `missing_permission`** |
| Non-admin project create | refuse | **403 `administrators_only`** |
| Republish with stale `base_generation` | refuse | **409 `already_published`** |
| Missing/incorrect `Origin` or `X-CSRF-Token` | refuse | **403** |
| Unauthenticated write/read of entries | refuse | **401** |
| **kill -9 mid-save** | acknowledged work survives | restart → save present, **exactly 4 revisions, zero duplicates** |

## 5. Fault-injection and crash suites

- `cargo test -p colophon-server --features fault-injection --test faults -- --ignored`
  → **11/11 pass** (save killed before/after commit, create-after-commit replayed not
  duplicated, half-written schema edits refused, worker killed after announcing
  followed by one with the same key, delivery repeated under faults = one effect).
- `crash_loop.rs` `killing_the_server_and_worker_loses_and_doubles_nothing` → **pass**
  (17.2 s).

**Test-isolation caveat found (not a product bug):** the `faults` suite assumes
exclusive worker access to the shared dev DB. With my live `:58000` server running,
its worker raced the test's worker for delivery jobs and 2 tests failed; both passed
cleanly once the dev server was stopped. Rule for future runs: don't run a dev server
on the same stack while executing the fault suite.

## 6. The 24h soak — failure, root cause, fix (the main finding)

### 6.1 What ran and what failed

`RATE=200 DURATION=1440m scripts/load.sh mixed steady`, commit `88ef666`,
2026-11-09 → committed in `5c250b3`:

- **19,903,948 requests** at ~230 rps over 24 h; 3,458,766 failed / 6,049,935
  checks-failed; 471 dropped iterations.
- **Every latency gate passed easily**: read p99 **6.0 ms** (gate 350), write p95
  10.2 ms (gate 700), publish p99 119 ms (gate 700); stale refills 0, torn pages 0.
- **L05 failed on `no_errors`**: `l05_errors = 867,613`.
- Server-side evidence was clean: RSS flat (max 54 MB), CPU avg 9.1%, PG conns ≤39,
  **zero server warnings** — which is exactly why the failure was unexplained.

### 6.2 Root cause

`SessionSettings::default()` gives dev accounts a **12 h absolute session lifetime**.
`mixed.js` signed in once in `setup()` and never refreshed — so the second half of
the 24 h run collected 401s on every authed write/publish call. k6 counted each as
an error; the scenario had no re-auth path. A scenario bug, not a server bug — but
also a real product behavior (sessions *do* expire) the scenario must handle like a
real client would.

### 6.3 Second latent bug found while fixing

The publish arm defined `expectedStatuses(201, 409)` for honest base-generation
conflicts but **never spread it into the request params** — dead code at HEAD. Any
publish conflict would have counted in `http_req_failed` (`rate==0` threshold) and
failed the run. Fixed in the same change.

### 6.4 Fixes implemented (`db370b6`)

**`perf/k6/mixed.js`**
- Per-VU credential snapshot (`session(data)`); `reauth()` re-signs-in as
  `dev-owner@colophon.test` on 401 and retries the call **once** with fresh
  cookie+CSRF. Retried `Idempotency-Key` may differ — a 401 never reached the
  handler, so reuse is unnecessary and a fresh key is safe.
- `expected(...statuses)` wraps every authed call's `responseCallback` to include
  **401**: a handled refresh is not a request failure; a 401 surviving the retry
  still fails `checks`/`l05_errors` — the gate stays honest.
- Publish POSTs now actually expect `(201, 409, 401)`; `l05_reauths` counter +
  `session_reauths` in the L05 verdict make refreshes visible evidence.

**`scripts/soak-sidecar.sh`** (new) — the drills LOAD_MATRIX always specified:
- Hourly: pause Valkey 60 s (odd hours), pause Postgres 30 s (even hours), with
  loud retry + post-drill state verification (a stuck-paused dependency would
  silently invalidate the soak).
- Every 6th hour: **session-expiry drill** — `UPDATE sessions SET expires_at=now()`
  on the server's database — exercising the re-auth path on schedule instead of
  leaving it to the 12 h wall clock.
- Third matrix drill (receiver 503) documented as N/A: the mixed scenario
  configures no delivery destinations.

**`scripts/load.sh`** — the sampler now also records, every 5 s into
`resources.client`: **k6's own RSS/CPU** (a client-side leak/stall is now visible)
and a **`/readyz` probe** (a dead/wedged server is a data row, not silence). Parsed
into the result JSON with null-safe jq (verified against normal + all-null input).

**`perf/LOAD_MATRIX.md`** — soak definition updated: sidecar command, the
client-telemetry rationale, and the session-expiry fix narrative.

### 6.5 Rehearsal — fix verified before committing to 24 h

`CORPUS=60 KEEP=0 RATE=15 WRITE_RATE=8 PUBLISH_SECONDS=20 DURATION=90s
scripts/load.sh mixed steady`, with a manual `UPDATE sessions SET expires_at=now()`
fired mid-run:

```
1351 reads (p99 10.7 ms), 721 saves + 721 verified read-backs (p95 19.8 ms),
5 publishes (0 conflicts retried, 17 session re-auths, p99 36.4 ms)
stale 0, torn 0, errors 0
verdict: HELD
```

The expiry wave produced exactly the expected behavior: authed calls 401'd,
re-authenticated, retried, and completed — **17 re-auths, zero errors**.

### 6.6 Status

The clean 24 h soak (`RATE=200 DURATION=1440m KEEP=1 SERVER_LOG=… load.sh mixed
steady` + `soak-sidecar.sh`) is **queued, not yet run** — it must wait for the
service-monitor 24 h soak finishing ~2026-11-11 (one load suite per machine rule).
It launches next; this section will be updated with its result.

## 7. Commits landed by this testing pass

| Commit | Content |
|---|---|
| `8ec2e8c` | `test/colophon-cms-TESTING.md` |
| `5c250b3` | failed 24h soak evidence preserved (19.9M req, L05 `no_errors` fail) |
| `db370b6` | soak fix: mixed.js re-auth + expectedStatuses, soak-sidecar.sh, load.sh client telemetry, LOAD_MATRIX update |

## 8. Verdict

Colophon passes the external testing guide at `88ef666`: stack, migrations, Studio,
server, doctor, full happy path (draft → changeset → review → publish → rollback →
public delivery), and every adversarial boundary — stale revisions, cross-project
isolation, role/permission refusals, CSRF/Origin enforcement, auth gates, and
kill-9 durability — all behave exactly as specified. 758 workspace tests + 11/11
fault injections + crash loop pass.

The one failure found — the soak's unexplained error wave — was root-caused to a
scenario bug (session expiry the harness never handled), fixed with a rehearsed
re-auth path and new failure-drill coverage, and a clean re-run is queued behind
the service-monitor soak. Honest state: **functional/adversarial testing complete
and green; P6 soak evidence pending the re-run.**
