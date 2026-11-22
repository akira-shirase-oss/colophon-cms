#!/usr/bin/env bash
# CI only: smoke.sh's database checks against a TLS-only PostgreSQL, the way they run against Neon
# (found live on the droplet, 2026-11-21: psql needs sslrootcert=system, and PGSSLROOTCERT in the
# environment breaks the colophon CLI, whose sqlx reads it as a file path):
#   - the server's database URL is written as install.sh writes Neon's, verify-full and no
#     sslrootcert; a private CA installed in the system store stands in for Neon's public one, so
#     psql passes only with libpq's sslrootcert=system;
#   - the colophon CLI runs in the same smoke run (the grants check, D-234), with a migration URL
#     naming the CA file (sqlx trusts the bundled public roots plus sslrootcert, never the system
#     store), and must not be handed PGSSLROOTCERT;
#   - so smoke's role check, round trip and (once present) grants check all pass in ONE run.
# The server itself isn't started: smoke's other checks fail here and are ignored.
#
#   sudo CI=true deploy/droplet/test-smoke-db.sh <colophon CLI>
set -euo pipefail
[[ "${CI:-}" == true ]] || { echo "refusing: this installs a CA into the system store; it runs in CI only" >&2; exit 2; }
[[ $EUID -eq 0 ]] || { echo "run with sudo" >&2; exit 2; }
cli=$1
cd "$(dirname "$0")"
fail() { echo "FAIL: $*" >&2; exit 1; }
command -v psql >/dev/null || fail "psql isn't installed"
[[ "$(psql --version | grep -oE '[0-9]+' | head -1)" -ge 16 ]] || fail "psql $(psql --version) is older than 16: no sslrootcert=system"
work=$(mktemp -d)
ca_store=/usr/local/share/ca-certificates/colophon-smoke-db-ci.crt
cleanup() { docker rm -f smoke-db-pg >/dev/null 2>&1 || true; rm -f "$ca_store"; update-ca-certificates --fresh >/dev/null 2>&1 || true; rm -rf "$work"; }
trap cleanup EXIT

openssl req -x509 -new -nodes -newkey rsa:2048 -days 1 -subj "/CN=smoke db CA" \
  -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign" -keyout "$work/ca.key" -out "$work/ca.pem" 2>/dev/null
openssl req -new -nodes -newkey rsa:2048 -subj "/CN=localhost" -keyout "$work/server.key" -out "$work/server.csr" 2>/dev/null
printf 'subjectAltName=DNS:localhost\nextendedKeyUsage=serverAuth\n' >"$work/ext"
openssl x509 -req -days 1 -in "$work/server.csr" -CA "$work/ca.pem" -CAkey "$work/ca.key" -CAcreateserial -extfile "$work/ext" -out "$work/server.pem" 2>/dev/null
printf 'local all all trust\nhostssl all all all scram-sha-256\nhostnossl all all all reject\n' >"$work/pg_hba.conf"
owner_pw=$(openssl rand -hex 16)
docker create --name smoke-db-pg -p 127.0.0.1::5432 -e POSTGRES_USER=owner -e POSTGRES_DB=colophon -e "POSTGRES_PASSWORD=$owner_pw" \
  --entrypoint sh postgres:18 -c 'set -eu; install -d -o postgres -m 0700 /tls
    install -o postgres -m 0600 /in/server.key /tls/server.key; install -o postgres -m 0644 /in/server.pem /in/pg_hba.conf /tls/
    exec docker-entrypoint.sh postgres -c ssl=on -c ssl_cert_file=/tls/server.pem -c ssl_key_file=/tls/server.key -c hba_file=/tls/pg_hba.conf' >/dev/null
docker cp "$work/." smoke-db-pg:/in >/dev/null && docker start smoke-db-pg >/dev/null
port=$(docker port smoke-db-pg 5432/tcp | sed -n 's/^127\.0\.0\.1:\([0-9][0-9]*\)$/\1/p' | head -1)
[[ -n "$port" ]] || fail "no host port for postgres"
for _ in $(seq 60); do docker exec smoke-db-pg pg_isready -q -h 127.0.0.1 2>/dev/null && break; sleep 1; done
# The private CA, trusted by the system store: what "system" means to libpq here.
install -m 0644 "$work/ca.pem" "$ca_store" && update-ca-certificates >/dev/null

creds=$work/creds
install -d -m 0700 "$creds"
umask 077
printf 'postgres://owner:%s@localhost:%s/colophon?sslmode=verify-full&sslrootcert=%s' "$owner_pw" "$port" "$work/ca.pem" >"$creds/migration_database_url"
openssl rand -hex 24 | tr -d '\n' >"$creds/app_password"
"$cli" admin init-roles --database-url-file "$creds/migration_database_url" --app-password-file "$creds/app_password" >/dev/null
"$cli" admin migrate --database-url-file "$creds/migration_database_url" >/dev/null
# As install.sh writes Neon's: verify-full, no sslrootcert.
printf 'postgres://colophon_app:%s@localhost:%s/colophon?sslmode=verify-full' "$(cat "$creds/app_password")" "$port" >"$creds/database_url"
umask 022

mkdir -p "$work/bin" && ln -sf "$cli" "$work/bin/colophon"
printf 'COLOPHON_PUBLIC_URL=https://localhost\nCOLOPHON_TRUSTED_PROXIES=127.0.0.1/32\n' >"$work/colophon.env"
# A clean environment, as root's on the droplet: no PG* of the caller's.
out=$(env -i PATH="$work/bin:$PATH" HOME="$work" SMOKE_SCOPE=local SMOKE_DOMAIN=localhost SMOKE_CREDENTIALS="$creds" \
  SMOKE_ENV_FILE="$work/colophon.env" bash ./smoke.sh 2>&1 || true)
echo "$out" | grep -E 'database role|round trip|migration grant|account usage' || true
grep -q "ok    the server's database role (colophon_app) has neither BYPASSRLS nor superuser" <<<"$out" \
  || fail "smoke's role check didn't pass over verify-full with system roots: $(grep -E 'database role' <<<"$out")"
echo "  ok    smoke's role check passes: psql verified the server against the system store (sslrootcert=system)"
grep -q 'ok    database round trip' <<<"$out" || fail "smoke's round trip didn't run: $(grep -E 'round trip' <<<"$out")"
echo "  ok    smoke's round trip ran over the same URL"
if grep -q 'no migration grant left behind' ./smoke.sh; then
  grep -q 'ok    no migration grant left behind' <<<"$out" \
    || fail "smoke's grants check (the colophon CLI, same run) failed: $(grep -E 'migration grant' <<<"$out")"
  echo "  ok    the CLI's grants check passed in the same run: it wasn't handed PGSSLROOTCERT"
fi
if grep -q 'account usage agrees with a recount' ./smoke.sh; then
  grep -q 'ok    account usage agrees with a recount of the rows' <<<"$out" \
    || fail "smoke's usage check (colophon admin usage-reconcile --check, same run) failed: $(grep -E 'account usage' <<<"$out")"
  echo "  ok    the CLI's usage recount agreed in the same run (D-235)"
fi
echo "smoke db: the role and grants checks pass in one run, psql on system roots, the CLI untouched"
