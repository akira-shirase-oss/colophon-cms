# Changelog

All notable changes to Colophon are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) (D-062): before 1.0 a minor
release may change `/api/v1`, and every binary reports its version, commit and build date
through `--version` and `GET /version`.

## [Unreleased]

### Added

- **Connect an AI app by the MCP address alone** (D-246): add `https://<your server>/mcp` to Claude, ChatGPT
  or Cursor as a custom connector, sign in, pick a project and approve. No API token to copy; the app acts as
  you, with your role, in that one project, and the Studio's Connected apps page disconnects it.

### Security

- The Studio's sign-in only returns to an address on its own origin (`/\evil.example` used to pass).

### Fixed

- **The Studio never waits past 15 seconds for an answer** (D-245): a request whose answer stalls on a
  flaky connection now stops with "Colophon didn't answer: try again", and publishing again sends the same
  request rather than a second one.

## [0.1.0-alpha.6] — 2026-11-23

### Added

- **A signed-in hint for sites on the same domain** (D-235): with `COLOPHON_SIGNED_IN_HINT_DOMAIN` set, a readable
  `colophon_signed_in=1` cookie follows the session (set on sign-in, refreshed with use, cleared on sign-out or a
  refused session), so a landing page can show "Open the Studio". It grants nothing: every request is still judged
  by its session.
- **Administrators disable and enable accounts** (D-235, migration 0096): `POST /api/v1/accounts/{identity}/disable`
  revokes every session the account holds and every API token it made in one transaction and logs it; the
  account then signs in like a wrong password until `/enable`. Nobody can disable their own account, and the last
  enabled administrator stays enabled.
- **The caller's usage** (D-235): `GET /api/v1/me/usage` answers projects, entries, files and storage, each with its
  limit (`null` when unbounded), and the Studio has a Usage page. `GET /auth/providers` also says whether
  registration is `open`, `invite` or `closed`, so the Studio's new `/studio/signin` and `/studio/signup` pages
  show a closed or invite-only installation before anyone fills a form.
- **Self-serve projects and per-account quotas** (D-235). `COLOPHON_REGISTRATION=open` lets any signed-in account
  create up to `COLOPHON_PROJECTS_PER_ACCOUNT` (3) projects it owns; `invite` (the default) and `closed` keep project
  creation to administrators. Each account's entries, assets and stored bytes are counted in the database by
  triggers, so every write path is bounded the same way (`COLOPHON_ACCOUNT_MAX_ENTRIES`, `_ASSETS`,
  `_STORAGE_BYTES`; 409 `quota_exceeded`), and an upload past the storage quota is refused before any of its bytes
  reach the object store. Administrators are exempt. `colophon admin usage-reconcile [--check]` recounts usage;
  the droplet's smoke check runs it.

### Removed

- **The 24-hour soak test** (D-240), by the maintainer's decision: `scripts/soak.sh`, `scripts/soak-sidecar.sh`
  and `scripts/load.sh`'s `DRILL_WINDOWS` are gone, and the soak is no longer part of v0.1.

### Security

- **No login CSRF, no session fixation, `__Host-` session cookies** (D-235). Sign-up and sign-in refuse a
  cross-site browser and any `Origin` but the installation's (`403 origin_not_allowed`); a request with neither
  `Origin` nor `Sec-Fetch-Site`, as a CLI sends, still works. Signing in, signing up and coming back from the
  identity provider rotate the session and end the one the browser presented; becoming the administrator with
  the setup code rotates it too. The session cookies are now `__Host-colophon_session` and
  `__Host-colophon_csrf`. **Upgrading:** nobody is signed out. The old names (`colophon_session`,
  `colophon_csrf`) are read in this release only, for sessions made before the upgrade, and swapped for new-name
  ones on first use. The release after this one stops reading them. Scripts that set the cookie by
  name must use the new names.
- **Sign-up guards and a staff sign-in log** (D-235, migration 0095). Sign-ups are capped per client address
  (`COLOPHON_SIGNUPS_PER_ADDRESS_HOUR`, default 10 an hour; every attempt counts, refused ones included; `429
  rate_limited` with `Retry-After`), and with `COLOPHON_TURNSTILE_SITE_KEY` and `COLOPHON_TURNSTILE_SECRET` set,
  must carry a Cloudflare Turnstile token, checked server-side and failing closed (`422 bot_check_failed`).
  `staff_auth_events` records sign-ups, refusals, sign-ins and sign-outs without passwords, tokens or emails
  (failures carry an HMAC of the email under `COLOPHON_AUTH_EVENT_KEY`), append-only, kept 90 days.
- **`COLOPHON_REGISTRATION` now decides who may sign up** (D-235). Until now it only decided who may create
  projects: `POST /auth/signup` made an account on every installation, `invite` and `closed` included. Now `closed`
  refuses every sign-up (`403 registration_closed`) and every first sign-in through the identity provider;
  `invite` takes a sign-up only with a valid, unclaimed project invitation (`invitation` in the body), claimed for
  exactly one account in the same transaction (`403 invitation_required` otherwise, alike for every reason), plus
  the one first-run account a new installation makes before its setup code is entered. The sign-up's `201` now
  says which project a claimed invitation joined, and the Studio's invitation page uses it. **Upgrading:** an
  installation that never set `COLOPHON_REGISTRATION` is invite-only after this upgrade; set `open` deliberately
  if anyone should be able to sign up. With a public identity provider (anyone can hold an account there), use
  `closed`: under `invite` a first sign-in through the provider still makes an (inert) identity.
- **Row security no longer lets an account create a project for another** (D-235): a new project's owner is the
  account creating it, enforced by a row policy and immutable afterwards. Only listed code may set the project
  context (`crates/colophon-db/context-writers.allow`, checked by a test), and SECURITY.md describes the threat model.
- **A site's public audience routes run as their own database role** (D-235, migration 0094). Sign-up, sign-in,
  magic links and sign-out take the project from the site's URL; they now switch to `colophon_audience`, which can
  reach only the audience's own tables, so the database itself refuses them any project's entries, revisions,
  assets, tokens, members or environments. Before, only review kept those statements to the audience tables.
- `cookie` 0.18.3 (RUSTSEC-2026-0594). Not reachable: Colophon uses no private cookie jar and doesn't
  fork.

### Fixed

- **A save behind a held row answers "try again", not an internal error** (D-239). A request
  queued on a row or lock someone held for seconds used to wait out the 5 s statement timeout and
  answer `500 internal`; it now gives up after 3 s with `503 unavailable` and `Retry-After`,
  having committed nothing, so retrying with the same `Idempotency-Key` saves exactly once. A
  statement that is slow on its own still answers `500` and is now logged by name
  (`fault: "statement_timeout"`, with its route) so it can be fixed rather than retried.
- **The worker's claims and sweeps no longer slow down as an installation grows** (D-237). Claiming
  deliveries read every waiting job, and every job of an environment, to take a handful; the asset
  jobs' deadline sweep read the whole table every 10 seconds; the delivery sweeps read an
  environment's history for each row they changed; and the media retention sweep counted a
  project's open work once per old asset, which at a few tens of thousands of assets took longer
  than it is allowed and never finished. Each now reads its own few rows. A delivery whose worker
  died mid-attempt resumes up to 10 seconds after its lease runs out (30 seconds by default),
  instead of at the next claim; its receipt says the update is pending meanwhile.
- **The releases page and the schedules list stay fast as an environment grows** (D-219). Each page
  now reads its releases' or schedules' changesets once as a set, so a database whose statistics
  lag behind its growth can no longer make one page read every changeset once per row.
- **A flood of new clients while Valkey is down can no longer reset everyone's rate limits**
  (D-238). With Valkey away each server keeps limits of its own, at most 100,000 keys; at that bound
  it used to start over, so a flood of new addresses handed every limited client, a password
  guesser's address included, a fresh burst. It now lets the least restrictive keys go first and
  keeps the ones actually being limited.
- **Stopping the server no longer cuts background work off mid-transaction**. The worker's
  sweeps, the pending-work fold, the media and table retention sweeps and the change-hint sender
  each stopped the moment shutdown began, even while a database transaction was committing, so
  whether that work was saved could stay unknown and the database logged "there is no
  transaction in progress". Each now finishes the step it is on and stops before the next; a step
  stuck behind a lock gives up on its own within four seconds and is rolled back, so shutdown still
  completes inside its deadline. Background work still waiting for a database connection when shutdown
  begins no longer starts at all, so it can't hold the stop up either (D-236).

## [0.1.0-alpha.5] — 2026-11-22

### Security

- **The key that signs media addresses rotates** (D-233): `colophon admin media-key rotate` makes a new one, and the
  one before it keeps verifying the links it signed; `--retire-previous` drops every older key at once, for a key
  that leaked. Every restore makes a new key and retires its source's, so a staging copy restored from production
  can no longer sign or verify production's links. Addresses now carry the key's id (`?kid=`): links signed
  before upgrading (at most 15 minutes old) stop working at the upgrade. Each rotation is recorded, never the key.
  A server reads the keys one read at a time, so an older read can never bring back a key a rotation retired.
  Upgrading runs migration 0086 (it works under a non-superuser owner such as Neon's, D-234).

## [0.1.0-alpha.4] — 2026-11-22

### Security

- **A nested `blob:` origin no longer stops the server** (RUSTSEC-2026-0592). `url` 2.5.8 and earlier took a
  `blob:` URL's origin by recursing into its inner URL without bound, a stack overflow that aborts the whole process.
  `PUT {P}/audience/origins` took each listed origin's `origin()` before checking its scheme, and before checking
  the caller's permission, so any signed-in account could stop the server with one request (about 100 KB of nested
  `blob:` prefixes). `url` is now 2.5.9, which follows the URL Standard, and the origin check refuses anything but
  http(s) with a host before it takes an origin. Every other `origin()` call already checked the scheme first.
- **One R2 token on both buckets is an explicit operator choice, not a refusal** (`deploy/droplet/r2-keys.sh`). The
  backups still get their own token by default; `r2_backup_shares_media=yes` in `/etc/colophon/install/` lets an
  operator who has one token use it for the backups too, with a warning on every install run, because a server
  compromise could then delete backups older than the bucket lock.
- **The droplet's rate limits key by the real visitor** (white-hat review of S0, 2026-11-21). Caddy now takes the
  client from `Cf-Connecting-Ip` when the peer is Cloudflare and tells the server that one address, replacing any
  `X-Forwarded-For`; the server trusts loopback alone. Trusting all of Cloudflare's ranges in the server let a
  request from a Cloudflare address (another account's Worker) choose its own client address, and with it a fresh
  per-address password limit for every attempt.
- **Caddy's admin API is a 0600 socket** (`/var/lib/caddy/admin.sock`), not TCP 2019, which every local process
  could reach and rewrite Caddy's config through. HTTP/3, which the firewall doesn't open, isn't advertised.
- **HSTS**: the droplet sends `Strict-Transport-Security: max-age=31536000` for its own host (no
  `includeSubDomains`: other hosts of the zone are on Cloudflare Pages; no preload yet). `smoke.sh` checks it.
- **Our own origin-pull certificate**: Cloudflare's default one is shared by every Cloudflare zone.
  `custom-origin-pull.sh` makes a client CA and certificate and uploads them to the zone (the token and the key
  from files, never a command line); `origin-pull-ca.sh --add-custom` / `--custom-only` move Caddy's trust over
  without downtime.
- **Releases are installed only with verified provenance**: `upgrade.sh` checks each archive with
  `gh attestation verify` against the release's own bundle (built by `release.yml` on the version's tag), which
  `release.yml` now publishes; `SHA256SUMS` alone came from the same place as the archives. `install.sh` installs
  `gh` from GitHub's repository with a pinned key.
- **The backups have their own R2 token** (`COLOPHON_BACKUP_S3_ACCESS_KEY`, `credentials/backup_s3_secret_key`),
  scoped to the backups bucket; the internet-facing server holds the media token alone.
- `smoke.sh` gives psql the database password through libpq's environment and the API token through a 0600 header
  file, never a command line; SSH allows no forwarding and 3 tries; the Redis password file is never briefly
  world-readable; the R2 ids are checked before they go into the env file; `upgrade.sh` refuses a switch when it
  can't read the migration status; the server's unit adds `ProcSubset=pid`, `PrivateUsers=yes` and
  `SystemCallFilter=~@resources`.

### Fixed

- **The droplet's deletion-ledger export wrote to the media bucket**: its unit set `COLOPHON_S3_BUCKET` with
  `Environment=`, which `EnvironmentFile=` (colophon.env's `colophon-media`) overrides. It is now set in the command.
- Every response from the droplet carries `X-Content-Type-Options: nosniff` (the JSON API's had none).
- The droplet's bare root opens the Studio (a 302 to `/studio/`) instead of the server's `route_not_found`.
- `install.sh` stops when the Caddyfile doesn't validate (`validate && reload` under `set -e` went on).
- **`smoke.sh`'s database checks work against Neon**: psql is given `sslrootcert=system` for a `verify-full` URL
  that names no CA (libpq otherwise looks for `~/.postgresql/root.crt`), in psql's own environment only:
  `PGSSLROOTCERT` in the shell broke the `colophon` CLI, whose driver reads it as a file path (found live on
  alpha.3).

## [0.1.0-alpha.3] — 2026-11-21

### Added

- **Colophon installs on a database whose owner is not a superuser, Neon's included** (D-234). `colophon admin
  init-roles` gives a login role's password as text when the server refuses a verifier (Neon: "only supports being
  given plaintext passwords"), over the connection's `verify-full` TLS, and grants the owner SET and INHERIT on
  Colophon's roles. `colophon admin migrate` runs four migrations (0011, 0012, 0051, 0082) from substitutes that pin
  no custom setting on a function, with the rights each migration needs granted inside that migration's own
  transaction (never visible to the application, gone with a crash's rollback); the database records the same
  migrations and checksums as a superuser's install, and `admin migrate-status` reports any such right left outside
  a migration (`smoke.sh` fails on it). A superuser's install is unchanged.

### Fixed

- **The droplet's `cloudflare-ips.sh` survives a re-run.** It replaced the 443 rules with `yes | ufw delete`, which
  under `pipefail` ended in SIGPIPE once ufw had read its one answer, so every run that found rules to replace
  (any re-run of `install.sh`) stopped after deleting one of them; its loop test had the same flaw. It now reads
  ufw's answers whole and deletes with `--force`, and runs the same on macOS for its test.
- **The droplet's `install.sh` stops when `init-roles` fails**, instead of going on to migrate without the roles.

## [0.1.0-alpha.2] — 2026-11-21

### Added

- A SvelteKit demo site (D-220): Butter & Thyme's recipes prerendered with SvelteKit 3 at one release per
  build (`?generation=N` on every read, a build that refuses an answer from another release), the release
  marker on every page, and a rebuild on publish with the other demo sites (`apps/sites/sveltekit`).
- A Next.js demo site (D-220a): the same recipes as a Next.js 16 static export, with one release pinned for all
  of Next's build workers before the build, and its own npm install, separate from the CMS's (`apps/sites/nextjs`).

### Changed

- The Studio takes the website's look (D-210a): page titles in Geist Pixel, a larger Overview whose Get
  started steps sit in the website's inverted panel, and a pixel icon with one line on every empty state.
  Work screens stay as dense as before.
- **An outage is logged once, not once per request** (D-224): a repeating fault (no database connection
  free, a full work bound, a dependency's failed probe) is said in full at once, then once per 10-second
  window while it lasts, then once when a whole window passes without it. Slow statements come in two
  tiers: INFO from `COLOPHON_DB_SLOW_STATEMENT_INFO_MS` (default 100) as evidence, WARN from
  `COLOPHON_DB_SLOW_STATEMENT_MS` (default 1000) as an alert.
- The S0 droplet's address is `https://colophonapp.404human.dev`, one level under the zone that Cloudflare's
  free certificate covers, and the hostname is one setting, `COLOPHON_PUBLIC_URL` in the droplet's env file:
  the install takes Caddy's site address from it and the smoke check its domain, refusing anything but a
  plain `https://<host>`.

### Changed — read before upgrading

- **A release's receipt no longer embeds what it changed** (D-185). `GET …/releases/{n}` drops
  `changes`; page them with `GET …/releases/{n}/changes?limit=&cursor=` (publishes first, then
  takedowns, each by entry id).
- **Lists name only entries your role reads** (D-185). A release's changes, a rollback preview
  and a publish request's `items` leave out entries the caller's role can't read (a request says
  how many in `hidden_items`) and blank titles it can't read (`title_hidden: true`), as the
  entries API does. Before, a role scoped to one collection or to its own entries could read
  other entries' titles and ids there. An approval now needs a role that reads every field of
  every item (`403 cannot_review_hidden`); asking for changes doesn't.
- **A rollback changes at most 190 entries** (D-185): past that `POST …/rollback` is refused
  `422 rollback_too_large` and the preview answers `too_large` with the totals. Before, a
  rollback of a few hundred entries failed with a 500 (its answer outgrew the idempotency
  record) and nothing was made.
- **A rollback preview is a promise**: `GET …/releases/{n}/rollback` carries the pins' hash as
  its `ETag` (and `hash`); `POST …/rollback` with that `If-Match` is refused `409
  stale_changeset` if anything moved since. The Studio always sends it.
- A `{generation}` that isn't a positive whole number answers `404` on release reads and `422
  invalid_generation` on rollbacks, in the usual error envelope.
- **Unpublished media is private** (D-229). `/media/v1/…` serves an upload to the public only once a
  release names it; until then it answers `404` (as if it didn't exist), except to a signed-in member
  of the project and to the short-lived signed addresses that preview answers, and delivery answers
  for signed-in site members, now carry (`?kid=&expires=&signature=`). A replaced image's bare address
  keeps showing the published version until the next publish. In a signed-in site member's answer,
  `current_url` is the published version's own address (the same as `url`): a member's signed address
  never opens a replacement nobody has published. A site that linked uploads by id before
  publishing them gets `404`s: use the addresses an answer gives.
- **JSON bodies are at most 1 MiB** (D-230) unless a route documents more: imports 16 MiB, bulk chunks
  and remote-MCP messages 2 MiB. Before, every signed-in route read up to the upload limit (21 MiB).

### Security

- A project API token is held to its creator's role as it is now (D-228): demoting the person who
  made it narrows it at its next request, and removing them (or their leaving) revokes it, with a
  `token.revoked` audit row giving the reason. Before, a removed owner's token kept working.
- Unpublished media is no longer served to anyone who knows its address (D-229; see "Changed").
- A connection has 10 seconds to send a request's headers, or it is closed
  (`COLOPHON_HEADER_TIMEOUT_MS`, D-230): the server set no header timeout at all before.
- Preview preflights, `/me` and the media origin count against the rate limits (D-231); a preflight's
  origin check read the database outside them.
- Destinations whose IPv6 address embeds a private IPv4 address (IPv4-compatible, 6to4) are refused,
  as are Teredo, site-local, NAT64 and discard-only addresses (D-231).
- Hostile input no longer stalls or crashes a parser (D-231): a condition listing many values is
  refused before its quadratic duplicate check, an import over 512 items is refused while it is read
  and before the collection is locked, oversized rich text is refused before it is copied out, and a
  migration operation with an empty path or a schedule at an extreme instant is refused, not a panic.
- Every parser of outside input is property-tested, the domain's parsers are also fuzzed in CI, and
  every attack class in `docs/security/attack-surface.md` has a regression test against the running
  server (D-231).
- An import's body is read only after its caller is known to belong to the project (D-230): a
  stranger's 16 MiB import is refused before any of it arrives, and no transaction waits on it.
- The proxies in `deploy/` keep idle connections to the server 5 seconds, below its 10 s header timeout,
  which also ends idle keep-alives: a reused connection the server had just closed answered 502 (D-230).

### Fixed

- **A delivery or media job's deadline sweep can no longer deadlock with a publish** (D-216). The
  sweep that fails jobs past their deadline now leaves a job another transaction holds for its next
  pass, and every update that must take several jobs takes them in one order. Under concurrent
  publishes the two could abort each other with a database deadlock, which reached a client as a
  500; a transaction the database aborts that way now answers 503 with `Retry-After`.

- **Revoking a throttled token, or changing its limits, takes effect on its next request**
  (D-214). A token refused for its quota and then revoked was still told `429` with a
  `Retry-After` instead of `401`, and removing or replacing its request ceiling kept refusing it
  until the old window ran out. The remembered refusal now ends as the change commits, on every
  server.

### Correction

- **The cold-read benchmark (L03) read fewer than a third of its corpus.** Its load test chose which
  entries to read by a counter each simulated user keeps for itself, so every user read the same
  entries in step: 675 of the 2,048 it was meant to cover. The test now numbers reads across all
  users and reads all 2,048. The published figure is replaced by a CI run of the fixed test (p99
  152.6 ms at 505 requests a second on an AMD EPYC 9V74, commit `079e44d`); the earlier laptop figure
  (p99 68.2 ms at 1,017 a second) came from different hardware, and the old and fixed tests run side
  by side showed no consistent latency difference. `docs/BENCHMARKS.md` has the details.

### Performance

- **An authenticated project request sets its database context in one round trip instead of four**
  (D-227): the identity, the membership or token with its permissions, the project and the environment
  are set by one database function, in the same order, under the caller's own policies. On a remote
  database each round trip saved is one network latency per request. Upgrading runs migration 0079,
  which adds those functions.

- The release check (D-217) runs on one release in 256 once the server has proved at startup that it
  catches a connection left inside a transaction; until then, and whenever that proof fails, it runs on
  every release (D-226).

- **The media worker picks up its next upload without reading every upload before it**. Each
  time it asked for work it read every job the table had ever held, finished ones included, and
  sorted the queue to take one to four; under a steady stream of uploads that grew to seconds a
  claim and the queue backed up. It now takes waiting jobs by one indexed lookup that stops at what
  it asked for. A job whose worker stopped answering (a crash) is put back in the queue, in its old
  place, by the worker's 10-second sweep, so it is picked up within 10 seconds of its lease running
  out. Upgrading runs migration 0080, which adds one index to the media job table.

- **Submitting work no longer waits on every other project's submissions** (D-216). The
  installation's 1 GiB bound on pending work metadata used to be counted from scratch on every
  submit, under one lock held for the whole request; it is now a running total checked at the end
  of each submit, so bulk jobs, uploads, releases and migration plans in different projects go
  through side by side. Upgrading runs migration 0078, which briefly holds writes to the work
  tables while it counts the existing total once.

- **An API token over its request quota no longer costs the database anything** (D-214). After
  its first refusal, a token that keeps sending is refused from memory and the rate limiter alone
  until its `Retry-After` runs out, so an agent stuck in a retry loop can't slow other projects'
  requests by queuing for database connections. The answer is the same `429` with `Retry-After`.

- Reads of what was live at a release use two index ranges instead of scanning publication
  history, and a rollback's pins are cut to the page before they meet the revisions; a new
  partial index (migration 0072) keeps the open set index-only. At 100k publication rows a
  rollback to a recent release read 50k buffers (60 ms) before; at 48k live entries a fitting
  rollback's pins took 1.2 s and now take under 100 ms (D-185).

## [0.1.0-alpha.1] — 2026-11-19

The first versioned state of the engine: pre-release until the P6 proof gate closes
(restore drill on clean infrastructure is done; usability drills and an independent
install are still ahead — see `docs/ROADMAP.md`).

### Read before upgrading

- **Migration 0077 checks for languages that differ only in case (D-212):** it refuses to run if
  a localization group already holds the same language twice in different case, and names each
  group and its tags. Keep one entry of each through the API, then migrate again (the
  `locale_case_duplicates` row of "Failure modes worth knowing" in `deploy/runbooks/upgrade.md`).
  Nothing is merged or deleted automatically. Otherwise it
  rewrites the stored tags that aren't already in canonical case (`PT-br` → `pt-BR`), in one short
  transaction, and from then on the database recases every tag written, whoever writes it.
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
- **The Studio's look (D-182):** one accent for the one primary action on a screen, a button
  hierarchy, one status badge that always pairs a word with its mark ("Live with changes", "In
  review", "Taken down"), and dialogs that keep to the screen. One button switches light and
  dark; until you press it the Studio follows your system. Search lives in the header (an icon
  on a phone); "Linked from" sits in the entry's Content tab; and after you publish, the Publish
  button comes back for your next edit instead of staying hidden behind "Live as Release N".
- **Requests and releases say when they undo a release:** a changeset and the release it makes
  carry `undo_of`, the release an undo reverses (migration 0071), beside `rollback_generation`.
- **One look for the Studio, the website and the docs (D-210):** the Studio takes the website's
  theme — white paper, one clear blue for the primary action, rounder corners — made readable:
  every text and surface pair meets WCAG AA in light and dark (a table checked on every push,
  `docs/design/contrast.md`), and form fields have a boundary you can see. Text is Inter
  throughout; the product's name and empty states use the website's Geist Pixel; DM Sans and
  JetBrains Mono are gone, so the Studio loads two font families instead of three. Statuses keep their
  words and icons; "Live with changes" is violet now, so it never looks like a button.
- **A deploy package for one server (S0, `deploy/droplet`):** one 2 GB droplet with Neon for
  Postgres, R2 for media and backups, and Cloudflare in front. `install.sh` sets the server up in one
  idempotent pass; `upgrade.sh` switches releases (migrations first, an atomic switch, a readiness
  check and a smoke test) and rolls back on its own if the new release doesn't come up, refuses a
  database a newer build has migrated, and will follow an `edge` channel, verifying every file's
  provenance first, once edge builds are published. The origin answers only Cloudflare (Authenticated Origin Pulls), SSH takes keys
  only, and a nightly encrypted logical backup to R2 (14 daily, 8 weekly) comes with its restore
  drill. `deploy/droplet/DEPLOY.md` leads with the fast path and the rehearsal's measured times.

### Security

The 2026-11-16 security fixes (D-148 to D-157, D-159 to D-169) and D-217. Each names the attack it
closes and carries a test that failed without it.

- **A cancelled request can't leave a database connection inside a transaction (D-217):** a
  request cancelled at the moment it started a transaction could return its database connection
  still inside that transaction, so the next request to borrow it wrote into a transaction nobody
  committed (a fresh sign-in could read back as signed out). Colophon now starts transactions so a
  cancel can't interrupt them, and checks every connection on release, closing one that comes back
  inside a transaction.
- Password work is limited per client address: sign-in, sign-up, magic-link sign-in and creating a
  site user allow 5 a second with a burst of 10 from one address (an IPv6 /64 counts as one),
  refused with `429` and `Retry-After` before any hashing, on top of the general limits. Set it
  with `COLOPHON_RATE_LIMIT_PASSWORD` (`off`, `<per second>` or `<per second>/<burst>`). Before,
  one address could keep every password slot busy and lock everyone else out of signing in. A
  paused sign-in is now refused without waiting for a slot, and password hashing leaves one core
  free by default (D-213).

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

- **`upgrade.sh --channel edge` says why it can't run yet:** with no edge release published it
  refuses and changes nothing, pointing you to the release channel (`upgrade.sh <version>`), instead
  of failing on a missing provenance bundle.
- **One entry per language in a localization group (D-212):** a group could hold the same
  language twice in different case (`fr` and `FR`), so a reader asking for French got either one at
  random. Locale tags are now stored in BCP 47's recommended case (`pt-br` → `pt-BR`,
  `zh-hant-tw` → `zh-Hant-TW`; only the case changes, no alias is mapped, so `iw` stays `iw`) at
  every input — create, save, localize, import, and the management API's entry list, whose
  `?locale=` filter now finds `fr` for `FR` (the public delivery API has no locale parameter yet) —
  and a second entry in the same language answers `409 locale_taken`. The database enforces it
  too, so direct writes are recased.
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

### Known limitations

- Backups capture the Postgres cluster only; object-store asset versioning is
  undecided for production (D-016's Garage has no bucket versioning).
- Retention for revisions, jobs, audit rows and diagnostic logs is policy in module
  18; the enforcement jobs are built for backups, not yet for those tables.
- Observability is structured logs plus health endpoints; per-signal recipes and the
  gaps (Prometheus-style metrics, OTLP export, the dropped-record counter) are
  documented in `deploy/runbooks/observability.md`.
- This tag is annotated but not signed; every release file instead carries signed build
  provenance (`gh attestation verify <file> --repo akira-shirase-oss/colophon-cms`), and the
  container image is attested too. Signed tags come before 0.1.0.
- The npm packages (`@colophon-cms/cli` and its five platform packages, `@colophon-cms/sdk`,
  `@colophon-cms/rich-text`) are pre-releases on the `next` tag: `npm install
  @colophon-cms/cli@next`. The standalone `colophon-mcp` binary isn't in the release archives or
  on npm yet; the server's own MCP endpoint (Streamable HTTP, on API tokens) is included.

## How to upgrade

`deploy/runbooks/upgrade.md` is the operator's procedure: verified backup →
`admin migrate-status` → binary swap → `admin migrate` → smoke checks, with rollback
rules per schema compatibility.
