#!/usr/bin/env bash
# scripts/run-smoke.sh and scripts/stall-signature.sh: a smoke's hard failures are re-run once only when the
# evidence says runner IO stall, and the re-run decides. A fake smoke writes the record and server log each case
# needs and counts its attempts; pressure.txt has one stalled second (12:00:05, 90% of it). Fails naming the case.
#
#   scripts/test-run-smoke.sh
set -euo pipefail
cd "$(dirname "$0")/.."
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Pressure: `full total` in microseconds, one sample a second; 12:00:05 stalls for $1 percent of the second, and
# the seconds around it barely at all (1%), so only 12:00:05 can carry a stall.
pressure() {
  local share=$1 total=1000000 s
  for s in 01 02 03 04 05 06 07; do
    if [[ $s == 05 ]]; then total=$(( total + share * 10000 )); else total=$(( total + 10000 )); fi
    echo "2026-11-20T12:00:${s}Z cpu: some avg10=1.00 avg60=1.00 avg300=1.00 total=1 full avg10=0.00 avg60=0.00 avg300=0.00 total=0 | io: some avg10=20.00 avg60=5.00 avg300=1.00 total=$total full avg10=20.00 avg60=5.00 avg300=1.00 total=$total | memory: some avg10=0.00 avg60=0.00 avg300=0.00 total=0 full avg10=0.00 avg60=0.00 avg300=0.00 total=0"
  done >"$work/pressure.txt"
}
pressure 90

refusal='{"timestamp":"2026-11-20T12:00:05.40Z","level":"WARN","target":"colophon_api::error","message":"request refused: no database connection free","code":"unavailable"}'
calm_refusal='{"timestamp":"2026-11-20T12:00:02.40Z","level":"WARN","target":"colophon_api::error","message":"request refused: no database connection free","code":"unavailable"}'
commit='{"timestamp":"2026-11-20T12:00:05.20Z","level":"WARN","target":"sqlx::query","message":"slow statement: execution time exceeded alert threshold","summary":"COMMIT","elapsed_secs":0.21}'
select_slow='{"timestamp":"2026-11-20T12:00:05.10Z","level":"WARN","target":"sqlx::query","message":"slow statement: execution time exceeded alert threshold","summary":"SELECT","elapsed_secs":0.21}'

# The fake smoke: attempt N writes what $work/plan's line N says (pass | latency | hard-stall | hard-calm |
# hard-noserver | hard-selects) and exits accordingly.
cat >"$work/smoke.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
n=$(( $(cat "$WORK/attempts" 2>/dev/null || echo 0) + 1 )); echo "$n" >"$WORK/attempts"
what=$(sed -n "${n}p" "$WORK/plan")
hard=0 status=0
: >"$SERVER_LOG"
case $what in
  pass) ;;
  latency) status=1 ;;
  hard-stall) hard=11 status=1; printf '%s\n' "$COMMIT" "$REFUSAL" >"$SERVER_LOG" ;;
  hard-calm) hard=11 status=1; printf '%s\n' "$COMMIT" "$CALM_REFUSAL" >"$SERVER_LOG" ;;
  hard-noserver) hard=10 status=1 ;;
  hard-selects) hard=11 status=1; printf '%s\n' "$SELECT_SLOW" "$REFUSAL" >"$SERVER_LOG" ;;
esac
echo "{\"endpoint\":\"x\",\"attempt\":$n,\"hard_failures_total\":$hard}" >"$RECORD_OUT"
exit "$status"
EOF
chmod +x "$work/smoke.sh"

fail=0
check() { # case, plan lines..., then: expected exit, expected attempts, expect stalled file (yes|no)
  local name=$1 exit_want=$2 attempts_want=$3 stalled_want=$4; shift 4
  rm -f "$work"/attempts "$work"/x.* ; printf '%s\n' "$@" >"$work/plan"
  local got=0
  WORK=$work COMMIT=$commit REFUSAL=$refusal CALM_REFUSAL=$calm_refusal SELECT_SLOW=$select_slow \
    scripts/run-smoke.sh x "$work/x.record.json" "$work/x.server.log" "$work/pressure.txt" -- "$work/smoke.sh" \
    >"$work/out" 2>&1 || got=$?
  local attempts; attempts=$(cat "$work/attempts")
  local stalled=no; [[ -f $work/x.stalled.record.json ]] && stalled=yes
  if [[ $got != "$exit_want" || $attempts != "$attempts_want" || $stalled != "$stalled_want" ]]; then
    echo "FAIL $name: exit $got (want $exit_want), attempts $attempts (want $attempts_want), stalled record $stalled (want $stalled_want)" >&2
    sed 's/^/    /' "$work/out" >&2; fail=1; return
  fi
  if [[ $stalled == yes ]]; then
    [[ $(jq -r '.stall.label' "$work/x.stalled.record.json") == "runner IO stall" ]] || { echo "FAIL $name: the first attempt isn't labelled" >&2; fail=1; }
    [[ $(jq -r '.rerun_of' "$work/x.record.json") == x.stalled.record.json ]] || { echo "FAIL $name: the re-run's record doesn't name the first" >&2; fail=1; }
    [[ -f $work/x.stalled.server.log ]] || { echo "FAIL $name: the first attempt's server log is gone" >&2; fail=1; }
  fi
}

check "passes first time" 0 1 no pass
check "a latency miss is judged by the verdict, never re-run" 1 1 no latency
check "a stall whose re-run passes is green" 0 2 yes hard-stall pass
check "a stall whose re-run fails is red, with no third run" 1 2 yes hard-stall hard-stall pass
check "hard failures in a calm second block, no re-run" 1 1 no hard-calm pass
check "hard failures the server never logged block, no re-run" 1 1 no hard-noserver pass
check "a stalled disk without a COMMIT queue blocks, no re-run" 1 1 no hard-selects pass

# The threshold, calibrated on the two stalls seen (20%): just under blocks, just over re-runs.
pressure 19
check "a 19% stall second blocks, no re-run" 1 1 no hard-stall pass
pressure 21
check "a 21% stall second with a COMMIT queue is re-run" 0 2 yes hard-stall pass
pressure 90

# The signature on its own: the numbers it judged by are in its answer.
printf '%s\n' "$commit" "$refusal" >"$work/stall.log"
sig=$(scripts/stall-signature.sh "$work/stall.log" "$work/pressure.txt")
[[ $(jq -r '.io_full_min' <<<"$sig") == 90 && $(jq -r '.commit_share_min' <<<"$sig") == 1 ]] \
  || { echo "FAIL the signature's numbers: $sig" >&2; fail=1; }

# D-224's logs: an outage is one WARN, then a summary a window (no code: not a failed request), and each refusal
# is a DEBUG line on colophon_api::refusals with its code; slow statements under a second are INFO without the
# "slow statement" message, only their elapsed_secs.
evidence='{"timestamp":"2026-11-20T12:00:05.60Z","level":"DEBUG","target":"colophon_api::refusals","message":"request refused: no database connection free","code":"unavailable"}'
calm_evidence='{"timestamp":"2026-11-20T12:00:02.60Z","level":"DEBUG","target":"colophon_api::refusals","message":"request refused: no database connection free","code":"unavailable"}'
info_commit='{"timestamp":"2026-11-20T12:00:05.30Z","level":"INFO","target":"sqlx::query","summary":"COMMIT","elapsed_secs":0.18}'
summary='{"timestamp":"2026-11-20T12:00:02.00Z","level":"WARN","target":"colophon_api::outages","message":"refused 40 requests in the last 10 s: no database connection free, since 2026-11-20T11:59:52Z","refused":40,"since":"2026-11-20T11:59:52Z"}'
signature() { # case, expected stall, log lines...
  local name=$1 want=$2; shift 2
  printf '%s\n' "$@" >"$work/d224.log"
  local got; got=$(scripts/stall-signature.sh "$work/d224.log" "$work/pressure.txt")
  [[ $(jq -r '.stall' <<<"$got") == "$want" ]] || { echo "FAIL $name: $got" >&2; fail=1; }
}
signature "refusals said only as DEBUG evidence, with INFO COMMITs, are a stall" true "$info_commit" "$evidence"
signature "a DEBUG refusal in a calm second blocks" false "$info_commit" "$evidence" "$calm_evidence"
signature "an outage's summary line in a calm second is not a failed request" true "$commit" "$refusal" "$summary"
# The signature's own floor: a statement under 100 ms isn't evidence, whatever reached the log (the server's layer
# drops them, but a log without it would offer every statement). Twenty fast COMMITs beside one slow SELECT are no
# COMMIT queue.
fast_commits=()
for i in $(seq -w 10 29); do
  fast_commits+=("{\"timestamp\":\"2026-11-20T12:00:05.${i}Z\",\"level\":\"INFO\",\"target\":\"sqlx::query\",\"summary\":\"COMMIT\",\"elapsed_secs\":0.002}")
done
signature "fast COMMITs don't make a COMMIT queue" false "$select_slow" "$refusal" "${fast_commits[@]}"

# The signature reads what the server logs, so every load run CI makes must log the evidence: refusals at DEBUG
# on their target, and statements at INFO (a short stall's COMMITs are 100-220 ms, under the WARN tier).
while IFS= read -r line; do
  for wanted in 'sqlx::query=info' 'colophon_api::refusals=debug'; do
    [[ $line == *"$wanted"* ]] || { echo "FAIL a CI load run's LOAD_LOG lacks $wanted: $line" >&2; fail=1; }
  done
done < <(grep -oE 'LOAD_LOG=[^ ]+' .github/workflows/ci.yml)
[[ $(grep -cE 'LOAD_LOG=' .github/workflows/ci.yml) -ge 2 ]] || { echo "FAIL: no LOAD_LOG found in CI" >&2; fail=1; }

(( fail == 0 )) && echo "run-smoke: only a runner IO stall is re-run, once, and the re-run decides; correctness blocks"
exit "$fail"
