# Installing Colophon

How to run Colophon on one machine with Docker alone, start it for the first time, keep it up
to date, back it up and fix it when it won't start. The design is D-172 (the image) and D-173
(the quick start); the secret settings are D-171. `scripts/quickstart.sh` runs every command in
"Install" and "First run" below on each CI push, so this page and the files can't drift apart.

## What you need

- Docker with Compose 2.24 or newer (Docker Desktop, or Docker Engine on Linux). Nothing else:
  no Rust, no Node, no database.
- About 1.5 GB of disk for the images, and 1 GB of memory for the whole stack at rest.
- Port 8080 free on 127.0.0.1 (or pick another with `COLOPHON_PORT`).

## Install

### From a release

Each release attaches a `compose.yaml` that pins its image by digest:

```sh
mkdir colophon && cd colophon
curl -fsSLO https://github.com/akira-shirase-oss/colophon-cms/releases/download/v<version>/compose.yaml
docker compose up -d --wait
```

### From a clone (before the first release, or to try `main`)

```sh
git clone https://github.com/akira-shirase-oss/colophon-cms.git
cd colophon-cms
docker build -f deploy/image/Dockerfile -t colophon:local .   # compiles Colophon inside Docker
cd deploy/quickstart
echo "COLOPHON_IMAGE=colophon:local" >.env
docker compose up -d --wait
```

The build takes about ten minutes on a laptop; `up --wait` then takes under a minute. Every
later command runs in the folder that holds `compose.yaml` (and `.env`).

`up --wait` returns once Colophon is ready. On the first start it generates every secret,
creates the database roles, applies the schema and creates the storage bucket; the same command
later just starts what is there.

## First run

1. Open <http://localhost:8080/studio/> (use `localhost`, not `127.0.0.1`: sign-in trusts the
   address Colophon was told it lives at).
2. The Studio says this Colophon is new. Create the first account.
3. Find the setup code the server printed when it started:

   ```sh
   docker compose logs colophon | grep "Setup code"
   ```

   The log also prints a link that fills the code in. The code works once, for an hour; restart
   Colophon (`docker compose restart colophon`) for a new one while nobody is an administrator.
4. Enter it: you are now the administrator. Create a project, invite people from its settings.

## What is running

| Service | What it does | Lifetime |
|---|---|---|
| `pod` | Owns the network namespace every service shares; publishes `127.0.0.1:8080` | always |
| `secrets` | Writes every secret once into four volumes, each file readable only by the service that uses it | first start; then a no-op |
| `postgres` | PostgreSQL 18, the only source of truth; listens on 127.0.0.1 inside the namespace, password authentication even on loopback | always |
| `valkey` | The disposable cache (128 MB, no persistence) | always |
| `garage` | Object storage for uploads | always |
| `storage` | Creates the `colophon` bucket and Colophon's key through Garage's admin API | each start, idempotent |
| `roles` | `colophon admin init-roles`: the database roles, with generated passwords | each start, idempotent |
| `migrate` | `colophon admin migrate`: the schema; refuses a database with migrations this image doesn't know | each start |
| `colophon` | The API, the worker and the Studio (`--role all`) as uid 65532 on a read-only root, all capabilities dropped | always |

Only Colophon's port leaves the namespace, and only on 127.0.0.1: PostgreSQL, Valkey and Garage
are unreachable from the host and from other containers, which is why they can speak plain
text — the same rule as everywhere else (S3 over HTTP and an unencrypted database only on
loopback). No secret is in `compose.yaml`, the image, or any container's environment; Colophon
reads them through `COLOPHON_*_FILE` settings.

Settings, in the environment or a `.env` file next to `compose.yaml`:

| Setting | Default | |
|---|---|---|
| `COLOPHON_PORT` | `8080` | The host port, on 127.0.0.1 only |
| `COLOPHON_IMAGE` | the release's image | Which Colophon image to run |

## Serving it to others

The quick start listens on 127.0.0.1 only. To serve it on a domain, put a TLS reverse proxy
(Caddy, nginx, a load balancer) in front of `127.0.0.1:8080`, and tell Colophon its address and
the proxy's, with a `compose.override.yaml` beside `compose.yaml`:

```yaml
services:
  colophon:
    environment:
      COLOPHON_PUBLIC_URL: https://cms.example.com
      COLOPHON_TRUSTED_PROXIES: 172.16.0.0/12   # the address your proxy connects from
```

Without `COLOPHON_TRUSTED_PROXIES`, every visitor looks like the proxy and shares one rate
limit. A managed PostgreSQL instead of the bundled one needs `sslmode=verify-full` in its URL
(module 18, "Database TLS").

## Upgrade

```sh
curl -fsSLO https://github.com/akira-shirase-oss/colophon-cms/releases/download/v<new version>/compose.yaml
docker compose pull
docker compose up -d --wait
```

`migrate` applies the new schema before the new Colophon starts. Rolling back to an older image
on a schema it doesn't know is refused (`docker compose logs migrate` says which migrations);
read `deploy/runbooks/upgrade.md` before going back a version.

## Back up and restore

The quick start's backup is a logical dump plus a copy of the uploads; the production path
(continuous WAL archiving and encrypted base backups, `deploy/runbooks/backup.md`) is not part
of it.

```sh
# The database, as the owner, with the password read inside the container (never on your shell):
docker compose exec -T postgres sh -c \
  'PGPASSWORD="$(cat /run/secrets/postgres/owner_password)" pg_dump -Fc -h 127.0.0.1 -U colophon colophon' \
  >colophon.dump
# The uploads, with Garage stopped so the copy is consistent:
docker compose stop garage
docker run --rm -v colophon-quickstart_garage:/data:ro -v "$PWD":/backup alpine:3.23 \
  tar -czf /backup/uploads.tar.gz -C /data .
docker compose start garage
```

Restore into a fresh install: start it once (`up -d --wait`), stop `colophon` and `garage`, load
the dump with the same `sh -c` wrapper around `pg_restore --clean --if-exists -h 127.0.0.1 -U
colophon -d colophon <colophon.dump`, untar the uploads into the `garage` volume, and
`docker compose up -d --wait` again. The volume names start with the compose project's name,
`colophon-quickstart` (`docker volume ls` lists them).

Keep the four `secrets-*` volumes with the data volumes: without them the generated passwords are
gone and the kept database refuses the new ones.

## When it won't start

`docker compose ps` shows which service stopped; `docker compose logs <service>` says why.

| Symptom | Cause and fix |
|---|---|
| `bind: address already in use` on 8080 | Something else has the port: `COLOPHON_PORT=8081 docker compose up -d --wait` |
| `colophon` unhealthy; `docker compose exec colophon colophon doctor --server http://127.0.0.1:8080` lists `database: down` | PostgreSQL isn't answering or refuses the password: see `docker compose logs postgres` |
| `cache: down` in the same list | Valkey isn't up; Colophon still serves from the database, slower |
| `roles` or `migrate` failed with `password authentication failed` | The `secrets-*` volumes were removed but the `postgres` volume was kept: restore the secrets volumes, or start over with `docker compose down --volumes` (this deletes everything) |
| `migrate` says a migration was previously applied but is missing | The database is newer than this image: run the newer image (see Upgrade) |
| `storage` failed | Garage didn't become healthy: `docker compose logs garage` |
| The Studio says it can't sign you in from this address | Open `http://localhost:<port>/studio/`, the address in `COLOPHON_PUBLIC_URL` |

## Remove it

```sh
docker compose down             # stop; keeps the data
docker compose down --volumes   # stop and delete every volume: the content, the uploads and the secrets
```
