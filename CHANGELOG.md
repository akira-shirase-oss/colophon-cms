# Changelog

All notable changes to Colophon are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) (D-062): before 1.0 a minor
release may change `/api/v1`, and every binary reports its version, commit and build date
through `--version` and `GET /version`.

## [Unreleased]

### Added

- **Secrets from files:** every secret setting also takes `<KEY>_FILE`, a path to a mounted
  secret; the admin commands take `COLOPHON_MIGRATION_DATABASE_URL_FILE` (D-171).
- **`colophon admin init-roles`:** creates the server's database roles with passwords from
  files, sent as SCRAM verifiers; refuses the development password; safe to rerun (D-171).
- **A container image and a one-command quick start:** `deploy/image/Dockerfile` (server, CLI
  and Studio on distroless static, non-root, 25 MB compressed) and
  `deploy/quickstart/compose.yaml` — `docker compose up -d --wait` brings up Colophon with
  PostgreSQL, Valkey and Garage, secrets generated on first start, only `127.0.0.1:8080`
  published (D-172, D-173). The Studio's first-run screens say where a container's setup code
  is. Operator guide: `deploy/runbooks/install.md`.

### Security

The 2026-11-16 security fixes (D-148 to D-157, D-159 to D-169). Each names the attack it
closes and carries a test that failed without it.

- Magic links land only on an origin the project lists, and spending one retires its
  siblings (D-148); requests are bounded per client address first, then per recipient with
  an escalating cooldown, then per project (D-153, D-161).
- Destinations (webhooks, purges, probes) reach only public https addresses; the worker
  ignores proxy settings from the environment (D-149).
- Password work shares a bounded gate, and verifying never holds a database row or
  connection (D-150).
- GraphQL refuses deep nesting, oversized complexity and fragment cycles before executing,
  and deep bracket nesting before parsing (D-151, D-160).
- Magic-link tokens and device codes never reach a log line (D-152).
- Sign-in guessing pauses the guessing (account, address) — never the account — with a
  per-account ceiling the owner's own addresses bypass; IPv6 clients are keyed by their /64
  (D-154, D-162). Sign-in keys are row-level secured by realm (D-166).
- The owner-protecting triggers yield to roles, not to settings (D-155).
- PostgreSQL over TLS (D-156); media at a bare address revalidates, versioned media is
  immutable (D-157); anonymous live-list streams have their own budget (D-159).
- Development mail and the public development OIDC client need a localhost public origin
  as well as a loopback listener (D-163).
- The public sign-in forms read at most 16 KiB of body (D-164); the retention sweep keeps
  pace with the tables strangers grow (D-165); deleting a project removes its audience and
  asset versions (migration 0061), and the application can no longer call it (D-167).
- The owner guards pin their `search_path`, and the project-existence check answers only
  its trigger (D-168); only the right password learns an audience account is disabled
  (D-169); a token on an unmatched path is redacted from logs (D-152); a magic link's token
  goes before a `return_to` fragment.

### Changed — read before upgrading

- **Magic links stop until origins are listed:** `PUT {P}/audience/origins` must name each
  site a link may land on; until then a request is `422 return_to_not_allowed` (D-148).
- **Private destinations need a flag:** a server reaches loopback, private or link-local
  addresses only with `COLOPHON_ALLOW_PRIVATE_DESTINATIONS=1` — development only (D-149).
- **Flags are strict:** a boolean setting is `1`/`true`/`0`/`false`; anything else stops the
  server at start-up instead of being read as off.
- **`409 too_many_attempts` is gone:** a paused sign-in answers the same `400
  sign_in_failed` a wrong password does (D-154).
- **New answers:** `429 rate_limited` with `Retry-After` for magic-link requests (D-153,
  D-161); `503` with `Retry-After` when password work is saturated (D-150); `413` for a
  public sign-in body over 16 KiB (D-164); `account_disabled` only to the right password
  (D-169).
- **Deleting a project is an operator's job:** `colophon_delete_project` runs from the
  owner's login, no longer from the application's (D-167).
- **Behind a reverse proxy, set `COLOPHON_TRUSTED_PROXIES`:** without it every client is the
  proxy, and every per-address bound — admission, sign-in pauses, magic links — becomes one
  shared bucket.
- **New settings:** `COLOPHON_PASSWORD_CONCURRENCY`, `COLOPHON_MAGIC_LINKS_PER_ADDRESS`,
  `COLOPHON_MAGIC_LINKS_PER_PROJECT`, `COLOPHON_EVENTS_PUBLIC_*` (module 18).
- **Licence:** MIT (`LICENSE`, D-158).
- **Security floor for rollback:** once deployed, never roll back to a binary without these
  fixes, whatever the schema allows (`deploy/runbooks/upgrade.md`, section 7).

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
  publishing waits on the maintainer (npm organisation, credentials — module 19);
  the licence is MIT (`LICENSE`, D-158).

## How to upgrade

`deploy/runbooks/upgrade.md` is the operator's procedure: verified backup →
`admin migrate-status` → binary swap → `admin migrate` → smoke checks, with rollback
rules per schema compatibility.
