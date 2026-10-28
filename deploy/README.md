# The local stack

Everything `colophon-server` depends on, for development and CI. **Development only**: the credentials below are
public, and every port is bound to `127.0.0.1`.

```sh
scripts/stack.sh up      # start, wait until healthy, then prove each service works
scripts/stack.sh check   # run the checks again
scripts/stack.sh down    # remove the containers, keep the Postgres and Garage data
scripts/stack.sh reset   # remove the containers and the data
```

Needs Docker with Compose, plus `curl`, `jq` and `openssl`. The first `up` downloads about 1 GB of images.

## Services

| Service | Image | On your machine | Credentials |
|---|---|---|---|
| PostgreSQL, the only source of truth | `postgres:18.6-trixie` | `postgresql://colophon:colophon-dev@localhost:55432/colophon` | owner and migrations: `colophon` / `colophon-dev`; the server: `colophon_app` / `colophon-dev` (`postgres/roles.sql`; row-level security applies to it) |
| Valkey, the disposable cache ([D-015](../docs/DECISIONS.md)) | `valkey/valkey:9.0.6-trixie` | `redis://:colophon-dev@localhost:56379` | password `colophon-dev` |
| Garage, S3-compatible storage ([D-016](../docs/DECISIONS.md)) | `dxflrs/garage:v2.5.0` | S3 API `http://localhost:53900` (region `garage`, path-style), admin API `http://localhost:53903` | access key `GK636f6c6f70686f6e2d646576`, secret key in `compose.yaml`, bucket `colophon-dev` |
| Keycloak, sign-in ([module 03](../docs/modules/03-auth.md)) | `keycloak/keycloak:26.8.1` | issuer `http://localhost:58080/realms/colophon`, admin console `http://localhost:58080/admin` | console: `admin` / `colophon-dev`; users `dev-owner` and `dev-editor`, password `colophon-dev` |

The Keycloak realm (`keycloak/colophon-realm.json`) has one client, `colophon-server` (secret
`colophon-dev-client-secret`). It allows only the Authorization Code flow with PKCE (S256), and only the redirect URIs
`http://localhost:{58000,58090,58091,58092,58093}/auth/callback` (the development server, the load test, the
testbed, the server's integration tests, and the Studio's), plus `58400` to `58405` (step 3.7: `scripts/first-run.sh` on
58400, and a second checkout's test runs, moved there with `COLOPHON_TEST_ORIGIN`, `COLOPHON_STUDIO_PORT`,
`COLOPHON_TESTBED_SERVER_PORT` and `LOAD_PORT`): the password grant and the implicit flow are off. Since step 3.7 the
realm allows **self-registration**: its sign-in page offers "Register", and `/auth/login?start=create` opens the form
directly (`prompt=create`). Accounts made there are ordinary users with no projects; email isn't verified (no mail
server). After editing the realm, run
`docker compose -f deploy/compose.yaml up -d --force-recreate keycloak`. Keycloak keeps no data of its
own, so `down` followed by `up` resets it to that file.

The two users' ids, which are their subjects in ID tokens, are fixed in the realm file: `dev-owner` is
`00000000-0000-4000-8000-00000000c001` and `dev-editor` is `…c002`. `scripts/migrate.sh` bootstraps `dev-owner` as
the installation's first administrator, the only person who can create projects ([D-043](../docs/DECISIONS.md)), so
tests create projects as `dev-owner` and invite `dev-editor`. A Keycloak started before step 1.8 has random ids:
recreate it as above.

`postgres/roles.sql` also creates `colophon_delivery` (step 2.2, [D-045](../docs/DECISIONS.md)), the read-only role
public delivery switches to, and `colophon_worker` (step 2.3, [D-046](../docs/DECISIONS.md)), the role the worker
switches to. A stack started before those steps needs `scripts/stack.sh up` once more to create them; the script is
safe to rerun.

If a port clashes with something else on your machine, move it: `COLOPHON_PG_PORT`, `COLOPHON_VALKEY_PORT`,
`COLOPHON_S3_PORT`, `COLOPHON_S3_ADMIN_PORT`, `COLOPHON_KEYCLOAK_PORT`. The bind address is deliberately not a setting.

## Several servers behind a balancer

`scripts/cluster.sh up [servers] [workers]` (milestone S, [D-075](../docs/DECISIONS.md)) starts API servers (default
3, ports 58511 up) and workers (default 2, ports 58521 up) from the release build, on 127.0.0.1 like everything
here, and HAProxy (`cluster/compose.yaml`, an image pinned by digest) at `http://localhost:58405`, a sign-in redirect
URI the realm already registers. Every server's public URL is the balancer's: sessions, sign-in attempts,
idempotency keys and caches live in PostgreSQL and Valkey, so any server answers any request. The script refuses to
start more processes than PostgreSQL has connections for, keeping a fifth spare. `status`, `kill N` (SIGKILL),
`stop N` (SIGTERM, a drain) and `start N` act on server N; `down` stops everything. State and logs are in
`target/cluster/`. `CLUSTER=1 scripts/load.sh <endpoint> <scenario>` runs any load scenario through the balancer,
and `scripts/failover.sh kill|drain` is the failover proof.

## What the checks prove

Docker's healthchecks only show that each container answered its own probe. `scripts/stack.sh check` uses each service
from this machine, through its published port, the way `colophon-server` will, and every service must also refuse
something:

| Service | Must work | Must refuse |
|---|---|---|
| PostgreSQL | speaks the PostgreSQL protocol on the published port; major version 18; data checksums on; a write reads back | a wrong password |
| Valkey | a value with an expiry reads back; memory capped at 128 MiB with LRU eviction; no snapshots and no append-only file | any command before the password |
| Garage | cluster healthy; the bucket exists; an object stores, reads back byte for byte, deletes, and is gone | an anonymous read; a wrong secret; an upload whose bytes don't match its SHA-256 |
| Keycloak | the realm's issuer is stable; a full Authorization Code + PKCE sign-in returns an ID token for the right user with our nonce | a code used twice; a code without its PKCE verifier; a wrong password; a sign-in without PKCE; an unregistered redirect URI; the password grant |

The script only decodes the ID token. Colophon's OIDC client verifies it (`colophon-auth`, [D-031](../docs/DECISIONS.md)).

## Upgrading an image

Images are pinned by tag **and** digest, so the stack can't change underneath you. To upgrade one, change the tag,
then look up the new digest:

```sh
docker buildx imagetools inspect postgres:18.7-trixie | grep '^Digest'
```

Put both into `compose.yaml`, run `scripts/stack.sh reset && scripts/stack.sh up`, and keep the change only if every
check passes.
