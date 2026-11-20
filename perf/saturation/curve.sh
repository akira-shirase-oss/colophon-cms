#!/usr/bin/env bash
# D-223: one write path's saturation curve against a running server. For each rate: a warm-up at that rate (k6's
# result discarded), then the measured run, and per step the rate asked, the rate achieved, p50/p95/p99, dropped
# iterations, failed requests and the 1-minute load average, labelled with the caller's context.
#
#   perf/saturation/curve.sh <scenario> <base-url> "<rate> <rate> ..." [measured seconds, default 40] [warm-up, default 10]
#
# Labels from the environment, written into every row: CURVE_DELAY_MS (server-Postgres delay each way),
# CURVE_RTT_US (the measured TCP connect to Postgres), CURVE_REPEAT, CURVE_CPU (the machine). Prints a
# tab-separated table with a header; each run's k6 output goes to stderr and its summary to CURVE_OUT/ if set.
# The server must be up with the dev accounts provisioned (`colophon admin account`). Laptop numbers are
# development observations: say the machine and its load beside them.
set -euo pipefail
cd "$(dirname "$0")/../.."
scenario=$1 base=$2 rates=$3 seconds=${4:-40} warmup=${5:-10}
k6=$(command -v k6 || echo "$HOME/.local/bin/k6")
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
run() { # rate, seconds, summary path
  "$k6" run --quiet --no-color --summary-export "$3" \
    -e BASE_URL="$base" -e PUBLIC_URL="$base" -e SCENARIO=steady -e RATE="$1" -e DURATION="${2}s" \
    -e P99_MS=60000 "perf/k6/$scenario.js" >&2 || true
}
printf 'scenario\tdelay_ms\trtt_us\trepeat\tcpu\tasked/s\tachieved/s\tp50_ms\tp95_ms\tp99_ms\tdropped\tfailed\tload\n'
for rate in $rates; do
  (( warmup > 0 )) && run "$rate" "$warmup" "$tmp/warm.json"
  load=$(uptime | sed -E 's/.*load averages?: //; s/,//g' | awk '{print $1}')
  run "$rate" "$seconds" "$tmp/s.json"
  [[ -n ${CURVE_OUT:-} ]] && mkdir -p "$CURVE_OUT" && cp "$tmp/s.json" "$CURVE_OUT/$scenario-d${CURVE_DELAY_MS:-0}-r${CURVE_REPEAT:-1}-$rate.json"
  jq -r --arg s "$scenario" --arg r "$rate" --arg load "$load" \
    --arg d "${CURVE_DELAY_MS:-}" --arg rtt "${CURVE_RTT_US:-}" --arg rep "${CURVE_REPEAT:-}" --arg cpu "${CURVE_CPU:-}" '
    [$s, $d, $rtt, $rep, $cpu, $r,
     (.metrics.http_reqs.rate | floor),
     (.metrics.http_req_duration.med * 1000 | round / 1000),
     (.metrics.http_req_duration."p(95)" * 1000 | round / 1000),
     (.metrics.http_req_duration."p(99)" * 1000 | round / 1000),
     (.metrics.dropped_iterations.count // 0),
     (.metrics.http_req_failed.passes // 0), $load] | @tsv' "$tmp/s.json"
done
