#!/usr/bin/env bash
# The smokes job's verdict (scripts/smoke-verdict.sh, D-209 addendum) against the REAL list, with the real outcomes
# and p99s of train/hardening-2's two runs:
# - strict: deleting an entry fails (its smoke fails unlisted); planting a passing smoke fails ("remove it");
# - flapping: either outcome passes under the ceiling; past the ceiling fails whatever the list says; no measurement,
#   no ceiling, or one run's numbers only is refused;
# - a listed smoke that didn't run fails; an entry without an owner is refused.
#
#   scripts/test-smoke-verdict.sh
set -euo pipefail
cd "$(dirname "$0")/.."
list=perf/expected-smoke-failures.tsv
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
ran=$(grep -oE 'smoke .*scripts/load\.sh [a-z][a-z-]*' .github/workflows/ci.yml | awk '{print $NF}' | sort -u | paste -sd' ' -)
[[ $(wc -w <<<"$ran") -ge 20 ]] || { echo "FAIL: read only '$ran' from ci.yml" >&2; exit 1; }

# The second run (CI 44551888337): assets and bulk failed; the flappers passed. noisy-agent's L08 held (its worst
# ratio isn't in that run's log line, so 1.6, under the gate's 2, stands for "held").
printf 'audience\t580.731\t\nmigrations\t408.022\t\nstages\t128.171\t\nnoisy-agent\t18.466\t1.6\nassets\t300\t\nbulk\t2500\t\n' >"$tmp/run2.tsv"
# The first run (CI 44543394086): all six failed; noisy-agent's L08 ratios were 5.79 and 6.56.
printf 'audience\t747.707\t\nmigrations\t681.471\t\nstages\t254.202\t\nnoisy-agent\t103.657\t6.56\nassets\t619.375\t\nbulk\t2876.506\t\n' >"$tmp/run1.tsv"

verdict() { scripts/smoke-verdict.sh "$@" >"$tmp/out" 2>&1; }
expect_pass() { verdict "$@" || { echo "FAIL: $label should pass:"; cat "$tmp/out"; exit 1; }; }
expect_fail() {
  if verdict "$@"; then echo "FAIL: $label should fail:"; cat "$tmp/out"; exit 1; fi
  grep -q -- "$want" "$tmp/out" || { echo "FAIL: $label: no '$want' in:"; cat "$tmp/out"; exit 1; }
}
with() { { cat "$list"; printf '%s\n' "$1"; } >"$tmp/list.tsv"; }

label='the second run'; expect_pass "$list" "$ran" "assets bulk" "$tmp/run2.tsv"
grep -q '^pass: migrations, p99 408.022 ms (41% of its ceiling' "$tmp/out" || { echo "FAIL: no margin line:"; cat "$tmp/out"; exit 1; }
label='the second run with every flapper failing its limit, under its ceiling'
expect_pass "$list" "$ran" "assets bulk audience migrations stages" "$tmp/run2.tsv"
grep -q '^flap: stages failed its limit' "$tmp/out" || { echo "FAIL: no flap line:"; cat "$tmp/out"; exit 1; }
# noisy-agent left the list with D-214 (CI 44579308658: L08 1.25x and 1.19x). The first run's failure (6.56x, the
# D-214 defect) is now an unlisted failing smoke, and fails the job.
label='the first run, noisy-agent failing unlisted'; want='smoke noisy-agent failed and is not on'
expect_fail "$list" "$ran" "assets audience bulk migrations noisy-agent stages" "$tmp/run1.tsv"
# The expiry: a temporary ceiling whose decision's row is in DECISIONS (its fix merged) is refused. D-214's is.
with $'noisy-agent\tflapping\tD-214\tlearning-a4\tl08<=8 EXPIRES-WITH D-214\tL08 5.79x, 6.56x → held\tpool-acquire queueing'
cp "$tmp/list.tsv" "$tmp/expired.tsv"
label='mutant: D-214 merged while noisy-agent keeps its temporary ceiling'; want="temporary ceiling expired with D-214"
if scripts/smoke-verdict.sh "$tmp/expired.tsv" "$ran" "assets bulk" "$tmp/run2.tsv" >"$tmp/out" 2>&1; then echo "FAIL: $label accepted"; cat "$tmp/out"; exit 1; fi
grep -q -- "$want" "$tmp/out" || { echo "FAIL: $label: no '$want' in:"; cat "$tmp/out"; exit 1; }

label='a strict entry that passed'; want='smoke bulk PASSED: remove it'
expect_fail "$list" "$ran" "assets" "$tmp/run2.tsv"
grep -v '^assets	' "$list" >"$tmp/deleted.tsv"
label='mutant: assets deleted from the list'; want='smoke assets failed and is not on'
expect_fail "$tmp/deleted.tsv" "$ran" "assets bulk" "$tmp/run2.tsv"
with $'warm-baseline\tstrict\tD-000\tnobody\t-\tplanted\tnothing'
label='mutant: a passing smoke planted as strict'; want='smoke warm-baseline PASSED: remove it'
expect_fail "$tmp/list.tsv" "$ran" "assets bulk" "$tmp/run2.tsv"

sed 's/^migrations\t408.022/migrations\t1200/' "$tmp/run2.tsv" >"$tmp/over.tsv"
label='mutant: a flapper past its ceiling while PASSING its outcome'; want='migrations is past its CEILING: p99 1200 ms > 1000 ms'
expect_fail "$list" "$ran" "assets bulk" "$tmp/over.tsv"
grep -v '^stages	' "$tmp/run2.tsv" >"$tmp/unmeasured.tsv"
label='a flapper with nothing measured'; want='stages has no p99 measured this run'
expect_fail "$list" "$ran" "assets bulk" "$tmp/unmeasured.tsv"

with $'gone\tstrict\tD-000\tnobody\t-\tplanted\tnothing'
label='a listed smoke that did not run'; want='lists gone, which did not run'
expect_fail "$tmp/list.tsv" "$ran" "assets bulk gone" "$tmp/run2.tsv"

refuse() {
  printf '%s\n' "$1" >"$tmp/bad.tsv"
  if verdict "$tmp/bad.tsv" "$ran" "assets" "$tmp/run2.tsv"; then echo "FAIL: $label accepted"; exit 1; fi
  grep -q -- "$want" "$tmp/out" || { echo "FAIL: $label: no '$want' in:"; cat "$tmp/out"; exit 1; }
}
label='an entry without an owner'; want='needs a decision, an owner'
refuse $'assets\tstrict\tD-218\t\t-\tx\tsomething'
label='a flapper without a ceiling'; want='needs a ceiling'
refuse $'stages\tflapping\tD-209\tlearning-95\t-\tp99 254 → 128\tsomething'
label="a flapper with one run's numbers"; want="needs both runs' measurements"
refuse $'stages\tflapping\tD-209\tlearning-95\tp99<=400\tp99 254 ms\tsomething'

echo "smoke verdict: both runs judged as the list says (noisy-agent off the list since D-214; a temporary ceiling refused once its decision merges); a deleted entry, a planted passing smoke, a flapper past its ceiling or unmeasured, a smoke that didn't run, and an entry missing its owner, ceiling or runs each fail"
