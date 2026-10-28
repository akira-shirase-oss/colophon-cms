Everything `colophon-server` depends on, for development and CI, is in `deploy/compose.yaml`. It is for development only: its credentials are public, and every port is bound to `127.0.0.1`, which is deliberately not a setting.

```sh runnable
scripts/stack.sh up
scripts/stack.sh check
scripts/stack.sh down
scripts/stack.sh reset
```

`up` starts everything, waits up to 5 minutes until each container is healthy, creates the database roles, then runs the checks; `check` runs the checks again; `down` removes the containers and keeps the PostgreSQL and Garage data; `reset` removes the data too. It needs Docker with Compose, `curl`, `jq` and `openssl`, and runs on macOS's bash 3.2. The first `up` downloads about 1 GB of images.

Then apply the migrations and make `dev-owner` the first administrator:

```sh runnable
scripts/migrate.sh
```

## Services

| Service | Image | On your machine | Development credentials |
|---|---|---|---|
| PostgreSQL, the only source of truth | `postgres:18.6-trixie` | `localhost:55432`, database `colophon` | owner and migrations: `colophon` / `colophon-dev`; the server: `colophon_app` / `colophon-dev` |
| Valkey, the disposable cache | `valkey/valkey:9.0.6-trixie` | `localhost:56379` | password `colophon-dev` |
| Garage, S3-compatible storage | `dxflrs/garage:v2.5.0` | S3 API `localhost:53900` (region `garage`, path-style), admin API `localhost:53903` | access key `GK636f6c6f70686f6e2d646576`, secret in `deploy/compose.yaml`; bucket `colophon-dev` |
| Keycloak, sign-in | `keycloak/keycloak:26.8.1` | issuer `http://localhost:58080/realms/colophon`, console at `/admin` | console `admin` / `colophon-dev`; users `dev-owner` and `dev-editor`, password `colophon-dev` |

Images are pinned by tag and digest, so the stack can't change underneath you. If a port clashes with something else, move it with `COLOPHON_PG_PORT`, `COLOPHON_VALKEY_PORT`, `COLOPHON_S3_PORT`, `COLOPHON_S3_ADMIN_PORT` or `COLOPHON_KEYCLOAK_PORT`, and change the matching URL in `deploy/local.env`.

Valkey is started so it can't keep anything or grow without limit: no snapshots, no append-only file, 128 MiB with LRU eviction. Garage isn't used by the server yet; it is there for media, which comes in P4.

## Database roles

`scripts/stack.sh up` runs `deploy/postgres/roles.sql` after PostgreSQL starts. It is safe to run again, and a stack started before a role existed needs it once more:

| Role | Login | What it is |
|---|---|---|
| `colophon` | yes | The container's superuser: owns the tables, runs the migrations. |
| `colophon_api` | no | The application's privileges, which the migrations grant. |
| `colophon_app` | yes | What the server signs in as: a member of `colophon_api`, which row-level security applies to. |
| `colophon_delivery` | no | Published copies only, never drafts. `colophon_app` may switch to it for one transaction. Since step 2.2. |
| `colophon_worker` | no | Jobs across projects, never content. Switched to the same way. Since step 2.3. |

[Self-hosting requirements](/operations/requirements/#database-roles) explains why the roles are split.

## Keycloak

Keycloak keeps no data of its own: its realm (`deploy/keycloak/colophon-realm.json`) is imported on every fresh start, so `down` then `up` resets it. Its sign-in pages use Colophon's theme (`deploy/keycloak-theme/`, D-053).

- One client, `colophon-server`, with the public secret `colophon-dev-client-secret`, which the server refuses to use unless it listens on a loopback address.
- Only the Authorization Code flow with PKCE (`S256`); the password grant and the implicit flow are off.
- Only these redirect addresses, each `/auth/callback`: port 58000 (the development server), 58090 (load tests), 58091 (the testbed), 58092 (the server's integration tests), 58093 (the Studio's tests), and 58400 to 58405 (step 3.7: `scripts/first-run.sh` on 58400, the first-run journey on 58402, and a second checkout's test runs moved there with `COLOPHON_TEST_ORIGIN`, `COLOPHON_STUDIO_PORT`, `COLOPHON_TESTBED_SERVER_PORT` and `LOAD_PORT`).
- **Self-registration is on** (step 3.7): the sign-in page offers *Register*, and `/auth/login?start=create` opens the form directly. Accounts made there are ordinary users with no projects and no administrator rights; email isn't verified, because the stack has no mail server. A production identity provider decides for itself whether to allow sign-up.
- Two users whose ids, their subjects in ID tokens, are fixed: `dev-owner` is `00000000-0000-4000-8000-00000000c001` and `dev-editor` is `…c002`. `scripts/migrate.sh` makes `dev-owner` the installation's first administrator, so tests create projects as `dev-owner` and invite `dev-editor`.

A Keycloak started before step 1.8 has random user ids. Recreate it, and do the same after editing the realm:

```sh runnable
docker compose -f deploy/compose.yaml up -d --force-recreate keycloak
```

## Ports the other processes use

Everything listens on `127.0.0.1` or `localhost`:

| Port | What |
|---|---|
| 58000 | `colophon-server` in development (`deploy/local.env`), with the Studio at `/studio` |
| 58099 | the reference site from `scripts/demo.sh` |
| 58090, 58096, 58097 | `scripts/load.sh`: its server, test receiver and site |
| 58091, 58100, 58101 | the testbed: its server, dev server and preview |
| 58092 | the server's integration tests |
| 58093, 58094 | the Studio's Playwright tests: its server, and the reference site one suite builds |
| 58400 | `scripts/first-run.sh`: a brand-new installation, below |
| 58402 | the Studio's first-run journey test, on a database of its own |

## A brand-new installation

To try what a new operator sees, from sign-up to the first release, without the `dev-owner` bootstrap:

```sh runnable
scripts/first-run.sh
```

It builds the server, the CLI and the Studio, creates an empty database of its own (`colophon_first_run`, or `COLOPHON_TEST_DATABASE`), applies the migrations without bootstrapping anyone, and starts the server with the Studio and the worker on `http://localhost:58400`. The server prints a one-time setup code to that terminal; sign up or sign in at `/studio/`, enter the code, and you are the installation's first administrator (D-061). `--reset` starts again from an empty database; Ctrl-C stops the server and keeps the database. It needs the stack running, with a Keycloak recreated after step 3.7's realm change.

## A database of your own

The scripts, the Rust stack tests and the Playwright configs all follow `COLOPHON_TEST_DATABASE`: set it to a database name and both database URLs in `deploy/local.env` point at that database on the same PostgreSQL, instead of `colophon`. `scripts/check.sh` uses a fresh `colophon_check` on every run, `scripts/demo.sh` keeps `colophon_demo`, `scripts/first-run.sh` keeps `colophon_first_run`, and a checkout whose migrations are ahead of the others should use one too, so it never migrates the database other checkouts share. A database that every run adds to grows until saves slow down: on 2026-10-27, on an Apple M2 Pro laptop, the entries load scenario had a p99 of 183.8 ms against a local database grown to 5 GB, and 52.4 ms against a fresh one (both results are in `perf/results/`).

## What the checks prove

A container's healthcheck only shows that it answered its own probe. `scripts/stack.sh check` uses each service from your machine, through its published port, the way the server will, and makes each one refuse something:

| Service | Must work | Must refuse |
|---|---|---|
| PostgreSQL | the protocol on the published port; major version 18; data checksums on; a write reads back | a wrong password |
| Valkey | a value with an expiry reads back; memory capped at 128 MiB with LRU eviction; no snapshots and no append-only file | any command before the password |
| Garage | cluster healthy; the bucket exists; an object stores, reads back byte for byte, deletes, and is gone | an anonymous read; a wrong secret; an upload whose bytes don't match its SHA-256 |
| Keycloak | a stable issuer; a full Authorization Code + PKCE sign-in returns an ID token for the right user with our nonce | a code used twice; a code without its PKCE verifier; a wrong password; a sign-in without PKCE; an unregistered redirect; the password grant |

The script only decodes the ID token; the server's own sign-in verifies it (`colophon-auth`, D-031). CI runs `scripts/stack.sh up` from a clean clone on every push and pull request (D-017).

## Upgrading an image

Change the tag in `deploy/compose.yaml`, look up the new digest (`docker buildx imagetools inspect IMAGE:TAG`), put both in, run `scripts/stack.sh reset` and `scripts/stack.sh up`, and keep the change only if every check passes. The full text is in [deploy/README.md](https://github.com/akira-shirase-oss/colophon-cms/blob/main/deploy/README.md).
