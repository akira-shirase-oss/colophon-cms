#!/usr/bin/env bash
# Whether a smoke's hard failures are a runner IO stall, from evidence (learning-60, 2026-11-20): every second in
# which the server logged a failed request must show the runner's disk stalled (all its tasks stalled on IO for
# 25% or more of that second or one either side, from /proc/pressure/io's `full` total) and a COMMIT queue (of the
# slow statements within five seconds of it, 90% or more are COMMITs, the fsync a stalled disk delays). A failure the server never logged (a
# wrong body, a check that failed on a 200) has no such seconds, so it is never attributed to the runner.
#
#   scripts/stall-signature.sh <server.log> <pressure.txt>
#
# Prints one JSON object: {stall, reason, failure_seconds, io_full_min (% of a second, exact), io_full_avg10_min
# (reported, not judged), commit_share_min, slow_statements}.
# Exits 0 either way; the caller reads `stall`.
set -euo pipefail
log=$1 pressure=$2
IO_FULL_MIN=25 COMMIT_SHARE_MIN=0.9

# The pressure samples, one a second: the share of that second the runner's tasks were all stalled on IO, from
# the change in `full total` (microseconds) since the sample before. `full avg10` lags a short stall (a 0.4 s stall
# barely moves a 10-second average), so it is reported beside the exact figure, not judged.
samples=$(awk '{
  if (!match($0, /\| io: [^|]*full avg10=[0-9.]+[^|]*total=[0-9]+/)) next
  io = substr($0, RSTART, RLENGTH); avg = io; sub(/.*full avg10=/, "", avg); sub(/ .*/, "", avg)
  total = io; sub(/.*total=/, "", total)
  printf "{\"t\":\"%s\",\"avg10\":%s,\"total\":%s}\n", $1, avg, total
}' "$pressure" 2>/dev/null | jq -sc '
  map(. + {t: (.t[0:19] + "Z" | fromdateiso8601)})
  | [range(1; length) as $i | .[$i - 1] as $a | .[$i] as $b | select($b.t > $a.t)
     | {t: $b.t, avg10: $b.avg10, full: ((($b.total - $a.total) / (($b.t - $a.t) * 10000)) * 100 | round / 100)}]' || echo '[]')

jq -Rnc --argjson samples "$samples" --argjson io_min "$IO_FULL_MIN" --argjson share_min "$COMMIT_SHARE_MIN" '
  def second: (.timestamp // "")[0:19] + "Z" | try fromdateiso8601 catch null;
  [inputs | fromjson? | select(type == "object")] as $lines
  # Failed requests: a WARN or ERROR naming a code or status, not the slow-statement evidence itself.
  | [$lines[] | select((.level == "WARN" or .level == "ERROR") and (.code or .status) and .target != "sqlx::query")
       | second | select(. != null)] | unique as $failures
  | [$lines[] | select(.target == "sqlx::query" and ((.message // "") | startswith("slow statement")))
       | {t: second, commit: ((.summary // "") == "COMMIT")} | select(.t != null)] as $slow
  | ($samples | map({key: (.t | tostring), value: .full}) | from_entries) as $io
  | ($samples | map({key: (.t | tostring), value: .avg10}) | from_entries) as $avg10
  | [$failures[] as $s
       | {io: ([$s - 1, $s, $s + 1] | map($io[tostring] // empty) | max // 0),
          avg10: ([$s - 1, $s, $s + 1] | map($avg10[tostring] // empty) | max // 0),
          near: [$slow[] | select(.t >= $s - 5 and .t <= $s + 5)]}
       | . + {share: (if (.near | length) > 0 then ([.near[] | select(.commit)] | length) / (.near | length) else 0 end)}] as $per
  | {failure_seconds: ($failures | length),
     io_full_min: ([$per[].io] | min // null),
     io_full_avg10_min: ([$per[].avg10] | min // null),
     commit_share_min: ([$per[].share] | min // null),
     slow_statements: ($slow | length)}
  | . + (if .failure_seconds == 0 then {stall: false, reason: "the server logged no failed request: nothing to attribute to the runner"}
         elif .io_full_min < $io_min then {stall: false, reason: "a failure second without the disk stalled (io full \(.io_full_min)% < \($io_min)%)"}
         elif .commit_share_min < $share_min then {stall: false, reason: "a failure second without a COMMIT queue (COMMIT share \(.commit_share_min) < \($share_min))"}
         else {stall: true, reason: "every failure second had the disk stalled and a COMMIT queue"} end)
' <"$log"
