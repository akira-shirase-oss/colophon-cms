<script>
	import Status from '#lib/components/Status.svelte';
</script>

What a self-hosted Colophon needs today, read from the code, and what is missing before anyone should run it for real. Colophon is **pre-release**: v0.1 is not out, there are no release binaries, and it is **not for production**. Everything below has run on one laptop and in CI, never as somebody else's install. Until P6 the honest use of this page is a test install you can throw away.

## What runs

| Part | Needed | What it is |
|---|---|---|
| `colophon-server` | yes | One Rust binary: the management API, public delivery, the Studio's static files and the background worker. `--role all` (both in one process) is the v0.1 reference profile. |
| PostgreSQL | yes | The only source of truth. Everything that matters, including pending work, is in it. |
| An OpenID Connect provider | yes, for anyone to sign in | People sign in there; Colophon keeps its own sessions. |
| Valkey (or another Redis-protocol server) | no | A disposable cache for public reads. |
| The Studio build | for editors | Static files built with Node, served by `colophon-server` at `/studio`. |
| `colophon` | for the operator | The CLI: migrations, and the first administrator if you don't use the setup code. |
| Your website | optional | Anything that reads `/content/v1`; `apps/web` is the reference site, a Node 24 runner. |

Not used yet: S3-compatible object storage. Garage is in the local stack for media, which arrives in P4; the server reads no storage settings today. <Status planned="P4" />

## Building it

There are no binaries or container images to download, so you build from a clone:

- **Rust 1.99.1**, the toolchain pinned in `rust-toolchain.toml`. Install [rustup](https://rustup.rs) and it fetches the pin on the first build. A `cargo` that isn't rustup's (Homebrew's, for example) ignores the pin, and `scripts/check-toolchain.sh` says so.
- **No database to compile**: `.cargo/config.toml` sets `SQLX_OFFLINE=true`, so the compile-time query checks use the data committed in `.sqlx/`.
- **Node.js 22.17 or newer** for the Studio (`package.json` engines; CI uses Node 24), and **Node 24** for the reference site, which runs its TypeScript directly.

```sh runnable
cargo build --release -p colophon-server
cargo build --release -p colophon-cli
npm ci
npm run build -w @colophon/studio
```

The binaries land in `target/release/`, the Studio in `apps/studio/build`. `colophon-server --version` prints `0.0.0`: note the commit you built from instead.

## PostgreSQL

- **Version 18.** The local stack pins 18.6, `scripts/stack.sh check` refuses any other major version, and CI runs nothing else. The role setup in `deploy/postgres/roles.sql` uses `WITH INHERIT FALSE, SET TRUE`, which needs PostgreSQL 16 or newer.
- **Extensions** `btree_gist` (migration 0008, for the constraint that keeps publication histories from overlapping) and `pg_trgm` (migration 0015, for title search). Both ship with PostgreSQL's contrib package and are trusted, so the database's owner may create them.
- **Plain connections only.** The server and the CLI are built without a TLS client for PostgreSQL (`sqlx` has no TLS feature in `Cargo.toml`): keep the database on the same host or a private network.
- **Connections.** Each server process opens up to `COLOPHON_DB_MAX_CONNECTIONS` (16) for the API and the readiness probe, plus `COLOPHON_WORKER_DB_CONNECTIONS` (4) when it runs the worker. `colophon admin` uses one.

### Database roles

Colophon separates who owns the schema from who serves requests, and row-level security applies to everything the server does:

| Role | Login | What it is |
|---|---|---|
| `colophon` | yes | The migrator: owns the tables, runs `colophon admin migrate` and `colophon admin bootstrap`. Locally it is the container's superuser. Never give it to the server. |
| `colophon_api` | no | The application's privileges: what migrations grant. Migration 0001 creates it if it is missing. |
| `colophon_app` | yes | The login the server uses (`COLOPHON_DATABASE_URL`): a member of `colophon_api` and nothing more, never a superuser, without `BYPASSRLS`. |
| `colophon_delivery` | no | What public delivery may read: published copies only, never drafts. The server switches to it for each public read (`SET LOCAL ROLE`). |
| `colophon_worker` | no | What the worker may touch: jobs across projects, their destinations and the releases they announce, never content. Switched to the same way. |

Create the roles **before the first migration**, as `deploy/postgres/roles.sql` does for the local stack: migrations 0008 and 0009 grant to `colophon_delivery` and `colophon_worker` and fail if they don't exist. `colophon_app` holds those two with `INHERIT FALSE, SET TRUE`, so it has their privileges only after switching, and loses its own while switched.

Only a superuser migrator has been tested. Reading the SQL, a migrator that isn't one also needs to own the database's tables (it creates them), to be able to create `colophon_api` or find it already there, `CREATE` on the database for the two extensions, and membership of `colophon_delivery`, because migration 0011 makes that role the owner of the `delivery_environment` function.

## Valkey (optional)

- Any Redis-protocol server, at a `redis://` URL with a password; tested with Valkey 9.0.6. No TLS here either: same host or private network.
- It is disposable: nothing in it is ever the only copy, so it needs no persistence. The stack runs it with no snapshots, no append-only file, a 128 MiB cap and LRU eviction.
- Without it, public reads go to PostgreSQL and readiness reports the cache `not_configured`. When it fails, the server keeps serving from PostgreSQL and reports `degraded`, never unready.

## An OpenID Connect provider

Colophon has no passwords of its own. Any provider that does the following should work; only Keycloak 26.8.1 has been tested:

- a discovery document at the issuer's `/.well-known/openid-configuration`, whose `issuer` is exactly `COLOPHON_OIDC_ISSUER`;
- the Authorization Code flow with PKCE (`S256`), scope `openid profile`, for a confidential client that authenticates with `client_secret_post`;
- ID tokens signed with RS256, PS256, ES256 or EdDSA (never `none` or HMAC), with a stable `sub`: a person is their issuer and subject, never their email address;
- `https://` endpoints (plain `http://` is accepted only for `localhost`, `127.0.0.1` and `::1`);
- the redirect URI `COLOPHON_PUBLIC_URL` plus `/auth/callback` registered for the client;
- optionally, `create` in its discovery document's `prompt_values_supported`, so the Studio's *Create an account* opens the provider's sign-up form directly (OpenID Connect Prompt Create 1.0). Without it, that link opens the sign-in form. Whether anyone may sign up is the provider's decision; an account grants nothing in Colophon by itself.

Only administrators create projects. The first one is made in either of two ways: the operator runs `colophon admin bootstrap --subject S` after the first migration, with the migrator's credentials ([Command line](/reference/cli/#colophon-admin-bootstrap)); or, while nobody is an administrator, the server prints a one-time setup code to its standard error at every start, and the first person to sign in to the Studio and enter it becomes the administrator ([setup codes](/reference/api/auth/#setting-up-a-fresh-installation), D-061). Collected logs capture standard error, so a code can sit in them for its hour; it dies at first use. To never issue one, bootstrap before the first start. Once anyone is an administrator, no HTTP route can make another.

## Network

- **The server speaks plain HTTP** on `COLOPHON_LISTEN_ADDR` (default `127.0.0.1:58000`) and does no TLS itself, by design: module 18 puts a TLS proxy or CDN in front. Anywhere but `localhost` you need that proxy, because the session cookie is always `Secure` and browsers won't keep it over plain HTTP.
- **Every role listens.** A separate worker process (`--role worker`) still serves the API on its own address, so firewall it like the API.
- **Outbound:** the identity provider; and the destinations you configure (webhooks, CDN purges, probes), which must be `https://` and public unless the server listens on loopback.
- **PostgreSQL and Valkey** never need to be reachable from outside.

[Configuration](/operations/configuration/) lists every setting, and [the local stack](/operations/local-stack/) the development ports.

## Sizing

Nobody has measured a production install. The load results in [Performance](/resources/performance/) come from one laptop, each with its machine and date, and say nothing about capacity elsewhere.

## What is not ready

- **No release:** no versions, binaries, container images, packages or `init` command, and no licence chosen yet (O-014 proposes Apache-2.0). Versions, binaries, packages and container images are milestone V (D-062); the release itself with signed artifacts is P6. <Status planned="V, P6" />
- **One server, proven.** Several servers or workers against one database are designed for (leases, `SKIP LOCKED`, rate limits counted in PostgreSQL), but no multi-instance stack behind a load balancer has been tested yet (D-062). <Status planned="S" />
- **No backups or restore procedure.** [Backups and restore](/operations/backups/) explains what P6 plans and what not to rely on meanwhile. <Status planned="P6" />
- **Upgrades need downtime.** Migrations are forward-only and not yet written for old and new servers side by side; see [Upgrades and migrations](/operations/upgrades/).
- **No metrics or trace export.** Logs go to stdout, as JSON lines by default. <Status planned="P6" />
- **No admission budgets** (F09): nothing refuses new work before the database or disk fills. <Status planned="P6" />
- **Unsigned webhooks.** Receivers can deduplicate by `Idempotency-Key` but can't verify the sender yet (D-046). <Status planned="not scheduled" />
- **One administrator**, no grant flow for more. <Status planned="not scheduled" />
- **Readiness doesn't check migrations.** A server started against a database that hasn't been migrated reports `ready` and fails requests. Migrate first.
