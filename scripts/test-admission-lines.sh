#!/usr/bin/env bash
# perf-compare's admission reader (scripts/admission-lines.awk) against the harness's real output
# shape: libtest prints "test <name> ... " and the test's first line together, so N=1 shares a line
# with it (perf-compare 44697592934 lost every N=1 row that way). Every level must come out, once.
#
#   scripts/test-admission-lines.sh
set -euo pipefail
cd "$(dirname "$0")/.."
out=$(awk -v round=1 -v side=after -v mode=one-project -f scripts/admission-lines.awk <<'TXT'

running 1 test
test admissions_a_second_by_concurrent_submitters_in_one_project ... N= 1  admissions=  3012  per_second=   301.2  p50_ms=   3.28  p99_ms=   4.19
N= 8  admissions=  3766  per_second=   375.8  p50_ms=  20.69  p99_ms=  28.11
test admissions_a_second_by_concurrent_submitters_in_one_project has been running for over 60 seconds
N=32  admissions=  3449  per_second=   341.8  p50_ms=  84.68  p99_ms= 142.32
N=64  admissions=  3288  per_second=   322.6  p50_ms= 183.95  p99_ms= 266.21
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 41.33s
TXT
)
levels=$(jq -sr 'map(.N) | join(" ")' <<<"$out") || { echo "FAIL: not JSON lines:"; echo "$out"; exit 1; }
[[ $levels == "1 8 32 64" ]] || { echo "FAIL: levels read were '$levels', want '1 8 32 64':"; echo "$out"; exit 1; }
first=$(head -n1 <<<"$out")
want='{"round":1,"side":"after","mode":"one-project","N":1,"admissions":3012,"per_second":301.2,"p50_ms":3.28,"p99_ms":4.19}'
[[ $first == "$want" ]] || { echo "FAIL: N=1 read as '$first', want '$want'"; exit 1; }
echo "admission lines: every level read once, N=1 behind libtest's 'test ... ' prefix included"
