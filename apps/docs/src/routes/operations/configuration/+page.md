<script>
	import Status from '#lib/components/Status.svelte';
</script>

`colophon-server` reads its configuration once, at startup, from `COLOPHON_*` environment variables, and nothing is parsed again afterwards (`crates/colophon-server/src/config.rs`). It checks every value and reports **every** problem in one message, so one restart fixes them all, then exits with code 1. A variable that is set but blank is an error rather than "use the default": it usually means a failed secret mount. Secret values (the database and cache URLs, the client secret) are never printed, not even in those messages.

For the local stack, `deploy/local.env` holds working values. Load it into your shell, then start the server:

```sh runnable
set -a; . deploy/local.env; set +a
cargo run --release -p colophon-server
```

This page lists every `COLOPHON_*` variable the server, the worker inside it, the command line, the scripts and the tests read, found with `git grep COLOPHON_` over the code on the R.2 branch, plus the few other variables they read by name (`XDG_CONFIG_HOME` and `HOME` for the CLI's credentials, `SQLX_OFFLINE` for the build).

## The process role

| Setting | Default | Accepted | What it does |
|---|---|---|---|
| `--role` or `COLOPHON_ROLE` | `all` | `api`, `worker`, `all` | `api` serves HTTP without the worker; `worker` and `all` serve HTTP **and** run the worker. Anything else exits 2. |

Every role binds `COLOPHON_LISTEN_ADDR` and answers every route, the API included: a separate worker process is today a whole server with the worker switched on (D-046 records this). Give each process its own listen address. [The worker](/operations/worker/) has the details.

## The server

| Variable | Default | Accepted | What it does |
|---|---|---|---|
| `COLOPHON_DATABASE_URL` | required | A PostgreSQL URL | The application's login (`colophon_app` locally), which row-level security applies to. Never the table owner's. |
| `COLOPHON_LISTEN_ADDR` | `127.0.0.1:58000` | An IP address and port | Where it listens, plain HTTP. On a loopback address, the development rules apply (below). |
| `COLOPHON_PUBLIC_URL` | `http://localhost:58000` | An `http(s)` origin, no path | The address browsers use: the sign-in redirect is this plus `/auth/callback`, and it is the first trusted origin. |
| `COLOPHON_TRUSTED_ORIGINS` | none | Origins, comma-separated | More origins allowed to send cookie-authenticated writes, such as a separate preview site. |
| `COLOPHON_CACHE_URL` | none | A `redis://` URL | Valkey, for `/content/v1` answers and readiness. Without it, public reads go to PostgreSQL and readiness reports the cache `not_configured`. |
| `COLOPHON_STUDIO_DIR` | none | A directory | A Studio build (`apps/studio/build`) to serve at `/studio`. Checked at startup: no build there, no start. |
| `COLOPHON_LOG` | `info` | `tracing` filter directives | What is logged, for example `info,colophon_server=debug`. A filter that doesn't parse stops startup. |
| `COLOPHON_LOG_FORMAT` | `json` | `json`, `pretty` | One JSON object per line, or readable text. `deploy/local.env` sets `pretty`. |
| `COLOPHON_REQUEST_TIMEOUT_MS` | 10,000 | 100 to 120,000 | The deadline for a response's headers; a request that misses it gets `504`. |
| `COLOPHON_MAX_BODY_BYTES` | 1,048,576 (1 MiB) | 1,024 to 16,777,216 | The largest request body; larger gets `413`. |
| `COLOPHON_DB_MAX_CONNECTIONS` | 16 | 1 to 1,000 | The API's database pool. When it is full for 2 seconds, a request gets `503` with `Retry-After: 1` (D-033). |
| `COLOPHON_PROBE_INTERVAL_MS` | 2,000 | 100 to 60,000 | How often readiness probes the database and the cache. |
| `COLOPHON_PROBE_TIMEOUT_MS` | 1,000 | 50 to 30,000 | One probe's deadline; must be shorter than the interval. |
| `COLOPHON_DRAIN_DELAY_MS` | 5,000 | 0 to 60,000 | After a shutdown signal, how long `/readyz` says `draining` before the listener closes. `deploy/local.env` sets 500. |
| `COLOPHON_SHUTDOWN_GRACE_MS` | 25,000 | 1,000 to 600,000 | From the signal, the hard deadline for requests still running; must be longer than the drain delay. `deploy/local.env` sets 5,000. |

## Sign-in

All three, or none. Without them the server starts, but nobody can sign in, so the Studio and the CLI can't be used.

| Variable | What it does |
|---|---|
| `COLOPHON_OIDC_ISSUER` | The identity provider's issuer, exactly as its discovery document states it. `colophon admin bootstrap` uses it as its default `--issuer`. |
| `COLOPHON_OIDC_CLIENT_ID` | This server's client id at the provider (`colophon-server` locally). |
| `COLOPHON_OIDC_CLIENT_SECRET` | Its secret, sent as `client_secret_post`. The development realm's public secret is refused unless the server listens on a loopback address. |

[Self-hosting requirements](/operations/requirements/#an-openid-connect-provider) lists what the provider must support.

## The worker

Read by every role, used only by `worker` and `all`.

| Variable | Default | Accepted | What it does |
|---|---|---|---|
| `COLOPHON_WORKER_POLL_MS` | 500 | 10 to 60,000 | How long it sleeps when it found nothing to do. With work waiting, it claims again as soon as a slot frees. |
| `COLOPHON_WORKER_LEASE_MS` | 30,000 | 1,000 to 600,000 | How long a claim on a job lasts. A crashed worker's jobs are claimed again after it. |
| `COLOPHON_WORKER_ATTEMPT_MS` | 10,000 | 100 to 60,000 | One delivery's deadline; must be shorter than the lease, so no request outlives its claim. |
| `COLOPHON_WORKER_SLOTS` | 8 | 1 to 64 | Deliveries in flight at once. |
| `COLOPHON_WORKER_DB_CONNECTIONS` | 4 | 1 to 100 | The worker's own pool, so deliveries can't take the connections editors' requests need. |
| `COLOPHON_WORKER_LOCATION` | `local` | 1 to 63 lowercase letters, digits and dashes, starting with a letter or digit | Where this worker runs: probes pinned to a location run only on workers there, and their evidence names it. |

## Rules between settings

Each of these is checked at startup, with the others:

- `COLOPHON_PROBE_TIMEOUT_MS` is shorter than `COLOPHON_PROBE_INTERVAL_MS`.
- `COLOPHON_WORKER_ATTEMPT_MS` is shorter than `COLOPHON_WORKER_LEASE_MS`.
- `COLOPHON_DRAIN_DELAY_MS` is shorter than `COLOPHON_SHUTDOWN_GRACE_MS`.
- The three `COLOPHON_OIDC_*` variables are all set or all absent.
- The development client secret (`colophon-dev-client-secret`) only runs on a loopback `COLOPHON_LISTEN_ADDR`.
- `COLOPHON_PUBLIC_URL` and each trusted origin are bare `http(s)` origins: a path, query or fragment is an error.

## What a loopback address changes

When `COLOPHON_LISTEN_ADDR` is a loopback address (`127.0.0.1`, `::1`), the server treats itself as a development server:

- website connections (webhooks, CDN purges and probes) may use `http://` and private addresses, so a receiver on your own machine works;
- the development realm's public client secret is allowed.

On any other address, connections must be `https://` and resolve to public addresses only, checked when saved and again before every attempt.

## Fixed values

These are constants in the code, not settings:

| What | Value | Where |
|---|---|---|
| Waiting for a pooled database connection | 2 s, then `503` | `colophon_db::PoolSettings` |
| One SQL statement (server and worker) | 5 s | `colophon_db::PoolSettings` |
| One SQL statement (`colophon admin`) | 5 min | `crates/colophon-cli/src/admin.rs` |
| A session | 12 hours from sign-in, or 2 hours unused | D-032 |
| A device sign-in code; a CLI token | 10 minutes; 30 days | `crates/colophon-api/src/device.rs` |
| A delivery job's lifetime | 1 hour from its release | `colophon_domain::job` |
| Attempts per job | 5, or 10 for a probe | `colophon_domain::job` |
| Retry backoff | full jitter, doubling from 1 s, capped at 60 s; a `Retry-After` up to 60 s is respected | `colophon_domain::job` |
| A probe's "not yet" | retried no sooner than 5 s later | `colophon_domain::receipt` |
| Failing jobs that ran out of time | every 10 s | `crates/colophon-worker/src/lib.rs` |
| Stopping the worker and probes at shutdown | 5 s | `crates/colophon-server/src/lib.rs` |

## The command line

| Variable | Read by | What it does |
|---|---|---|
| `COLOPHON_SERVER` | `colophon login`, `colophon doctor` | The default `--server`. |
| `COLOPHON_CONFIG_DIR` | every command that signs in | Where `credentials.json` lives. Without it, `XDG_CONFIG_HOME/colophon`, then `HOME/.config/colophon`. |
| `COLOPHON_MIGRATION_DATABASE_URL` | `colophon admin migrate`, `colophon admin bootstrap` | The table owner's connection URL. Never give it to the server. |
| `COLOPHON_OIDC_ISSUER` | `colophon admin bootstrap` | The default `--issuer`. |

[Command line](/reference/cli/) has every flag.

## The local stack and the scripts

| Variable | Default | Read by | What it does |
|---|---|---|---|
| `COLOPHON_PG_PORT` | 55432 | `deploy/compose.yaml` | PostgreSQL's port on `127.0.0.1`. |
| `COLOPHON_VALKEY_PORT` | 56379 | `deploy/compose.yaml` | Valkey's port. |
| `COLOPHON_S3_PORT`, `COLOPHON_S3_ADMIN_PORT` | 53900, 53903 | `deploy/compose.yaml` | Garage's S3 and admin ports. |
| `COLOPHON_KEYCLOAK_PORT` | 58080 | `deploy/compose.yaml` | Keycloak's port, and so the issuer's address. |
| `COLOPHON_TEST_DATABASE` | none (`colophon_check` in `scripts/check.sh`, `colophon_demo` in `scripts/demo.sh`) | `scripts/local-env.sh` and the scripts that source it, the Rust stack tests, the Playwright configs | A database name: both database URLs point at it instead of `colophon`, on the same PostgreSQL. 1 to 63 lowercase letters, digits and underscores, starting with a letter. |
| `QUICK` | `0` | `scripts/check.sh` | `1` skips the integration half. |

Moving a port in `deploy/compose.yaml` doesn't change `deploy/local.env`: edit the URLs there to match. `scripts/load.sh` also reads `TOTAL`, `RATE`, `VUS`, `DURATION`, `P99_MS`, `KEEP`, `LOAD_PORT`, `SITE_PORT`, `RECEIVER_PORT` and `SERVER_LOG`; its header comment explains them, and [Performance](/resources/performance/) the results.

## Test-only variables

None of these does anything in a normal server.

| Variable | Default | Read by | What it does |
|---|---|---|---|
| `COLOPHON_FAULT` | none | the server, built with the `fault-injection` feature only | The named point at which the process exits with code 86, as a crash would: `entry.create:before-commit`, `entry.create:after-commit`, `entry.save:before-commit`, `entry.save:after-commit`, `worker:after-dispatch`. A normal build has no fault points at all. |
| `COLOPHON_CRASH_LOOP_KILLS` | 12 | `crates/colophon-server/tests/crash_loop.rs` | How many times the crash loop kills the server. |
| `COLOPHON_CRASH_LOOP_SEED` | 2400 | `crates/colophon-server/tests/crash_loop.rs` | The seed for the kill timing; each run prints its own. |
| `COLOPHON_TEST_DATABASE` | none | `crates/colophon-server/tests/common/mod.rs` | As above: the server under test and the owner connection use that database. |
| `COLOPHON_TEST_PG_HOST` | `127.0.0.1:55432` | `crates/colophon-db/tests/common/mod.rs` | Where the database tests create a fresh database for each test. |
| `COLOPHON_DATABASE_URL`, `COLOPHON_CACHE_URL` | the stack's URLs | two ignored stack tests (`colophon-db`'s ping, `colophon-server`'s cache probe) | They reuse the server's variables. |

The build itself reads one more: `.cargo/config.toml` sets `SQLX_OFFLINE=true`, so compiling uses the query data committed in `.sqlx/` and needs no database.

## Not configurable yet

- TLS: the server speaks plain HTTP, and its PostgreSQL and Valkey clients are built without TLS. Put a TLS proxy in front and keep both stores on a private network. <Status planned="P6" />
- Metrics and trace export, log budgets and admission budgets (module 18). <Status planned="P6" />
- Per-project or per-destination worker limits beyond one delivery in flight per destination (module 05). <Status planned="not scheduled" />
