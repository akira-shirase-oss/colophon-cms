#!/usr/bin/env bash
# This runner's speed, for D-221's and D-223's scaling: three short back-to-back runs and the median of their req/s,
# so one noisy run can't set the scale.
#
#   scripts/runner-speed.sh            health: prints `<median req/s>\t<cpu>` (D-221)
#   scripts/runner-speed.sh write      the write probe (D-223): entries throughput, saves with If-Match and reads,
#                                      each run in a project of its own (entries' setup creates one), so later smokes'
#                                      counts, caches and lock queues start clean. With PRESSURE_FILE, a run whose
#                                      window has a second the disk stalled for 20% or more (scripts/stall-share.sh,
#                                      the stall signature's bar) is discarded. Prints `<median req/s>\t<source>\t<kept>
#                                      \t<discarded>`, source `probe`, or `fallback` (and no req/s) when no run counted.
#
# Needs the stack and k6, as scripts/load.sh does (LOAD_SH names another, for tests). The runs' own output goes to stderr.
set -euo pipefail
cd "$(dirname "$0")/.."
mode=${1:-health}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
# The true median: the middle value, or the mean of the middle two when a run was lost or discarded (taking the
# lower of two would read the runner slower than it measured, and loosen the limits scaled by it).
median() { sort -n | awk '{v[NR] = $1} END {if (NR % 2) print v[(NR + 1) / 2]; else if (NR) printf "%d\n", (v[NR / 2] + v[NR / 2 + 1]) / 2}'; }
if [[ $mode == write ]]; then
  kept=0 discarded=0
  : >"$tmp/rps"
  for n in 1 2 3; do
    start=$(date -u +%s)
    KEEP=0 TOTAL=4000 P99_MS=60000 RECORD_OUT="$tmp/write-$n.json" "${LOAD_SH:-scripts/load.sh}" entries throughput >&2 || true
    end=$(date -u +%s)
    rps=$(jq -r '.rps // empty' "$tmp/write-$n.json" 2>/dev/null || true)
    [[ -n $rps ]] || { discarded=$(( discarded + 1 )); echo "write probe run $n: no result" >&2; continue; }
    if [[ -n ${PRESSURE_FILE:-} && -f $PRESSURE_FILE ]]; then
      share=$(scripts/stall-share.sh "$PRESSURE_FILE" "$start" "$end")
      if awk -v s="$share" 'BEGIN {exit !(s >= 20)}'; then
        discarded=$(( discarded + 1 )); echo "write probe run $n: discarded, the disk stalled ${share}% of a second" >&2; continue
      fi
    fi
    echo "$rps" >>"$tmp/rps"; kept=$(( kept + 1 ))
  done
  rps=$(median <"$tmp/rps")
  if [[ -n $rps ]]; then source=probe; else source=fallback; fi
  printf '%s\t%s\t%s\t%s\n' "${rps:-}" "$source" "$kept" "$discarded"
  exit 0
fi
for n in 1 2 3; do
  KEEP=0 TOTAL=40000 P99_MS=1000 RECORD_OUT="$tmp/health-$n.json" scripts/load.sh health throughput >&2 || true
done
rps=$(for n in 1 2 3; do jq -r '.rps // empty' "$tmp/health-$n.json" 2>/dev/null || true; done | median)
cpu=$(jq -r '.machine.cpu // "unknown"' "$tmp/health-1.json" 2>/dev/null || echo unknown)
printf '%s\t%s\n' "${rps:-}" "$cpu"
