# 18 · operations (`deploy/`)

> Running Colophon: the local stack, production topology, observability, backups, restore and runbooks. It must be
> maintainable without its original author (spec §33).

| | |
|---|---|
| Milestones | P0 (local Compose stack), P3 (first deployment of the reference site), P6 (backups, restore drill, upgrades, runbooks, budgets) |
| Proposed owner | Claude builds; the maintainer runs the drills |
| Depends on | colophon-server, PostgreSQL 18, a Redis-protocol cache (Valkey, D-015), S3-compatible storage (Garage locally, D-016), Keycloak, a CDN |
| Used by | the maintainer, independent installers, contributors |
| Spec | §04, §27, §32, §33 |

## Why it exists

A system that only its author can restore is a demo, not a product. v0.1's definition of done includes a restore drill
on clean infrastructure and an independent install, so operations are a module with deliverables, not an afterthought.

## End goal

- One command brings up a local stack.
- Production runs on a documented, non-root, least-privilege topology.
- Telemetry explains every failure without leaking content or secrets.
- A restore onto clean infrastructure works: a new epoch, the deletion ledger replayed, external effects reconciled
  before anything is dispatched.

## Responsibilities

- Compose files and container images.
- The production topology.
- Health endpoints.
- Logs, traces and metrics.
- Backups and restore.
- Upgrades.
- Budgets.
- Runbooks.
- Retention.

## Not its job

- Kubernetes manifests (not required for the first install, spec §32).
- Rewriting PostgreSQL, the cache or a CDN. Managed infrastructure is fine; the Rust requirement applies to application
  code (spec §03).

## Breakdown

### Local stack (spec §32)
- `deploy/compose.yaml` runs (D-017; done in step 0.2 unless marked later):
  - PostgreSQL 18;
  - Valkey, configured so it cannot persist anything and cannot grow without limit (D-015);
  - Garage, a single-node S3-compatible test object store (D-016);
  - Keycloak with the `colophon` development realm;
  - `colophon-server` in the API and worker roles (later in P0);
  - the Studio's development server (later in P0).
- Every port is bound to `127.0.0.1`. `scripts/stack.sh up` starts the stack and proves each service from outside:
  through its published port, including a request each one must refuse. CI runs it from a clean clone.
- `colophon doctor --json` reports readiness per dependency. A cache outage shows as degraded while authoritative writes
  stay available (spec §04).

### Production topology (spec §03, §26, §32)
- An external TLS proxy or CDN in front.
- Non-root containers with read-only file systems where possible, and resource limits.
- PostgreSQL and the cache on a private network; their ports are never published.
- Separate origins for administration, public content, assets and preview.
- Production and staging have separate credentials, buckets and cache namespaces.
- Outbound destinations are restricted.
- **Container builds:** multi-stage builds with `--locked`. Match the C library between the build image and the runtime
  image: Debian 13-based Rust build images and a Debian 12 distroless runtime can fail at startup (rust-mastery,
  "Known blind spots").

### Health (spec §32; built in step 0.3)
- `/livez`: the process responds. It checks nothing else.
- `/readyz`: this role can safely serve its required operations. Background probes (every 2 s, 1 s deadline) write the
  state; the handler only reads it, so `/readyz` costs no database work however often it is called (a 5-million-request
  load run saw 13 database transactions, all from the probes). Statuses: `starting`, `ready`, `degraded` (200),
  `not_ready`, `draining` (503). A supervisor marks a dependency down if its probe ever stops, so readiness can't
  freeze at "up". The database probe uses one connection of its own, never the request pool (D-075): under load
  that pool is busy, and a probe waiting for it called a healthy database down on every server at once, which left
  the balancer nothing to send to (found by `scripts/failover.sh`). Each process therefore holds one more database
  connection than its pool.
- Several servers behind a load balancer (milestone S, D-075; `scripts/cluster.sh`, `deploy/cluster/`): the balancer
  checks `/readyz` every 500 ms and drops a server after two failures, so a server asked to stop leaves the rotation
  within about a second of saying `draining`; `COLOPHON_DRAIN_DELAY_MS` (default 5 s) must stay longer than that.
  The balancer retries a request no server answered (refused, or closed without a response) on another, holding the
  body to do so; writes carry `Idempotency-Key`, so a write a dying server did commit is replayed, not repeated.
  The sum of every process's database connections stays under PostgreSQL's `max_connections` with room to spare.
- Rate limits (D-076): `COLOPHON_RATE_LIMIT_PER_ADDRESS` (default `300/600`: requests a second and burst),
  `COLOPHON_RATE_LIMIT_PER_CREDENTIAL` (`150/300`) and `COLOPHON_RATE_LIMIT_DELIVERY` (`1000/2000`), each `off` to
  disable; `COLOPHON_TRUSTED_PROXIES`, comma-separated addresses or networks (a load balancer, a CDN's ranges), whose
  `X-Forwarded-For` names the client. Behind a proxy that isn't listed, every client looks like the proxy and shares
  one count. Counts live in Valkey (`colophon:rl:*` keys that expire on their own); with Valkey down each server
  counts alone, so a cluster of N admits up to N times the rate until it is back. `scripts/load.sh` and
  `scripts/cluster.sh` raise the limits so far that they never refuse (one machine sends everything), and still
  pay for every check.
- `colophon doctor [--json]` reports what `/readyz` reports, with the CLI exit codes (0 ready, 5 otherwise).
  - A cache failure can be *degraded* rather than unready when the bounded fallback is available.
  - A database failure makes writes unavailable.
- Service health is checked from another failure domain.

### Observability (spec §32; audit written in P6 — `deploy/runbooks/observability.md`)
- **Logs:** structured JSON through `tracing` (`COLOPHON_LOG_FORMAT=json`; `pretty` locally),
  `COLOPHON_LOG` an `EnvFilter`. Built; there is deliberately no `/metrics` endpoint — the
  signals below are exposed on the log lines, and the runbook states which.
- **Correlation fields (built):** request id (UUIDv7 minted, or a sanitised client
  `x-request-id`, echoed on the response header and the error envelope), `method`, `route`
  (low-cardinality template), `path` without the query string, `latency`, `status`, and the
  deployment's `version`/`commit` on every request-attributed line. Work events carry their
  own ids (`job`, `generation`, `attempts`, `project`, `identity`, `changeset`) where the
  event has one — the bare request span does not resolve a project, by design.
- **Metrics as emitted fields or documented queries:** HTTP errors/latency (`tower_http=debug`
  response lines), save conflicts (`stale_version` error codes), publish lag
  (`release committed` → `job attempt recorded` shared `generation`), queue age and
  failed/uncertain deliveries (`jobs` SQL + `held`), cache hit ratio (`Colophon-Cache`
  header), model budget consumption (reconciler warnings). **Gaps, listed not hidden:**
  database pools/locks and WAL size, cache memory/eviction, the dropped-record counter and
  the in-app log budget are infra queries or unbuilt slices; OTLP export is unbuilt (the spec
  marks it optional — the request span is already the trace root it would hang off).
- **Log budgets:** the 7-day/1 GiB/16 KiB-record spec is a platform log-driver duty today;
  the in-app counter and cap are not built — the synchronous writer means pressure shows as
  latency, not silent loss, which is honest but not the spec's shape.
- **Never logged:** draft text, model prompts, customer addresses or credentials (spec §27) —
  by rule and review: `path` drops the query string, client ids are sanitised, the envelope
  names `code` never internals.

### Backups (spec §32; capture and archiving built in P6, D-125)
- WAL archiving is live on the local stack: `archive_mode=on`, `wal_level=replica`, `archive_timeout=60s` (an idle
  database still switches within the minute, so the 15-minute recovery-point objective holds), through
  `deploy/postgres/archive-wal.sh` into the named `postgres-wal-archive` volume — the local stand-in for object
  storage, since the stock Postgres image ships no S3 tooling; production points the same `archive_command` contract
  at a bucket. `deploy/postgres/restore-wal.sh` is its `restore_command`. `scripts/stack.sh check` proves a switched
  segment lands. O5: nothing anywhere deletes archived WAL; retention is a documented future job, and the runbook
  says so.
- `scripts/backup.sh` captures encrypted base backups: `pg_basebackup` in-cluster as the `colophon_backup` role
  (REPLICATION, no table rights — backup credentials separate from application credentials), `-X stream` for the
  WAL a consistent start needs, `pg_verifybackup` against PostgreSQL's own `backup_manifest` at capture and again on
  decrypted bytes in `verify`. Output: `backups/<UTC-stamp>/{base.tar.gz.gpg,manifest.json}`, gpg-symmetric because
  `age` is not installed locally (age identity file when it is); the passphrase comes only from a file or
  `COLOPHON_BACKUP_PASSPHRASE_FILE`, never a flag.
- The manifest (`format: 1`) records the checkout's commit, Postgres and pg_basebackup versions, non-system schemas,
  the applied `_sqlx_migrations`, the key reference (never the key), database and cluster sizes, the WAL archive
  state, and `deletion_ledger` — `null` until the ledger slice lands, `{"present":true,"rows":N}` after.
- `scripts/backup.sh daily` is the scheduled unit (D-130): create + verify + `admin ledger-export` into the backup's
  own directory (the interval the restore's ledger stage replays) + retention applied + an audit line in
  `<root>/daily.jsonl`. The platform's scheduler launches it (a documented cron line, launchd or a systemd timer —
  no daemon). `scripts/backup.sh retention [--apply]` enforces the keep policy — the newest verified backup of each
  of the newest 7 capture days and 4 ISO weeks, counted over days/weeks that exist; the newest verified is always
  kept, and a directory that fails retention's own verification (manifest fields + payload SHA-256 + decrypts to a
  data directory — deliberately no pg_verifybackup, no docker) is **never pruned**, only printed `hold`. Proven by
  `scripts/backup.sh retention-drill` → `perf/results/…-retention-drill-….json`.
- Targets: a 15-minute recovery-point objective and a one-hour recovery-time objective on the reference profile. Both
  require demonstrated restore drills; a SQL dump alone is not this strategy. The first dated drill artifact is
  `perf/results/2026-11-04T173702Z-backup-drill-apple-m2-pro-12c.json` (capture 252 s, verify 138 s, 2.2 GB encrypted
  from a 12.4 GB dev cluster — laptop numbers about the reference profile, not a capacity promise).
- Asset objects are versioned at the application level (D-136, migration 0051): every verified landing lives at a
  content-addressed `public/…/{asset}/{sha}` key recorded in `asset_versions`, and Garage's lack of bucket versioning
  no longer matters — an object is immutable once written, so a backup's manifest plus the rows is enough to name
  every byte a restore must find, and the public-orphan sweep collects any object no row serves from. **Still owed:**
  the table-retention jobs below. The cache does not need restoring for correctness: no backup includes it.

### Restore procedure (spec §33; F13, F14; D-126, migration 0049; built in P6)

`scripts/restore.sh` restores a captured backup onto **clean, disposable infrastructure** — a scratch Postgres
container, volume and Valkey on their own 127.0.0.1 ports, nothing of the live stack's — in six ordered stages,
each gated by the previous stage's marker (`status` prints them; `blocked` markers record why; a done stage is
never silently rerun without `--force`; `deploy/runbooks/restore.md` is the operator's copy):

1. **prepare:** decrypt and unpack the payload into a fresh scratch volume — clean infrastructure, never the live
   stack's directories.
2. **verify:** `pg_verifybackup` on the restored bytes before the cluster first starts, then the schema check —
   `admin restore verify` compares the restored `_sqlx_migrations` against the manifest's list and the binary's
   embedded set (`colophon_db::restore::check_schema`): a backup carrying a migration this binary doesn't know is
   refused; a schema behind the binary reports `pending` and passes — nothing migrates silently inside verify.
3. **epoch:** `admin restore epoch` advances every environment's epoch exactly once — the new recovery epoch (D4)
   that invalidates the old timeline's cache generations, delivery scope keys and worker leases.
4. **ledger:** every `--ledger` export replays the interval the restored `deletion_ledger` lacks (anti-joined by
   row id — `id > max` would hide out-of-order rows). A gap the files don't cover, or a row that can't replay,
   blocks the stage — O2 keeps serving off.
5. **reconcile:** `admin restore hold` freezes every unfinished job as **`held`** — a real `jobs.status` the claim
   query never selects, leases cleared, the state it was taken from recorded (`was=running` marks in-flight
   attempts whose external effect is unknown — F13). `admin restore jobs` is the operator's inventory: the held
   outbox plus due schedules and open asset, bulk and content-migration work. **`--resume-jobs` is the explicit
   enable** (`admin restore resume` → `pending` with a fresh lifetime) after reconciliation — O3.
6. **serve:** only after all five markers, `colophon-server --role api` starts on the scratch stack and `/readyz`
   must answer — the gate that finally serves.

The drill (`scripts/restore.sh drill`) seeds an in-flight job leased to a worker the restore won't have, captures
a backup, deletes an entry and edits another **after** capture, exports the ledger, restores, and asserts the new
epoch, the replayed tombstone, the absent edit, the held job and serving — the dated record is
`perf/results/<stamp>-restore-drill-<machine>.json`. `colophon-db/tests/restore.rs` is the stack regression:
a template-cloned "backup" gets a new epoch, its pending and in-flight jobs held with leases cleared, held jobs
unclaimable through the real claim path, and `resume` returning them to pending; `tests/ledger.rs` covers the
replay side (F14).

A backup existing is not proof of a working restore.

### Upgrades (spec §32; D-130 — runbook written and its local halves rehearsed in P6)
- The procedure is `deploy/runbooks/upgrade.md`: release notes and compatibility → pre-flight
  checks (`colophon doctor`, `admin migrate-status` baseline) → a `verify`-passing backup → the
  **new** binary's `admin migrate-status` listing what it will apply → binary swap →
  `colophon admin migrate` as the migrator, never at startup → smoke checks (`/livez`,
  `/readyz`, `/version`, `doctor`, a real authenticated round-trip).
- `admin migrate-status` (built in P6) is the read-only schema check the runbook gates on:
  `current` | `behind` with the pending list | `unknown` at **exit 4** — the database carrying
  a migration the binary doesn't know means the schema is newer than the code, and serving it
  or rolling a binary back onto it would be guessing. Proven on the real stack by
  `colophon-server/tests/migrate_status.rs` and `scripts/upgrade-rehearsal.sh` (dated record in
  `perf/results/`).
- Binaries roll back only against a compatible schema. Otherwise, restore or forward-fix with an explicit data-loss and
  reconciliation plan.

### Budgets (spec §33)

| Resource | Default guard |
|---|---|
| HTTP API | 1 MiB JSON input, page size ≤ 100, 2 s ordinary query budget; 429/503 before unbounded queues form |
| Jobs | 10,000 pending jobs per project and 1 GiB pending payload metadata per install |
| Worker pools | 8 publishing/delivery tasks (2 per destination), 4 image jobs, 2 AI jobs |
| Cache | 1 GiB reference budget with explicit eviction |
| Logs | 7 days and 1 GiB, 16 KiB record cap, visible dropped-record counter |
| Storage | Warn at 70%, reject optional growth at 85%, block unsafe writes before 90% |

### Runbooks (spec §33)
- **PostgreSQL unavailable:**
  - fail writes honestly and stop accepting durable jobs;
  - keep serving only eligible previously public cache data within its TTL;
  - publish role-specific degraded status;
  - verify migrations and recovery before admitting traffic again.
- **Cache unavailable:**
  - disable the failed cache path;
  - coalesce identical public reads and cap the database fallback;
  - rebuild keys gradually;
  - never flush unrelated namespaces or replay business jobs.
- **Publishing or CDN backlog:**
  - inspect the oldest due work, provider rate limits, generations and credentials;
  - retry only the current desired generation;
  - never purge the whole zone automatically.
- **Suspected credential leak:**
  - revoke the scoped principal and rotate related secrets;
  - pause its new actions and preserve the audit trail;
  - investigate actions already executed.
- **Disk pressure:**
  - stop unsafe admission and preserve diagnostics;
  - apply measured retention;
  - never delete WAL to free space.

### Retention (spec §27)
- Unpinned revisions and resolved delivery metadata: 90 days.
- Terminal ordinary jobs: 30 days.
- Audit: 90 days.
- Diagnostic logs: 7 days with a byte cap.
- Deletions record a durable tombstone, purge caches, and delete unreferenced media asynchronously.

## Invariants

| ID | Invariant |
|---|---|
| O1 | No database or cache port is published outside the private network. |
| O2 | Restore never serves traffic before a new epoch and the deletion-ledger replay. |
| O3 | Restore never dispatches outbound work before reconciliation. |
| O4 | Logs never contain draft text, prompts, personal data or credentials. |
| O5 | WAL is never deleted to free disk space. |

## Interfaces

- `deploy/compose.yaml`, container files, runbooks in `deploy/runbooks/`.
- `colophon doctor`, `/livez`, `/readyz`, metrics and trace export.

## Failure modes

| Situation | Behaviour |
|---|---|
| Restore without a complete deletion ledger | Public serving blocked until the gap is resolved |
| Upgrade with an incompatible schema | Rollback refused; forward-fix or restore with a stated data-loss window |
| Telemetry exporter down | Not applicable today: the synchronous writer has no exporter and drops nothing; log pressure shows as latency |

## Tests and evidence

- F13 and F14.
- A restore drill on clean infrastructure (`perf/results/2026-11-04T194355Z-restore-drill-…`).
- An upgrade rehearsal (`scripts/upgrade-rehearsal.sh`, its dated record in `perf/results/`),
  plus the backup-retention drill (`scripts/backup.sh retention-drill`).
- L07 (24-hour soak: memory, WAL and disk growth explained).
- L12 (PostgreSQL slow or unavailable).
- Each drill produces a dated report with commands and outcomes.

## Build order

1. **P0:** local Compose stack, `/livez`, `/readyz`.
2. **P3:** first production deployment for the reference site; structured logs.
3. **P6:** backups, WAL archiving, restore drill, upgrade runbook, budgets, runbooks, metrics, trace export.

## Open decisions

- **O-012:** hosting for the reference site.
- Managed PostgreSQL or self-hosted for the reference deployment.
