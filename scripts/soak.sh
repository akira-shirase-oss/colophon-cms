#!/usr/bin/env bash
# Memory/leak soak for a release build of colophon-server against the local stack (PERF-B,
# milestone S): holds a steady request rate for minutes while sampling the server's RSS and
# file-descriptor count, then fits the post-warmup samples and fails if memory still grows.
#
#   scripts/soak.sh <endpoint> [minutes]
#
# endpoint        any perf/k6 scenario (default entries — drafts, validation, audit writes)
# minutes         soak length (default 10; P6's 24-hour stress is the long gate)
#
# Knobs: RATE (default half of load.sh's steady rate — sustained pressure, not saturation),
# VUS, SAMPLE_SECONDS (default 10), WARMUP_SECONDS (default 60 — allocator and caches settle
# before the slope is measured), MAX_GROWTH_PCT (default 10 — median RSS of the last third of
# the post-warmup window vs the first third), MAX_SLOPE_KB_PER_MIN (default 512 — least-squares
# slope over the same window), LOAD_PORT, SERVER_LOG.
#
# Externally a task or connection leak can only show up as memory or descriptors, so those are
# the two signals sampled; a leaked task that holds nothing is out of scope for a black-box
# soak. The 24-hour run in P6 is the gate this script rehearses.
set -euo pipefail
cd "$(dirname "$0")/.."

endpoint="${1:-entries}"
minutes="${2:-10}"
if [[ ! -f "perf/k6/$endpoint.js" ]]; then
  echo "usage: scripts/soak.sh <endpoint> [minutes] — perf/k6/$endpoint.js does not exist" >&2
  exit 64
fi
if ! [[ "$minutes" =~ ^[0-9]+$ ]] || (( minutes < 2 )); then
  echo "error: minutes must be an integer >= 2 (the window needs room past warmup)" >&2
  exit 64
fi

# Sustained-pressure defaults: half of load.sh's steady rate for the endpoints a soak is
# meaningful on (a leak shows under pressure held over time, not at the saturation knee).
case "$endpoint" in
  entries|changesets|stages|collaboration|migrations|deletion|inverses|transfer|locales) RATE="${RATE:-500}" ;;
  session|accounts|cli|members|roles|tokens|projects|setup|schedules|collection-delete|modelling) RATE="${RATE:-500}" ;;
  delivery|site|site-starter) RATE="${RATE:-1000}" ;;
  publishing) RATE="${RATE:-20}" ;;
  bulk) RATE="${RATE:-125}" ;;
  assets) RATE="${RATE:-125}" ;;
  graphql) RATE="${RATE:-250}" ;;
  openapi|studio) RATE="${RATE:-2500}" ;;
  live) RATE="${RATE:-100}" ;;
  rich-text) RATE="${RATE:-1000}" ;;
  *) RATE="${RATE:-500}" ;;
esac
VUS="${VUS:-128}"
SAMPLE_SECONDS="${SAMPLE_SECONDS:-10}"
WARMUP_SECONDS="${WARMUP_SECONDS:-60}"
MAX_GROWTH_PCT="${MAX_GROWTH_PCT:-10}"
MAX_SLOPE_KB_PER_MIN="${MAX_SLOPE_KB_PER_MIN:-512}"
# Registered with the Keycloak realm (deploy/keycloak/colophon-realm.json): the browser
# sign-in flow the k6 setup drives needs the callback whitelisted — an unregistered port
# gets an error page, not a login form. 58090 is load.sh's, 58091/58092 the other harnesses'.
port="${LOAD_PORT:-58093}"
base_url="http://127.0.0.1:$port"
public_url="http://localhost:$port"

k6=$(command -v k6 || true)
[[ -n "$k6" ]] || k6="$HOME/.local/bin/k6"
[[ -x "$k6" ]] || { echo "error: k6 not found; run scripts/install-k6.sh" >&2; exit 1; }
for tool in cargo jq curl lsof; do
  command -v "$tool" >/dev/null || { echo "error: $tool is required" >&2; exit 1; }
done

work=$(mktemp -d)
server_pid=""
k6_pid=""
# shellcheck disable=SC2329 # invoked by the EXIT trap below
cleanup() {
  if [[ -n "$k6_pid" ]] && kill -0 "$k6_pid" 2>/dev/null; then kill "$k6_pid" 2>/dev/null || true; fi
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then kill -TERM "$server_pid" 2>/dev/null || true; fi
  wait 2>/dev/null || true
  if [[ -n "${SERVER_LOG:-}" && -f "$work/server.log" ]]; then cp "$work/server.log" "$SERVER_LOG"; fi
}
trap cleanup EXIT

PATH="$(brew --prefix rustup 2>/dev/null || echo /nonexistent)/bin:$HOME/.local/bin:$PATH"
export PATH
./scripts/check-toolchain.sh >/dev/null
cargo build --quiet --release --locked -p colophon-server -p colophon-cli

# The soak server: identical spawn to scripts/load.sh's — the stack's settings, its own port,
# warnings-only logging, production file limit, rate limits that never refuse but still run.
(
  # shellcheck source=scripts/local-env.sh
  . scripts/local-env.sh
  export COLOPHON_LISTEN_ADDR="127.0.0.1:$port" COLOPHON_PUBLIC_URL="http://localhost:$port" COLOPHON_LOG=warn COLOPHON_LOG_FORMAT=json
  ulimit -n "${COLOPHON_LOAD_MAXFILES:-16384}" 2>/dev/null || true
  export COLOPHON_RATE_LIMIT_PER_ADDRESS="${COLOPHON_RATE_LIMIT_PER_ADDRESS:-1000000/1000000}" \
    COLOPHON_RATE_LIMIT_PER_CREDENTIAL="${COLOPHON_RATE_LIMIT_PER_CREDENTIAL:-1000000/1000000}" \
    COLOPHON_RATE_LIMIT_DELIVERY="${COLOPHON_RATE_LIMIT_DELIVERY:-1000000/1000000}"
  if [[ "$endpoint" == "live" ]]; then export COLOPHON_EVENTS_LIFETIME_MS="${COLOPHON_EVENTS_LIFETIME_MS:-15000}"; fi
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

# The held pressure: one steady run for the whole soak. Its p99 threshold is lenient — the
# gate here is the server's shape over time, not latency.
"$k6" run --quiet --no-color --summary-export "$work/k6.json" \
  -e BASE_URL="$base_url" -e PUBLIC_URL="$public_url" -e SCENARIO=steady \
  -e VUS="$VUS" -e RATE="$RATE" -e DURATION="${minutes}m" -e P99_MS="${P99_MS:-5000}" \
  -e RECEIVER_URL="http://127.0.0.1:58096" -e SITE_URL="http://127.0.0.1:58097" \
  "perf/k6/$endpoint.js" >"$work/k6.log" 2>&1 &
k6_pid=$!

samples="$work/samples.jsonl"
: >"$samples"
echo "soaking $endpoint at $RATE/s for ${minutes}m, sampling every ${SAMPLE_SECONDS}s (warmup ${WARMUP_SECONDS}s)"
t=0
end=$(( minutes * 60 ))
while (( t <= end )); do
  if ! kill -0 "$server_pid" 2>/dev/null; then
    echo "error: the server died $(( t ))s into the soak" >&2
    cat "$work/server.log" >&2
    exit 1
  fi
  rss=$(ps -o rss= -p "$server_pid" 2>/dev/null | tr -d ' ' || echo 0)
  fds=$(lsof -p "$server_pid" 2>/dev/null | tail -n +2 | wc -l | tr -d ' ')
  jq -cn --argjson t "$t" --argjson rss "${rss:-0}" --argjson fds "${fds:-0}" \
    '{t_seconds: $t, rss_kb: $rss, fds: $fds}' >>"$samples"
  sleep "$SAMPLE_SECONDS"
  t=$(( t + SAMPLE_SECONDS ))
done
wait "$k6_pid" 2>/dev/null || true
k6_pid=""

server_warnings=$(grep -c '"level":"\(WARN\|ERROR\)"' "$work/server.log" || true)

# The fit: over samples after warmup, the least-squares RSS slope (KB/min) and the growth
# between the first and last thirds' medians. Medians, not endpoints — a single GC-timed
# sample must not decide the gate.
read -r analysis <<<"$(jq -cs --argjson warmup "$WARMUP_SECONDS" '
  def median: sort | if length == 0 then 0 elif length % 2 == 1 then .[length / 2 | floor]
    else ((.[length / 2 - 1] + .[length / 2]) / 2) end;
  ([.[] | select(.t_seconds >= $warmup)] | sort_by(.t_seconds)) as $w
  | ($w | length) as $n
  | ($w | map(.t_seconds / 60)) as $xs | ($w | map(.rss_kb)) as $ys
  | ($xs | add / $n) as $mx | ($ys | add / $n) as $my
  | ([range(0; $n) | ($xs[.] - $mx) * ($ys[.] - $my)] | add) as $num
  | ([range(0; $n) | ($xs[.] - $mx) * ($xs[.] - $mx)] | add) as $den
  | ($w[0:($n / 3)] | map(.rss_kb) | median) as $first
  | ($w[(2 * $n / 3):] | map(.rss_kb) | median) as $last
  | ($w | map(.fds)) as $f
  | {slope_kb_per_min: (if $den == 0 then 0 else $num / $den end),
     first_median_kb: $first, last_median_kb: $last,
     growth_pct: (if $first == 0 then 0 else ($last - $first) * 100 / $first end),
     max_rss_kb: ($ys | max), fds_first: $f[0], fds_last: $f[-1], samples: $n}' "$samples")"

if [[ "$(uname -s)" == "Darwin" ]]; then
  cpu=$(sysctl -n machdep.cpu.brand_string)
  cores=$(sysctl -n hw.ncpu)
else
  cpu=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//')
  cores=$(nproc)
fi
machine=$(printf '%s-%sc' "$cpu" "$cores" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '-' | sed 's/-$//')
stamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)
commit=$(git rev-parse --short HEAD)
dirty=$(git status --porcelain | grep -c . || true)

verdict=$(jq -n --argjson a "$analysis" --argjson maxpct "$MAX_GROWTH_PCT" --argjson maxslope "$MAX_SLOPE_KB_PER_MIN" '
  if $a.samples < 6 then "inconclusive"
  elif $a.growth_pct > $maxpct or $a.slope_kb_per_min > $maxslope then "leak-suspect"
  else "flat" end')

jq -n --argjson a "$analysis" --arg verdict "$verdict" --arg endpoint "$endpoint" \
  --arg stamp "$stamp" --arg commit "$commit" --argjson dirty "$dirty" --arg machine "$machine" \
  --argjson minutes "$minutes" --argjson rate "$RATE" --argjson vus "$VUS" --argjson warmup "$WARMUP_SECONDS" \
  --argjson maxpct "$MAX_GROWTH_PCT" --argjson maxslope "$MAX_SLOPE_KB_PER_MIN" \
  --argjson warnings "$server_warnings" --slurpfile k6 "$work/k6.json" --slurpfile s "$samples" '
  {scenario: "soak", endpoint: $endpoint, date: $stamp, commit: $commit, dirty: $dirty, machine: $machine,
   duration_minutes: $minutes, rate: $rate, vus: $vus, warmup_seconds: $warmup,
   budgets: {max_growth_pct: $maxpct, max_slope_kb_per_min: $maxslope},
   verdict: $verdict, analysis: $a,
   requests: ($k6[0].metrics.http_reqs.count // 0), failed: ($k6[0].metrics.http_req_failed.passes // 0),
   server_warnings: $warnings, samples: $s}' \
  >"$work/result.json"
jq -r '"\(.endpoint) soak: \(.requests) req over \(.duration_minutes)m, RSS \(.analysis.first_median_kb|floor)->\(.analysis.last_median_kb|floor) KB (\(.analysis.growth_pct*100|round/100)%), slope \(.analysis.slope_kb_per_min*100|round/100) KB/min, fds \(.analysis.fds_first)->\(.analysis.fds_last), verdict \(.verdict)"' "$work/result.json"

requests=$(jq -r '.requests' "$work/result.json")
expected=$(( RATE * minutes * 60 ))
# A soak that held no pressure proves nothing — half the expected count is already generous
# (a scenario's setup work costs the first seconds), and a setup that never signs on sends none.
if (( requests < expected / 2 )); then
  echo "FAIL: only $requests of ~$expected expected requests arrived — the load never landed" >&2
  tail -20 "$work/k6.log" >&2
  exit 1
fi
if (( server_warnings > 0 )); then
  echo "FAIL: the server logged $server_warnings warning(s) or error(s) under load" >&2
  exit 1
fi

# The gates held — only then is it evidence worth keeping.
file="perf/results/${stamp//:/}-$endpoint-soak-$machine.json"
cp "$work/result.json" "$file"
echo "result: $file"
if [[ "$verdict" == "leak-suspect" ]]; then
  echo "FAIL: memory kept growing after warmup — investigate before calling it a leak or flat" >&2
  exit 1
fi
if [[ "$verdict" == "inconclusive" ]]; then
  echo "FAIL: too few post-warmup samples — lengthen the soak or shorten the warmup" >&2
  exit 1
fi
echo "PASS: no unbounded memory or descriptor growth over the soak"
