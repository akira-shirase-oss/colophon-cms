`colophon-server` reads its configuration once, at startup, from `COLOPHON_*` environment variables (`crates/colophon-server/src/config.rs`). Every problem is reported together, so one restart fixes them all, and a variable that is set but blank is an error rather than "use the default": it usually means a failed secret mount. Secret values are never printed.

For the local stack, `deploy/local.env` holds working values. Load it into your shell before starting the server:

```sh runnable
set -a; . deploy/local.env; set +a
cargo run --release -p colophon-server
```

`colophon-server --role api|worker|all` (or `COLOPHON_ROLE`) picks what the process runs: the APIs, the background worker, or both, the default.

## The server

| Variable | Default | Allowed |
|---|---|---|
| `COLOPHON_DATABASE_URL` | required | The application's PostgreSQL login (`colophon_app` locally), never the owner's. |
| `COLOPHON_LISTEN_ADDR` | `127.0.0.1:58000` | An address and port. |
| `COLOPHON_PUBLIC_URL` | `http://localhost:58000` | The origin browsers reach the server at: the sign-in redirect and the first trusted origin. |
| `COLOPHON_TRUSTED_ORIGINS` | none | More origins, comma-separated, allowed to send cookie-authenticated writes. |
| `COLOPHON_CACHE_URL` | none | A Redis-protocol URL (Valkey). Without it, reads skip the cache and readiness says `not_configured`. |
| `COLOPHON_STUDIO_DIR` | none | A Studio build to serve at `/studio`; checked at startup. |
| `COLOPHON_LOG` | `info` | Log filter directives. |
| `COLOPHON_LOG_FORMAT` | `json` | `json` or `pretty`. |
| `COLOPHON_REQUEST_TIMEOUT_MS` | 10,000 | 100 to 120,000. |
| `COLOPHON_MAX_BODY_BYTES` | 1 MiB | 1 KiB to 16 MiB. |
| `COLOPHON_DB_MAX_CONNECTIONS` | 16 | 1 to 1,000. |
| `COLOPHON_PROBE_INTERVAL_MS` | 2,000 | 100 to 60,000: how often readiness checks the database and cache. |
| `COLOPHON_PROBE_TIMEOUT_MS` | 1,000 | 50 to 30,000, and shorter than the interval. |
| `COLOPHON_DRAIN_DELAY_MS` | 5,000 | 0 to 60,000: how long readiness says `draining` before the listener closes. |
| `COLOPHON_SHUTDOWN_GRACE_MS` | 25,000 | 1,000 to 600,000, and longer than the drain delay. |

## Sign-in

All three, or none (without them, nobody can sign in):

| Variable | Meaning |
|---|---|
| `COLOPHON_OIDC_ISSUER` | The identity provider's issuer, exactly as its discovery document states it. |
| `COLOPHON_OIDC_CLIENT_ID` | This server's client id at the provider. |
| `COLOPHON_OIDC_CLIENT_SECRET` | Its secret. The development realm's public secret is refused unless the server listens on a loopback address. |

## The worker

| Variable | Default | Allowed |
|---|---|---|
| `COLOPHON_WORKER_POLL_MS` | 500 | 10 to 60,000: how often it looks for due jobs. |
| `COLOPHON_WORKER_LEASE_MS` | 30,000 | 1,000 to 600,000: how long a claim lasts. |
| `COLOPHON_WORKER_ATTEMPT_MS` | 10,000 | 100 to 60,000, and shorter than the lease, so no request outlives its claim. |
| `COLOPHON_WORKER_SLOTS` | 8 | 1 to 64 attempts in flight at once. |
| `COLOPHON_WORKER_DB_CONNECTIONS` | 4 | 1 to 100, a pool of its own so deliveries can't starve requests. |
| `COLOPHON_WORKER_LOCATION` | `local` | 1 to 63 lowercase letters, digits and dashes: where its probes' evidence says it looked from. |

## Used by the tools, not the server

| Variable | Used by |
|---|---|
| `COLOPHON_MIGRATION_DATABASE_URL` | `colophon admin migrate` and `admin bootstrap`: the database owner's login. Never give it to the server. |
| `COLOPHON_SERVER` | The CLI's default `--server`. |
| `COLOPHON_CONFIG_DIR` | Where the CLI keeps `credentials.json`. |
| `COLOPHON_TEST_DATABASE` | The scripts and tests: points both database URLs at another database on the same PostgreSQL, so a branch's migrations never touch the shared one. |
| `COLOPHON_FAULT` | Only in builds with the `fault-injection` feature: the named point to exit at, for crash tests. A normal build has no way to exit on request. |
