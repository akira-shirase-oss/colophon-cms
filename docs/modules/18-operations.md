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
- `colophon doctor [--json]` reports what `/readyz` reports, with the CLI exit codes (0 ready, 5 otherwise).
  - A cache failure can be *degraded* rather than unready when the bounded fallback is available.
  - A database failure makes writes unavailable.
- Service health is checked from another failure domain.

### Observability (spec §32)
- **Logs:** structured JSON through `tracing`, with optional OpenTelemetry-compatible trace export.
- **Correlation fields:** request ID, project, actor, revision, release, job, destination and deployment version.
- **Metrics:**
  - HTTP errors and latency;
  - save conflicts and publish lag;
  - queue age and failed or uncertain deliveries;
  - cache hit ratio;
  - database pools and locks, and WAL size;
  - cache memory and eviction;
  - model budget consumption;
  - dropped logs.
- **Log budgets:** 7 days and 1 GiB of diagnostics per install, whichever comes first. Records are capped at 16 KiB,
  with a visible dropped-record counter. Host and container logs are capped separately.
- **Never logged:** draft text, model prompts, customer addresses or credentials (spec §27).

### Backups (spec §32)
- Encrypted daily base backups plus continuous PostgreSQL WAL archiving (point-in-time recovery).
- Targets: a 15-minute recovery-point objective and a one-hour recovery-time objective on the reference profile. Both
  require demonstrated restore drills; a SQL dump alone is not this strategy.
- Object-store assets are versioned.
- A consistent backup manifest records schemas, migrations, encryption-key references and the deletion ledger.
- Backup credentials are separate from application credentials. The cache does not need restoring for correctness.

### Restore procedure (spec §33; F13, F14)
1. Restore onto clean infrastructure with outbound integrations disabled.
2. Verify schema, assets and permissions.
3. Register a **new recovery epoch** before serving.
4. Reapply the off-host **deletion ledger**. If the required ledger interval can't be reconstructed, public serving
   stays blocked.
5. Reconcile external effects (CDN, site deployments, webhooks) before replaying any outbox work.
6. Explicitly enable current jobs.

A backup existing is not proof of a working restore.

### Upgrades (spec §32)
- Read the release notes → verify the backup → rehearse on staging → apply additive schema → deploy compatible binaries →
  smoke checks → backfill → remove old fields only after the support window.
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
| Telemetry exporter down | Application continues; dropped-record counter rises |

## Tests and evidence

- F13 and F14.
- A restore drill on clean infrastructure.
- An upgrade rehearsal.
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
