# Deploying Colophon to one droplet (S0)

S0 in `docs/SAAS_PLAN.md`: Colophon on a 2 GB DigitalOcean droplet in Toronto, against Neon,
behind Cloudflare at `https://colophonapp.404human.dev`, used by the maintainer only. This is the
production path without containers (`deploy/production/README.md`), filled in for that host.

**Before you start:** everything in [CREATE.md](CREATE.md) exists. Secrets are typed or pasted
only on the droplet, into root-only files; nothing secret goes into chat, an issue or git.

**Exit gate (SAAS_PLAN S0):** health checks green (`smoke.sh`), a restore drill from Neon's
point-in-time restore recorded (step 12), idle memory recorded (`smoke.sh` prints it).

## The fast path

Once [CREATE.md](CREATE.md)'s accounts exist, one script installs everything, and another upgrades,
rolls back and follows the edge channel. Rehearsed on an Ubuntu 24.04 systemd container (Neon, R2
and Cloudflare stood in for; see "What the rehearsal proved" below): **install 70 s** (target ≈ 15
min including your pastes), **upgrade 7 s with no failed request** (target < 2 min, < 5 s
unavailable), **rollback 5 s** (target < 1 min), and a release that fails its checks rolled itself
back.

```sh
ssh root@<droplet-ip>
git clone --depth 1 https://github.com/akira-shirase-oss/colophon-cms /opt/colophon-src
/opt/colophon-src/deploy/droplet/install.sh
```

`install.sh` runs steps 1–11 below in order and is safe to run again: each step checks before it
acts. It asks only for what can't be made on the machine — the Neon URL, the R2 secret and two ids,
Cloudflare's origin certificate and key, your SSH address — each read without echo into a root-only
file, and it fixes Neon's `sslmode=require&channel_binding=require` to `verify-full` itself. It
ends with `smoke.sh`; the checks through Cloudflare pass once DNS and Authenticated Origin Pulls
are on.

### Upgrades, rollbacks and the two channels

```sh
/opt/colophon-src/deploy/droplet/upgrade.sh 0.1.0-alpha.3      # a tagged release
/opt/colophon-src/deploy/droplet/upgrade.sh --channel edge     # the newest build of main (once edge.yml lands)
/opt/colophon-src/deploy/droplet/upgrade.sh --rollback         # back to the release before
/opt/colophon-src/deploy/droplet/upgrade.sh --status
```

An upgrade verifies the archives against `SHA256SUMS`, unpacks to
`/opt/colophon/releases/<version>`, migrates with the new binary while the old one serves, flips
the `current` symlink, restarts (Caddy holds requests meanwhile), waits for `/readyz`, runs the
local smoke, and **rolls back by itself** if any of that fails. It refuses a database migrated by a
newer build than the one it would switch to. Migrations are never undone: each release's
migrations work with the previous release's binary, so `--rollback` swaps binaries only. The last
three releases are kept.

- **The release channel (default)** is for normal use: tagged, changelogged, the archives the
  release workflow published. It installs only archives whose provenance verifies: built by
  `release.yml` on `refs/tags/v<version>` of this repository, checked with `gh attestation
  verify` against the release's own bundle (`colophon-<version>.intoto.jsonl`; no GitHub login),
  then against `SHA256SUMS`. `SHA256SUMS` alone comes from the same place as the archives, so it
  proves they arrived intact, not who built them. A release without a bundle is refused. A
  rehearsal from a local directory may set `COLOPHON_RELEASE_UNVERIFIED=rehearsal`, which is said
  loudly and refused for releases from GitHub.
- **The edge channel** is for a fix you need before the next release: the newest build of `main`
  (D-211). **It is available once `edge.yml` lands (D-211); until then use the release channel**
  — `upgrade.sh --channel edge` says so and changes nothing. It installs only what it can prove: `SHA256SUMS`'s provenance (built by `edge.yml` on
  `refs/heads/main` of this repository), every file against `SHA256SUMS`, and the build's commit
  equal to the attested one; anything else is fetched again once, then refused. Go back to the
  release channel at the next release.

After a release that changes this repository's deploy files, `git -C /opt/colophon-src pull` first.

## The steps the scripts take (and the manual fallback)

The commands run on the droplet as root unless they say otherwise. `$CHECKOUT` is a copy of this
repository on the droplet (step 2). **The version is in one place:**

```sh
export COLOPHON_VERSION=0.1.0-alpha.3    # the release this guide installs; change only this to upgrade
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
ufw allow from <your-ip> to any port 22 proto tcp comment 'ssh: maintainer'
ufw --force enable

# SSH: keys only, no forwarding (X11, agent, TCP), 3 tries, 30 s to log in. Checked before it's
# applied, so a typo can't lock you out (install.sh puts the old file back if sshd -t refuses).
printf '%s\n' 'PasswordAuthentication no' 'KbdInteractiveAuthentication no' 'PermitRootLogin prohibit-password' \
  'X11Forwarding no' 'AllowAgentForwarding no' 'AllowTcpForwarding no' 'MaxAuthTries 3' 'LoginGraceTime 30' \
  >/etc/ssh/sshd_config.d/10-colophon.conf
sshd -t && systemctl reload ssh

# gh, for upgrade.sh's provenance checks, from GitHub's own repository (Ubuntu's is too old). Its
# key must be 7F38BBB59D064DBCB3D84D725612B36462313325; install.sh checks that and exports only it.

# Security updates install themselves; the machine never reboots on its own (a reboot restarts
# Colophon mid-request). The reminder below says when one is waiting.
echo 'Unattended-Upgrade::Automatic-Reboot "false";' >/etc/apt/apt.conf.d/52colophon-no-auto-reboot
```

**If your home address changes**, SSH is closed to you, but the DigitalOcean **Droplet Console**
(the droplet's page → Access → Launch Droplet Console) reaches the machine regardless of ufw.
From it, replace the rule, then change the DigitalOcean cloud firewall's SSH rule to match:

```sh
ufw delete allow from <old-ip> to any port 22 proto tcp; ufw allow from <new-ip> to any port 22 proto tcp comment 'ssh: maintainer'
```

**A reboot reminder:** `test -f /var/run/reboot-required && cat /var/run/reboot-required.pkgs`
says when an installed update waits for one; reboot at a quiet moment (`systemctl reboot`), then
run `smoke.sh`.

Keep the DigitalOcean cloud firewall from CREATE.md as well: two layers, either one enough.

## 2. Colophon's binaries and the Studio

`install.sh` does this through `upgrade.sh --fetch "$COLOPHON_VERSION"`, which unpacks into
`/opt/colophon/releases/<version>` and points `/usr/local/bin/colophon-server`, `colophon` and the
Studio through `/opt/colophon/current`. By hand, use that same command: the commands below are what
it does, for reading.

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

> The release `v0.1.0-alpha.3` carries the checksummed musl archives this step installs. Building
> from source on a 2 GB droplet is not recommended (the Rust build needs more memory than the host has).

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

**Neon's owner is not a superuser** (D-234), and the two commands above handle that themselves:
`init-roles` gives `colophon_app`'s password as text when Neon refuses a SCRAM verifier ("Neon
only supports being given plaintext passwords"; over this `verify-full` connection), and grants
the owner SET and INHERIT on Colophon's roles; `migrate` runs four migrations (0011, 0012, 0051,
0082) from substitutes that need no superuser, and gives the owner roles CREATE on `public`
**inside each migration's own transaction only**: no other session ever sees it, and a run that
dies (a dropped connection, a kill) takes it with its rollback. The database records the same
migrations and checksums as a superuser's install, so `admin migrate-status` reads them alike;
it also reports any colophon role holding CREATE on `public`, or the owner inheriting
`colophon_deleter`, and `smoke.sh` fails on either (`admin migrate` removes both). Proved on a
fresh cluster with such an owner by `scripts/db-nonsuperuser-check.sh` (in CI), a run killed
mid-migration included. Migration 0048 turns the owner's INHERIT on `colophon_deleter` off on
purpose; nothing leaves it on.

To look at the database with `psql` itself, add `&sslrootcert=system` to the URL: libpq checks
`verify-full` against `~/.postgresql/root.crt` unless told to use the system's roots (Colophon's
own driver always uses them).

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
nano /etc/colophon/colophon.env          # the R2 account id and both tokens' access key ids (CREATE.md 6)
umask 077; cat >/etc/colophon/credentials/s3_secret_key          # the MEDIA token's secret, Ctrl-D
cat >/etc/colophon/credentials/backup_s3_secret_key              # the BACKUPS token's secret, Ctrl-D
chmod 0400 /etc/colophon/credentials/s3_secret_key /etc/colophon/credentials/backup_s3_secret_key
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

# The site address is the host of COLOPHON_PUBLIC_URL in /etc/colophon/colophon.env (one setting):
( . $CHECKOUT/deploy/droplet/public-host.sh && caddyfile_for_host $CHECKOUT/deploy/droplet/Caddyfile \
    "$(public_host /etc/colophon/colophon.env)" >/etc/caddy/Caddyfile.new ) \
  && install -m 0644 /etc/caddy/Caddyfile.new /etc/caddy/Caddyfile && rm /etc/caddy/Caddyfile.new
$CHECKOUT/deploy/droplet/origin-pull-ca.sh       # which origin-pull CAs Caddy trusts (below); the shared one's fingerprint checked
$CHECKOUT/deploy/droplet/cloudflare-ips.sh       # Caddy's trusted ranges, ufw 443; Colophon trusts loopback alone
caddy validate --config /etc/caddy/Caddyfile && systemctl restart caddy   # restart the first time: the admin API moves to a socket
systemctl restart colophon
```

What the Caddyfile does, each checked by `test-caddy.sh` in CI:

- **The client's address.** From a peer in Cloudflare's ranges, the client is `Cf-Connecting-Ip`
  (Cloudflare sets it), and Caddy tells the server that one address, replacing any
  `X-Forwarded-For` that came in. The server trusts loopback alone
  (`COLOPHON_TRUSTED_PROXIES=127.0.0.1/32`). Trusting Cloudflare's ranges in the server as well
  let a request from a Cloudflare address (another account's Worker) put any address it liked
  to the left of its own and have it believed: a fresh address for every password attempt.
- **The admin API** is a socket only Caddy's user and root can open,
  `/var/lib/caddy/admin.sock` (0600), not TCP 2019, which every local process could reach and
  reconfigure Caddy through. `systemctl reload caddy` uses it.
- **HTTP/1.1 and HTTP/2 only**: HTTP/3 is UDP, which the firewall doesn't open.
- **The bare root** (`https://colophonapp.404human.dev/`) redirects to `/studio/` with a 302 (the
  server itself answers `route_not_found` there); no other path is redirected.
- **HSTS** for this host: `max-age=31536000`, without `includeSubDomains` (the zone's other hosts
  are on Cloudflare Pages) and without preload, a later and deliberate step.

### Our own origin-pull certificate

Cloudflare's default origin-pull certificate is **one certificate shared by every Cloudflare
zone**: trusting its CA keeps out everything that isn't Cloudflare, but not another Cloudflare
account that points a hostname at this droplet (Caddy's SNI check is all that stands in the way).
The end state is a certificate of our own, uploaded to this zone. On your own machine, with a
Cloudflare API token for this zone only (Zone → SSL and Certificates → Edit):

```sh
deploy/droplet/custom-origin-pull.sh generate ~/aop               # a client CA and certificate, 0700
scp ~/aop/ca.pem root@<droplet>:/root/origin-pull-ca.pem
ssh root@<droplet> '$CHECKOUT/deploy/droplet/origin-pull-ca.sh --add-custom /root/origin-pull-ca.pem && systemctl reload caddy'
export CLOUDFLARE_API_TOKEN=…                                     # read from your password manager; never on a command line
deploy/droplet/custom-origin-pull.sh upload <zone-id> ~/aop       # print <zone-id> ~/aop shows the calls instead
deploy/droplet/custom-origin-pull.sh status <zone-id>             # until "active"
deploy/droplet/custom-origin-pull.sh enable <zone-id>
ssh root@<droplet> '$CHECKOUT/deploy/droplet/origin-pull-ca.sh --custom-only && systemctl reload caddy && $CHECKOUT/deploy/droplet/smoke.sh'
```

The order keeps the site up: the origin trusts both CAs before Cloudflare switches, and drops the
shared one only once Cloudflare presents ours. Keep `~/aop/ca.key` offline (or delete it: it is
needed only to issue the next client certificate, before this one expires in 5 years) and delete
`client.key` once it is uploaded. Back out with `origin-pull-ca.sh --shared`.

## 8. The first administrator

```sh
journalctl -u colophon | grep "Setup code"
```

Open `https://colophonapp.404human.dev/studio/`, enter the code, and create your account. The
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
read. The droplet needs only the public half to encrypt: its own copy of the private half is for
the first restore drill, and then it goes. **Once your offline copy is confirmed and the restore
drill (step 12) has decrypted a dump with it, remove it from the droplet:**
`shred -u /etc/colophon/backup-age-identity`. Otherwise a break-in on the droplet reads every
off-site dump too. Later drills bring the key in from your offline store, as restore.md says.

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
curl -sS --max-time 5 --resolve colophonapp.404human.dev:443:<droplet-ip> https://colophonapp.404human.dev/livez \
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

## Upgrading by hand

`upgrade.sh` is the way (above). By hand, it is: install the new release's binaries and Studio
(step 2) into a new directory, `colophon admin migrate` with them (step 3), point the symlinks at it,
`systemctl restart colophon`, wait for `/readyz`, `smoke.sh`. Read the release's CHANGELOG for
anything marked Breaking first.

## What the rehearsal proved

Rehearsed in a privileged Ubuntu 24.04 container booting systemd, with a TLS-only `postgres:18`
behind a private CA standing in for Neon, a self-signed certificate for Cloudflare's origin
certificate, and a test CA and client certificate for Authenticated Origin Pulls:

| Proved there | Unproven until the real droplet |
|---|---|
| `install.sh` end to end (apt, PGDG's pg_dump 18, Caddy, ufw, sshd, redis, init-roles and migrations over `verify-full`, the unit, the setup code, the backup timers), 70 s | Neon itself (`verify-full` to its public CA, the direct endpoint, cold starts) |
| `upgrade.sh`: 7 s, 0 of 375 requests failed during the switch | Cloudflare: DNS, Full (strict), the real origin-pull client certificate |
| `--rollback`: 5 s; a failing release rolled back by itself | R2: uploads, the ledger export, the logical backup's upload |
| Caddy: 200 with the origin-pull client certificate, refused without it | systemd credentials (`LoadCredential=`): Docker can't provide them, so the rehearsal read the files directly |
| `smoke.sh`'s server checks, idle memory (24–38 MiB) | The release archives from `release.yml` and the edge channel's provenance on a real build |
| The Neon URL fix (pooled host → direct, `require` → `verify-full`), tested | The database round trip to Neon us-east-2 (the S0 droplet ended up in SFO2, San Francisco: 91 ms measured on 2026-11-21, so moving Neon nearer is open): `smoke.sh` prints p50/p95 of 20 × `SELECT 1` (warns over 20 ms); record the deploy-day number here: **p50 — ms, p95 — ms** (Neon's first query after idle was 367 ms, a cold start) |
| Pruning under the backups bucket's 7-day lock, tested with a stub | The lock rule itself on R2 |


## When something is wrong

| Symptom | Look at |
|---|---|
| `colophon` won't start | `journalctl -u colophon -n 100`: a refused setting names itself |
| `/readyz` says database down | Neon's status, the URL's `sslmode=verify-full`, the direct endpoint |
| Cloudflare 52x errors | `systemctl status caddy`, the origin certificate files, `ufw status` (443 from Cloudflare), Authenticated Origin Pulls on in Cloudflare (Caddy refuses connections without its client certificate) |
| Every visitor shares one rate limit | Caddy isn't telling the server `Cf-Connecting-Ip`: `/etc/caddy/cloudflare-trusted.caddy` lacks Cloudflare's ranges (rerun `cloudflare-ips.sh`), or `/etc/caddy/Caddyfile` predates `client_ip_headers` (rerun `install.sh`); `smoke.sh` says which |
| Uploads refused 503 | the `COLOPHON_S3_*` settings and `credentials/s3_secret_key` |
| A backup unit fails with "no COLOPHON_BACKUP_S3_ACCESS_KEY" | the backups' own R2 token (CREATE.md 6): its key id in `colophon.env`, its secret in `credentials/backup_s3_secret_key` |
| `upgrade.sh` refuses: "no provenance bundle" | a release published before `release.yml` shipped the bundle: upload one (`gh attestation download`, then `gh release upload`), or install a newer release |
