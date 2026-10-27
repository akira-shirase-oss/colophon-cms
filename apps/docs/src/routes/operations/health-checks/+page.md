<script>
	import Status from '#lib/components/Status.svelte';
</script>

Every `colophon-server` process, whatever its role, answers two health endpoints without sign-in, and `colophon doctor` reads one of them from a terminal. They answer different questions: `/livez` says whether the process should be restarted, `/readyz` whether it should get traffic. Both send `Cache-Control: no-store`, so no proxy keeps an old answer. The code is `crates/colophon-api/src/health.rs` and `crates/colophon-server/src/probes.rs`.

## GET /livez

```http
GET /livez HTTP/1.1

HTTP/1.1 200 OK
cache-control: no-store
content-type: application/json

{"status":"live"}
```

It checks nothing but that the process answers. A database outage doesn't fail it, on purpose: restarting a server can't fix a database. Use it for a process supervisor's restart decision, never for routing traffic.

## GET /readyz

Whether this process can safely serve now, with each dependency's state:

```http
GET /readyz HTTP/1.1

HTTP/1.1 200 OK
cache-control: no-store
content-type: application/json

{"status":"ready","role":"all","checks":{"database":"up","cache":"up"}}
```

| Field | Values |
|---|---|
| `status` | `starting`, `ready`, `degraded`, `not_ready` or `draining` |
| `role` | the process's role: `api`, `worker` or `all` |
| `checks.database` | `unknown`, `up` or `down` |
| `checks.cache` | `unknown`, `up`, `down` or `not_configured` |

The body names states only: never a host, a port or an error message. Why a probe failed goes to the log.

### Each status

| Database | Cache | Status | HTTP | Send traffic? |
|---|---|---|---|---|
| not probed yet | any | `starting` | 503 | no |
| down | any | `not_ready` | 503 | no |
| up | up, or not configured | `ready` | 200 | yes |
| up | down, or not probed yet | `degraded` | 200 | yes |
| any | any, after a shutdown signal | `draining` | 503 | no |

The cache can only make a server `degraded`, never unready: it is disposable, so losing it makes public reads slower (they come from PostgreSQL) but never wrong. A unit test holds every combination of the table (`every_combination_has_the_documented_status`), and another the exact JSON shape.

### Each check

| State | Meaning |
|---|---|
| `unknown` | Not probed yet: only in the first moments after start. |
| `up` | The last probe answered within its deadline. |
| `down` | The last probe failed or missed its deadline. |
| `not_configured` | No `COLOPHON_CACHE_URL`: this install has no cache. |

## How the probes work

The handler only reads a few numbers in memory. Background tasks do the checking:

- **The database:** `SELECT 1` through the API's own pool, every `COLOPHON_PROBE_INTERVAL_MS` (2 s), each within `COLOPHON_PROBE_TIMEOUT_MS` (1 s). So a pool that is full or a database that is slow shows as `down`.
- **The cache:** `PING` over one reused connection, at the same interval and deadline; after any failure it starts again from a fresh connection.
- Only changes are logged: an outage is one `dependency is down` warning with its cause, then one `dependency is up`, not a line every two seconds.
- A supervisor watches the probe tasks. If one ever stops without being asked, its dependency is marked `down`, so readiness can't freeze at "up".

Because nothing is probed per request, calling `/readyz` costs no database work however often it is called. On an Apple M2 Pro laptop (12 cores, 32 GB) on 2026-10-25, a million health requests at 74,613 a second, half of them `/readyz`, saw 5 database transactions during the whole run (`perf/baselines/health-throughput-apple-m2-pro-12c.json`). That is one laptop, not a capacity figure.

## Shutting down

On `SIGTERM` or Ctrl-C:

1. `/readyz` turns `draining` (503) at once and never comes back, whatever the probes say;
2. after `COLOPHON_DRAIN_DELAY_MS` (5 s) the listener stops accepting connections, so a load balancer has had time to notice;
3. requests in flight finish, while the worker carries on;
4. if requests are still running `COLOPHON_SHUTDOWN_GRACE_MS` (25 s) after the signal, the server stops waiting for them and logs that it abandoned them;
5. then the worker stops claiming and gets 5 s for the attempts it has in flight, the probes stop and the database pool closes, each within 5 s, and the process exits. A delivery cut off this way is claimed again once its lease runs out.

`crates/colophon-server/tests/lifecycle.rs` runs the real binary through this, and also stops the stack's real PostgreSQL and Valkey to check that readiness follows each outage and recovery.

## colophon doctor

```sh runnable
cargo run --release -p colophon-cli -- doctor --server http://localhost:58000
```

```text
server    http://localhost:58000
role      all
status    ready
cache     up
database  up
```

It reads `/readyz` rather than probing anything itself, so it says exactly what a load balancer would, and shows checks the server adds later without a CLI release. It exits 0 only for `ready`; 5 for any other status, and when the server doesn't answer within `--timeout-ms` (5,000 by default); 2 for a `--server` that isn't an `http(s)` URL; 6 for an answer that isn't a readiness report. `--json` prints one result, on failure too. [Command line](/reference/cli/#colophon-doctor) has the JSON.

## What readiness doesn't check

- **That the migrations are applied.** A server started against a database that is behind reports `ready` and then fails requests. Run the migrations before starting a new build ([Upgrades and migrations](/operations/upgrades/)).
- **The identity provider.** If it is down, sign-in fails but readiness stays `ready`: people already signed in keep working.
- **The worker's progress.** A worker that is up but whose deliveries fail shows in each release's receipt (*Needs attention*) and in `GET {P}/jobs`, not here. See [The worker](/operations/worker/).
- **Anything from another failure domain.** Module 18 wants health checked from outside the host; that is part of the production topology. <Status planned="P6" />

## Design

[Module 18, operations](/design/modules/18-operations/) ("Health"); built in step 0.3, with D-017 for the local stack's outside-in checks.
