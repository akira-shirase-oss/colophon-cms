# Changelog

All notable changes to Colophon are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) (D-062): before 1.0 a minor
release may change `/api/v1`, and every binary reports its version, commit and build date
through `--version` and `GET /version`.

## [Unreleased]

### Changed — read before upgrading

- **Images inside rich text must name real uploads** (D-177). A rich-text `image` whose
  `assetId` isn't an upload of the environment is refused `422 asset_not_found` at that
  image's pointer. Drafts made with the old Studio dialog hold random placeholder ids, so they
  are refused at their next save until each image is replaced or removed; the Studio marks the
  image in place and offers Replace and Remove. Revisions saved before migration 0066 record no rich-text references until saved again.
- **A replaced image reaches published pages at the next publish** (D-178). Delivery rows now
  carry `media`: the versioned, immutable URL of each upload the release recorded, with its
  size and renditions. A site using `url` keeps showing the released bytes after a replace
  until the entries using it are published again; `current_url` follows the newest bytes.
  `RENDER_VERSION` is 2, so cached delivery answers are refetched once.

### Performance

- Making an API token holds the project's creation lock for two statements instead of eight:
  the count of active tokens stays exact (one creator at a time per project), but queued
  creators no longer hold pooled connections long enough to starve reads.
- Watched writes no longer take PostgreSQL's NOTIFY lock (D-180): their live-list hints are
  queued in an unlogged table and sent by a pump every 100 ms, so concurrent saves commit in
  groups again instead of one at a time. A live list may hear a change up to 100 ms later.
- Lists of schedules and releases, and the changesets that stop an entry's deletion, read only the rows they
  return: titles and names are looked up after the limit, so a plan that misjudged a project's size can no
  longer pair every row with every changeset (a 120-row schedule list read 16,203 changeset rows, a 100-release
  page 10,150; the schedules load smoke's lists took three seconds).

### Added

- **The TypeScript SDK is ready for npm (D-170):** `@colophon-cms/sdk` gains
  `client.compatibility()` and a `compat` option (`warn` by default, `throw`, `off`) that say
  plainly when the server is too old or too new, by the CLI's rule (D-062). A packaging gate
  checks what npm would ship on every push — file allowlist, size budget, publint, Are the
  Types Wrong, and a scratch project that installs the tarballs, calls a stub server, compiles
  the README's examples and bundles the client for a browser (4.5 KB min+gzip).
- **`@colophon-cms/rich-text` (D-170):** the content renderer as its own zero-dependency
  package, built from the one renderer the Studio uses, so a site reading the delivery API
  renders rich text exactly as the Studio previews it.
- **The release workflow packs both (D-170):** `npm-pack` checks the SDK and rich-text
  tarballs and writes `npm publish --dry-run` of every package into the job summary;
  `npm-publish` publishes them after the CLI. It also no longer reads `npm-dist/x.tgz` as a
  GitHub repository, which would have failed the first tag's publish.
- **SDK replies name their generation:** `response.generation` is the `Colophon-Generation` a
  read answered from, so a page can pin the rest of its reads to it (`atGeneration`). A
  public-API report per package makes export changes visible in review, and
  `perf/node/sdk-delivery.mjs` measures the client's overhead and heap beside bare `fetch`.
- Draft preview on your own site, the backend (D-171, D-172): a person opens a preview of an
  entry (with its translations) or a publish request; the site exchanges the address's
  one-time code for a 15-minute bearer token and reads `/preview/v1`, which answers
  `/content/v1`'s shapes with the drafts laid over the latest release. Never cached, never
  indexed, re-checked against the issuer on every read, revocable. Set the address with
  `PUT {P}/preview-target`.
- Preview in the Studio (D-181): Settings → Website connections → "Preview on your site", and
  a Preview menu in the entry editor that shows the saved draft on your site side by side or
  in a new tab, refreshed after each save; a publish request's page previews exactly what it
  will publish.
- `/preview/v1` answers CORS for the environment's preview origin, so a static site can read
  previews from the browser (never with credentials).
- `colophon preview ENTRY` (or `--changeset ID`) prints a one-time preview address for your
  site.
- `@colophon-cms/sdk` preview helpers for your site (D-181): `previewClient`,
  `createPreviewHandler`, `previewTokenFromRequest`, `previewHeaders`, `exitPreviewHandler` and
  `listenForDraftUpdates`, with a guide for SvelteKit, Astro and Next.js
  (`docs/guides/preview.md`).
- **Secrets from files:** every secret setting also takes `<KEY>_FILE`, a path to a mounted
  secret; the admin commands take `COLOPHON_MIGRATION_DATABASE_URL_FILE` (D-173).
- **`colophon admin init-roles`:** creates the server's database roles with passwords from
  files, sent as SCRAM verifiers; refuses the development password; safe to rerun (D-173).
- **A container image and a one-command quick start:** `deploy/image/Dockerfile` (server, CLI
  and Studio on distroless static, non-root, 25 MB compressed) and
  `deploy/quickstart/compose.yaml` — `docker compose up -d --wait` brings up Colophon with
  PostgreSQL, Valkey and Garage, secrets generated on first start, only `127.0.0.1:8080`
  published (D-174, D-175). The Studio's first-run screens say where a container's setup code
  is. Operator guide: `deploy/runbooks/install.md`; without containers, `deploy/production/`
  (a hardened systemd unit with secrets as credentials, a Caddyfile, the steps).
- **Releases carry the image and the Studio:** the release workflow builds the image for amd64
  and arm64 from the release's own checked binaries, scans it (Trivy: no fixable critical CVE),
  and on a tag pushes it to `ghcr.io/akira-shirase-oss/colophon` with signed provenance; it
  attaches `colophon-studio-<version>.tar.gz` and the quick start's `compose.yaml`, pinned to the
  image's digest (`SHA256SUMS` now lists 24 files). An allocator job measures the musl server
  against a glibc build of the same commit (D-174, D-176).
- Images inside rich text are tracked like every upload: publish-checked, kept from deletion,
  listed under "Used in"; at most 200 distinct images per document (D-177).
- The Studio inserts images into rich text from the media library (pick, search or upload,
  then alt text, required), shows each image as itself with its state, and edits its alt text,
  replaces or removes it from the keyboard; a pasted or dropped image is uploaded and inserted.
- The Studio's Media page shows the whole library (paged, searchable, filtered by state and
  tag), uploads many files at once through a bounded queue, and opens each upload's details:
  its alt text, title, credit and tags (saved under If-Match, with a conflict state), versions,
  where it's used, replace with the republish warning, both addresses to copy, and delete.
- Renderer v2 (`@colophon-cms/rich-text`) gives images `width`, `height`, `srcset` and
  `sizes` from the delivery row's `media`; the SDK's `mediaFor(row)` builds that for a site,
  and the reference and demo sites use it.
- `COLOPHON_MEDIA_URL`: the origin delivery's media URLs name (default the public origin).
- The media library pages past 100 uploads (`cursor`, `next_cursor`), searches (`q`), filters
  by `state` and `tag`, and keeps alt text, title, credit and tags via
  `PATCH …/assets/{asset}` under `If-Match` (D-179).

### Security

The 2026-11-16 security fixes (D-148 to D-157, D-159 to D-169). Each names the attack it
closes and carries a test that failed without it.

- A content-migration run no longer un-gates an entry: the revision it wrote took the default
  `public` visibility, so publishing a migrated member-only entry served it to anonymous
  readers. The run's revision now keeps the gate of the draft it replaces, and revisions and
  publications runs already wrote are corrected (D-183, migration 0070).
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
- PostgreSQL over TLS, and a database on another machine only with `sslmode=verify-full`
  (D-156); media at a bare address revalidates, versioned media is immutable (D-157);
  anonymous live-list streams have their own budget (D-159).
- Development mail and the public development OIDC client need a localhost public origin
  as well as a loopback listener (D-163).
- The public sign-in forms read at most 16 KiB of body (D-164); the retention sweep keeps
  pace with the tables strangers grow (D-165); deleting a project removes its audience and
  asset versions (migration 0061), and the application can no longer call it (D-167).
- The owner guards pin their `search_path`, and the project-existence check answers only
  its trigger (D-168); only the right password learns an audience account is disabled
  (D-169); a token on an unmatched path is redacted from logs (D-152); a magic link's token
  goes before a `return_to` fragment.

### Fixed

- Integration tests that stop, pause or restart a local-stack service now refuse the shared stack
  before any `docker` call: they run in CI, under a compose project of your own, or with
  `COLOPHON_STACK_OWNER=1` (D-187b). Every such call goes through one guarded helper, and a test
  keeps it that way.

- The releases list pages: `GET {P}/releases` takes `?limit=` (1–100, default 100, what it
  answered before) and `?cursor=`, and its `next_cursor` is real. It used to stop at the
  newest 100 while the OpenAPI document advertised a cursor. The MCP `list_releases` tool
  takes the cursor too, and the Studio's Releases page has **Show older releases**.
- The OpenAPI document says only what the routes do: lists that answer in one page say how
  much it holds and that `next_cursor` is always `null`; the receipt schema lists every
  release field itself instead of through `allOf`, and the release, delivery and observation
  fields are marked required. `@colophon-cms/sdk`'s generated types follow.

### Changed — read before upgrading

- **The static Linux server allocates through mimalloc (D-176):** musl's own allocator ran
  allocation-heavy requests five times slower than glibc's on the same runner. Other builds are
  unchanged.
- **Magic links stop until origins are listed:** `PUT {P}/audience/origins` must name each
  site a link may land on; until then a request is `422 return_to_not_allowed` (D-148).
- **A database on another machine needs `sslmode=verify-full`:** the server and the admin
  commands refuse to start on a non-loopback `COLOPHON_DATABASE_URL` or
  `COLOPHON_MIGRATION_DATABASE_URL` with any other mode — `require` (what Neon's copied URL
  says) takes any certificate, and `verify-ca` takes any publicly trusted one — and on
  `channel_binding=require`, which the driver would ignore. Name a private CA with
  `sslrootcert`. A loopback host or a Unix socket is unchanged (D-156).
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
