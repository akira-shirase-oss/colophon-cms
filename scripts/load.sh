#!/usr/bin/env bash
# Load-tests a release build of colophon-server against the local stack, keeps the evidence,
# and fails on a threshold miss or a regression (D-018).
#
#   scripts/load.sh <endpoint> throughput            1,000,000 requests, back to back (TOTAL, VUS)
#   scripts/load.sh <endpoint> steady                a fixed rate for 60 s (RATE, DURATION, VUS)
#   scripts/load.sh <endpoint> <scenario> --baseline also save this run as the machine's baseline
#
# <endpoint> is a script in perf/k6/: health (the health checks), rich-text (validation),
# session (signed-in requests: session lookup and row-level security on every request),
# entries (drafts: reads, lists and If-Match saves, each read checked against the last save),
# content-types (the same for entries of a seven-field type, plus reads of its definition),
# cli (requests authenticated by a bearer token, as the CLI sends them) or
# studio (the Studio's static files; builds the Studio first).
#
# Environment overrides: TOTAL, VUS, RATE, DURATION, P99_MS (the k6 latency threshold),
# KEEP=0 (don't write a result file; for CI smoke runs), LOAD_PORT (default 58090).
#
# RATE and P99_MS default per endpoint. health and rich-text never touch the database: 20,000
# requests/s and a 25 ms p99. session waits on database round trips, so its steady rate is
# 2,000/s and its p99 threshold 100 ms (END_GOAL's editorial budget is p99 <= 700 ms); the
# baseline comparison still fails a p99 50% worse than the last accepted run. entries writes
# on a fifth of its requests: 500/s steady and a 250 ms p99 (END_GOAL: draft writes p99 <= 700 ms).
# studio reads a file per request: 5,000/s steady (at 10,000/s, rare 75 ms stalls made k6 drop
# 0.04% of its iterations; the result is kept in perf/results/).
#
# A result file goes to perf/results/ and a line to perf/results/history.jsonl. Numbers from
# a laptop are evidence about the code on that machine, not a production capacity claim;
# every result names its machine.
set -euo pipefail
cd "$(dirname "$0")/.."

endpoint="${1:-}"
scenario="${2:-}"
if [[ ! -f "perf/k6/$endpoint.js" ]] || [[ "$scenario" != throughput && "$scenario" != steady ]]; then
  echo "usage: scripts/load.sh health|rich-text|session|cli|entries|content-types|studio throughput|steady [--baseline]" >&2
  exit 2
fi
save_baseline=0
[[ "${3:-}" == "--baseline" ]] && save_baseline=1

k6=$(command -v k6 || true)
[[ -n "$k6" ]] || k6="$HOME/.local/bin/k6"
[[ -x "$k6" ]] || { echo "error: k6 not found; run scripts/install-k6.sh" >&2; exit 1; }
for tool in cargo docker jq curl; do
  command -v "$tool" >/dev/null || { echo "error: $tool is required" >&2; exit 1; }
done

port="${LOAD_PORT:-58090}"
# The open model needs its virtual users allocated up front: creating them mid-run makes k6
# miss its own schedule (a steady run on 2026-10-25 dropped 271 of 1.2 M iterations at 64).
if [[ "$scenario" == "steady" ]]; then default_vus=256; else default_vus=64; fi
VUS="${VUS:-$default_vus}"
case "$endpoint" in
  session | cli) RATE="${RATE:-2000}" P99_MS="${P99_MS:-100}" ;;
  entries | content-types) RATE="${RATE:-500}" P99_MS="${P99_MS:-250}" ;;
  studio) RATE="${RATE:-5000}" P99_MS="${P99_MS:-25}" ;;
  *) RATE="${RATE:-20000}" P99_MS="${P99_MS:-25}" ;;
esac
base_url="http://127.0.0.1:$port"
work=$(mktemp -d)
server_pid=""
# shellcheck disable=SC2329 # invoked by the EXIT trap below
cleanup() {
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
    kill -TERM "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  rm -rf "$work"
}
trap cleanup EXIT

scripts/check-toolchain.sh >/dev/null
scripts/migrate.sh
echo "building the release binaries"
cargo build --quiet --release --locked -p colophon-server -p colophon-cli
studio_dir=""
if [[ "$endpoint" == "studio" ]]; then
  echo "building the Studio"
  npm run build --silent -w @colophon/studio >/dev/null
  studio_dir="$PWD/apps/studio/build"
fi

# The load-test server: the local stack's settings, its own port, and warnings-only logging,
# as production would run (per-request lines are DEBUG).
(
  set -a
  # shellcheck source=/dev/null # plain KEY=value lines
  . deploy/local.env
  set +a
  # The public URL is the sign-in redirect URI registered in the development realm.
  export COLOPHON_LISTEN_ADDR="127.0.0.1:$port" COLOPHON_PUBLIC_URL="http://localhost:$port" COLOPHON_LOG=warn COLOPHON_LOG_FORMAT=json
  if [[ -n "$studio_dir" ]]; then export COLOPHON_STUDIO_DIR="$studio_dir"; fi
  exec target/release/colophon-server
) >"$work/server.log" 2>&1 &
server_pid=$!

ready=0
for _ in $(seq 1 100); do
  if target/release/colophon doctor --server "$base_url" --timeout-ms 500 >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 0.2
done
if (( ready == 0 )); then
  echo "error: the server never became ready (is the stack up? scripts/stack.sh up)" >&2
  target/release/colophon doctor --server "$base_url" >&2 || true
  cat "$work/server.log" >&2
  exit 1
fi

db_transactions() {
  docker compose -f deploy/compose.yaml exec -T postgres psql -U colophon -XAtqc \
    "select xact_commit + xact_rollback from pg_stat_database where datname = 'colophon'"
}
sleep 1.5 # let Postgres publish its statistics
tx_before=$(db_transactions)

echo "running k6: $endpoint $scenario"
k6_status=0
"$k6" run --quiet --no-color \
  --summary-export "$work/summary.json" \
  -e BASE_URL="$base_url" -e PUBLIC_URL="http://localhost:$port" -e SCENARIO="$scenario" \
  -e TOTAL="${TOTAL:-1000000}" -e VUS="$VUS" \
  -e RATE="$RATE" -e DURATION="${DURATION:-60s}" \
  -e P99_MS="$P99_MS" \
  "perf/k6/$endpoint.js" || k6_status=$?

sleep 1.5
tx_after=$(db_transactions)

# The server must still be healthy after the run, and must not have logged any warnings.
target/release/colophon doctor --server "$base_url" >/dev/null || {
  echo "error: the server is not ready after the run" >&2
  k6_status=1
}
server_warnings=$(grep -c '"level":"\(WARN\|ERROR\)"' "$work/server.log" || true)

commit=$(git rev-parse --short HEAD)
dirty=$([[ -n "$(git status --porcelain -- crates Cargo.lock perf/k6)" ]] && echo true || echo false)
if [[ "$(uname -s)" == "Darwin" ]]; then
  cpu=$(sysctl -n machdep.cpu.brand_string)
  cores=$(sysctl -n hw.ncpu)
  memory_gb=$(( $(sysctl -n hw.memsize) / 1024 / 1024 / 1024 ))
else
  cpu=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//')
  cores=$(nproc)
  memory_gb=$(( $(grep MemTotal /proc/meminfo | awk '{print $2}') / 1024 / 1024 ))
fi
machine=$(printf '%s-%sc' "$cpu" "$cores" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '-' | sed 's/-$//')
stamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)

record=$(jq -c \
  --arg scenario "$scenario" --arg endpoint "$endpoint" --arg stamp "$stamp" --arg commit "$commit" --argjson dirty "$dirty" \
  --arg cpu "$cpu" --argjson cores "$cores" --argjson memory_gb "$memory_gb" --arg machine "$machine" \
  --arg os "$(uname -s) $(uname -r)" --arg k6 "$("$k6" version | awk '{print $2}')" \
  --argjson tx "$(( tx_after - tx_before ))" --argjson warnings "$server_warnings" \
  --argjson k6_status "$k6_status" \
  --arg total "${TOTAL:-1000000}" --arg vus "$VUS" --arg rate "$RATE" --arg duration "${DURATION:-60s}" \
  '{
    scenario: $scenario, endpoint: $endpoint, date: $stamp, commit: $commit, dirty: $dirty,
    machine: { id: $machine, cpu: $cpu, cores: $cores, memory_gb: $memory_gb, os: $os },
    k6: $k6,
    params: (if $scenario == "throughput" then { total: ($total | tonumber), vus: ($vus | tonumber) }
             else { rate: ($rate | tonumber), duration: $duration, vus: ($vus | tonumber) } end),
    requests: .metrics.http_reqs.count,
    rps: (.metrics.http_reqs.rate | floor),
    failed: (.metrics.http_req_failed.passes // 0),
    checks_failed: (.metrics.checks.fails // 0),
    dropped: (.metrics.dropped_iterations.count // 0),
    latency_ms: (.metrics.http_req_duration | {
      avg: (.avg * 1000 | round / 1000), p50: (.med * 1000 | round / 1000),
      p90: (."p(90)" * 1000 | round / 1000), p95: (."p(95)" * 1000 | round / 1000),
      p99: (."p(99)" * 1000 | round / 1000), p999: (."p(99.9)" * 1000 | round / 1000),
      max: (.max * 1000 | round / 1000) }),
    database_transactions_during_run: $tx,
    server_warnings: $warnings,
    thresholds_passed: ($k6_status == 0)
  }' "$work/summary.json")

echo "$record" | jq .

if [[ "${KEEP:-1}" == "1" ]]; then
  mkdir -p perf/results perf/baselines
  file="perf/results/${stamp//:/}-$endpoint-$scenario-$machine.json"
  echo "$record" | jq . >"$file"
  echo "$record" >>perf/results/history.jsonl
  echo "saved $file"
fi

status=$k6_status
if (( server_warnings > 0 )); then
  echo "FAIL: the server logged $server_warnings warning(s) or error(s) under load" >&2
  status=1
fi

baseline="perf/baselines/$endpoint-$scenario-$machine.json"
# Only like with like: a 10,000-request smoke spends a visible share of its requests opening
# pool connections and preparing statements, which a million-request run amortizes away, so
# a run with different parameters relies on its absolute thresholds alone.
if [[ -f "$baseline" ]] && ! jq -e --argjson now "$record" '.params == $now.params' "$baseline" >/dev/null; then
  echo "not compared with $baseline: different parameters ($(echo "$record" | jq -c .params) vs $(jq -c .params "$baseline")); absolute thresholds only"
elif [[ -f "$baseline" ]]; then
  # A regression: throughput 25% lower, or p99 50% higher (plus 1 ms, so microsecond noise on
  # a tiny p99 doesn't fail the run). The baseline is per machine; never compare across them.
  verdict=$(jq -n --argjson now "$record" --slurpfile base "$baseline" '
    $base[0] as $b |
    [ (if $now.scenario == "throughput" and $now.rps < ($b.rps * 0.75) then "throughput \($now.rps)/s is below 75% of the baseline \($b.rps)/s" else empty end),
      (if $now.latency_ms.p99 > ($b.latency_ms.p99 * 1.5 + 1) then "p99 \($now.latency_ms.p99) ms is above 1.5x the baseline \($b.latency_ms.p99) ms + 1 ms" else empty end)
    ] | join("; ")')
  verdict=${verdict//\"/}
  if [[ -n "$verdict" ]]; then
    echo "FAIL: regression against $baseline: $verdict" >&2
    status=1
  else
    echo "no regression against $baseline"
  fi
fi
if (( save_baseline == 1 )); then
  if (( status == 0 )); then
    echo "$record" | jq . >"$baseline"
    echo "saved the baseline $baseline"
  else
    echo "not saving a failed run as the baseline" >&2
  fi
fi

if (( status == 0 )); then
  echo "load test passed: $endpoint $scenario, $(echo "$record" | jq -r '"\(.requests) requests, \(.rps)/s, p99 \(.latency_ms.p99) ms, \(.failed) failed, \(.database_transactions_during_run) database transactions"')"
fi
exit "$status"
