#!/usr/bin/env bash
# Runs one load smoke for CI, and re-runs it ONCE when its hard failures are a runner IO stall
# (scripts/stall-signature.sh; learning-60, 2026-11-20), so a stalled disk on a shared runner doesn't fail
# correctness while a real failure always does:
#
#   scripts/run-smoke.sh <name> <record.json> <server.log> <pressure.txt> -- <command...>
#
# - The command runs with SERVER_LOG and RECORD_OUT set to the two paths (as scripts/load.sh expects).
# - It passes: exit 0. It fails without hard failures (a latency miss): exit 1, no re-run; the verdict judges it.
# - It fails with hard failures and NOT the stall signature: exit 1, no re-run. Correctness blocks.
# - It fails with hard failures AND the signature: the first attempt's record and log are kept as
#   <name>.stalled.record.json / .stalled.server.log, labelled "runner IO stall" with the numbers (a `stall` field),
#   and the smoke runs once more. That re-run decides: its record (with `rerun_of`) is the one the verdict reads,
#   and its exit status is this script's. Never more than one re-run. RUN_SMOKE_BEFORE_RERUN, if set, is run with the
#   stalled smoke's stem just before that re-run, and at no other time.
set -euo pipefail
name=$1 record=$2 log=$3 pressure=$4
shift 4
[[ ${1:-} == -- ]] || { echo "usage: run-smoke.sh <name> <record> <server-log> <pressure> -- <command...>" >&2; exit 2; }
shift
here=$(cd "$(dirname "$0")" && pwd)

run() { env SERVER_LOG="$log" RECORD_OUT="$record" "$@"; }

status=0
run "$@" || status=$?
(( status != 0 )) || exit 0

hard=$(jq -r '.hard_failures_total // 0' "$record" 2>/dev/null || echo 0)
if [[ ! $hard =~ ^[0-9]+$ ]] || (( hard == 0 )); then
  exit "$status" # a latency miss, or no record: the verdict judges it as it stands
fi
[[ -f $pressure ]] || { echo "::warning::$name: $hard hard failures and no pressure samples to judge a stall by; not re-run"; exit "$status"; }

signature=$("$here/stall-signature.sh" "$log" "$pressure")
if [[ $(jq -r '.stall' <<<"$signature") != true ]]; then
  echo "::warning::$name: $hard hard failures, not a runner IO stall ($(jq -r '.reason' <<<"$signature")); not re-run"
  exit "$status"
fi

stalled_record=${record%.record.json}.stalled.record.json
stalled_log=${log%.server.log}.stalled.server.log
jq -c --argjson s "$signature" '. + {stall: ($s + {label: "runner IO stall"})}' "$record" >"$stalled_record"
rm -f "$record"
[[ -f $log ]] && mv "$log" "$stalled_log"
echo "::warning::$name: runner IO stall ($hard hard failures; io full ≥ $(jq -r '.io_full_min' <<<"$signature")% of each failure second, COMMIT share $(jq -r '.commit_share_min' <<<"$signature")); first attempt kept as $(basename "$stalled_record"), re-running it once"

# A caller's hook, before the re-run only, with the stalled smoke's stem (its record path without .record.json):
# the smokes job keeps the stalled attempt's statement counts and resets them, so they don't mix with the re-run's.
if [[ -n ${RUN_SMOKE_BEFORE_RERUN:-} ]]; then
  "$RUN_SMOKE_BEFORE_RERUN" "${record%.record.json}" || echo "::warning::$name: the pre-re-run hook failed; re-running anyway"
fi

rerun=0
run "$@" || rerun=$?
if [[ -s $record ]]; then
  jq -c --arg first "$(basename "$stalled_record")" '. + {rerun_of: $first}' "$record" >"$record.tmp" && mv "$record.tmp" "$record"
fi
exit "$rerun"
