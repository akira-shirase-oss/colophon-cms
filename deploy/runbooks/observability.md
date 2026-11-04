# Runbook: observability

What the running system tells you and what it does not (spec §32 "Operational signals",
module 18). This document is an audit, not a dashboard: every line below names what exists
today, the command that proves it, or the honest gap — no metric is claimed that no field
carries.

## The one model

Colophon emits **structured logs**, not a metrics endpoint. `tracing` writes one JSON
object per event (`COLOPHON_LOG_FORMAT=json`; `pretty` for humans locally), each event
carries its request span's fields inline, and `COLOPHON_LOG` is a standard `EnvFilter`
(`info` default; `info,tower_http=debug` turns on per-request lines — DEBUG is deliberate:
at serving rates, an INFO line per request would cost more than the requests). There is no
`/metrics` and no Prometheus text format: the spec asks for the signals to be *exposed*,
and they are exposed on the lines and endpoints named below. If an operation needs a
number, it is a log line or a SQL query away — the recipes say which.

## What a request line carries

A `tower_http=debug` response line, as emitted (proven live, this checkout):

```json
{"timestamp":"…","level":"DEBUG","message":"finished processing request",
 "latency":"273 μs","status":200,"target":"tower_http::trace::on_response",
 "span":{"commit":"75cec8a…","method":"GET","path":"/readyz",
         "request_id":"01a1a847-…","route":"/readyz","version":"0.1.0-alpha.1",
         "name":"request"}}
```

- `request_id` is a UUIDv7 minted per request, or the caller's `x-request-id` after
  sanitisation (printable ASCII, bounded length — unsafe client ids are dropped, never
  logged raw). It is echoed on the `x-request-id` response header and inside every error
  envelope (`error.request_id`), so a user report maps to exactly one log line.
- `route` is the matched route template — low cardinality; unmatched paths log
  `<unmatched>` rather than the raw path shape. `path` is the URI path **without the query
  string**, so tokens in query strings can never reach the logs.
- `version` + `commit` ride every request-attributed line: which binary served this
  request is a log fact, not a deploy guess (`GET /version` reports the same pair).
- `latency` is the response time in microseconds; `status` the HTTP status. Errors are
  logged where they happen with the envelope's `code` (`stale_version`, `not_ready`, …)
  rather than only here — a 200 with an error `code` upstream is how save conflicts read.

Beyond the request span, work events carry their own correlation ids where the spec names
them: `job`, `generation` and `attempts` on worker attempt lines; `project`/`project_id`,
`changeset`, `schedule` and `identity` (the actor) on API events; `generation` is the
release number wherever a publish is involved. They sit beside the request span's fields
in the same JSON object, so one request is traceable request → project → release → job →
destination (the destination id rides the job it belongs to, not the log line — see below).

## The spec's signals, signal by signal

| Spec signal | Where it is today | Honest state |
|---|---|---|
| request ID | `span.request_id` + `x-request-id` header + error envelope | built, proven |
| deployment version | `span.version` + `span.commit`, `GET /version` | built (this branch), proven |
| project, actor, revision, release | `project`/`project_id`, `identity`, `generation`, `changeset` on API events; save conflicts are `stale_version` errors inside the request span | built where the event has one — a bare request span does not resolve a project, by design |
| job, destination | `job`, `generation`, `attempts` on worker lines; the destination id is on the job row and in job events | built on worker lines; a destination-name field per delivery line is a possible refinement, not a spec gap — the job id joins it |
| HTTP errors and latency | `tower_http=debug` `finished processing request` lines: `latency`, `status` | built; aggregate by scraping logs |
| save conflicts | `stale_version`/`value_taken` error `code`s on the request line and in the error envelope | built |
| publish lag | `release committed` (API) → `job attempt recorded` (worker) events share `generation`/`changeset`; lag is the timestamp difference, or `jobs` age in SQL | derivable; no dedicated field — document the recipe, don't pretend it's a gauge |
| queue age | `jobs` table: `min(next_attempt_at) where status='pending'`; the restore inventory prints it too | SQL recipe below; not emitted periodically |
| failed/uncertain deliveries | `jobs.status` (`failed`/`held`), per-attempt `job attempt recorded` with attempts count, destination `503` retries | built; `held` after restore is the uncertain set |
| cache hit ratio | `Colophon-Cache` response header: `hit`/`miss`/`coalesced`/`bypass` — the L11 load run scored it this way | built; ratio = log scrape over the header's values |
| database pools and locks, WAL size | not emitted by the app — Postgres owns them: `pg_stat_activity`, `pg_stat_locks`, `pg_wal_lsn_diff` (recipe below) | honest gap: infra queries, runbook-listed; emitting them is a later slice |
| cache memory and eviction | Valkey `INFO memory` / `keyspace_hits`/`keyspace_misses` on the sidecar port | honest gap: infra query, runbook-listed |
| model budget consumption | the budget reconciler frees reservations and warns on settle/release failures (`job`, `error` fields) | built as warnings; a per-project consumption figure is a `provider_budgets`/`usage` query |
| dropped logs | nothing is dropped silently: the fmt writer is synchronous, so log pressure shows as latency, not loss | the spec's dropped-record counter is **not built** — there is nothing to count, which the sync-writer trade-off makes honest; a nonblocking writer would need the counter |
| OpenTelemetry-compatible traces | not built (spec marks export optional) | gap acknowledged; the request span is already the trace root an OTLP layer would hang off |
| log budget (7 days / 1 GiB / 16 KiB records) | not enforced by the app — container/host log drivers cap separately (spec allows) | **not built** in-app; production sets the log driver's rotation; the record cap is unimplemented |

**Never logged (O4):** draft text, model prompts, customer addresses and credentials stay
out by rule and by review — `path` drops the query string, request ids are sanitised, and
the error envelope names `code`, never internals. The audit's check is reading the log
during a smoke run; it stays a rule reviewers hold, not a filter that claims to work.

## Recipes

```sh
# Turn on per-request lines against a deployment (they are DEBUG by policy):
COLOPHON_LOG='info,tower_http=debug'

# Requests per status/route over a captured log:
jq -r 'select(.target=="tower_http::trace::on_response") | "\(.span.route) \(.status)"' \
  server.log | sort | uniq -c | sort -rn

# p99-ish latency per route (logs carry μs as text; awk buckets it):
jq -r 'select(.target=="tower_http::trace::on_response") | .latency' server.log \
  | tr -d ' μs' | sort -n | awk '{a[NR]=$1} END{print "p50",a[int(NR*.5)], "p99",a[int(NR*.99)]}'

# Errors by code:
jq -r 'select(.code) | .code' server.log | sort | uniq -c | sort -rn

# Save conflicts (409s that were stale_version):
jq -r 'select(.code=="stale_version") | .span.path' server.log | sort | uniq -c

# Cache hit share on delivery reads (the Colophon-Cache response header — log the header
# upstream, or measure at the client like perf/k6/delivery.js does):
#   k6 scores hit/miss/coalesced/bypass; no app-side counter exists.

# Queue age and backlog (as the migrator, read-only):
psql "$COLOPHON_MIGRATION_DATABASE_URL" -XAtq -c \
  "select status, count(*), min(next_attempt_at) from jobs group by status"

# WAL and lock pressure (Postgres owns these — the app does not):
psql "$COLOPHON_MIGRATION_DATABASE_URL" -XAtq -c \
  "select pg_wal_lsn_diff(pg_current_wal_lsn(), '0/0') as wal_bytes"
psql "$COLOPHON_MIGRATION_DATABASE_URL" -XAtq -c \
  "select state, count(*) from pg_stat_activity group by state"
```

## Health endpoints are the standing answer

`/livez` is process responsiveness only. `/readyz` reads the last background probe
(every 2 s, 1 s deadline — never inline work, D-075's own probe connection) and answers
`starting`/`ready`/`degraded`/`not_ready`/`draining`, JSON with per-dependency detail;
`colophon doctor --json` prints the same view with exit 5 unless ready. A dependency whose
probe stops reporting is marked down — readiness cannot freeze at "up". Behind the load
balancer (`scripts/cluster.sh`, `deploy/cluster/`) `/readyz` polled at 500 ms is the
traffic gate: `draining` drops a server within about a second of shutdown being asked.

## What to watch first

1. `readyz` leaving `ready` — the published degraded/not_ready signal is the alarm.
2. `hold` lines in `scripts/backup.sh retention` output — a backup directory that doesn't
   verify is a person-problem, flagged by the schedule.
3. `pg_wal` growth — the archiver failing is visible as disk, before anything else breaks
   (backup runbook's failure table).
4. `stale_version`/429 climbs on the error-code scrape — client pressure, not a crash.
