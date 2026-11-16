# Colophon load matrix — stability and scale evidence

Goal: concrete, committed evidence that every module holds under million-scale load, across
success, error, overload and dependency-failure paths — the "can it take traffic" record for
v0.1 (P6's proof gate: L01–L14, the fault matrix F01–F14, the 24-hour soak).

Rules (workspace standard): full runs are ≥1,000,000 requests (`scripts/load.sh <endpoint>
throughput|steady|stress`); results land in `perf/results/` + `history.jsonl` with machine and
commit; misses and regressions fail the run — a failing run is kept, never rerun until green.
**Honesty:** every number here is an Apple M2 Pro development observation with the whole stack
on one machine — evidence of behavior, saturation shape and failure handling, never a
production-capacity claim. `CLUSTER=1` runs measure the multi-instance topology separately.

Run order: single-instance first (stack up, release build), then `CLUSTER=1` shapes, then the
serial dependency-failure matrix, then the 24-hour mixed soak.

| Module | Scenario (`perf/k6/`) | What it proves | Evidence status |
|---|---|---|---|
| Health/admission | `health` | /livez /readyz at full rate, zero failures | committed (5M throughput, 20k/s steady) |
| Contract surface | `openapi` | served OpenAPI doc, full GETs + 304 revalidations | committed |
| Rich text | `rich-text` | validator at 20k/s+ throughput, invalid never passes | committed |
| Session/RLS | `session` | session lookup + row-level security on every request | committed |
| First-run | `setup`, `projects` | setup-code refusals, admin-only project creation | committed |
| Editing (multi-user) | `entries`, `content-types`, `modelling`, `inverses`, `locales` | concurrent saves with If-Match, mirrored references, localized writes — reads checked against last save | committed |
| Collaboration | `collaboration`, `live` | comments/tasks on etags; `{P}/events` SSE held open beside steady saves | committed |
| Schema ops | `migrations`, `roles`, `members`, `stages` | plan/dry-run/apply under load, role edits, invitations, stage-gated publishes | committed (full runs pending on the newest per ROADMAP notes) |
| Publishing pipeline | `changesets`, `publishing`, `schedules`, `deletion` | hash-bound reviews, release commits, outbox leases, scheduled publishes, tombstones | committed |
| Delivery reads | `delivery`, `cold-cache`, `stampede`, `cdn-warm` | anonymous latest/pinned reads, Postgres fallback, coalesced stampedes, per-region TTFB | committed (L03/L04/L10 evidence in results) |
| Automation surface | `cli`, `tokens`, `quotas`, `noisy-agent`, `bulk`, `graphql` | bearer auth, token scoping, rate limits, agent throttling (L08), bounded bulk, GraphQL reads | committed (L08 stored) |
| Media | `media-load`, `assets` | L14: uploads + fake/oversized rejection while editorial p95 holds | committed |
| Import/export | `import-scale`, `transfer` | L09: 10k-record import, per-item outcomes, resumable | committed |
| Audience | `audience`, `audience-sites` | site-user auth churn, gated reads, demo-site read shapes | committed + stress knee recorded (~1.8k rps) |
| Sites | `site`, `site-starter` | reference site + starter bundle while publishing every 2s | committed |
| Mixed editorial | `mixed` | L05: reads + If-Match writes + publishes at once, per-class budgets | committed |
| Spike/overload | `spike` | L06: 1x→5x delivery reads, controlled 429s, clean recovery | committed |
| Warm baseline | `warm-baseline` | L02: spec's 300/s warm mix for 30 min, hit-share + per-window p99 | committed (540k, 0 failures, hits 99.97%) |
| Smoke walk | `smoke` | L01: one signed-in user's whole journey, every answer checked | committed |
| **Dependency failure (serial only)** | `cache-down` (L11), `postgres-trouble` (L12), `purge-trouble` (L13) | Valkey pause → fallback, Postgres pause + worker SIGKILL → honest 5xx + resume, CDN 503 → bounded backoff | committed per D-127 — rerun on current commit |
| Cluster topology | `failover`, `CLUSTER=1 <scenario>` | kill/drain a server under load through HAProxy — only in-flight requests lost | committed (D-075) — spot-rerun entries/delivery |
| Micro-benchmarks | `scripts/bench.sh` + `compare main` | per-request hot-path medians, PR regression gate | on demand |
| **24-hour soak** | mixed-weight run (warm baseline + entries + publishes + failure drills cycling) | P6 gate: no leak, no drift, honest behavior for 24 h | **OWED — the last evidence before v0.1** |

## Gaps to close this pass

1. Re-run the dependency-failure trio (L11–L13) and `failover` on the current commit — evidence
   exists but predates P7.
2. Confirm full ≥1M results exist for `modelling`, `deletion`, `roles`, `tokens`, `migrations`,
   `collaboration`, `live`, `audience` — several were "full load runs pending" in earlier status;
   run any missing.
3. Define and run the P6 24-hour soak: weighted mix (reads 70%, saves 10%, publishes 5%,
   audience/sign-in 5%, misc 10%) plus hourly failure drills; commit the run + RSS/connection
   series.
4. Update this table's Evidence column to the exact result filename after each run.

## 2026-11-07 pass — first-time scenarios + fault drills on current commit

- Pass: accounts 303,744@337/s, inverses 1,300,200, quotas 1,000,006, transfer 1,000,130, audience 1,000,524
- Fault trio pass: cache-down 77,898 (0 failed), postgres-trouble 24,061 (0 failed), purge-trouble 7,994 (0 failed)
- Cluster pass: failover-kill + failover-drain (cluster3s2w) — 6,001 writes, 0 lost, both verdict pass
- Kept misses: openapi p99 101ms (455KB doc), deletion p99 339ms, tokens p99 157ms; collection-delete 144 failed + 239 warnings — open, needs classification
- Remaining: monitor-style 24h mixed soak (P6 gate) not yet started

## collection-delete classified (2026-11-07)

The earlier run's 144 failures + 239 warnings were **load contamination**, not a defect: a
jsrt sustained harness ran concurrently on the same machine (killed mid-run for exactly this).
Verified by three clean reproductions on the same commit — 20k and 200k bounded slices plus the
full 1M-iteration rerun (1,800,125 requests, 0 failed, 0 warnings, p99 93.8ms). Evidence:
`perf/results/2026-11-07T130953Z-collection-delete-throughput-apple-m2-pro-12c.json`.
Rule confirmed: never run two load suites on one machine at once.

## P6 24-hour soak — definition

Run: `RATE=200 DURATION=1440m KEEP=1 SERVER_LOG=perf/results/soak-24h-server.log scripts/load.sh mixed steady`
(mixed = L05: reads + If-Match writes + publishes at once — the closest shape to real editorial
traffic; 200/s sustains ~17.3M requests/day). In parallel,
`scripts/soak-sidecar.sh 24 <log> <windows.jsonl>` cycles one failure drill per hour
(pause Valkey 60s, pause Postgres 30s, every sixth a session-expiry; the receiver 503
window applies only to scenarios that configure delivery destinations — the mixed soak
configures none) and appends each completed drill's window — pause/resume plus
`SOAK_DRILL_GRACE_S` (default 120 s) of recovery — as a JSONL record. Pass the file to
load.sh as `DRILL_WINDOWS=<path>`: mixed.js stamps every failed assertion
`L05FAIL <epoch_s>` on k6's console (VU state cannot reach handleSummary — k6 metrics
only aggregate, and open()/fs are init-stage only), load.sh tees that stream and
re-weighs the stamps against the windows before gating —
the L05 `no_errors` gate then means *zero failures outside the announced windows*, with
the in/out split recorded under `l05.drills`. Server WARN/ERROR lines get the same
treatment (`drills.warnings_in/outside_windows` in the result), and a missing or
malformed window line counts its errors as outside — drills can excuse evidence, never
hide it. `SOAK_DRILL_INTERVAL_S` compresses the cadence for shorter validations (the
1 h proof runs used ~8 min drills so all three drill types fire inside the hour).
Both against the same `load.sh` server, never two load suites at once.

Measured: per-window p99 stays inside the gate, RSS/connection curves flat (the result's
`resources` block now also carries k6's own RSS/CPU and a /readyz probe series per 5 s —
added after the 2026-11-09 run failed L05 with 867,613 unexplained errors and left no
client-side evidence), outbox drains after each drill, zero `uncertain` states left
unresolved at end.

Scenario fix that preceded the re-run (2026-11-11): the first soak's errors were the dev
accounts' 12 h absolute session expiry expiring mid-run — the second half of the day
collected 401s. `perf/k6/mixed.js` now snapshots credentials per VU and re-authenticates
(dev-owner sign-in) on 401 with one retry, and publish/write VUs read `Colophon-Generation`
and If-Match honestly instead of racing. A session-expiry drill is part of the sidecar's
rotation so the re-auth path is exercised, not just the fault endpoints.

Honest bound: this is one machine's development observation over a day — evidence of
stability and failure behavior, not a production capacity claim.
