#!/usr/bin/env bash
# Runs Colophon locally, end to end, for trying it by hand: the server with the Studio and the
# worker on http://localhost:58000, and the reference site's runner on http://127.0.0.1:58099,
# against a database of its own (colophon_demo, or COLOPHON_TEST_DATABASE), never the one the
# other checkouts share. The first run seeds a "Demo site" project: five articles, three published
# as release 1, two waiting for review, the site's webhook and a probe of the live site. Ctrl-C
# stops everything; the demo database stays for next time.
#
#   scripts/demo.sh           start (seeding the demo database the first time)
#   scripts/demo.sh --reset   start again from an empty demo database
#
# Needs the stack (scripts/stack.sh up), Node 24 and npm ci.
set -euo pipefail
cd "$(dirname "$0")/.."

export COLOPHON_TEST_DATABASE="${COLOPHON_TEST_DATABASE:-colophon_demo}"
server_port=58000
site_port=58099
state=target/demo
reset=0
[[ "${1:-}" == "--reset" ]] && reset=1

scripts/check-toolchain.sh >/dev/null
for tool in node docker curl; do
  command -v "$tool" >/dev/null || { echo "error: $tool is required" >&2; exit 1; }
done
for port in "$server_port" "$site_port"; do
  if curl -s -o /dev/null "http://127.0.0.1:$port/"; then
    echo "error: something is already listening on port $port; stop it first" >&2
    exit 1
  fi
done

psql_admin() {
  docker compose -f deploy/compose.yaml exec -T postgres psql -U colophon -d postgres -XAtqc "$1"
}
if (( reset == 1 )); then
  psql_admin "DROP DATABASE IF EXISTS $COLOPHON_TEST_DATABASE WITH (FORCE)" >/dev/null
  rm -rf "$state"
fi
fresh=0
if [[ -z "$(psql_admin "SELECT 1 FROM pg_database WHERE datname = '$COLOPHON_TEST_DATABASE'")" ]]; then
  psql_admin "CREATE DATABASE $COLOPHON_TEST_DATABASE" >/dev/null
  fresh=1
  rm -rf "$state"
fi
mkdir -p "$state"

scripts/migrate.sh
echo "building the server and the Studio"
cargo build --quiet --release --locked -p colophon-server -p colophon-cli
npm run build --silent -w @colophon/studio >/dev/null

server_pid=""
site_pid=""
# shellcheck disable=SC2329 # invoked by the EXIT trap below
cleanup() {
  for pid in "$site_pid" "$server_pid"; do
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
      kill -TERM "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
    fi
  done
}
trap cleanup EXIT

(
  # shellcheck source=scripts/local-env.sh
  . scripts/local-env.sh
  export COLOPHON_STUDIO_DIR="$PWD/apps/studio/build" COLOPHON_LOG=warn
  exec target/release/colophon-server
) >"$state/server.log" 2>&1 &
server_pid=$!

project=""
[[ -f "$state/project" ]] && project=$(cat "$state/project")
# The runner adopts the project of the first announcement it gets when it isn't told one.
SITE_API="http://localhost:$server_port" SITE_PROJECT="$project" SITE_ROOT="$state/site" \
  SITE_LISTEN="127.0.0.1:$site_port" node apps/web/src/runner.ts >"$state/site.log" 2>&1 &
site_pid=$!

ready=0
for _ in $(seq 1 100); do
  if target/release/colophon doctor --server "http://127.0.0.1:$server_port" --timeout-ms 500 >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 0.2
done
if (( ready == 0 )); then
  echo "error: the server never became ready; see $state/server.log" >&2
  exit 1
fi

if (( fresh == 1 )) || [[ -z "$project" ]]; then
  echo "seeding the demo project"
  project=$(node scripts/demo-seed.ts "http://localhost:$server_port" "http://127.0.0.1:$site_port")
  echo "$project" >"$state/project"
fi

cat <<EOF

Colophon is running locally (database $COLOPHON_TEST_DATABASE).

  Studio        http://localhost:$server_port/studio/
                sign in as dev-owner or dev-editor, password colophon-dev
  Demo project  http://localhost:$server_port/studio/projects/$project
  Live site     http://127.0.0.1:$site_port/   (rebuilt on every release)
  Public API    http://localhost:$server_port/content/v1/$project/production/articles

Logs: $state/server.log, $state/site.log. Ctrl-C stops everything.
EOF
wait "$server_pid"
