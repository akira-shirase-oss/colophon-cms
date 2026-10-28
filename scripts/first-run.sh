#!/usr/bin/env bash
# A brand-new Colophon installation, for trying the first-run journey by hand (step 3.7, D-061):
# the server with the Studio and the worker on http://localhost:58400, against an empty database
# of its own (colophon_first_run, or COLOPHON_TEST_DATABASE) that nobody has bootstrapped. The
# server prints a setup code to this terminal; sign up in the Studio, enter the code, and you are
# the administrator. Ctrl-C stops the server; the database stays as it was.
#
#   scripts/first-run.sh           start (from an empty database the first time)
#   scripts/first-run.sh --reset   start again from an empty database
#
# Needs the stack (scripts/stack.sh up) with the realm that allows sign-up and registers port
# 58400's callback (deploy/keycloak; recreate Keycloak after changing it), and npm ci.
set -euo pipefail
cd "$(dirname "$0")/.."

export COLOPHON_TEST_DATABASE="${COLOPHON_TEST_DATABASE:-colophon_first_run}"
if [[ ! "$COLOPHON_TEST_DATABASE" =~ ^[a-z][a-z0-9_]{0,62}$ ]]; then
  echo "error: COLOPHON_TEST_DATABASE must be a plain database name" >&2
  exit 2
fi
port=58400
reset=0
[[ "${1:-}" == "--reset" ]] && reset=1

scripts/check-toolchain.sh >/dev/null
for tool in docker curl npm; do
  command -v "$tool" >/dev/null || { echo "error: $tool is required" >&2; exit 1; }
done
if curl -s -o /dev/null "http://127.0.0.1:$port/"; then
  echo "error: something is already listening on port $port; stop it first" >&2
  exit 1
fi

psql_admin() {
  docker compose -f deploy/compose.yaml exec -T postgres psql -U colophon -d postgres -XAtqc "$1"
}
if (( reset == 1 )); then
  psql_admin "DROP DATABASE IF EXISTS $COLOPHON_TEST_DATABASE WITH (FORCE)" >/dev/null
fi
if [[ -z "$(psql_admin "SELECT 1 FROM pg_database WHERE datname = '$COLOPHON_TEST_DATABASE'")" ]]; then
  psql_admin "CREATE DATABASE $COLOPHON_TEST_DATABASE" >/dev/null
fi

echo "building the server, the CLI and the Studio"
cargo build --quiet --release --locked -p colophon-server -p colophon-cli
npm run build --silent -w @colophon/studio >/dev/null

# shellcheck source=scripts/local-env.sh
. scripts/local-env.sh
# Migrations only: no `admin bootstrap`, which is the point.
target/release/colophon admin migrate
echo "Open http://localhost:$port/studio/ (dev users: dev-owner / dev-editor, password colophon-dev)"
exec env \
  COLOPHON_LISTEN_ADDR="127.0.0.1:$port" \
  COLOPHON_PUBLIC_URL="http://localhost:$port" \
  COLOPHON_STUDIO_DIR="$PWD/apps/studio/build" \
  target/release/colophon-server
