# Colophon as a SaaS — plan

Status: **plan, not built.** Written 2026-11-13. Nothing here is deployed yet; every claim about today's code
cites where it lives. Numbers measured on the droplet do not exist yet and are marked *unmeasured*.

## The goal

Colophon runs as its own hosted product: a person signs up, gets a workspace, creates a project, models content,
publishes, and is never handed a terminal. The first users are the maintainer's friends; payments are bypassed.

Decisions already made (2026-11-13, by the maintainer):

- **Independent product.** Colophon, Service Monitor and Durable Runner are separate SaaS products with their own
  repositories, accounts, logins, dashboards and droplets. They meet only through public APIs and tokens.
- **One 2 GB DigitalOcean droplet** for Colophon.
- **Neon Postgres** is the database.
- **Landing page content is edited in Colophon** — Colophon's own landing page and the other two products'.
- **Payments are bypassed** for now: a free plan with limits, no billing code.

## Where Colophon is today

- Accounts: password sign-up and sign-in exist (`POST /auth/signup`, module 03), plus optional OIDC through
  Keycloak. **Signing up grants nothing**: only the installation's first administrator can create projects
  (`docs/modules/03-auth.md`, "the only kind of person who can create projects").
- Hostile-tenant open registration is an explicit non-goal (spec §01, module 03 §Non-goals).
- Projects, roles, members, invites, tokens, connections, releases, schedules (D-060), media and the audience
  surfaces are built (P0–P7). The Studio is the strongest UI of the three products (`@colophon/ui`, D-056).
- Runtime dependencies: PostgreSQL (`COLOPHON_DATABASE_URL`, `COLOPHON_MIGRATION_DATABASE_URL`), Valkey/Redis
  (`COLOPHON_CACHE_URL`, used by quotas), S3-compatible media (Garage in `deploy/compose.yaml`), optional Keycloak.
- Postgres features in use: `LISTEN`/`pg_notify`, advisory locks, `FOR UPDATE SKIP LOCKED`.
- Backups: `scripts/backup.sh` uses `pg_basebackup`/`pg_verifybackup` (D-130).
- Readiness: the 24-hour soak and the v0.1 release are still ahead (see `ROADMAP.md`).

## The gaps between "installable CMS" and "SaaS"

| Gap | What has to change |
|---|---|
| Who can create a project | A signed-up, verified account can create projects up to its plan's limit, not only the installation administrator. |
| Open registration | Beta signup is **invite-code gated** — friends get codes. This keeps the hostile-tenant non-goal honest until per-tenant quotas, abuse limits and isolation tests exist. |
| Email | Verification, password reset and invites need an SMTP provider (Postmark, Resend or SES). |
| Plan limits | A free plan with explicit limits: projects per account, entries, media bytes, API requests per minute (quotas already key on project and token). Limits show in the UI before they bite. |
| Onboarding | Sign up → verify → create workspace/project from a starter (`FirstRun`, `NewProject`, `starters.ts` exist) → first publish. |
| Account lifecycle | Delete account, export project content, leave project. |
| Neon | See below. |
| Backups | `pg_basebackup` cannot run against Neon. Use Neon's point-in-time restore and branches; keep the ledger export. Restore drills prove the new path. |
| Media | Garage on a 2 GB droplet competes for memory; prefer DigitalOcean Spaces or R2 (S3 API). Whether Colophon's S3 adapter works unchanged against them is *unverified*. |
| Keycloak | Not on the droplet (memory). Password accounts only for the beta. |

## Neon

- Use Neon's **direct (unpooled) endpoint** for the server: `LISTEN`/`pg_notify` and session advisory locks do not
  survive a transaction-mode pooler. Size the connection pool to Neon's compute limits.
- Migrations run through `COLOPHON_MIGRATION_DATABASE_URL` with the owner role; the app runs with a lower role.
- Pick the Neon region closest to the droplet region; every query now crosses a network, so laptop latency numbers
  do not carry over.
- Neon free-tier compute can suspend when idle; the first request after a suspend is slow. Decide whether that is
  acceptable for the beta or keep compute always on.

## Droplet layout (one 2 GB droplet)

```
Caddy (TLS, HTTP/2)  →  colophon-server (API + Studio + worker loop)
                         ├── Valkey (maxmemory capped, e.g. 128 MB)
                         ├── Neon Postgres  (managed, remote)
                         └── Spaces/R2      (media, remote)
```

- systemd units, `deploy/droplet/` with env template, Caddyfile and runbook.
- Memory budget is *unmeasured*. Measure idle and under load before inviting anyone; add a swap file as a guard,
  not as capacity.

## Landing pages

The landing pages for Colophon, Service Monitor and Durable Runner are content in **one Colophon project**.
Each page is rendered and exported to static hosting (Cloudflare Pages) on publish, so a Colophon outage stops
editing, not the other products' landing pages. That keeps the products independent.

Note: the older landing page (`apps/site` on `site/new-look`) is unmerged and breaks on main's data; rebuild it on
main rather than reviving the branch.

## Integrations (public API only)

- **Service Monitor:** the user creates an API token in Service Monitor and pastes it into Colophon's
  Connections settings; Colophon registers its sites as monitors. Without it, Service Monitor users just add the
  URLs. Colophon never depends on Service Monitor being up.
- **Durable Runner:** the publish webhook submits post-publish jobs (static export, cache purge) and nightly
  ledger exports to a Durable Runner workspace with its API key. Scheduled publishing itself stays in Colophon
  (D-060) — it commits the release and the schedule state in one transaction, which an at-least-once external
  runner cannot preserve.

## Phases and exit gates

| Phase | Outcome | Exit gate (evidence) |
|---|---|---|
| S0 Deploy as-is | Colophon on the droplet against Neon, used by the maintainer only, for the landing pages. | Health checks green, restore drill from Neon PITR recorded, idle memory recorded. |
| S1 Accounts | Invite-code signup, email verification, password reset, self-serve project creation within limits. | Tests: a non-admin verified account creates a project; an unverified or over-limit one cannot; tenant isolation tests across two accounts. |
| S2 Onboarding | Starter-based first run to first publish in the Studio. | Playwright journey: signup → first publish, on the droplet. |
| S3 Limits | Plan limits enforced and shown in the UI. | Tests at each limit edge; noisy-tenant run shows other tenants unaffected. |
| S4 Load on the droplet | The existing load scenarios run against the droplet + Neon, ≥1M requests, load generated from another machine. | Results committed under `perf/results/` with droplet spec and commit hash; thresholds pass or failures recorded. |
| S5 Friends beta | Invite codes go out. | Runbook for incidents, backup/restore and account deletion exercised once. |

The 24-hour soak can run on the droplet during S4 and count as real-hardware evidence.

## Open questions

1. Neon compute: always on, or accept cold starts?
2. Media storage: DigitalOcean Spaces or Cloudflare R2?
3. Email provider for verification and invites?
4. Free-plan limits: how many projects, entries and media bytes per friend?
