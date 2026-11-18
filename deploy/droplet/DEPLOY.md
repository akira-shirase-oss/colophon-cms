# Deploying Colophon to one droplet (S0)

S0 in `docs/SAAS_PLAN.md`: Colophon on a 2 GB DigitalOcean droplet in Toronto, against Neon,
behind Cloudflare at `https://app.colophon.404human.dev`, used by the maintainer only. This is the
production path without containers (`deploy/production/README.md`), filled in for that host.

**Before you start:** everything in [CREATE.md](CREATE.md) exists. Secrets are typed or pasted
only on the droplet, into root-only files; nothing secret goes into chat, an issue or git.

**Exit gate (SAAS_PLAN S0):** health checks green (`smoke.sh`), a restore drill from Neon's
point-in-time restore recorded (step 12), idle memory recorded (`smoke.sh` prints it).

The commands run on the droplet as root unless they say otherwise. `$CHECKOUT` is a copy of this
repository on the droplet (step 2). **The version is in one place:**

```sh
export COLOPHON_VERSION=0.1.0-alpha.1    # the release this guide installs; change only this to upgrade
```

## 1. The host

```sh
ssh root@<droplet-ip>
apt-get update && apt-get -y upgrade
apt-get install -y ufw jq curl ca-certificates unattended-upgrades redis-server debian-keyring debian-archive-keyring apt-transport-https age rclone
# pg_dump 18 for the logical backup (step 10), from PostgreSQL's own repository:
install -d /usr/share/postgresql-common/pgdg
curl -fsSo /usr/share/postgresql-common/pgdg/apt.postgresql.org.asc https://www.postgresql.org/media/keys/ACCC4CF8.asc
echo "deb [signed-by=/usr/share/postgresql-common/pgdg/apt.postgresql.org.asc] https://apt.postgresql.org/pub/repos/apt noble-pgdg main" >/etc/apt/sources.list.d/pgdg.list
apt-get update && apt-get install -y postgresql-client-18
dpkg-reconfigure -f noninteractive unattended-upgrades
timedatectl set-timezone UTC

# A swap file: a guard against a memory spike, not capacity (SAAS_PLAN: memory is unmeasured).
fallocate -l 1G /swapfile && chmod 600 /swapfile && mkswap /swapfile && swapon /swapfile
echo '/swapfile none swap sw 0 0' >>/etc/fstab

# Firewall: SSH from you, 443 from Cloudflare only (step 7 adds those), nothing else.
ufw default deny incoming && ufw default allow outgoing
ufw allow from <your-ip> to any port 22 proto tcp
ufw --force enable
```

Keep the DigitalOcean cloud firewall from CREATE.md as well: two layers, either one enough.

## 2. Colophon's binaries and the Studio

From a release, each file checked against `SHA256SUMS` (`deploy/production/README.md` step 1):

```sh
v=$COLOPHON_VERSION; arch=x86_64
base=https://github.com/akira-shirase-oss/colophon-cms/releases/download/v$v
cd /tmp && curl -fsSLO "$base/SHA256SUMS"
for f in colophon-server-$v-$arch-unknown-linux-musl.tar.gz colophon-cli-$v-$arch-unknown-linux-musl.tar.gz colophon-studio-$v.tar.gz; do curl -fsSLO "$base/$f"; done
sha256sum --ignore-missing -c SHA256SUMS
tar -xzf colophon-server-$v-$arch-unknown-linux-musl.tar.gz -C /usr/local/bin colophon-server
tar -xzf colophon-cli-$v-$arch-unknown-linux-musl.tar.gz -C /usr/local/bin colophon
mkdir -p /usr/local/share/colophon
tar -xzf colophon-studio-$v.tar.gz -C /usr/local/share/colophon
mv /usr/local/share/colophon/colophon-studio-$v /usr/local/share/colophon/studio
colophon-server --version && colophon --version

# This repository's deploy files, at the same version:
git clone --depth 1 --branch v$v https://github.com/akira-shirase-oss/colophon-cms /opt/colophon-src
CHECKOUT=/opt/colophon-src
```

> The draft release `v0.1.0-alpha.1` is cut once the hardening batch and D-183 are merged; its
> checksummed musl archives are what this step installs. Building from source on a 2 GB droplet
> is not recommended (the Rust build needs more memory than the host has).

## 3. Neon: the roles and the schema

Use Neon's **direct** connection string (no `-pooler` in the host): the server needs `LISTEN` and
session advisory locks, which a transaction pooler breaks. Change its ending to
`?sslmode=verify-full` — Neon's `?sslmode=require&channel_binding=require` is refused (D-156):
`require` accepts any certificate, and the driver doesn't do channel binding.

```sh
install -d -m 0700 /etc/colophon/credentials
# Paste the owner URL (postgres://<owner>:<password>@<endpoint>.neon.tech/colophon?sslmode=verify-full)
# and press Ctrl-D. It is never echoed or stored in shell history.
umask 077; cat >/etc/colophon/credentials/migration_database_url
# The application's own password: generated here, never seen.
head -c 32 /dev/urandom | base64 | tr '+/' '-_' | tr -d '=\n' >/etc/colophon/credentials/app_password
chmod 0400 /etc/colophon/credentials/*

colophon admin init-roles --database-url-file /etc/colophon/credentials/migration_database_url \
  --app-password-file /etc/colophon/credentials/app_password
colophon admin migrate --database-url-file /etc/colophon/credentials/migration_database_url
```

The server's own URL is the same endpoint with the application login:

```sh
host=$(sed -E 's#^postgres(ql)?://[^@]+@([^/]+)/.*#\2#' /etc/colophon/credentials/migration_database_url)
printf 'postgres://colophon_app:%s@%s/colophon?sslmode=verify-full' \
  "$(cat /etc/colophon/credentials/app_password)" "$host" >/etc/colophon/credentials/database_url
chmod 0400 /etc/colophon/credentials/database_url
```

If the first connection fails on the certificate, stop: `verify-full` against Neon is S0's first
real proof (SAAS_PLAN "Neon"), and weakening it is not the fix.

## 4. The cache (Valkey's API, from Ubuntu's `redis-server`)

```sh
pass=$(head -c 32 /dev/urandom | base64 | tr '+/' '-_' | tr -d '=\n')
cat >/etc/redis/conf.d-colophon.conf <<EOF
bind 127.0.0.1 -::1
protected-mode yes
maxmemory 128mb
maxmemory-policy allkeys-lru
requirepass $pass
EOF
echo 'include /etc/redis/conf.d-colophon.conf' >>/etc/redis/redis.conf
systemctl restart redis-server
printf 'redis://:%s@127.0.0.1:6379' "$pass" >/etc/colophon/credentials/cache_url
chmod 0400 /etc/colophon/credentials/cache_url; unset pass
```

The cache holds nothing that isn't in Neon; losing it costs latency, not data.

## 5. Settings and the remaining secret

```sh
install -m 0644 $CHECKOUT/deploy/droplet/colophon.env.template /etc/colophon/colophon.env
nano /etc/colophon/colophon.env          # the R2 account id and access key id
umask 077; cat >/etc/colophon/credentials/s3_secret_key   # paste the R2 secret, Ctrl-D
chmod 0400 /etc/colophon/credentials/s3_secret_key
```

## 6. The service

```sh
install -m 0644 $CHECKOUT/deploy/production/colophon.service /etc/systemd/system/colophon.service
systemctl daemon-reload && systemctl enable --now colophon
colophon doctor --server http://127.0.0.1:58000
journalctl -u colophon -n 50 --no-pager
```

## 7. Caddy, Cloudflare's certificate and its address ranges

```sh
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' >/etc/apt/sources.list.d/caddy-stable.list
apt-get update && apt-get install -y caddy

# Cloudflare's Origin Certificate (CREATE.md 8): paste each, Ctrl-D.
umask 077; cat >/etc/caddy/origin.pem; cat >/etc/caddy/origin.key
chown root:caddy /etc/caddy/origin.pem /etc/caddy/origin.key && chmod 0640 /etc/caddy/origin.pem /etc/caddy/origin.key

install -m 0644 $CHECKOUT/deploy/droplet/Caddyfile /etc/caddy/Caddyfile
$CHECKOUT/deploy/droplet/cloudflare-ips.sh       # Caddy's and Colophon's trusted ranges, ufw 443
caddy validate --config /etc/caddy/Caddyfile && systemctl reload caddy
systemctl restart colophon
```

## 8. The first administrator

```sh
journalctl -u colophon | grep "Setup code"
```

Open `https://app.colophon.404human.dev/studio/`, enter the code, and create your account. The
code works once; after that, the installation has its administrator.

## 9. Email (optional for S0)

Sign-in links by email need a provider (D-145). With a Resend key:
`cat >/etc/colophon/credentials/resend_api_key` (Ctrl-D), uncomment the two `COLOPHON_MAIL_*`
lines in `colophon.env`, and add to the unit with `systemctl edit colophon`:

```ini
[Service]
LoadCredential=resend_api_key:/etc/colophon/credentials/resend_api_key
Environment=COLOPHON_RESEND_API_KEY_FILE=%d/resend_api_key
```

## 10. Backups: Neon's point-in-time restore, the deletion ledger, and a dump Neon doesn't hold

`scripts/backup.sh` (pg_basebackup) and its WAL archive retention (D-201) are for self-hosted
PostgreSQL: Neon takes no pg_basebackup and keeps its own WAL. S0's backups are:

- **Neon's point-in-time restore**, over the plan's window (CREATE.md 4): a bad write, undone;
- **the deletion ledger**, exported daily to the `colophon-backups` bucket, so a restore to an
  earlier moment replays what was deleted after it (O2, `deploy/runbooks/restore.md` step 4);
- **a logical backup**, daily: `pg_dump -Fc` over `verify-full`, encrypted with age, to
  `colophon-backups/logical/`, keeping the newest of 14 days and of 8 weeks. It doesn't depend on
  Neon at all: it covers losing the Neon account or project, not just a bad write;
- **media:** the `colophon-media` bucket itself (Colophon keeps a replaced image's old versions).

The age key: generated here, its public half stays on the droplet, and you keep a copy of the
private half **offline** (a password manager or paper). Without it, the logical backups can't be
read; the droplet's own copy is for restore drills.

```sh
age-keygen -o /etc/colophon/backup-age-identity 2>/dev/null && chmod 0400 /etc/colophon/backup-age-identity
age-keygen -y /etc/colophon/backup-age-identity >/etc/colophon/backup-age-recipient
cat /etc/colophon/backup-age-identity    # copy the AGE-SECRET-KEY-… line to your offline store, then clear the screen

install -m 0755 $CHECKOUT/deploy/droplet/backup-logical.sh /usr/local/bin/colophon-backup-logical
for unit in colophon-ledger-export.service colophon-ledger-export.timer colophon-backup-logical.service colophon-backup-logical.timer; do
  install -m 0644 $CHECKOUT/deploy/droplet/$unit /etc/systemd/system/
done
systemctl daemon-reload
systemctl enable --now colophon-ledger-export.timer colophon-backup-logical.timer
systemctl start colophon-ledger-export colophon-backup-logical
journalctl -u colophon-ledger-export -u colophon-backup-logical -n 10 --no-pager
colophon-backup-logical latest           # the newest dump and its age in hours
```

The ledger-export unit needs D-191 (the CLI reads `COLOPHON_S3_SECRET_KEY_FILE`), which is in
`v0.1.0-alpha.1`.

## 11. The smoke check

```sh
$CHECKOUT/deploy/droplet/smoke.sh
```

Then make a project API token in the Studio (full access) for one upload test, and:

```sh
umask 077; cat >/root/smoke-token        # paste, Ctrl-D
SMOKE_PROJECT=<project id> SMOKE_TOKEN_FILE=/root/smoke-token $CHECKOUT/deploy/droplet/smoke.sh
```

Revoke that token in the Studio afterwards and `rm /root/smoke-token`.

From **your own machine** (not the droplet), check that only Cloudflare reaches it:

```sh
curl -sS --max-time 5 --resolve app.colophon.404human.dev:443:<droplet-ip> https://app.colophon.404human.dev/livez \
  && echo "FAIL: the droplet answers directly" || echo "ok: only Cloudflare reaches 443"
```

Record the smoke output (it includes idle memory) in `deploy/runbooks/install-feedback.md`.

## 12. The restore drill (S0 exit gate)

Prove the backup is a restore before relying on it:

1. Note the time, then create something in the Studio (an entry "restore drill"), publish it,
   and delete another entry.
2. In Neon, create a **branch** from the main branch **at the noted time** (before step 1's
   changes). It gets its own direct connection string.
3. On the droplet, run a second server against the branch, on another loopback port, without
   touching the real service:

   ```sh
   umask 077; cat >/root/drill_url     # the branch's URL with colophon_app and the app password, ?sslmode=verify-full
   COLOPHON_DATABASE_URL_FILE=/root/drill_url COLOPHON_LISTEN_ADDR=127.0.0.1:58010 \
     COLOPHON_PUBLIC_URL=http://127.0.0.1:58010 /usr/local/bin/colophon-server --role api &
   curl -s http://127.0.0.1:58010/readyz | jq .
   ```

4. Replay the deletion ledger from the newest export (`deploy/runbooks/restore.md` step 4), check
   that the deleted entry stays deleted and the "restore drill" entry is absent (it came after
   the branch point), then stop the drill server, `rm /root/drill_url`, and delete the branch.
5. Record the times and what you checked in `deploy/runbooks/install-feedback.md`.

### And from the logical backup (not Neon at all)

On any machine with Docker and your offline age key (your laptop is fine), restore the newest
dump into a scratch PostgreSQL and look at it:

```sh
rclone copy r2:colophon-backups/logical/<newest>.dump.age .      # or download it in R2's dashboard
age --decrypt -i <your-identity-file> -o colophon.dump <newest>.dump.age
docker run -d --name drill -e POSTGRES_PASSWORD=drill -p 127.0.0.1:55499:5432 postgres:18
sleep 5 && docker exec -i drill createdb -U postgres colophon
docker exec -i drill pg_restore -U postgres -d colophon --no-owner <colophon.dump
docker exec -i drill psql -U postgres -d colophon -Atc "select count(*) from entries; select count(*) from revisions;"
docker rm -f drill && rm colophon.dump
```

The counts should match the Studio's at the dump's time. Record them, with the dump's name, in
`deploy/runbooks/install-feedback.md`.

## Upgrading

Install the new release's binaries and Studio (step 2), `colophon admin migrate` (step 3), then
`systemctl restart colophon` and `smoke.sh`. Migrations run before the new server starts; read
the release's CHANGELOG for anything marked Breaking first.

## When something is wrong

| Symptom | Look at |
|---|---|
| `colophon` won't start | `journalctl -u colophon -n 100`: a refused setting names itself |
| `/readyz` says database down | Neon's status, the URL's `sslmode=verify-full`, the direct endpoint |
| Cloudflare 52x errors | `systemctl status caddy`, the origin certificate files, `ufw status` (443 from Cloudflare) |
| Every visitor shares one rate limit | `COLOPHON_TRUSTED_PROXIES` lacks Cloudflare's ranges: rerun `cloudflare-ips.sh` |
| Uploads refused 503 | the `COLOPHON_S3_*` settings and `credentials/s3_secret_key` |
