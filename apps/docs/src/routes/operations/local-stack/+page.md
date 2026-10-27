Everything `colophon-server` depends on, for development and CI, is in `deploy/compose.yaml`. It is for development only: its credentials are public, and every port is bound to `127.0.0.1`, which is deliberately not a setting.

```sh runnable
scripts/stack.sh up
scripts/stack.sh check
scripts/stack.sh down
scripts/stack.sh reset
```

`up` starts everything, waits until each container is healthy, then runs the checks; `check` runs the checks again; `down` removes the containers and keeps the PostgreSQL and Garage data; `reset` removes the data too. It needs Docker with Compose, `curl`, `jq` and `openssl`. The first `up` downloads about 1 GB of images.

## Services

| Service | Image | On your machine | Development credentials |
|---|---|---|---|
| PostgreSQL, the only source of truth | `postgres:18.6-trixie` | `localhost:55432`, database `colophon` | owner and migrations: `colophon` / `colophon-dev`; the server: `colophon_app` / `colophon-dev`, which row-level security applies to |
| Valkey, the disposable cache | `valkey/valkey:9.0.6-trixie` | `localhost:56379` | password `colophon-dev` |
| Garage, S3-compatible storage | `dxflrs/garage:v2.5.0` | S3 API `localhost:53900` (region `garage`, path-style), admin API `localhost:53903` | in `deploy/compose.yaml`; bucket `colophon-dev` |
| Keycloak, sign-in | `keycloak/keycloak:26.8.1` | issuer `http://localhost:58080/realms/colophon`, console `/admin` | console `admin` / `colophon-dev`; users `dev-owner` and `dev-editor`, password `colophon-dev` |

Images are pinned by tag and digest, so the stack can't change underneath you. If a port clashes with something else, move it with `COLOPHON_PG_PORT`, `COLOPHON_VALKEY_PORT`, `COLOPHON_S3_PORT`, `COLOPHON_S3_ADMIN_PORT` or `COLOPHON_KEYCLOAK_PORT`.

The database has more roles than the two logins: `colophon_delivery`, which public reads switch to and which can't read drafts, and `colophon_worker`, which the worker switches to. A stack started before those roles existed needs `scripts/stack.sh up` once more; the script is safe to rerun.

Keycloak keeps no data of its own: its realm (`deploy/keycloak/colophon-realm.json`) is imported on every fresh start, so `down` then `up` resets it. It allows only the Authorization Code flow with PKCE, and only the server's development redirect addresses. The two users' ids are fixed in the realm, and `scripts/migrate.sh` makes `dev-owner` the installation's first administrator. After editing the realm:

```sh runnable
docker compose -f deploy/compose.yaml up -d --force-recreate keycloak
```

## What the checks prove

A container's healthcheck only shows that it answered its own probe. `scripts/stack.sh check` uses each service from your machine, through its published port, the way the server will, and makes each one refuse something:

| Service | Must work | Must refuse |
|---|---|---|
| PostgreSQL | the protocol on the published port; major version 18; data checksums on; a write reads back | a wrong password |
| Valkey | a value with an expiry reads back; memory capped at 128 MiB with LRU eviction; no snapshots and no append-only file | any command before the password |
| Garage | cluster healthy; the bucket exists; an object stores, reads back byte for byte, deletes, and is gone | an anonymous read; a wrong secret; an upload whose bytes don't match its SHA-256 |
| Keycloak | a stable issuer; a full Authorization Code + PKCE sign-in returns an ID token for the right user with our nonce | a code used twice; a code without its PKCE verifier; a wrong password; a sign-in without PKCE; an unregistered redirect; the password grant |

## Upgrading an image

Change the tag in `deploy/compose.yaml`, look up the new digest (`docker buildx imagetools inspect <image>:<tag>`), put both in, run `scripts/stack.sh reset` and `scripts/stack.sh up`, and keep the change only if every check passes. The full text is in [deploy/README.md](https://github.com/akira-shirase-oss/colophon-cms/blob/main/deploy/README.md).
