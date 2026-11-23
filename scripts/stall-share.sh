#!/usr/bin/env bash
# The worst disk-stall share of any second in a window, from a job's /proc/pressure/io samples (pressure.txt, one a
# second): the share of that second all tasks were stalled on IO, from the change in `full total`, as
# scripts/stall-signature.sh judges a failure second. Prints a percentage (0 when the window has no samples).
#
#   scripts/stall-share.sh <pressure.txt> <start epoch> <end epoch>
set -euo pipefail
pressure=$1 start=$2 end=$3
awk '{
  if (!match($0, /\| io: [^|]*full avg10=[0-9.]+[^|]*total=[0-9]+/)) next
  io = substr($0, RSTART, RLENGTH); total = io; sub(/.*total=/, "", total)
  printf "{\"t\":\"%s\",\"total\":%s}\n", $1, total
}' "$pressure" 2>/dev/null | jq -rs --argjson s "$start" --argjson e "$end" '
  map(. + {t: (.t[0:19] + "Z" | fromdateiso8601)})
  | [range(1; length) as $i | .[$i - 1] as $a | .[$i] as $b | select($b.t > $a.t and $b.t >= $s and $b.t <= $e)
     | (($b.total - $a.total) / (($b.t - $a.t) * 10000))]
  | (max // 0) * 100 | round / 100' 2>/dev/null || echo 0
