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
# (migration_database_url, s3_secret_key, origin.pem, origin.key) and the ids (r2_account_id,
# r2_access_key_id, ssh_ip); COLOPHON_RELEASE_BASE a local release directory (upgrade.sh).
set -euo pipefail
export COLOPHON_VERSION="${COLOPHON_VERSION:-0.1.0-alpha.1}"
here=$(cd "$(dirname "$0")" && pwd)
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
if [[ ! -f /etc/ssh/sshd_config.d/10-colophon.conf ]]; then
  printf '%s\n' 'PasswordAuthentication no' 'KbdInteractiveAuthentication no' 'PermitRootLogin prohibit-password' >/etc/ssh/sshd_config.d/10-colophon.conf
  sshd -t && systemctl reload ssh && say "ssh: keys only"
fi
echo 'Unattended-Upgrade::Automatic-Reboot "false";' >/etc/apt/apt.conf.d/52colophon-no-auto-reboot

step "2. the release: v$COLOPHON_VERSION, checksums verified"
"$here/upgrade.sh" --fetch "$COLOPHON_VERSION"

step "3. Neon: the roles and the schema, over verify-full"
install -d -m 0700 "$CREDS"
ask "$CREDS/migration_database_url" "Neon's DIRECT connection string for the owner role (no -pooler in the host)"
url=$(cat "$CREDS/migration_database_url")
# Neon's dashboard copies ?sslmode=require&channel_binding=require: verify-full is the only mode
# Colophon takes off this machine (D-156), and it refuses channel_binding rather than ignore it.
fixed=$(sed -E 's/([?&])channel_binding=[^&]*&?/\1/; s/sslmode=[a-z-]+/sslmode=verify-full/; s/[?&]$//' <<<"$url")
[[ "$fixed" == *sslmode=verify-full* ]] || fixed="$fixed$([[ "$fixed" == *\?* ]] && echo '&' || echo '?')sslmode=verify-full"
[[ "$fixed" != *-pooler* ]] || { echo "error: that's Neon's pooled endpoint; use the direct one (no -pooler)" >&2; rm -f "$CREDS/migration_database_url"; exit 1; }
if [[ "$fixed" != "$url" ]]; then (umask 077; chmod 0600 "$CREDS/migration_database_url"; printf '%s' "$fixed" >"$CREDS/migration_database_url"; chmod 0400 "$CREDS/migration_database_url"); say "set sslmode=verify-full"; fi
[[ -s "$CREDS/app_password" ]] || { (umask 077; head -c 32 /dev/urandom | base64 | tr '+/' '-_' | tr -d '=\n' >"$CREDS/app_password"); chmod 0400 "$CREDS/app_password"; say "generated the application's password"; }
colophon admin init-roles --database-url-file "$CREDS/migration_database_url" --app-password-file "$CREDS/app_password" >/dev/null && say "roles ready"
if [[ ! -s "$CREDS/database_url" ]]; then
  read -r rest <<<"$(sed -E 's#^postgres(ql)?://[^@]+@##' "$CREDS/migration_database_url")"
  (umask 077; printf 'postgres://colophon_app:%s@%s' "$(cat "$CREDS/app_password")" "$rest" >"$CREDS/database_url"); chmod 0400 "$CREDS/database_url"
  say "the server's database URL (colophon_app) written"
fi

step "4. the cache, on loopback"
if [[ ! -s "$CREDS/cache_url" ]]; then
  pass=$(head -c 32 /dev/urandom | base64 | tr '+/' '-_' | tr -d '=\n')
  printf 'bind 127.0.0.1 -::1\nprotected-mode yes\nmaxmemory 128mb\nmaxmemory-policy allkeys-lru\nrequirepass %s\n' "$pass" >/etc/redis/conf.d-colophon.conf
  chmod 0640 /etc/redis/conf.d-colophon.conf && chown root:redis /etc/redis/conf.d-colophon.conf
  grep -q 'conf.d-colophon.conf' /etc/redis/redis.conf || echo 'include /etc/redis/conf.d-colophon.conf' >>/etc/redis/redis.conf
  (umask 077; printf 'redis://:%s@127.0.0.1:6379' "$pass" >"$CREDS/cache_url"); chmod 0400 "$CREDS/cache_url"; unset pass
  say "cache password generated"
fi
systemctl restart redis-server && systemctl enable redis-server >/dev/null 2>&1 && say "cache running"

step "5. settings and the R2 secret"
[[ -f "$ENV_FILE" ]] || install -m 0644 "$here/colophon.env.template" "$ENV_FILE"
account=$(value r2_account_id "Your Cloudflare account id (R2)")
access=$(value r2_access_key_id "The R2 token's access key id")
sed -i "s#<r2-account-id>#$account#; s#<r2-access-key-id>#$access#" "$ENV_FILE"
ask "$CREDS/s3_secret_key" "The R2 token's secret access key"

step "6. the service"
install -m 0644 "$here/../production/colophon.service" /etc/systemd/system/colophon.service
systemctl daemon-reload && systemctl enable colophon >/dev/null 2>&1 && say "colophon.service installed"

step "7. Caddy: the origin certificate, Authenticated Origin Pulls, Cloudflare's ranges"
ask /etc/caddy/origin.pem "Cloudflare's Origin Certificate (the PEM, -----BEGIN CERTIFICATE----- …)"
ask /etc/caddy/origin.key "Its private key (-----BEGIN PRIVATE KEY----- …)"
chown root:caddy /etc/caddy/origin.pem /etc/caddy/origin.key && chmod 0440 /etc/caddy/origin.pem /etc/caddy/origin.key
install -m 0644 "$here/Caddyfile" /etc/caddy/Caddyfile
"$here/origin-pull-ca.sh"
"$here/cloudflare-ips.sh"
caddy validate --config /etc/caddy/Caddyfile >/dev/null 2>&1 && systemctl reload-or-restart caddy && say "caddy serving 443"

step "8. the switch: migrate, start, check (upgrade.sh)"
"$here/upgrade.sh" "$COLOPHON_VERSION"

step "9. the first administrator"
code=$(journalctl -u colophon --no-pager 2>/dev/null | grep -o 'Setup code[^"]*' | tail -1 || true)
if [[ -n "$code" ]]; then say "$code — open https://app.colophon.404human.dev/studio/ and enter it"; else say "no setup code in the log: the installation already has an administrator"; fi

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
