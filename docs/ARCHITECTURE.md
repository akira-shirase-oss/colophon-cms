# Architecture

A modular monolith in Rust with a separate worker role, three SvelteKit frontends, and PostgreSQL as the only source of
truth. Derived from spec §02, §03, §14–16; deviations are in [DECISIONS.md](DECISIONS.md).

## System overview

```text
   Editors              Developers / scripts / agents              Readers
      |                          |                                     |
  Svelte Studio          colophon CLI · TS SDK · MCP adapter          CDN (Cloudflare)
  (static SPA)                   |                                     |
      \__________________________|                                     |
                     |                                                 |
           Rust management API  /api/v1                     Rust delivery API  /content/v1
           (authn, authz, validation, idempotency)          (published projections only)
                     |                                                 |
           Domain services: rules, state machines          Redis-compatible cache
                     |                                       (generation-keyed, disposable)
                     |                                                 |
           ┌─────────┴──────────────── PostgreSQL 18 ──────────────────┘
           │  revisions · releases · publication history · permissions
           │  jobs · outbox · idempotency · audit · budgets
           │
       Worker role (leases, retries, reconciliation)
           │
   CDN purge · site build/promote · evidence probes · media processing · webhooks
           │
   Delivery evidence  →  receipts in the Studio and CLI
```

Arrows show responsibilities, not separate services. Version 0.1 ships **one server binary** that runs as
`--role api`, `--role worker` or `--role all`. Management and public reads are split into separate processes only if
load tests justify it (spec §03).

## Sources of truth

| Store | Owns | Never |
|---|---|---|
| PostgreSQL | Content, revisions, schemas, releases, publication history, sessions, memberships, credentials (hashed), jobs, outbox/inbox, idempotency records, audit, budgets, tombstones | Waits on an external call inside a transaction |
| Redis-compatible cache | Generation-keyed public query results, expendable rate counters, optional worker wake-ups | Holds the only copy of anything, decides conflicts, or enforces a hard budget |
| CDN | Cached public responses and public assets | Caches drafts, previews, sessions, or personalized responses |
| Object storage | Uploaded originals (quarantine and approved), derivatives, build artifacts | Is written to before an upload intent exists |
| Identity provider (Keycloak via OIDC) | Login and MFA | Decides CMS permissions (Rust and Postgres do) |

## The core model

```text
Entry ──► Revisions (immutable JSONB; schema version; author: human or agent; base revision; hash)
             │  save requires expected_revision; a stale save returns 409
             ▼
Changeset (exact entry, revision, schema and asset hashes)
             │
             ▼
Review / approval bound to the changeset hash (any edit, policy change or expiry invalidates it)
             │
             ▼
Release commit, one transaction:
   lock the environment's generation row → check the base generation → write release items →
   close and open publication-history intervals → generation += 1 → audit + outbox rows
             │
             ▼
Outbox ──► worker (lease, attempt generation, deadline) ──► CDN purge / site build / webhook
             │
             ▼
Delivery record per destination: pending → dispatched → observed | retrying | failed | uncertain | superseded
             │
             ▼
Evidence: generation marker or response hash, location, time, errors  →  delivery receipt
```

### Release identity: epoch and generation

- Each environment has a **recovery epoch** and a **generation** counter. A release is identified by
  `(epoch, generation)`.
- Generations only increase. Rollback creates a *new, higher* generation that points at older revisions.
- Restoring a backup starts a new epoch before serving traffic, so cache entries from before the restore can never be
  mistaken for new ones.
- Cache keys, SDK pinned reads and site build markers all carry both values.

## Request paths

| Path | Steps |
|---|---|
| Editor save | Studio → `PUT …/entries/{id}` with `If-Match` → authorize → validate (schema, rich text, references) → one transaction: compare draft pointer, insert revision, move pointer, audit → `200` with the new revision, or `409` with the current one |
| Publish | Studio/CLI → `POST …/releases/{id}/publish` with an idempotency key → recheck approval, authority and base generation → one transaction (above) → `202 accepted` with an operation ID → the worker delivers → receipts update |
| Public read | Reader → CDN → `/content/v1/…` → resolve the active generation from Postgres on an origin miss → Redis (keyed by epoch + generation + query) → published projection in Postgres on a miss |
| Pinned read (time travel) | `/content/v1/…?generation=N` → publication-history intervals map each entry to the revision live at N → revocation rules still apply |
| Preview | Studio issues a 15-minute capability scoped to the exact revision set → separate preview origin validates it on every fetch → `no-store`, `noindex` |

## Process topology

- **Server binary** (`colophon-server`): the composition root. It loads and validates config once, builds adapters,
  and runs the API role, the worker role or both. It also serves the Studio's static assets on the administration
  origin, with `/api` routed to Rust on the same origin.
- **Separate origins:** administration (Studio + `/api`), public content (`/content`), assets (uploads and
  derivatives), and preview (sandboxed). Uploads never come from the administration origin.
- **Private network** for PostgreSQL and the cache. Production and staging have separate credentials, buckets and
  cache namespaces.
- **Node.js is build tooling only.** No SvelteKit server code touches PostgreSQL.

## Repository layout (target)

```text
colophon/
├── apps/
│   ├── studio/                 # SvelteKit SPA: the editor
│   ├── web/                    # reference blog: the maintainer's own site
│   └── docs/                   # mdsvex + Pagefind documentation site
├── crates/
│   ├── colophon-domain/        # rules, state machines, port traits; no I/O dependencies
│   ├── colophon-db/            # SQLx adapter: transactions, outbox, RLS context
│   ├── colophon-auth/          # OIDC, sessions, roles, machine credentials, policy
│   ├── colophon-api/           # Axum management API /api/v1
│   ├── colophon-delivery/      # public read API /content/v1, cache, CDN adapter, probes
│   ├── colophon-worker/        # outbox dispatcher, leases, retries, reconciliation, schedules
│   ├── colophon-media/         # upload intents, quarantine, verification, derivatives
│   ├── colophon-automation/    # agent tools, bulk jobs, budgets, MCP adapter, optional AI assist
│   ├── colophon-sim/           # deterministic simulator and invariant checkers
│   ├── colophon-bench/         # Criterion micro-benchmarks of the hot paths (D-074)
│   ├── colophon-cli/           # binary: colophon
│   ├── colophon-mcp/           # binary: colophon-mcp, the MCP server on stdio, an API client (D-068)
│   └── colophon-server/        # binary: colophon-server (composition root)
├── packages/
│   ├── sdk/                    # generated TypeScript client + pinned-read helpers
│   ├── ui/                     # Material 3 tokens and shared Svelte components (Bits UI)
│   └── content-renderer/       # Tiptap JSON → safe output, shared by Studio and web
├── migrations/                 # reviewed SQL migrations
├── schemas/                    # versioned content-schema definitions and fixtures
├── templates/blog/             # blog template manifest, schemas, fixtures
├── tests/                      # contracts, faults, security, load (k6), crash-loop, fixtures
├── deploy/                     # Compose, container files, runbooks
└── docs/                       # these documents, ADRs, threat model
```

### Crate dependency direction

```text
colophon-server ──► api, delivery, worker, media, automation, auth, db
colophon-cli ─────► shared request/response types (crate choice open), HTTP client
colophon-mcp ─────► HTTP client only: it calls /api/v1 with an API token, like any script (D-068)
api / delivery / worker / media / automation ──► domain (+ auth for policy)
db ──► implements domain and auth ports
sim ──► domain, plus in-memory and fault-injecting port implementations
bench ──► domain, auth (benchmarks only; nothing depends on it)
domain ──► nothing that does I/O
```

Dependencies point inward. Because `colophon-domain/Cargo.toml` does not list `tokio`, `axum`, `sqlx` or `reqwest`,
domain code cannot import them; the build enforces the boundary (rust-mastery chapter 10, "Ports and adapters").

## Cross-cutting limits (initial; spec §26, §33)

| Resource | Limit |
|---|---|
| API request body | 1 MiB (uploads stream separately) |
| Page size | default 20, maximum 100 |
| Rich-text depth / relation expansion depth | 16 / 2 |
| Ordinary database query | 2 s deadline |
| Database pools | API 20, worker 10 (reference profile) |
| Outbox external attempt | 10 s deadline, ≤ 5 attempts, capped backoff with jitter, 1 h job lifetime |
| Worker pools | 8 publishing/delivery (≤ 2 per destination), 4 image, 2 AI |
| Pending jobs | 10,000 per project; 1 GiB pending payload metadata per install |
| Bulk agent chunk | ≤ 100 records and ≤ 1 MiB |
| Cache | 1 GiB reference budget, explicit eviction |
| Logs | 7 days and 1 GiB per install, 16 KiB per record, visible dropped-record counter |
| Storage | warn at 70%, reject optional growth at 85%, block unsafe writes before 90% |

## Technology baseline

Version lines are from the rust-mastery crate guide (checked 2026-10-22). Re-check with `cargo info` and freeze a
tested set at P0 (spec §02).

| Need | Choice |
|---|---|
| Runtime / HTTP | `tokio` 1.x, `axum` 0.8 (`/{param}` paths), `tower` 0.5, `tower-http` 0.7 |
| Database | `sqlx` 0.9 with reviewed SQL migrations; PostgreSQL 18 |
| Errors | `thiserror` 2 in libraries; `anyhow` 1 in the two binaries only |
| Observability | `tracing` + `tracing-subscriber`; optional OpenTelemetry export (keep the `opentelemetry*` family on one version line) |
| IDs / time | `uuid` v7 newtypes; `jiff` for IANA time zones (scheduled publishing) |
| CLI | `clap` 4 (derive) |
| Outbound HTTP | one shared `reqwest` 0.13 client (rustls) with connect and request timeouts |
| Secrets / tokens | Redacting newtypes for in-memory secrets (no `Display`, redacted `Debug`); `aws-lc-rs` for random tokens, SHA-256 and constant-time comparison |
| Sign-in | An OpenID Connect relying party on `reqwest` and `jsonwebtoken` (`aws-lc-rs` backend), not an OIDC crate (D-031) |
| Testing | `proptest`, `insta`, real PostgreSQL from the local stack (a fresh database per test), `cargo-nextest`, `cargo-deny` |
| Frontend | SvelteKit + TypeScript + Vite, scoped Svelte CSS over M3 tokens (Tailwind deferred, D-025), Bits UI, Tiptap, mdsvex, Pagefind, Vitest, Playwright |
| Design system | Colophon's own (D-053): OKLCH tokens in one stylesheet, Tailwind CSS v4, owned shadcn-svelte components on Bits UI primitives, Inter (self-hosted); contrast tested from the tokens |
| Load testing | k6 arrival-rate scenarios |
