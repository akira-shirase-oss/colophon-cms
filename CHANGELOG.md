# Changelog

All notable changes to Colophon are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) (D-062): before 1.0 a minor
release may change `/api/v1`, and every binary reports its version, commit and build date
through `--version` and `GET /version`.

## [0.1.0-alpha.1]

The first versioned state of the engine: pre-release until the P6 proof gate closes
(restore drill on clean infrastructure is done; usability drills and an independent
install are still ahead — see `docs/ROADMAP.md`).

### Added

- **Content engine:** collections and singletons with a Strapi-style type builder
  (slugs, references that never dangle, objects, lists, blocks, shared components,
  unique fields, defaults, patterns, conditional fields, locales), insert-only
  revision history, changesets bound to their hash, reviews, scheduled publishing and
  entry deletion as tombstones the database refuses while anything depends on them.
- **Publishing:** releases on a publisher's base generation, non-overlapping
  publication intervals, a `/content/v1` delivery API served as `colophon_delivery`
  with a generation-keyed cache, ETags, tombstoned unpublishes, CDN purge jobs, and
  probes with receipts per destination.
- **Identity:** OpenID Connect sign-in, server-side sessions with CSRF and `Origin`
  checks, email-and-password accounts, project members, single-use invitations, roles
  as data including field lists and `own` conditions, project API tokens with an
  append-only audit log, and a first-run setup code.
- **Surfaces:** the Studio (Svelte, served by the server at `/studio`), `colophon`
  CLI (device sign-in, content commands, machine-readable envelope v2),
  `@colophon-cms/sdk`, a bounded GraphQL endpoint, a Streamable HTTP MCP server on
  API tokens, and JSONL import/export with Strapi and Sanity adapters.
- **Operations:** the local Compose stack (PostgreSQL 18, Valkey, Garage, Keycloak),
  `/livez`/`/readyz` with background probes, structured JSON logs with request ids,
  latency and deployment identity, encrypted cluster base backups with WAL archiving,
  `backup.sh daily` with verified-backup retention, the six-stage restore procedure
  with a recovery epoch and deletion-ledger replay, and `admin migrate-status` as the
  upgrade gate.
- **Safety:** an outbox the worker claims under leases, idempotency-keyed deliveries,
  bounded everything (rate limits in Valkey, request/queue/body caps, token and cost
  budgets reserved in Postgres), agents held draft-only, and per-tenant row-level
  security.
- **Verification:** fault and load matrices with stored evidence (`perf/results/`),
  a deterministic simulator differentially checked against Postgres, crash and
  crash-loop tests, and a restore drill that seeds an in-flight job and asserts the
  held outbox, the replayed tombstone and serving.

### Known limitations

- Backups capture the Postgres cluster only; object-store asset versioning is
  undecided for production (D-016's Garage has no bucket versioning).
- Retention for revisions, jobs, audit rows and diagnostic logs is policy in module
  18; the enforcement jobs are built for backups, not yet for those tables.
- Observability is structured logs plus health endpoints; per-signal recipes and the
  gaps (Prometheus-style metrics, OTLP export, the dropped-record counter) are
  documented in `deploy/runbooks/observability.md`.
- The release workflow is written and reviewed but has not yet run on GitHub;
  publishing waits on the maintainer (licence confirmation, npm organisation,
  credentials — module 19).

## How to upgrade

`deploy/runbooks/upgrade.md` is the operator's procedure: verified backup →
`admin migrate-status` → binary swap → `admin migrate` → smoke checks, with rollback
rules per schema compatibility.
