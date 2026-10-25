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
| PostgreSQL, the only source of truth | `postgres:18.6-trixie` | `postgresql://colophon:colophon-dev@localhost:55432/colophon` | user `colophon`, password `colophon-dev` |
| Valkey, the disposable cache ([D-015](../docs/DECISIONS.md)) | `valkey/valkey:9.0.6-trixie` | `redis://:colophon-dev@localhost:56379` | password `colophon-dev` |
| Garage, S3-compatible storage ([D-016](../docs/DECISIONS.md)) | `dxflrs/garage:v2.5.0` | S3 API `http://localhost:53900` (region `garage`, path-style), admin API `http://localhost:53903` | access key `GK636f6c6f70686f6e2d646576`, secret key in `compose.yaml`, bucket `colophon-dev` |
| Keycloak, sign-in ([module 03](../docs/modules/03-auth.md)) | `keycloak/keycloak:26.8.1` | issuer `http://localhost:58080/realms/colophon`, admin console `http://localhost:58080/admin` | console: `admin` / `colophon-dev`; users `dev-owner` and `dev-editor`, password `colophon-dev` |

The Keycloak realm (`keycloak/colophon-realm.json`) has one client, `colophon-server` (secret
`colophon-dev-client-secret`). It allows only the Authorization Code flow with PKCE (S256), and only the redirect URI
`http://localhost:58000/auth/callback`: the password grant and the implicit flow are off. Keycloak keeps no data of its
own, so `down` followed by `up` resets it to that file.

If a port clashes with something else on your machine, move it: `COLOPHON_PG_PORT`, `COLOPHON_VALKEY_PORT`,
`COLOPHON_S3_PORT`, `COLOPHON_S3_ADMIN_PORT`, `COLOPHON_KEYCLOAK_PORT`. The bind address is deliberately not a setting.

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

The script only decodes the ID token. Verifying its signature is the job of Colophon's OIDC client (P1).

## Upgrading an image

Images are pinned by tag **and** digest, so the stack can't change underneath you. To upgrade one, change the tag,
then look up the new digest:

```sh
docker buildx imagetools inspect postgres:18.7-trixie | grep '^Digest'
```

Put both into `compose.yaml`, run `scripts/stack.sh reset && scripts/stack.sh up`, and keep the change only if every
check passes.
