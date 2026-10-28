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
# modelling (pages of objects, lists and blocks whose quotes reference people: saves, refused
# references, reads, lists and "referenced by" reads),
# cli (requests authenticated by a bearer token, as the CLI sends them), members (member lists,
# invitation lookups, role changes and new invitations), roles (requests by a member holding a
# custom role, the caller's permissions, role lists and versioned role edits), tokens (requests
# with a read-only and a field-narrowed API token, audit log pages, and tokens made and revoked),
# schedules (schedule lists, and scheduling and cancelling a day ahead), projects (who may create
# projects: refused creations and /me), setup (setup codes refused on a set-up installation, and
# /me), changesets (reading, listing, reviewing and creating changesets),
# delivery (anonymous /content/v1 reads, latest and pinned), publishing (whole publishes, five
# requests each, with the worker announcing every release to a local receiver; needs node),
# studio (the Studio's static files; builds the Studio first) or site (the reference site's pages,
# served by its runner while a release is published every two seconds; needs node).
#
# Environment overrides: TOTAL, VUS, RATE, DURATION, P99_MS (the k6 latency threshold),
# KEEP=0 (don't write a result file; for CI smoke runs), LOAD_PORT (default 58090),
# SERVER_LOG=<path> (keep the server's log there); for stress, START_RATE, STEP_SECONDS and
# MAX_STEPS (below); CLUSTER=1 (run against the servers and
# workers scripts/cluster.sh started, through its balancer, instead of one server of its own;
# the result records how many, and is compared only with runs of the same shape: D-075).
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
if [[ ! -f "perf/k6/$endpoint.js" ]] || [[ "$scenario" != throughput && "$scenario" != steady && "$scenario" != stress ]]; then
  echo "usage: scripts/load.sh health|rich-text|session|cli|members|roles|tokens|schedules|projects|setup|changesets|delivery|publishing|entries|content-types|modelling|studio|site throughput|steady|stress [--baseline]" >&2
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
if [[ "$scenario" == "steady" || "$scenario" == "stress" ]]; then default_vus=256; else default_vus=64; fi
VUS="${VUS:-$default_vus}"
case "$endpoint" in
  session | projects | setup | delivery | site) RATE="${RATE:-2000}" P99_MS="${P99_MS:-100}" ;;
  # Half its requests list entries (a content type lookup too): about 2,000/s at most here, so
  # its steady rate is half that (a run at 2,000/s fell behind; the result is kept).
  cli | members | roles | tokens | schedules | changesets) RATE="${RATE:-1000}" P99_MS="${P99_MS:-100}" ;;
  entries | content-types | modelling) RATE="${RATE:-500}" P99_MS="${P99_MS:-250}" ;;
  studio) RATE="${RATE:-5000}" P99_MS="${P99_MS:-25}" ;;
  # RATE counts publishes here, five requests each, and every request writes and commits: on the
  # reference laptop that tops out around 500 to 700 requests a second, so the steady rate is 40
  # publishes (200 requests) a second; a run at 100 fell behind (the result is kept). The p99
  # threshold is END_GOAL's editorial write budget, 700 ms.
  publishing) RATE="${RATE:-40}" P99_MS="${P99_MS:-700}" ;;
  *) RATE="${RATE:-20000}" P99_MS="${P99_MS:-25}" ;;
esac
base_url="http://127.0.0.1:$port"
public_url="http://localhost:$port"
cluster="${CLUSTER:-0}"
cluster_dir="$PWD/target/cluster"
topology='{"servers": 1, "workers": 0, "balancer": null}'
if [[ "$cluster" == "1" ]]; then
  base_url="http://127.0.0.1:58405"
  public_url="http://localhost:58405"
  servers=$(cat "$cluster_dir/servers" 2>/dev/null || echo 0)
  workers=$(find "$cluster_dir" -name 'worker-*.pid' 2>/dev/null | wc -l | tr -d ' ')
  topology=$(jq -cn --argjson servers "$servers" --argjson workers "$workers" '{servers: $servers, workers: $workers, balancer: "haproxy"}')
fi
work=$(mktemp -d)
server_pid=""
receiver_pid=""
site_pid=""
site_port="${SITE_PORT:-58097}"
receiver_port="${RECEIVER_PORT:-58096}"
# shellcheck disable=SC2329 # invoked by the EXIT trap below
cleanup() {
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
    kill -TERM "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  if [[ -n "$receiver_pid" ]]; then
    kill "$receiver_pid" 2>/dev/null || true
    wait "$receiver_pid" 2>/dev/null || true
  fi
  if [[ -n "$site_pid" ]]; then
    kill "$site_pid" 2>/dev/null || true
    wait "$site_pid" 2>/dev/null || true
  fi
  if [[ -n "${SERVER_LOG:-}" && -f "$work/server.log" ]]; then cp "$work/server.log" "$SERVER_LOG"; fi
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

# The publishing scenario's webhook destination.
if [[ "$endpoint" == "publishing" ]]; then
  command -v node >/dev/null || { echo "error: the publishing scenario needs node" >&2; exit 1; }
  node perf/receiver/receiver.mjs "$receiver_port" >"$work/receiver.log" 2>&1 &
  receiver_pid=$!
fi

# The site scenario's runner: builds each release it hears about and serves it (apps/web). It
# adopts the project the scenario creates from the first announcement.
if [[ "$endpoint" == "site" ]]; then
  command -v node >/dev/null || { echo "error: the site scenario needs node" >&2; exit 1; }
  SITE_API="$base_url" SITE_ROOT="$work/site" SITE_LISTEN="127.0.0.1:$site_port" \
    node apps/web/src/runner.ts >"$work/site.log" 2>&1 &
  site_pid=$!
fi

# How much each cluster process had logged before the run: only what the run adds counts.
cluster_log_sizes() {
  for file in "$cluster_dir"/*.log; do
    [[ -e "$file" ]] && printf '%s %s\n' "$file" "$(wc -c <"$file" | tr -d ' ')"
  done
}
if [[ "$cluster" == "1" ]]; then
  if ! target/release/colophon doctor --server "$base_url" --timeout-ms 1000 >/dev/null 2>&1; then
    echo "error: CLUSTER=1, but the balancer isn't ready (scripts/cluster.sh up)" >&2
    exit 1
  fi
  cluster_log_sizes >"$work/log-sizes"
else
  # The load-test server: the local stack's settings, its own port, and warnings-only logging,
  # as production would run (per-request lines are DEBUG).
  (
    # shellcheck source=scripts/local-env.sh
    . scripts/local-env.sh
    # The public URL is the sign-in redirect URI registered in the development realm.
    export COLOPHON_LISTEN_ADDR="127.0.0.1:$port" COLOPHON_PUBLIC_URL="http://localhost:$port" COLOPHON_LOG=warn COLOPHON_LOG_FORMAT=json
    # One machine sends every request: rate limits (D-076) that never refuse, so each request
    # still pays for its check and the run measures it.
    export COLOPHON_RATE_LIMIT_PER_ADDRESS="${COLOPHON_RATE_LIMIT_PER_ADDRESS:-1000000/1000000}" \
      COLOPHON_RATE_LIMIT_PER_CREDENTIAL="${COLOPHON_RATE_LIMIT_PER_CREDENTIAL:-1000000/1000000}" \
      COLOPHON_RATE_LIMIT_DELIVERY="${COLOPHON_RATE_LIMIT_DELIVERY:-1000000/1000000}"
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
fi

# The database the server uses: `colophon`, or COLOPHON_TEST_DATABASE's (scripts/local-env.sh
# checked it is a plain name).
database="${COLOPHON_TEST_DATABASE:-colophon}"
db_transactions() {
  docker compose -f deploy/compose.yaml exec -T postgres psql -U colophon -XAtqc \
    "select xact_commit + xact_rollback from pg_stat_database where datname = '$database'"
}
sleep 1.5 # let Postgres publish its statistics
tx_before=$(db_transactions)

# Stress (milestone S, D-077): the steady scenario at rising rates on one server (or the
# cluster), until a step breaks its own thresholds. Each step runs STEP_SECONDS (default 20) at
# a rate 25% above the last, from START_RATE (default a quarter of the endpoint's RATE), at most
# MAX_STEPS (default 15). The knee is the highest rate that held; the failure mode is how the
# first step that didn't hold broke: errors, falling behind (k6 couldn't start iterations on
# time because answers were too slow), or latency over P99_MS. A measurement, not a pass/fail
# gate: it exits 0 unless the server is unhealthy afterwards.
if [[ "$scenario" == "stress" ]]; then
  step_seconds="${STEP_SECONDS:-20}"
  rate="${START_RATE:-$(( RATE / 4 ))}"
  (( rate >= 10 )) || rate=10
  max_steps="${MAX_STEPS:-15}"
  steps='[]'
  knee=0
  failure="none within $max_steps steps"
  echo "stressing $endpoint from $rate/s, ${step_seconds} s a step, p99 gate $P99_MS ms"
  for _ in $(seq 1 "$max_steps"); do
    load_average=$(uptime | sed 's/.*averages*: //' | awk '{print $1}' | tr -d ,)
    step_status=0
    "$k6" run --quiet --no-color --summary-export "$work/step.json" \
      -e BASE_URL="$base_url" -e PUBLIC_URL="$public_url" -e SCENARIO=steady \
      -e VUS="$VUS" -e RATE="$rate" -e DURATION="${step_seconds}s" \
      -e P99_MS="$P99_MS" -e RECEIVER_URL="http://127.0.0.1:$receiver_port" -e SITE_URL="http://127.0.0.1:$site_port" \
      "perf/k6/$endpoint.js" >"$work/step.log" 2>&1 || step_status=$?
    entry=$(jq -c --argjson rate "$rate" --argjson status "$step_status" --arg load "$load_average" '{
      rate: $rate, rps: (.metrics.http_reqs.rate | floor),
      p99_ms: (.metrics.http_req_duration."p(99)" * 1000 | round / 1000),
      failed: (.metrics.http_req_failed.passes // 0), checks_failed: (.metrics.checks.fails // 0),
      dropped: (.metrics.dropped_iterations.count // 0), held: ($status == 0), load_average: ($load | tonumber? // null)
    }' "$work/step.json")
    steps=$(jq -c --argjson entry "$entry" '. + [$entry]' <<<"$steps")
    echo "  $(jq -r '"\(.rate)/s: \(.rps) requests/s, p99 \(.p99_ms) ms, \(.failed) failed, \(.dropped) dropped: " + (if .held then "held" else "broke" end)' <<<"$entry")"
    if (( step_status != 0 )); then
      failure=$(jq -r --argjson gate "$P99_MS" '
        if .failed > 0 or .checks_failed > 0 then "errors"
        elif .dropped > 0 then "fell behind"
        elif .p99_ms > $gate then "latency"
        else "thresholds" end' <<<"$entry")
      break
    fi
    knee=$rate
    rate=$(( rate * 5 / 4 ))
  done
  healthy=true
  target/release/colophon doctor --server "$base_url" >/dev/null || healthy=false
  if [[ "$(uname -s)" == "Darwin" ]]; then
    cpu=$(sysctl -n machdep.cpu.brand_string)
    cores=$(sysctl -n hw.ncpu)
  else
    cpu=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//')
    cores=$(nproc)
  fi
  machine=$(printf '%s-%sc' "$cpu" "$cores" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '-' | sed 's/-$//')
  shape=""
  [[ "$cluster" == "1" ]] && shape="-cluster$(echo "$topology" | jq -r '"\(.servers)s\(.workers)w"')"
  stamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  record=$(jq -cn --arg endpoint "$endpoint" --arg stamp "$stamp" --arg commit "$(git rev-parse --short HEAD)" \
    --argjson dirty "$([[ -n "$(git status --porcelain -- crates Cargo.lock perf/k6)" ]] && echo true || echo false)" \
    --arg machine "$machine" --arg cpu "$cpu" --argjson cores "$cores" --argjson topology "$topology" \
    --argjson gate "$P99_MS" --argjson step_seconds "$step_seconds" --argjson steps "$steps" \
    --argjson knee "$knee" --arg failure "$failure" --argjson healthy "$healthy" \
    '{scenario: "stress", endpoint: $endpoint, date: $stamp, commit: $commit, dirty: $dirty,
      machine: {id: $machine, cpu: $cpu, cores: $cores}, topology: $topology,
      p99_gate_ms: $gate, step_seconds: $step_seconds, knee_rate: $knee, failure_mode: $failure,
      healthy_after: $healthy, steps: $steps}')
  echo "$record" | jq '{endpoint, knee_rate, failure_mode, healthy_after, topology}'
  if [[ "${KEEP:-1}" == "1" ]]; then
    mkdir -p perf/results
    file="perf/results/${stamp//:/}-$endpoint-stress-$machine$shape.json"
    echo "$record" | jq . >"$file"
    echo "$record" >>perf/results/history.jsonl
    echo "saved $file"
  fi
  [[ "$healthy" == true ]]
  exit
fi

echo "running k6: $endpoint $scenario"
k6_status=0
"$k6" run --quiet --no-color \
  --summary-export "$work/summary.json" \
  -e BASE_URL="$base_url" -e PUBLIC_URL="$public_url" -e SCENARIO="$scenario" \
  -e TOTAL="${TOTAL:-1000000}" -e VUS="$VUS" \
  -e RATE="$RATE" -e DURATION="${DURATION:-60s}" \
  -e P99_MS="$P99_MS" -e RECEIVER_URL="http://127.0.0.1:$receiver_port" -e SITE_URL="http://127.0.0.1:$site_port" \
  "perf/k6/$endpoint.js" || k6_status=$?

sleep 1.5
tx_after=$(db_transactions)

# The server must still be healthy after the run, and must not have logged any warnings.
target/release/colophon doctor --server "$base_url" >/dev/null || {
  echo "error: the server is not ready after the run" >&2
  k6_status=1
}
if [[ "$cluster" == "1" ]]; then
  server_warnings=0
  while read -r file size; do
    added=$(tail -c +"$(( size + 1 ))" "$file" | grep -c '"level":"\(WARN\|ERROR\)"' || true)
    server_warnings=$(( server_warnings + added ))
  done <"$work/log-sizes"
else
  server_warnings=$(grep -c '"level":"\(WARN\|ERROR\)"' "$work/server.log" || true)
fi

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
# Runs through the balancer are their own series: file names and baselines carry the shape.
shape=""
[[ "$cluster" == "1" ]] && shape="-cluster$(echo "$topology" | jq -r '"\(.servers)s\(.workers)w"')"
stamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)

record=$(jq -c \
  --arg scenario "$scenario" --arg endpoint "$endpoint" --arg stamp "$stamp" --arg commit "$commit" --argjson dirty "$dirty" \
  --arg cpu "$cpu" --argjson cores "$cores" --argjson memory_gb "$memory_gb" --arg machine "$machine" \
  --arg os "$(uname -s) $(uname -r)" --arg k6 "$("$k6" version | awk '{print $2}')" \
  --argjson tx "$(( tx_after - tx_before ))" --argjson warnings "$server_warnings" \
  --argjson k6_status "$k6_status" --argjson topology "$topology" \
  --arg total "${TOTAL:-1000000}" --arg vus "$VUS" --arg rate "$RATE" --arg duration "${DURATION:-60s}" \
  '{
    scenario: $scenario, endpoint: $endpoint, date: $stamp, commit: $commit, dirty: $dirty,
    machine: { id: $machine, cpu: $cpu, cores: $cores, memory_gb: $memory_gb, os: $os },
    topology: $topology,
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
  file="perf/results/${stamp//:/}-$endpoint-$scenario-$machine$shape.json"
  echo "$record" | jq . >"$file"
  echo "$record" >>perf/results/history.jsonl
  echo "saved $file"
fi

status=$k6_status
if (( server_warnings > 0 )); then
  echo "FAIL: the server logged $server_warnings warning(s) or error(s) under load" >&2
  status=1
fi

baseline="perf/baselines/$endpoint-$scenario-$machine$shape.json"
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
