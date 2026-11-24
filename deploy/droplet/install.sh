#!/usr/bin/env bash
# The S0 droplet, installed in one run (deploy/droplet/DEPLOY.md explains each step; target ≈ 15
# minutes once CREATE.md's accounts exist). Safe to re-run: every step checks before it acts, so a
# second run only fills in what is missing and re-checks the rest. You are asked only for what can't
# be made here — the secrets and two ids — each read without echo, written root-only (umask 077).
#
#   sudo deploy/droplet/install.sh                # from a checkout of this repository on the droplet
#
# COLOPHON_VERSION (default below) picks the release. For a rehearsal, COLOPHON_INSTALL_ANSWERS names
# a directory holding the answers as files instead of prompts, named as their targets
# (migration_database_url, s3_secret_key, backup_s3_secret_key, origin.pem, origin.key) and the ids
# (r2_account_id, r2_access_key_id, r2_backup_access_key_id, ssh_ip); COLOPHON_RELEASE_BASE a local
# release directory (upgrade.sh; with COLOPHON_RELEASE_UNVERIFIED=rehearsal, as it says).
set -euo pipefail
export COLOPHON_VERSION="${COLOPHON_VERSION:-0.1.0-alpha.7}"
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=deploy/droplet/public-host.sh
. "$here/public-host.sh"
# shellcheck source=deploy/droplet/r2-keys.sh
. "$here/r2-keys.sh"
CREDS=/etc/colophon/credentials
ENV_FILE=/etc/colophon/colophon.env
answers="${COLOPHON_INSTALL_ANSWERS:-}"
started=$SECONDS
step() { printf '\n== %s  (%d s)\n' "$*" "$((SECONDS - started))"; }
say() { printf '   %s\n' "$*"; }
[[ $EUID -eq 0 ]] || { echo "error: run as root" >&2; exit 1; }
umask 022

# ask <file> <prompt>: the answer into a root-only file, unless it's there already.
ask() {
  local file=$1 prompt=$2 name
  [[ -s "$file" ]] && { say "kept $(basename "$file")"; return 0; }
  name=$(basename "$file")
  if [[ -n "$answers" ]]; then
    [[ -s "$answers/$name" ]] || { echo "error: no answer $answers/$name" >&2; exit 1; }
    (umask 077; cp "$answers/$name" "$file")
  else
    printf '\n   %s\n   Paste it, then press Enter and Ctrl-D (nothing is shown):\n' "$prompt"
    (umask 077; stty -echo 2>/dev/null || true; cat >"$file"; stty echo 2>/dev/null || true)
  fi
  chmod 0400 "$file"
  [[ -s "$file" ]] || { echo "error: $name is empty" >&2; rm -f "$file"; exit 1; }
  say "saved $name (root only)"
}
# value <name> <prompt>: a non-secret answer (an id), asked once and kept in /etc/colophon/install.
value() {
  local name=$1 prompt=$2 file=/etc/colophon/install/$1
  mkdir -p /etc/colophon/install
  if [[ ! -s "$file" ]]; then
    if [[ -n "$answers" ]]; then cp "$answers/$name" "$file"; else read -rp "   $prompt: " v; printf '%s' "$v" >"$file"; fi
  fi
  tr -d ' \n\r' <"$file"
}

step "1. the host: packages, swap, firewall, SSH, updates"
need=()
for p in ufw jq curl ca-certificates unattended-upgrades redis-server age rclone gnupg debian-keyring debian-archive-keyring apt-transport-https; do
  dpkg -s "$p" >/dev/null 2>&1 || need+=("$p")
done
if ((${#need[@]})); then apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq "${need[@]}" >/dev/null; say "installed ${need[*]}"; else say "base packages present"; fi
if ! command -v pg_dump >/dev/null || [[ "$(pg_dump --version | grep -oE '[0-9]+' | head -1)" != 18 ]]; then
  install -d /usr/share/postgresql-common/pgdg
  curl -fsSo /usr/share/postgresql-common/pgdg/apt.postgresql.org.asc https://www.postgresql.org/media/keys/ACCC4CF8.asc
  echo "deb [signed-by=/usr/share/postgresql-common/pgdg/apt.postgresql.org.asc] https://apt.postgresql.org/pub/repos/apt $(sed -n "s/^VERSION_CODENAME=//p" /etc/os-release)-pgdg main" >/etc/apt/sources.list.d/pgdg.list
  apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq postgresql-client-18 >/dev/null
  say "installed pg_dump 18"
fi
# gh, for upgrade.sh's provenance checks (gh attestation verify, offline against the release's
# bundle; no GitHub login). From GitHub's own repository: Ubuntu's gh is too old for attestations.
# Its signing key is taken only if it is the one pinned here.
GH_APT_KEY=7F38BBB59D064DBCB3D84D725612B36462313325
if ! command -v gh >/dev/null || ! gh attestation verify --help >/dev/null 2>&1; then
  keydir=$(mktemp -d)
  curl -fsSLo "$keydir/keyring.gpg" https://cli.github.com/packages/githubcli-archive-keyring.gpg
  gpg --no-default-keyring --keyring "$keydir/import.gpg" --import "$keydir/keyring.gpg" 2>/dev/null
  gpg --no-default-keyring --keyring "$keydir/import.gpg" --list-keys --with-colons 2>/dev/null | grep -qx "fpr:::::::::$GH_APT_KEY:" \
    || { rm -rf "$keydir"; echo "refusing: GitHub CLI's apt key isn't the pinned $GH_APT_KEY" >&2; exit 1; }
  gpg --no-default-keyring --keyring "$keydir/import.gpg" --export "$GH_APT_KEY" >/usr/share/keyrings/githubcli-archive-keyring.gpg
  rm -rf "$keydir"
  chmod 0644 /usr/share/keyrings/githubcli-archive-keyring.gpg
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" >/etc/apt/sources.list.d/github-cli.list
  apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq gh >/dev/null
  say "installed gh $(gh --version | head -1 | awk '{print $3}') (provenance checks)"
fi
if ! command -v caddy >/dev/null; then
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | gpg --dearmor --yes -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' >/etc/apt/sources.list.d/caddy-stable.list
  apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq caddy >/dev/null
  say "installed caddy"
fi
if ! swapon --show | grep -q /swapfile && [[ "${COLOPHON_INSTALL_NO_SWAP:-0}" != 1 ]]; then
  [[ -f /swapfile ]] || { fallocate -l 1G /swapfile && chmod 600 /swapfile && mkswap /swapfile >/dev/null; }
  if swapon /swapfile 2>/dev/null; then
    grep -q '^/swapfile' /etc/fstab || echo '/swapfile none swap sw 0 0' >>/etc/fstab
    say "swap: 1 GB guard"
  else
    say "swap: couldn't enable /swapfile here (a container?); skipped"
  fi
fi
# The address you SSH from: the session's own when there is one (none from the Droplet Console).
connected=${SSH_CLIENT:-}; connected=${connected%% *}
hint=""; [[ -n "$connected" ]] && hint=" (Enter for $connected, the address of this session)"
ssh_ip=$(value ssh_ip "Your own address for SSH$hint")
if [[ -z "$ssh_ip" && -n "$connected" ]]; then ssh_ip=$connected; printf '%s' "$ssh_ip" >/etc/colophon/install/ssh_ip; fi
[[ -n "$ssh_ip" ]] || { rm -f /etc/colophon/install/ssh_ip; echo "error: SSH needs an address to allow (yours); nothing opened" >&2; exit 1; }
ufw default deny incoming >/dev/null && ufw default allow outgoing >/dev/null
ufw status | grep -q "22/tcp.*ALLOW.*$ssh_ip" || ufw allow from "$ssh_ip" to any port 22 proto tcp comment 'ssh: maintainer' >/dev/null
ufw --force enable >/dev/null && say "ufw: 22 from $ssh_ip, nothing else yet"
# SSH: keys only, no forwarding of any kind, few tries. Written when it differs (an older install's
# file is brought up to date), checked by sshd -t before the reload, and put back if it fails.
sshd_conf=/etc/ssh/sshd_config.d/10-colophon.conf
sshd_want=$(printf '%s\n' 'PasswordAuthentication no' 'KbdInteractiveAuthentication no' 'PermitRootLogin prohibit-password' \
  'X11Forwarding no' 'AllowAgentForwarding no' 'AllowTcpForwarding no' 'MaxAuthTries 3' 'LoginGraceTime 30')
if [[ "$(cat "$sshd_conf" 2>/dev/null)" != "$sshd_want" ]]; then
  sshd_old=$(mktemp); cp -p "$sshd_conf" "$sshd_old" 2>/dev/null || : >"$sshd_old"
  printf '%s\n' "$sshd_want" >"$sshd_conf"
  if sshd -t; then
    systemctl reload ssh && say "ssh: keys only, no forwarding, 3 tries"
  else
    if [[ -s "$sshd_old" ]]; then cp -p "$sshd_old" "$sshd_conf"; else rm -f "$sshd_conf"; fi
    rm -f "$sshd_old"
    echo "refusing: sshd -t rejected the new $sshd_conf; the old one is back, ssh untouched" >&2; exit 1
  fi
  rm -f "$sshd_old"
fi
echo 'Unattended-Upgrade::Automatic-Reboot "false";' >/etc/apt/apt.conf.d/52colophon-no-auto-reboot

step "2. the release: v$COLOPHON_VERSION, checksums verified"
"$here/upgrade.sh" --fetch "$COLOPHON_VERSION"

step "3. Neon: the roles and the schema, over verify-full"
# verify-full checks Neon's certificate against the system's public roots; without the bundle
# every connection fails (as inside the postgres image, which has none).
[[ -s /etc/ssl/certs/ca-certificates.crt ]] || { echo "refusing: /etc/ssl/certs/ca-certificates.crt is missing; verify-full to Neon needs it (apt-get install ca-certificates)" >&2; exit 1; }
install -d -m 0700 "$CREDS"
ask "$CREDS/migration_database_url" "Neon's connection string for the owner role (as the dashboard gives it; the pooled host is made direct)"
url=$(cat "$CREDS/migration_database_url")
# Neon's dashboard hands out the pooled host with ?sslmode=require&channel_binding=require:
# neon-url.sh makes it the direct host over verify-full, and says what it changed.
# shellcheck source=deploy/droplet/neon-url.sh
. "$here/neon-url.sh"
fixed=$(neon_url_fix "$url")
if [[ "$fixed" != "$url" ]]; then
  (umask 077; chmod 0600 "$CREDS/migration_database_url"; printf '%s' "$fixed" >"$CREDS/migration_database_url"; chmod 0400 "$CREDS/migration_database_url")
  for note in "${NEON_URL_NOTES[@]}"; do say "Neon URL: $note"; done
fi
[[ -s "$CREDS/app_password" ]] || { (umask 077; head -c 32 /dev/urandom | base64 | tr '+/' '-_' | tr -d '=\n' >"$CREDS/app_password"); chmod 0400 "$CREDS/app_password"; say "generated the application's password"; }
# A failure stops here: `cmd && say` under set -e went on to migrate without the roles (2026-11-21).
# On Neon (no superuser) init-roles gives the password as text and the owner its memberships (D-234).
init_err=$(mktemp)
if ! colophon admin init-roles --database-url-file "$CREDS/migration_database_url" --app-password-file "$CREDS/app_password" >/dev/null 2>"$init_err"; then
  cat "$init_err" >&2
  echo "refusing: the roles step failed (colophon admin init-roles; its error is above); nothing after it was run" >&2
  # A failed statement that held the password as text may be in the database server's log.
  if grep -q 'rotate the application' "$init_err"; then
    echo "and ROTATE the application's password before running this again: rm $CREDS/app_password $CREDS/database_url (a new one is generated), and if colophon_app already exists, ALTER ROLE colophon_app PASSWORD to the new one as the owner" >&2
  fi
  rm -f "$init_err"
  exit 1
fi
rm -f "$init_err"
say "roles ready"
if [[ ! -s "$CREDS/database_url" ]]; then
  read -r rest <<<"$(sed -E 's#^postgres(ql)?://[^@]+@##' "$CREDS/migration_database_url")"
  (umask 077; printf 'postgres://colophon_app:%s@%s' "$(cat "$CREDS/app_password")" "$rest" >"$CREDS/database_url"); chmod 0400 "$CREDS/database_url"
  say "the server's database URL (colophon_app) written"
fi

step "4. the cache, on loopback"
if [[ ! -s "$CREDS/cache_url" ]]; then
  pass=$(head -c 32 /dev/urandom | base64 | tr '+/' '-_' | tr -d '=\n')
  # Created 0640 (umask 027), so the password is never in a world-readable file, not even briefly.
  (umask 027; printf 'bind 127.0.0.1 -::1\nprotected-mode yes\nmaxmemory 128mb\nmaxmemory-policy allkeys-lru\nrequirepass %s\n' "$pass" >/etc/redis/conf.d-colophon.conf)
  chmod 0640 /etc/redis/conf.d-colophon.conf && chown root:redis /etc/redis/conf.d-colophon.conf
  grep -q 'conf.d-colophon.conf' /etc/redis/redis.conf || echo 'include /etc/redis/conf.d-colophon.conf' >>/etc/redis/redis.conf
  (umask 077; printf 'redis://:%s@127.0.0.1:6379' "$pass" >"$CREDS/cache_url"); chmod 0400 "$CREDS/cache_url"; unset pass
  say "cache password generated"
fi
systemctl restart redis-server && systemctl enable redis-server >/dev/null 2>&1 && say "cache running"

step "5. settings and the R2 secret"
[[ -f "$ENV_FILE" ]] || install -m 0644 "$here/colophon.env.template" "$ENV_FILE"
# Two R2 tokens (CREATE.md), each scoped to one bucket: the media token the server holds, and the
# backups' own, which only the backup units load. The ids are checked before they go into the env
# file: 32 hex characters each, nothing a sed expression could read as more.
hex32() { [[ "$2" =~ ^[0-9a-f]{32}$ ]] || { rm -f "/etc/colophon/install/$1"; echo "refusing: $1 must be 32 lowercase hex characters (asked again on the next run)" >&2; exit 1; }; }
account=$(value r2_account_id "Your Cloudflare account id (R2)"); hex32 r2_account_id "$account"
access=$(value r2_access_key_id "The media R2 token's access key id (Object Read & Write on colophon-media only)"); hex32 r2_access_key_id "$access"
if r2_backup_shared /etc/colophon/install; then
  # One token on both buckets, by the operator's choice (r2-keys.sh): the backups' files hold the media token's,
  # so the backup units read their usual files and a rotated media token reaches them on the next run.
  backup_access=$access
  printf '%s' "$access" >/etc/colophon/install/r2_backup_access_key_id
else
  backup_access=$(value r2_backup_access_key_id "The backups R2 token's access key id (Object Read & Write on colophon-backups only)"); hex32 r2_backup_access_key_id "$backup_access"
fi
grep -q '^COLOPHON_BACKUP_S3_ACCESS_KEY=' "$ENV_FILE" || echo 'COLOPHON_BACKUP_S3_ACCESS_KEY=<r2-backup-access-key-id>' >>"$ENV_FILE"
sed -i "s#<r2-account-id>#$account#; s#<r2-access-key-id>#$access#; s#<r2-backup-access-key-id>#$backup_access#" "$ENV_FILE"
ask "$CREDS/s3_secret_key" "The media R2 token's secret access key"
if r2_backup_shared /etc/colophon/install; then
  (umask 077; rm -f "$CREDS/backup_s3_secret_key"; cp "$CREDS/s3_secret_key" "$CREDS/backup_s3_secret_key"); chmod 0400 "$CREDS/backup_s3_secret_key"
else
  ask "$CREDS/backup_s3_secret_key" "The backups R2 token's secret access key"
fi
if ! r2_backup_check "$access" "$backup_access" "$CREDS/s3_secret_key" "$CREDS/backup_s3_secret_key" /etc/colophon/install; then
  # Two tokens expected and the backups' id or secret is the media one: ask for them again on the next run.
  r2_backup_shared /etc/colophon/install || rm -f /etc/colophon/install/r2_backup_access_key_id "$CREDS/backup_s3_secret_key"
  exit 1
fi
# The hostname is one setting: COLOPHON_PUBLIC_URL. Caddy's site address and the links below follow it.
public=$(public_url "$ENV_FILE") || { echo "refusing: fix COLOPHON_PUBLIC_URL in $ENV_FILE, then run this again" >&2; exit 1; }
host=${public#https://}
say "public address: $public"

step "6. the service"
install -m 0644 "$here/../production/colophon.service" /etc/systemd/system/colophon.service
systemctl daemon-reload && systemctl enable colophon >/dev/null 2>&1 && say "colophon.service installed"

step "7. Caddy: the origin certificate, Authenticated Origin Pulls, Cloudflare's ranges"
ask /etc/caddy/origin.pem "Cloudflare's Origin Certificate (the PEM, -----BEGIN CERTIFICATE----- …)"
ask /etc/caddy/origin.key "Its private key (-----BEGIN PRIVATE KEY----- …)"
chown root:caddy /etc/caddy/origin.pem /etc/caddy/origin.key && chmod 0440 /etc/caddy/origin.pem /etc/caddy/origin.key
rendered=$(mktemp)
caddyfile_for_host "$here/Caddyfile" "$host" >"$rendered" || { rm -f "$rendered"; echo "refusing: no Caddyfile for $host" >&2; exit 1; }
install -m 0644 "$rendered" /etc/caddy/Caddyfile && rm -f "$rendered" && say "Caddy's site address: $host (from COLOPHON_PUBLIC_URL)"
"$here/origin-pull-ca.sh"
"$here/cloudflare-ips.sh"
# A Caddyfile that doesn't validate stops the install (`validate && reload` under set -e went on).
caddy validate --config /etc/caddy/Caddyfile >/dev/null 2>&1 \
  || { caddy validate --config /etc/caddy/Caddyfile >&2 || true; echo "refusing: /etc/caddy/Caddyfile doesn't validate (above); Caddy left as it was" >&2; exit 1; }
# The unit's reload asks the admin API at the NEW config's address, a socket a Caddy started with
# an older config (admin on TCP 2019) hasn't opened: the first time, restart instead.
if [[ -S /var/lib/caddy/admin.sock ]] && systemctl is-active --quiet caddy; then systemctl reload caddy; else systemctl restart caddy; fi
say "caddy serving 443 (admin API: the 0600 socket /var/lib/caddy/admin.sock)"

step "8. the switch: migrate, start, check (upgrade.sh)"
"$here/upgrade.sh" "$COLOPHON_VERSION"

step "9. the first administrator"
code=$(journalctl -u colophon --no-pager 2>/dev/null | grep -o 'Setup code[^"]*' | tail -1 || true)
if [[ -n "$code" ]]; then say "$code — open $public/studio/ and enter it"; else say "no setup code in the log: the installation already has an administrator"; fi

step "10. backups: the ledger export and the logical dump"
if [[ ! -s /etc/colophon/backup-age-identity ]]; then
  (umask 077; age-keygen -o /etc/colophon/backup-age-identity 2>/dev/null); chmod 0400 /etc/colophon/backup-age-identity
  age-keygen -y /etc/colophon/backup-age-identity >/etc/colophon/backup-age-recipient
  say "backup key made. COPY THIS LINE TO YOUR OFFLINE STORE NOW (it is shown once):"
  [[ -n "$answers" ]] || grep '^AGE-SECRET-KEY-' /etc/colophon/backup-age-identity | sed 's/^/      /'
fi
install -m 0755 "$here/backup-logical.sh" /usr/local/bin/colophon-backup-logical
for unit in colophon-ledger-export.service colophon-ledger-export.timer colophon-backup-logical.service colophon-backup-logical.timer; do
  install -m 0644 "$here/$unit" /etc/systemd/system/
done
systemctl daemon-reload && systemctl enable --now colophon-ledger-export.timer colophon-backup-logical.timer >/dev/null 2>&1
systemctl start colophon-backup-logical && say "first logical backup: $(colophon-backup-logical latest 2>/dev/null || echo 'not yet')"

step "11. smoke"
"$here/smoke.sh" || say "smoke reported failures above (through Cloudflare needs DNS and Authenticated Origin Pulls in place)"
printf '\ninstall: done in %d s (v%s)\n' "$((SECONDS - started))" "$COLOPHON_VERSION"
