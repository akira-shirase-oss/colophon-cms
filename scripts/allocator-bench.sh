#!/usr/bin/env bash
# The Linux allocator, measured (D-176): the same commit's colophon-server built two ways — the
# release's static musl binary and a glibc one — run in turn on one machine, interleaved over
# several rounds so drift on a shared runner hits both, under two k6 throughput runs each:
# rich-text validation (parsing, validating and re-serializing JSON: allocation-heavy, and no
# database) and /livez (little allocation: the control). perf/k6/allocator.js is the load.
#
#   scripts/allocator-bench.sh musl=path/to/colophon-server glibc=path/to/colophon-server \
#     [--rounds 3] [--total 200000] [--out perf/results]
#
# The server runs without a database (validation and /livez need none) and with rate limits
# off, as scripts/load.sh runs it. Prints a table and writes
# <out>/<stamp>-allocator-<machine>.json with every round and the medians. Needs k6 and jq.
set -euo pipefail
cd "$(dirname "$0")/.."

rounds=3
total=200000
out=perf/results
builds=()
while [[ $# -gt 0 ]]; do
  case $1 in
    --rounds) rounds=$2; shift 2 ;;
    --total) total=$2; shift 2 ;;
    --out) out=$2; shift 2 ;;
    *=*) builds+=("$1"); shift ;;
    *) echo "usage: scripts/allocator-bench.sh NAME=BINARY NAME=BINARY [--rounds N] [--total N] [--out DIR]" >&2; exit 2 ;;
  esac
done
[[ ${#builds[@]} -ge 2 ]] || { echo "name at least two builds: musl=… glibc=…" >&2; exit 2; }

readonly PORT=58032
readonly BASE="http://127.0.0.1:$PORT"
work=$(mktemp -d "${TMPDIR:-/tmp}/colophon-allocator.XXXXXX")
server=""
cleanup() {
  if [[ -n "$server" ]]; then kill "$server" 2>/dev/null || true; fi
  rm -rf "$work"
}
trap cleanup EXIT

start() {
  COLOPHON_DATABASE_URL=postgres://nobody:nothing@127.0.0.1:1/none \
    COLOPHON_LISTEN_ADDR="127.0.0.1:$PORT" COLOPHON_LOG=warn COLOPHON_LOG_FORMAT=json \
    COLOPHON_RATE_LIMIT_PER_ADDRESS=off COLOPHON_RATE_LIMIT_PER_CREDENTIAL=off \
    COLOPHON_RATE_LIMIT_DELIVERY=off COLOPHON_RATE_LIMIT_PASSWORD=off "$1" >"$work/server.log" 2>&1 &
  server=$!
  for _ in $(seq 100); do
    curl -fs -o /dev/null "$BASE/livez" && return 0
    sleep 0.1
  done
  echo "the server didn't start:" >&2
  tail -20 "$work/server.log" >&2
  return 1
}

stop() {
  kill "$server" 2>/dev/null || true
  wait "$server" 2>/dev/null || true
  server=""
}

# run <mode> -> "requests_per_second p99_ms"; a failed check fails the measurement.
run() {
  if ! BASE_URL=$BASE MODE=$1 TOTAL=$total VUS=64 \
    k6 run --quiet --summary-export "$work/summary.json" perf/k6/allocator.js >"$work/k6.log" 2>&1; then
    echo "k6 $1 failed:" >&2
    tail -20 "$work/k6.log" >&2
    return 1
  fi
  jq -r '"\(.metrics.http_reqs.rate) \(.metrics.http_req_duration."p(99)")"' "$work/summary.json"
}

rows='[]'
for round in $(seq "$rounds"); do
  for build in "${builds[@]}"; do
    name=${build%%=*} binary=${build#*=}
    start "$binary"
    rich=$(run rich-text)
    health=$(run livez)
    read -r rich_rps rich_p99 <<<"$rich"
    read -r health_rps health_p99 <<<"$health"
    rss_kb=$(ps -o rss= -p "$server" | tr -d ' ')
    stop
    printf 'round %s  %-6s rich-text %8.0f/s p99 %6.2f ms   health %8.0f/s p99 %6.2f ms   rss %s MB\n' \
      "$round" "$name" "$rich_rps" "$rich_p99" "$health_rps" "$health_p99" $((rss_kb / 1024))
    rows=$(jq -c --arg name "$name" --argjson round "$round" \
      --argjson rich_rps "$rich_rps" --argjson rich_p99 "$rich_p99" \
      --argjson health_rps "$health_rps" --argjson health_p99 "$health_p99" --argjson rss_kb "$rss_kb" \
      '. + [{build: $name, round: $round, rich_text: {rps: $rich_rps, p99_ms: $rich_p99},
             health: {rps: $health_rps, p99_ms: $health_p99}, rss_kb_after: $rss_kb}]' <<<"$rows")
  done
done

cpu=$( (grep -m1 'model name' /proc/cpuinfo 2>/dev/null | cut -d: -f2- || sysctl -n machdep.cpu.brand_string) | sed 's/^ *//')
cores=$(getconf _NPROCESSORS_ONLN)
machine=$(printf '%s-%sc' "$cpu" "$cores" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '-' | sed 's/-$//')
stamp=$(date -u +%Y-%m-%dT%H%M%SZ)
mkdir -p "$out"
file="$out/$stamp-allocator-$machine.json"
jq -n --arg stamp "$stamp" --arg machine "$machine" --arg cpu "$cpu" --argjson cores "$cores" \
  --arg commit "$(git rev-parse HEAD 2>/dev/null || echo unknown)" --argjson total "$total" \
  --argjson rows "$rows" '
  def median: sort | if length % 2 == 1 then .[length / 2 | floor] else (.[length / 2 - 1] + .[length / 2]) / 2 end;
  {kind: "allocator", stamp: $stamp, commit: $commit,
   machine: {id: $machine, cpu: $cpu, cores: $cores},
   method: "each build in turn, interleaved; k6 throughput, 64 VUs, \($total) requests per run; no database, rate limits off; k6 on the same machine",
   rounds: $rows,
   medians: ($rows | group_by(.build) | map({key: .[0].build, value: {
     rich_text_rps: (map(.rich_text.rps) | median), rich_text_p99_ms: (map(.rich_text.p99_ms) | median),
     health_rps: (map(.health.rps) | median), health_p99_ms: (map(.health.p99_ms) | median),
     rss_kb_after: (map(.rss_kb_after) | median)}}) | from_entries)}' >"$file"
echo "medians:"
jq -r '.medians | to_entries[] | "  \(.key): rich-text \(.value.rich_text_rps | floor)/s p99 \(.value.rich_text_p99_ms * 100 | floor / 100) ms; health \(.value.health_rps | floor)/s p99 \(.value.health_p99_ms * 100 | floor / 100) ms; rss \(.value.rss_kb_after / 1024 | floor) MB"' "$file"
echo "recorded $file"
