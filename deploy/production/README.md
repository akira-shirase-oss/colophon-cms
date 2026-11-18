# Production on one host, without containers

The binary path, for a Linux host you run yourself: the release's static `colophon-server`
under systemd, Caddy in front for HTTPS, PostgreSQL and object storage wherever you keep them.
For one machine with Docker, the quick start is simpler
([runbooks/install.md](../runbooks/install.md)); this is for when you'd rather not run
containers, or the database is managed elsewhere.

Status: these files pass `systemd-analyze verify` and `caddy validate` in CI (the `quickstart`
job). They haven't yet been followed end to end on a real host; when they are, the notes go in
`../runbooks/install-feedback.md`.

| File | Goes to | What it is |
|---|---|---|
| `colophon.service` | `/etc/systemd/system/` | The unit: secrets as systemd credentials, a dynamic user, no writable file system |
| `colophon.env` | `/etc/colophon/colophon.env` | The settings that aren't secret |
| `Caddyfile` | `/etc/caddy/Caddyfile` | HTTPS for your domain, proxied to the server on loopback |

## Steps

1. **Binaries and the Studio**, from a release (each checked against `SHA256SUMS`):

   ```sh
   v=0.1.0  # the release
   base=https://github.com/akira-shirase-oss/colophon-cms/releases/download/v$v
   arch=x86_64   # or aarch64
   curl -fsSLO "$base/SHA256SUMS"
   for f in colophon-server-$v-$arch-unknown-linux-musl.tar.gz colophon-cli-$v-$arch-unknown-linux-musl.tar.gz colophon-studio-$v.tar.gz; do
     curl -fsSLO "$base/$f"
   done
   sha256sum --ignore-missing -c SHA256SUMS
   sudo tar -xzf colophon-server-$v-$arch-unknown-linux-musl.tar.gz -C /usr/local/bin colophon-server
   sudo tar -xzf colophon-cli-$v-$arch-unknown-linux-musl.tar.gz -C /usr/local/bin colophon
   sudo mkdir -p /usr/local/share/colophon
   sudo tar -xzf colophon-studio-$v.tar.gz -C /usr/local/share/colophon
   sudo mv /usr/local/share/colophon/colophon-studio-$v /usr/local/share/colophon/studio
   ```

2. **The database.** PostgreSQL 18 with an owner login that runs migrations. The server's own
   roles come from the CLI, with a password you generate (D-173):

   ```sh
   sudo install -d -m 0700 /etc/colophon/credentials
   head -c 32 /dev/urandom | base64 | tr '+/' '-_' | tr -d '=\n' | sudo tee /etc/colophon/credentials/app_password >/dev/null
   echo 'postgres://owner:…@db.example.com/colophon?sslmode=verify-full' | sudo tee /etc/colophon/credentials/migration_database_url >/dev/null
   sudo chmod 0400 /etc/colophon/credentials/*
   sudo colophon admin init-roles --database-url-file /etc/colophon/credentials/migration_database_url \
     --app-password-file /etc/colophon/credentials/app_password
   sudo colophon admin migrate --database-url-file /etc/colophon/credentials/migration_database_url
   ```

   Then `/etc/colophon/credentials/database_url` is
   `postgres://colophon_app:<app_password>@db.example.com/colophon?sslmode=verify-full`.
   A database on another machine must use `sslmode=verify-full` (D-156): the server refuses
   to start on anything weaker. On Neon, use the **direct** (unpooled) endpoint: `LISTEN` and
   session advisory locks don't survive a transaction-mode pooler (docs/SAAS_PLAN.md).

3. **The cache.** `/etc/colophon/credentials/cache_url`, e.g. `redis://:<password>@127.0.0.1:6379`
   for a Valkey on the same host (bound to loopback). Without one, delivery reads from
   PostgreSQL every time; leave the credential and its `LoadCredential=`/`Environment=` lines out.

4. **Object storage.** `COLOPHON_S3_*` in `colophon.env`, the secret key in
   `/etc/colophon/credentials/s3_secret_key`. Garage is what Colophon is tested against.
   DigitalOcean Spaces and Cloudflare R2 speak the same S3 API but are **not verified yet**:
   try an upload end to end before relying on one.

5. **The service and the proxy.**

   ```sh
   sudo install -m 0644 colophon.env /etc/colophon/colophon.env     # then edit it
   sudo install -m 0644 colophon.service /etc/systemd/system/colophon.service
   sudo systemctl daemon-reload && sudo systemctl enable --now colophon
   colophon doctor --server http://127.0.0.1:58000                 # ready?
   sudo install -m 0644 Caddyfile /etc/caddy/Caddyfile
   sudo systemctl edit caddy      # add: [Service] Environment=COLOPHON_DOMAIN=cms.example.com
   sudo systemctl restart caddy
   ```

6. **The first administrator.** The setup code is in the service's log:
   `journalctl -u colophon | grep "Setup code"`. Open `https://cms.example.com/studio/`, create
   the first account and enter it.

Upgrades follow `../runbooks/upgrade.md` (migrations first, with `admin migrate-status` as the
gate); backups `../runbooks/backup.md`.
