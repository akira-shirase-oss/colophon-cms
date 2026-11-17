#!/usr/bin/env bash
# The database TLS rule (D-156) against a real PostgreSQL and a private certificate authority.
# A throwaway postgres container that takes TLS only, with a server certificate for `localhost`
# signed by a CA made here, and the real `colophon` binary connecting to it
# (`colophon admin migrate-status`, which only reads):
#
#   verify-full, the CA named, the name on the certificate   connects
#   verify-full, the CA named, an address the cert doesn't name   refused in the handshake
#   verify-full, no CA named (the public roots only)          refused in the handshake
#   plain text (sslmode=disable)                              refused by the server
#   sslmode=require to another machine                        refused before connecting
#   sslmode=verify-ca to another machine                      refused before connecting
#   channel_binding=require                                   refused before connecting
#
# The container is this script's own, on 127.0.0.1 — never the stack's — and is removed on
# exit. COLOPHON_TLS_CHECK_PORT picks its port (default 55441, bumped while taken). Needs
# docker, jq and openssl; builds target/release/colophon if it is missing. Runs on macOS's
# bash 3.2.
set -euo pipefail
cd "$(dirname "$0")/.."

PORT="${COLOPHON_TLS_CHECK_PORT:-55441}"
COLOPHON_BIN=target/release/colophon
CONTAINER="colophon-tls-check-$$"
# The throwaway container's own password; it never leaves this script.
PASSWORD="tls-check-$$"

checks=0
failures=0
pass() { checks=$((checks + 1)); printf '  ok    %s\n' "$1"; }
fail() { checks=$((checks + 1)); failures=$((failures + 1)); printf '  FAIL  %s\n' "$1"; }

work=$(mktemp -d)
cleanup() {
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

if [[ ! -x "$COLOPHON_BIN" ]]; then
  cargo build --quiet --release --locked -p colophon-cli
fi
while (echo >"/dev/tcp/127.0.0.1/$PORT") 2>/dev/null; do PORT=$((PORT + 1)); done
image=$(docker compose -f deploy/compose.yaml config --format json | jq -r '.services.postgres.image')

# A CA, and a server certificate it signs for `localhost` only (no IP address), both expiring
# in two days. webpki wants a CA marked as one and a server certificate with a subjectAltName.
mkdir -p "$work/tls"
cat >"$work/openssl.cnf" <<'CNF'
[req]
distinguished_name = dn
prompt = no
[dn]
CN = colophon tls check
[ca]
basicConstraints = critical, CA:TRUE
keyUsage = critical, keyCertSign, cRLSign
subjectKeyIdentifier = hash
[server]
basicConstraints = critical, CA:FALSE
keyUsage = critical, digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = DNS:localhost
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid
CNF
(
  cd "$work"
  openssl req -x509 -new -nodes -newkey rsa:2048 -days 2 -config openssl.cnf -extensions ca \
    -subj "/CN=Colophon TLS check CA" -keyout ca.key -out tls/ca.pem 2>/dev/null
  openssl req -new -nodes -newkey rsa:2048 -config openssl.cnf -subj "/CN=localhost" \
    -keyout tls/server.key -out server.csr 2>/dev/null
  openssl x509 -req -days 2 -in server.csr -CA tls/ca.pem -CAkey ca.key -CAcreateserial \
    -extfile openssl.cnf -extensions server -out tls/server.pem 2>/dev/null
)
# TLS or nothing: the socket inside the container is for the image's own initialisation.
cat >"$work/tls/pg_hba.conf" <<'HBA'
local     all all     trust
hostssl   all all all scram-sha-256
hostnossl all all all reject
HBA

# PostgreSQL insists its key belongs to it and nobody else can read it, so the files are copied
# in and handed over before the image's entrypoint starts the server.
docker create --name "$CONTAINER" -p "127.0.0.1:$PORT:5432" \
  -e POSTGRES_USER=colophon -e POSTGRES_DB=colophon -e "POSTGRES_PASSWORD=$PASSWORD" \
  --entrypoint sh "$image" -c '
    set -eu
    install -d -o postgres -m 0700 /tls
    install -o postgres -m 0600 /tls-in/server.key /tls/server.key
    install -o postgres -m 0644 /tls-in/server.pem /tls-in/pg_hba.conf /tls/
    exec docker-entrypoint.sh postgres -c ssl=on -c ssl_cert_file=/tls/server.pem \
      -c ssl_key_file=/tls/server.key -c hba_file=/tls/pg_hba.conf' >/dev/null
docker cp "$work/tls/." "$CONTAINER:/tls-in" >/dev/null
docker start "$CONTAINER" >/dev/null
ready=0
for _ in $(seq 1 60); do
  # The image's first start runs a server on its socket only; TCP answers once the real one is up.
  if docker exec "$CONTAINER" pg_isready -q -h 127.0.0.1 -p 5432 >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 1
done
if [[ "$ready" != 1 ]]; then
  docker logs "$CONTAINER" 2>&1 | tail -20 >&2
  echo "error: the TLS-only postgres never became ready (logs above)" >&2
  exit 1
fi
echo "database TLS against a private CA (postgres on 127.0.0.1:$PORT, TLS only):"

# attempt <url>: runs migrate-status with the URL in the environment (never an argument, so it
# stays out of the process list); leaves the exit code in $code and both outputs in $out.
attempt() {
  code=0
  out=$(COLOPHON_MIGRATION_DATABASE_URL="$1" "$COLOPHON_BIN" admin migrate-status 2>&1) || code=$?
}
# connects <what> <url>
connects() {
  attempt "$2"
  if [[ "$code" == 0 && "$out" == *"schema behind"* ]]; then
    pass "$1"
  else
    fail "$1: exit $code: $out"
  fi
}
# refused <what> <url> <exit code> <text the error must hold>
refused() {
  attempt "$2"
  if [[ "$code" != "$3" ]]; then
    fail "$1: expected exit $3, got $code: $out"
  elif [[ "$out" != *"$4"* ]]; then
    fail "$1: the error doesn't say '$4': $out"
  elif [[ "$out" == *"$PASSWORD"* ]]; then
    fail "$1: the error shows the password"
  else
    # The reason, as the operator sees it, so the log shows why each refusal happened.
    pass "$1"
    printf '        %s\n' "$(printf '%s\n' "$out" | grep -m1 '^error: ' | cut -c1-200)"
  fi
}

ca="$work/tls/ca.pem"
base="postgres://colophon:$PASSWORD"
connects "verify-full with the CA, to the name on the certificate" \
  "$base@localhost:$PORT/colophon?sslmode=verify-full&sslrootcert=$ca"
refused "verify-full with the CA, to an address the certificate doesn't name" \
  "$base@127.0.0.1:$PORT/colophon?sslmode=verify-full&sslrootcert=$ca" 5 "certificate"
refused "verify-full without the CA (an issuer nobody trusts)" \
  "$base@localhost:$PORT/colophon?sslmode=verify-full" 5 "certificate"
refused "plain text, to a server that takes TLS only" \
  "$base@localhost:$PORT/colophon?sslmode=disable" 5 "no encryption"
# Another machine's name that can't resolve (RFC 2606): a refusal before connecting exits 2
# (usage) and names the fix; an attempt to connect would have exited 5.
refused "sslmode=require to another machine" \
  "$base@db.colophon.invalid:$PORT/colophon?sslmode=require" 2 "sslmode=verify-full"
refused "sslmode=verify-ca to another machine, even with the CA named" \
  "$base@db.colophon.invalid:$PORT/colophon?sslmode=verify-ca&sslrootcert=$ca" 2 "sslmode=verify-full"
refused "channel_binding=require, which the driver would ignore" \
  "$base@localhost:$PORT/colophon?sslmode=verify-full&sslrootcert=$ca&channel_binding=require" 2 \
  "Remove channel_binding"

if ((failures > 0)); then
  echo "$failures of $checks database TLS checks failed" >&2
  exit 1
fi
echo "all $checks database TLS checks passed"
