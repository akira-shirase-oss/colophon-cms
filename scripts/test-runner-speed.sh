#!/usr/bin/env bash
# scripts/runner-speed.sh's write probe (D-223) and scripts/stall-share.sh: a probe run inside a disk stall (20% of a
# second or more) is discarded, the median of the rest counts, and with none left the source is `fallback` with no
# req/s, so a bad probe can never scale a limit. A fake load.sh writes each run's record and the pressure samples of
# its window. Fails naming the case.
#
#   scripts/test-runner-speed.sh
set -euo pipefail
cd "$(dirname "$0")/.."
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# One pressure line at epoch second $1 with io full total $2 (microseconds).
sample() { printf '%s cpu: some avg10=0.00 avg60=0.00 avg300=0.00 total=0 full avg10=0.00 avg60=0.00 avg300=0.00 total=0 | io: some avg10=0.00 avg60=0.00 avg300=0.00 total=%s full avg10=0.00 avg60=0.00 avg300=0.00 total=%s | memory: some avg10=0.00 avg60=0.00 avg300=0.00 total=0 full avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' "$(date -u -r "$1" +%FT%TZ 2>/dev/null || date -u -d "@$1" +%FT%TZ)" "$2" "$2"; }

# stall-share on its own: a 90% second inside the window, 1% outside it.
t=1700000000
{ sample $t 1000000; sample $((t + 1)) 1010000; sample $((t + 2)) 1910000; } >"$work/p.txt"
[[ $(scripts/stall-share.sh "$work/p.txt" $((t + 2)) $((t + 2))) == 90 ]] || { echo "FAIL: stall-share of the stalled second" >&2; exit 1; }
[[ $(scripts/stall-share.sh "$work/p.txt" $((t + 1)) $((t + 1))) == 1 ]] || { echo "FAIL: stall-share of a calm second" >&2; exit 1; }

# The fake load.sh: run N of the plan (one line each: `<rps> <stall %>`, or `fail`) writes its record and two
# pressure samples for its own window, the second one stalled by that share.
cat >"$work/load.sh" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
n=$(( $(cat "$WORK/n" 2>/dev/null || echo 0) + 1 )); echo "$n" >"$WORK/n"
read -r rps share <<<"$(sed -n "${n}p" "$WORK/plan")"
[[ $rps == fail ]] && exit 1
# A second either side, as a real run lasts seconds: each run's window then holds its own samples only.
sleep 1
# One running total across runs, as /proc/pressure/io's is: only this run's second adds its stall.
now=$(date -u +%s); base=$(cat "$WORK/total" 2>/dev/null || echo 1000000)
# The second before the run, then the run's own second (inside the window the probe measures).
{ "$WORK/sample.sh" $(( now - 1 )) "$base"; "$WORK/sample.sh" "$now" $(( base + share * 10000 )); } >>"$PRESSURE_FILE"
echo $(( base + share * 10000 )) >"$WORK/total"
echo "{\"rps\": $rps}" >"$RECORD_OUT"
sleep 1
FAKE
{ echo '#!/usr/bin/env bash'; declare -f sample; echo 'sample "$@"'; } >"$work/sample.sh"
chmod +x "$work/load.sh" "$work/sample.sh"

fail=0
probe() { # case, expected output, plan lines...
  local name=$1 want=$2; shift 2
  rm -f "$work/n" "$work/total"; : >"$work/pressure.txt"; printf '%s\n' "$@" >"$work/plan"
  local got; got=$(WORK=$work LOAD_SH=$work/load.sh PRESSURE_FILE=$work/pressure.txt scripts/runner-speed.sh write 2>/dev/null | tr '\t' ' ')
  [[ $got == "$want" ]] || { echo "FAIL $name: got [$got], want [$want]" >&2; fail=1; }
}
probe "three calm runs: their median" "1200 probe 3 0" "1000 1" "1200 2" "1500 0"
probe "a run inside a stall is discarded" "1100 probe 2 1" "1000 1" "300 85" "1200 3"
probe "every run inside a stall: fallback, no req/s" " fallback 0 3" "300 85" "250 60" "400 25"
probe "the probe fails: fallback" " fallback 0 3" fail fail fail
(( fail == 0 )) && echo "runner-speed: a write-probe run inside a disk stall is discarded; none left is a fallback, never a scale"
exit "$fail"
