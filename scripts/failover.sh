#!/usr/bin/env bash
# The failover proof (milestone S, D-075): under write load through the balancer, take one server
# away and bring it back, then check in the database that every acknowledged write exists exactly
# once and nothing else was written.
#
#   scripts/failover.sh kill    SIGKILL server 2 ten seconds in: a crash
#   scripts/failover.sh drain   SIGTERM server 2 ten seconds in: a drain, which must cost no
#                               request even on its first try
#
# Server 2 starts again five seconds later. Needs the cluster with at least two servers
# (scripts/cluster.sh up) and k6. Environment: RATE (logical writes a second, default 200),
# DURATION (default 30s), KEEP=0 (don't write a result file). Results go to perf/results/ like
# scripts/load.sh's, and say what was measured on which machine; they are not capacity claims.
set -euo pipefail
cd "$(dirname "$0")/.."

mode="${1:-}"
if [[ "$mode" != kill && "$mode" != drain ]]; then
  sed -n '6,8p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
fi
k6=$(command -v k6 || true)
[[ -n "$k6" ]] || k6="$HOME/.local/bin/k6"
[[ -x "$k6" ]] || { echo "error: k6 not found; run scripts/install-k6.sh" >&2; exit 1; }

cluster_dir="target/cluster"
servers=$(cat "$cluster_dir/servers" 2>/dev/null || echo 0)
if (( servers < 2 )) || ! target/release/colophon doctor --server http://127.0.0.1:58405 --timeout-ms 1000 >/dev/null 2>&1; then
  echo "error: needs the cluster with at least two servers (scripts/cluster.sh up)" >&2
  exit 1
fi
workers=$(find "$cluster_dir" -name 'worker-*.pid' | wc -l | tr -d ' ')
victim=2
rate="${RATE:-200}"
duration="${DURATION:-30s}"
database="${COLOPHON_TEST_DATABASE:-colophon}"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

echo "failover ($mode): $rate writes/s for $duration through $servers servers; server $victim goes at 10 s, returns at 15 s"
"$k6" run --quiet --no-color --summary-export "$work/summary.json" \
  -e BASE_URL=http://127.0.0.1:58405 -e PUBLIC_URL=http://localhost:58405 \
  -e MODE="$mode" -e RATE="$rate" -e DURATION="$duration" \
  perf/k6/failover.js >"$work/k6.log" 2>&1 &
k6_pid=$!
sleep 10
if [[ "$mode" == kill ]]; then
  scripts/cluster.sh kill "$victim"
else
  scripts/cluster.sh stop "$victim"
fi
sleep 5
scripts/cluster.sh start "$victim"
k6_status=0
wait "$k6_pid" || k6_status=$?

project=$(grep -o 'failover-project [0-9a-f-]*' "$work/k6.log" | head -1 | cut -d' ' -f2)
if [[ -z "$project" ]]; then
  echo "error: k6 didn't set up a project" >&2
  cat "$work/k6.log" >&2
  exit 1
fi
read -r entries distinct < <(docker compose -f deploy/compose.yaml exec -T postgres psql -U colophon -d "$database" -XAtqc \
  "select count(*), count(distinct draft_title) from entries where project_id = '$project' and collection = 'articles'" | tr '|' ' ')
metric() { jq -r --arg name "$1" '.metrics[$name].count // 0' "$work/summary.json"; }
acknowledged=$(metric acknowledged)
first_failures=$(metric first_attempt_failures)
retried=$(metric retried)
logical_failures=$(metric logical_failures)
requests=$(jq -r '.metrics.http_reqs.count' "$work/summary.json")

verdict=pass
reasons=()
(( k6_status == 0 )) || { verdict=fail; reasons+=("k6 thresholds failed (status $k6_status)"); }
(( entries == acknowledged )) || { verdict=fail; reasons+=("$entries articles stored for $acknowledged acknowledged creates"); }
(( distinct == entries )) || { verdict=fail; reasons+=("$(( entries - distinct )) articles duplicated"); }

if [[ "$(uname -s)" == "Darwin" ]]; then
  cpu=$(sysctl -n machdep.cpu.brand_string)
  cores=$(sysctl -n hw.ncpu)
else
  cpu=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//')
  cores=$(nproc)
fi
machine=$(printf '%s-%sc' "$cpu" "$cores" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '-' | sed 's/-$//')
stamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)
record=$(jq -cn \
  --arg mode "$mode" --arg stamp "$stamp" --arg commit "$(git rev-parse --short HEAD)" --arg machine "$machine" \
  --arg cpu "$cpu" --argjson cores "$cores" --argjson servers "$servers" --argjson workers "$workers" \
  --argjson rate "$rate" --arg duration "$duration" --argjson requests "$requests" \
  --argjson acknowledged "$acknowledged" --argjson first "$first_failures" --argjson retried "$retried" \
  --argjson logical "$logical_failures" --argjson entries "$entries" --argjson distinct "$distinct" \
  --arg verdict "$verdict" --arg reasons "${reasons[*]:-}" \
  '{scenario: ("failover-" + $mode), date: $stamp, commit: $commit,
    machine: {id: $machine, cpu: $cpu, cores: $cores},
    topology: {servers: $servers, workers: $workers, balancer: "haproxy"},
    timeline: {server: 2, leaves_at_s: 10, returns_at_s: 15, how: (if $mode == "kill" then "SIGKILL" else "SIGTERM (drain)" end)},
    params: {rate: $rate, duration: $duration},
    requests: $requests, acknowledged: $acknowledged, first_attempt_failures: $first,
    retried: $retried, logical_failures: $logical,
    stored: {articles: $entries, distinct_titles: $distinct},
    verdict: $verdict, reasons: $reasons}')
echo "$record" | jq .
if [[ "${KEEP:-1}" == "1" ]]; then
  file="perf/results/${stamp//:/}-failover-$mode-$machine-cluster${servers}s${workers}w.json"
  echo "$record" | jq . >"$file"
  echo "$record" >>perf/results/history.jsonl
  echo "saved $file"
fi
[[ "$verdict" == pass ]]
