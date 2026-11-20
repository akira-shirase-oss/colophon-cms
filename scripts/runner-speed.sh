#!/usr/bin/env bash
# This runner's speed, for D-221's scaling: three short back-to-back health runs, and the median of their req/s, so
# one noisy run can't set the scale. Prints `<median req/s>\t<cpu>` (tab-separated) on stdout; the runs' own output
# goes to stderr.
#
#   scripts/runner-speed.sh            (needs the stack and k6, as scripts/load.sh does)
set -euo pipefail
cd "$(dirname "$0")/.."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for n in 1 2 3; do
  KEEP=0 TOTAL=40000 P99_MS=1000 RECORD_OUT="$tmp/health-$n.json" scripts/load.sh health throughput >&2 || true
done
rps=$(for n in 1 2 3; do jq -r '.rps // empty' "$tmp/health-$n.json" 2>/dev/null || true; done | sort -n | awk '{v[NR] = $1} END {if (NR) print v[int((NR + 1) / 2)]}')
cpu=$(jq -r '.machine.cpu // "unknown"' "$tmp/health-1.json" 2>/dev/null || echo unknown)
printf '%s\t%s\n' "${rps:-}" "$cpu"
