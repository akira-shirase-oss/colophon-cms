#!/usr/bin/env bash
# The multi-instance local stack (milestone S, D-075): API servers and workers from the release
# build, behind HAProxy at http://localhost:58405 (a sign-in redirect URI the development realm
# already registers). Every server's public URL is the balancer's, so sessions, CSRF and
# sign-in work whichever server answers: their state is in Postgres and Valkey, never in a
# process.
#
#   scripts/cluster.sh up [servers] [workers]   start them (default 3 and 2), and the balancer
#   scripts/cluster.sh down                     stop everything (SIGTERM, then the balancer)
#   scripts/cluster.sh status                   each process, and whether it is ready
#   scripts/cluster.sh kill N                   SIGKILL server N: what a crash looks like
#   scripts/cluster.sh stop N                   SIGTERM server N and wait: a drain
#   scripts/cluster.sh start N                  start server N again and wait until it is ready
#
# The local stack must be up (scripts/stack.sh up). COLOPHON_TEST_DATABASE picks the database,
# as everywhere (scripts/local-env.sh). State and logs: target/cluster/. Ports: servers
# 58511.., workers 58521.., all on 127.0.0.1. The database connections the processes may open
# are summed and kept under the server's max_connections, with room for migrations and tests.
set -euo pipefail
cd "$(dirname "$0")/.."

dir="$PWD/target/cluster"
balancer_port=58405
balancer_url="http://127.0.0.1:$balancer_port"
# Short enough for tests, longer than the balancer needs to see a draining server (its checks
# run every 500 ms and two failures mark a server down).
drain_ms="${COLOPHON_DRAIN_DELAY_MS:-2000}"
server_connections="${COLOPHON_DB_MAX_CONNECTIONS:-16}"
worker_connections=4
compose=(docker compose -f deploy/cluster/compose.yaml)

usage() {
  sed -n '9,14p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

server_port() { echo $((58510 + $1)); }
worker_port() { echo $((58520 + $1)); }

ready() {
  target/release/colophon doctor --server "$1" --timeout-ms 500 >/dev/null 2>&1
}

wait_ready() {
  local url=$1 what=$2
  for _ in $(seq 1 150); do
    if ready "$url"; then
      return 0
    fi
    sleep 0.2
  done
  echo "error: $what never became ready at $url" >&2
  return 1
}

# Starts one process in the background, appending to its log; records its pid.
launch() {
  local kind=$1 index=$2 port=$3 role=$4 connections=$5
  (
    # shellcheck source=scripts/local-env.sh
    . scripts/local-env.sh
    export COLOPHON_LISTEN_ADDR="127.0.0.1:$port" COLOPHON_PUBLIC_URL="http://localhost:$balancer_port" \
      COLOPHON_LOG="${COLOPHON_LOG:-warn}" COLOPHON_LOG_FORMAT=json \
      COLOPHON_DRAIN_DELAY_MS="$drain_ms" COLOPHON_DB_MAX_CONNECTIONS="$connections"
    exec target/release/colophon-server --role "$role"
  ) >>"$dir/$kind-$index.log" 2>&1 &
  echo $! >"$dir/$kind-$index.pid"
}

pid_of() { cat "$dir/$1.pid" 2>/dev/null || true; }

alive() {
  local pid
  pid=$(pid_of "$1")
  [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null
}

start_server() {
  local index=$1
  alive "server-$index" && { echo "server $index is already running" >&2; return 1; }
  launch server "$index" "$(server_port "$index")" api "$server_connections"
  wait_ready "http://127.0.0.1:$(server_port "$index")" "server $index"
}

write_haproxy_config() {
  local servers=$1
  {
    cat <<'EOF'
# Written by scripts/cluster.sh (D-075). Health: /readyz, which says `draining` (503) as soon
# as a server is asked to stop, so it leaves the rotation before it stops accepting.
global
    log stdout format raw local0 warning
    maxconn 4096

defaults
    mode http
    log global
    option httplog
    timeout connect 2s
    timeout client 30s
    timeout server 30s
    timeout http-request 10s
    timeout http-keep-alive 10s
    # A request that never reached a server (refused, reset before a response) goes to another:
    # writes carry Idempotency-Key, so even one that did reach a dying server is replayed, not
    # repeated, by the server that answers.
    retries 3
    option redispatch 1
    retry-on conn-failure empty-response
    # A request is retried only if HAProxy still holds all of it: keep bodies (up to its
    # buffer, 16 KiB) until a server answers.
    option http-buffer-request

frontend api
    bind :8080
    option forwardfor
    default_backend servers

backend servers
    balance leastconn
    option httpchk
    http-check send meth GET uri /readyz ver HTTP/1.1 hdr Host localhost
    http-check expect status 200
    default-server check inter 500ms fastinter 250ms downinter 1s fall 2 rise 2
EOF
    for index in $(seq 1 "$servers"); do
      echo "    server s$index host.docker.internal:$(server_port "$index")"
    done
  } >"$dir/haproxy.cfg"
}

check_connection_budget() {
  local servers=$1 workers=$2
  local available used
  available=$(docker compose -f deploy/compose.yaml exec -T postgres psql -U colophon -XAtqc "show max_connections")
  # Every process also keeps one connection for its readiness probe (D-075), and each worker a
  # pool of COLOPHON_WORKER_DB_CONNECTIONS (default 4).
  used=$(( servers * (server_connections + 1) + workers * (worker_connections + 4 + 1) ))
  # Leave a fifth for migrations, tests and psql.
  if (( used > available * 4 / 5 )); then
    echo "error: $servers servers and $workers workers may open $used database connections; Postgres allows $available (keep $(( available / 5 )) spare)" >&2
    exit 1
  fi
  echo "database connections: up to $used of $available"
}

up() {
  local servers=${1:-3} workers=${2:-2}
  [[ "$servers" =~ ^[1-9]$ && "$workers" =~ ^[0-9]$ ]] || usage
  mkdir -p "$dir"
  for file in "$dir"/*.pid; do
    [[ -e "$file" ]] || continue
    if alive "$(basename "$file" .pid)"; then
      echo "error: the cluster is already up (scripts/cluster.sh down first)" >&2
      exit 1
    fi
  done
  rm -f "$dir"/*.pid
  scripts/check-toolchain.sh >/dev/null
  check_connection_budget "$servers" "$workers"
  scripts/migrate.sh
  cargo build --quiet --release --locked -p colophon-server -p colophon-cli
  for index in $(seq 1 "$servers"); do
    launch server "$index" "$(server_port "$index")" api "$server_connections"
  done
  for index in $(seq 1 "$workers"); do
    launch worker "$index" "$(worker_port "$index")" worker "$worker_connections"
  done
  for index in $(seq 1 "$servers"); do
    wait_ready "http://127.0.0.1:$(server_port "$index")" "server $index"
  done
  for index in $(seq 1 "$workers"); do
    wait_ready "http://127.0.0.1:$(worker_port "$index")" "worker $index"
  done
  echo "$servers" >"$dir/servers"
  write_haproxy_config "$servers"
  COLOPHON_CLUSTER_DIR="$dir" "${compose[@]}" up -d --force-recreate --wait >/dev/null
  wait_ready "$balancer_url" "the balancer"
  echo "up: $servers servers and $workers workers behind http://localhost:$balancer_port"
}

down() {
  local pids=()
  for file in "$dir"/*.pid; do
    [[ -e "$file" ]] || continue
    local pid
    pid=$(cat "$file")
    if kill -0 "$pid" 2>/dev/null; then
      kill -TERM "$pid" 2>/dev/null || true
      pids+=("$pid")
    fi
    rm -f "$file"
  done
  for pid in "${pids[@]}"; do
    while kill -0 "$pid" 2>/dev/null; do sleep 0.1; done
  done
  COLOPHON_CLUSTER_DIR="$dir" "${compose[@]}" down >/dev/null 2>&1 || true
  echo "down"
}

status() {
  for file in "$dir"/*.pid; do
    [[ -e "$file" ]] || { echo "not running"; return 0; }
    local name port state
    name=$(basename "$file" .pid)
    case "$name" in
      server-*) port=$(server_port "${name#server-}") ;;
      worker-*) port=$(worker_port "${name#worker-}") ;;
    esac
    if ! alive "$name"; then
      state="stopped"
    elif ready "http://127.0.0.1:$port"; then
      state="ready"
    else
      state="not ready"
    fi
    printf '%-9s pid %-7s 127.0.0.1:%s  %s\n' "$name" "$(pid_of "$name")" "$port" "$state"
  done
  if ready "$balancer_url"; then echo "balancer  http://localhost:$balancer_port  ready"; else echo "balancer  not ready"; fi
}

server_index() {
  [[ "${1:-}" =~ ^[1-9]$ ]] || usage
  echo "$1"
}

case "${1:-}" in
  up) shift; up "$@" ;;
  down) down ;;
  status) status ;;
  kill)
    index=$(server_index "${2:-}")
    alive "server-$index" || { echo "server $index isn't running" >&2; exit 1; }
    kill -KILL "$(pid_of "server-$index")"
    echo "killed server $index"
    ;;
  stop)
    index=$(server_index "${2:-}")
    alive "server-$index" || { echo "server $index isn't running" >&2; exit 1; }
    pid=$(pid_of "server-$index")
    kill -TERM "$pid"
    while kill -0 "$pid" 2>/dev/null; do sleep 0.1; done
    echo "server $index drained and stopped"
    ;;
  start)
    index=$(server_index "${2:-}")
    start_server "$index"
    echo "server $index is ready"
    ;;
  *) usage ;;
esac
