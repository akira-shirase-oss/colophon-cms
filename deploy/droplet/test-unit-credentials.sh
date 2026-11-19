#!/usr/bin/env bash
# CI only: deploy/production/colophon.service, exactly as shipped, on a real systemd (a GitHub
# runner VM), with its LoadCredential= lines reading root-only files. Proves the credentials path the
# S0 rehearsal couldn't (Docker gives systemd no credentials): the server reads its database and
# cache URLs from $CREDENTIALS_DIRECTORY, comes up ready against a TLS-only PostgreSQL over
# sslmode=verify-full (a CA of this script's own) and a password-protected Valkey, while the
# service's own (dynamic) user can't read the files where they are kept and the password is in no
# process environment.
#
# Uploads stay off: the unit's s3_secret_key credential is loaded, but a drop-in unsets
# COLOPHON_S3_SECRET_KEY_FILE (all of COLOPHON_S3_* or none); R2 is the Garage stand-in's job.
#
#   sudo CI=true deploy/droplet/test-unit-credentials.sh <colophon-server> <colophon> <studio-dir>
set -euo pipefail
[[ "${CI:-}" == true ]] || { echo "refusing: this installs a system-wide unit; it runs in CI only" >&2; exit 2; }
[[ $EUID -eq 0 ]] || { echo "run with sudo" >&2; exit 2; }
server_bin=$1 cli_bin=$2 studio=$3
cd "$(dirname "$0")/../.."
fail() { echo "FAIL: $*" >&2; journalctl -u colophon --no-pager -n 40 >&2 || true; exit 1; }
work=$(mktemp -d)
cleanup() { systemctl stop colophon 2>/dev/null || true; docker rm -f unit-cred-pg unit-cred-valkey >/dev/null 2>&1 || true; rm -rf "$work"; }
trap cleanup EXIT

# A TLS-only PostgreSQL behind a CA of this run's own, named localhost.
openssl req -x509 -new -nodes -newkey rsa:2048 -days 1 -subj "/CN=unit credentials CA" \
  -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign" -keyout "$work/ca.key" -out "$work/ca.pem" 2>/dev/null
openssl req -new -nodes -newkey rsa:2048 -subj "/CN=localhost" -keyout "$work/server.key" -out "$work/server.csr" 2>/dev/null
printf 'subjectAltName=DNS:localhost\nextendedKeyUsage=serverAuth\n' >"$work/ext"
openssl x509 -req -days 1 -in "$work/server.csr" -CA "$work/ca.pem" -CAkey "$work/ca.key" -CAcreateserial -extfile "$work/ext" -out "$work/server.pem" 2>/dev/null
printf 'local all all trust\nhostssl all all all scram-sha-256\nhostnossl all all all reject\n' >"$work/pg_hba.conf"
owner_pw=$(openssl rand -hex 16)
docker create --name unit-cred-pg -p 127.0.0.1:55498:5432 -e POSTGRES_USER=owner -e POSTGRES_DB=colophon -e "POSTGRES_PASSWORD=$owner_pw" \
  --entrypoint sh postgres:18 -c 'set -eu; install -d -o postgres -m 0700 /tls
    install -o postgres -m 0600 /in/server.key /tls/server.key; install -o postgres -m 0644 /in/server.pem /in/pg_hba.conf /tls/
    exec docker-entrypoint.sh postgres -c ssl=on -c ssl_cert_file=/tls/server.pem -c ssl_key_file=/tls/server.key -c hba_file=/tls/pg_hba.conf' >/dev/null
docker cp "$work/." unit-cred-pg:/in >/dev/null && docker start unit-cred-pg >/dev/null
for _ in $(seq 60); do docker exec unit-cred-pg pg_isready -q -h 127.0.0.1 2>/dev/null && break; sleep 1; done
cache_pw=$(openssl rand -hex 16)
docker run -d --name unit-cred-valkey -p 127.0.0.1:56398:6379 valkey/valkey:8 valkey-server --requirepass "$cache_pw" >/dev/null

# The droplet's layout: binaries, Studio, the CA readable by all (it's public), credentials root-only.
install -m 0755 "$server_bin" /usr/local/bin/colophon-server
install -m 0755 "$cli_bin" /usr/local/bin/colophon
mkdir -p /usr/local/share/colophon && rm -rf /usr/local/share/colophon/studio && cp -R "$studio" /usr/local/share/colophon/studio
install -d -m 0755 /etc/colophon && install -m 0644 "$work/ca.pem" /etc/colophon/ci-ca.pem
install -d -m 0700 /etc/colophon/credentials
umask 077
q="sslmode=verify-full&sslrootcert=/etc/colophon/ci-ca.pem"
printf 'postgres://owner:%s@localhost:55498/colophon?%s' "$owner_pw" "$q" >/etc/colophon/credentials/migration_database_url
openssl rand -hex 24 | tr -d '\n' >/etc/colophon/credentials/app_password
colophon admin init-roles --database-url-file /etc/colophon/credentials/migration_database_url --app-password-file /etc/colophon/credentials/app_password >/dev/null
colophon admin migrate --database-url-file /etc/colophon/credentials/migration_database_url >/dev/null
app_pw=$(cat /etc/colophon/credentials/app_password)
printf 'postgres://colophon_app:%s@localhost:55498/colophon?%s' "$app_pw" "$q" >/etc/colophon/credentials/database_url
printf 'redis://:%s@127.0.0.1:56398' "$cache_pw" >/etc/colophon/credentials/cache_url
printf 'unused-here' >/etc/colophon/credentials/s3_secret_key
chmod 0400 /etc/colophon/credentials/*
umask 022
printf 'COLOPHON_LISTEN_ADDR=127.0.0.1:58000\nCOLOPHON_PUBLIC_URL=http://localhost:58000\nCOLOPHON_LOG_FORMAT=json\n' >/etc/colophon/colophon.env

install -m 0644 deploy/production/colophon.service /etc/systemd/system/colophon.service
mkdir -p /etc/systemd/system/colophon.service.d
# No object store in this check: the server takes all of COLOPHON_S3_* or none, and refuses one set
# but blank, so the drop-in unsets the unit's only S3 variable rather than emptying it.
printf '[Service]\nUnsetEnvironment=COLOPHON_S3_SECRET_KEY_FILE\n' >/etc/systemd/system/colophon.service.d/no-s3.conf
systemctl daemon-reload && systemctl start colophon

ready=""
for _ in $(seq 60); do
  r=$(curl -fsS --max-time 2 http://127.0.0.1:58000/readyz 2>/dev/null || true)
  if jq -e '.status == "ready" and .checks.database == "up" and .checks.cache == "up"' <<<"$r" >/dev/null 2>&1; then ready=$r; break; fi
  sleep 1
done
[[ -n "$ready" ]] || fail "the unit never became ready: ${r:-no answer}"
echo "  ok    the shipped unit is ready, its database and cache URLs read from systemd credentials ($(jq -c .checks <<<"$ready"))"

pid=$(systemctl show colophon -p MainPID --value)
user=$(ps -o user= -p "$pid" | tr -d ' ')
[[ "$user" != root ]] || fail "the service runs as root"
echo "  ok    it runs as $user (DynamicUser), not root"
if tr '\0' '\n' <"/proc/$pid/environ" | grep -qF "$app_pw"; then fail "the database password is in the process environment"; fi
echo "  ok    the password is in no environment variable (only *_FILE paths)"
tr '\0' '\n' <"/proc/$pid/environ" | grep -q '^COLOPHON_DATABASE_URL_FILE=/run/credentials/colophon.service/database_url$' \
  || fail "COLOPHON_DATABASE_URL_FILE isn't the credentials directory's: $(tr '\0' '\n' <"/proc/$pid/environ" | grep DATABASE_URL_FILE)"
echo "  ok    COLOPHON_DATABASE_URL_FILE is \$CREDENTIALS_DIRECTORY/database_url"
if systemd-run --wait --quiet -p DynamicUser=yes -p User="$user" cat /etc/colophon/credentials/database_url >/dev/null 2>&1; then
  fail "the service's user can read /etc/colophon/credentials/database_url directly"
fi
echo "  ok    the service's user can't read /etc/colophon/credentials/ itself"
echo "unit credentials: the shipped unit reads its secrets only through systemd credentials, over verify-full"
