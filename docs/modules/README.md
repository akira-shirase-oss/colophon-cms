# Modules

One document per module. Each follows the same structure: **why it exists, end goal, responsibilities, what it is not
responsible for, a breakdown of what gets built, invariants, interfaces, failure modes, tests and evidence, build
order, and open decisions.**

Ownership is a proposal (O-009), not yet confirmed. Regardless of owner, database migrations, authentication changes,
publishing transitions and provider adapters always get a manual line-by-line review (spec §34).

## Rust backend

| # | Module | Path | One line | Milestones | Proposed owner |
|---|---|---|---|---|---|
| 01 | [domain](01-domain.md) | `crates/colophon-domain` | The rules: content model, workflow, releases, state machines, ports | P0–P4 | Maintainer, Claude pairs |
| 02 | [db](02-db.md) | `crates/colophon-db`, `migrations/` | PostgreSQL adapter: constraints, transactions, outbox, RLS | P1–P6 | Maintainer, Claude pairs |
| 03 | [auth](03-auth.md) | `crates/colophon-auth` | OIDC sign-in, sessions, roles, machine credentials, policy checks | P1, P2, P4, P5 | Claude; manual review |
| 04 | [api](04-api.md) | `crates/colophon-api` | The management API `/api/v1` shared by Studio, CLI, SDK and agents | P0–P5 | Claude |
| 05 | [worker](05-worker.md) | `crates/colophon-worker` | Durable background work: outbox, leases, retries, reconciliation, schedules | P2–P5 | Claude; manual review |
| 06 | [delivery](06-delivery.md) | `crates/colophon-delivery` | Public reads, time travel, caching, CDN purge, revocation, evidence probes | P2–P4 | Claude; manual review |
| 07 | [media](07-media.md) | `crates/colophon-media` | Upload intents, quarantine, verification, derivatives, asset lifecycle | P4, P6 | Claude |
| 08 | [cli](08-cli.md) | `crates/colophon-cli` | The `colophon` command: scriptable, stable JSON, honest exit codes | P1–P6 | Claude |
| 09 | [automation](09-automation.md) | `crates/colophon-automation` | Scoped agent tools, bulk jobs, budgets, MCP adapter, optional AI assist | P5 | Claude |
| 10 | [sim](10-sim.md) | `crates/colophon-sim`, `tests/crash-loop` | Deterministic simulation, crash loop, consistency checker | P1–P6 | Claude builds; maintainer defines invariants |

## Frontend and packages

| # | Module | Path | One line | Milestones | Proposed owner |
|---|---|---|---|---|---|
| 11 | [studio](11-studio.md) | `apps/studio` | The editor app: write, review, publish, receipts, time travel | P0–P6 | Claude |
| 12 | [content-renderer](12-content-renderer.md) | `packages/content-renderer` | One renderer for Studio preview and the public site | P0–P3 | Claude |
| 13 | [sdk](13-sdk.md) | `packages/sdk` | Generated TypeScript client with generation-pinned reads | P2–P3 | Claude |
| 14 | [ui](14-ui.md) | `packages/ui` | Design tokens and shared accessible components | P0–P1 | Claude |
| 15 | [web and blog template](15-web-blog.md) | `apps/web`, `templates/blog` | The reference blog, which is the maintainer's own site | P3–P4 | Claude; maintainer owns content and taste |
| 16 | [docs site](16-docs-site.md) | `apps/docs` | Guides, references, runbooks and technical write-ups | P3–P6 | Claude; maintainer writes the deep dives |

## Cross-cutting

| # | Module | Path | One line | Milestones | Proposed owner |
|---|---|---|---|---|---|
| 17 | [verification](17-verification.md) | `tests/` | Fault, load, security and contract matrices; the release evidence bundle | All | Shared |
| 18 | [operations](18-operations.md) | `deploy/` | Local stack, deployment, observability, backups, restore, runbooks | P0, P3, P6 | Claude; maintainer runs the drills |
| 19 | [open-source hygiene](19-oss.md) | repo root, `.github/` | License, governance, CI security, releases, support policy | P0, P6 | Maintainer decides; Claude scaffolds |
| — | [parked](parked.md) | — | What is deliberately out of v0.1, and when it may return | — | — |
