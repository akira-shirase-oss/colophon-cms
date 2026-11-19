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

# Every other smoke that ran, at a green p99 (D-221 lists them all as flapping, each judged by what it measured).
for run in run1 run2; do
  for smoke in $ran; do grep -q "^$smoke	" "$tmp/$run.tsv" || printf '%s\t9.5\t\n' "$smoke" >>"$tmp/$run.tsv"; done
done

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
expect_pass "$list" "$ran" "assets bulk audience migrations stages noisy-agent" "$tmp/run2.tsv"
grep -q '^flap: stages failed its limit' "$tmp/out" || { echo "FAIL: no flap line:"; cat "$tmp/out"; exit 1; }
# The first run: noisy-agent's L08 6.56x was the D-214 defect itself, under the temporary ceiling of 8 that D-214's
# merge retired; at the ordinary ceiling of 4 (2x its limit) that run fails, as it should now that the defect is fixed.
label='the first run, past the ordinary ceiling'; want='noisy-agent is past its CEILING: l08 6.56x > 4x'
expect_fail "$list" "$ran" "assets audience bulk migrations noisy-agent stages" "$tmp/run1.tsv"
sed 's/^noisy-agent\t103.657\t6.56/noisy-agent\t103.657\t3.1/' "$tmp/run1.tsv" >"$tmp/under4.tsv"
label='noisy-agent failing its limit under its ceiling'
expect_pass "$list" "$ran" "assets audience bulk migrations noisy-agent stages" "$tmp/under4.tsv"
grep -q '^flap: noisy-agent failed its limit, l08 3.1x, under its ceiling 4x' "$tmp/out" || { echo "FAIL: no noisy-agent flap line:"; cat "$tmp/out"; exit 1; }
# The expiry: a temporary ceiling whose decision's row is in DECISIONS (its fix merged) is refused. D-214's is.
sed 's/^\(noisy-agent\t[^\t]*\t[^\t]*\t[^\t]*\t\)l08<=4\t/\1l08<=8 EXPIRES-WITH D-214\t/' "$list" >"$tmp/expired.tsv"
grep -q 'EXPIRES-WITH D-214' "$tmp/expired.tsv" || { echo "FAIL: could not plant the expired ceiling"; exit 1; }
label='mutant: D-214 merged while noisy-agent keeps its temporary ceiling'; want="temporary ceiling expired with D-214"
if scripts/smoke-verdict.sh "$tmp/expired.tsv" "$ran" "assets bulk" "$tmp/run2.tsv" >"$tmp/out" 2>&1; then echo "FAIL: $label accepted"; cat "$tmp/out"; exit 1; fi
grep -q -- "$want" "$tmp/out" || { echo "FAIL: $label: no '$want' in:"; cat "$tmp/out"; exit 1; }

label='a strict entry that passed'; want='smoke bulk PASSED: remove it'
expect_fail "$list" "$ran" "assets" "$tmp/run2.tsv"
grep -v '^assets	' "$list" >"$tmp/deleted.tsv"
label='mutant: assets deleted from the list'; want='smoke assets failed and is not on'
expect_fail "$tmp/deleted.tsv" "$ran" "assets bulk" "$tmp/run2.tsv"
with $'cache-down\tstrict\tD-000\tnobody\t-\tplanted\tnothing'
label='mutant: a passing smoke planted as strict'; want='smoke cache-down PASSED: remove it'
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

# The integration job's scope (D-221): its smokes are integration/<endpoint>-<scenario>, read from its run_load lines.
iran=$(grep -oE 'run_load .*scripts/load\.sh [a-z][a-z-]* [a-z]+' .github/workflows/ci.yml | awk '{print "integration/" $(NF-1) "-" $NF}' | sort -u | paste -sd' ' -)
[[ $(wc -w <<<"$iran") -ge 15 ]] || { echo "FAIL: read only '$iran' from ci.yml's integration step" >&2; exit 1; }
# A green run: every integration smoke at its p99 from CI 44561341789's step (the steady ones all 1–21 ms).
for smoke in $iran; do printf '%s\t%s\t\n' "$smoke" 9.5; done >"$tmp/irun.tsv"
iverdict() { SMOKE_SCOPE=integration scripts/smoke-verdict.sh "$@" >"$tmp/out" 2>&1; }
label='integration: a green run (its scope ignores the smokes job entries)'
iverdict "$list" "$iran" "" "$tmp/irun.tsv" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
sed 's/^integration\/members-steady\t9.5/integration\/members-steady\t260/' "$tmp/irun.tsv" >"$tmp/iflap.tsv"
label='integration: members stalls to 260 ms, under its ceiling'
iverdict "$list" "$iran" "integration/members-steady" "$tmp/iflap.tsv" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
grep -q '^flap: integration/members-steady failed its limit, p99 260 ms, under its ceiling 400 ms' "$tmp/out" || { echo "FAIL: no members flap line:"; cat "$tmp/out"; exit 1; }
sed 's/^integration\/members-steady\t9.5/integration\/members-steady\t500/' "$tmp/irun.tsv" >"$tmp/iover.tsv"
label='integration mutant: members past its 2x ceiling'
if iverdict "$list" "$iran" "integration/members-steady" "$tmp/iover.tsv"; then echo "FAIL: $label accepted"; cat "$tmp/out"; exit 1; fi
grep -q 'integration/members-steady is past its CEILING: p99 500 ms > 400 ms' "$tmp/out" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
label='integration mutant: an unlisted (strict) integration smoke fails'
if iverdict "$list" "$iran" "integration/health-throughput" "$tmp/irun.tsv"; then echo "FAIL: $label accepted"; cat "$tmp/out"; exit 1; fi
grep -q 'smoke integration/health-throughput failed and is not on' "$tmp/out" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
# The steady smokes' tolerance expires with D-221: once its row is no longer Open, the list is refused.
sed 's/^\(| D-221 |.*\)Open:/\1Fixed:/' docs/DECISIONS.md >"$tmp/decisions-d221.md"
grep -q '^| D-221 |.*Fixed:' "$tmp/decisions-d221.md" || { echo "FAIL: could not mark D-221 done"; exit 1; }
label='integration mutant: D-221 done while its tolerance is still listed'
if SMOKE_DECISIONS="$tmp/decisions-d221.md" SMOKE_SCOPE=integration scripts/smoke-verdict.sh "$list" "$iran" "" "$tmp/irun.tsv" >"$tmp/out" 2>&1; then echo "FAIL: $label accepted"; cat "$tmp/out"; exit 1; fi
grep -q 'temporary ceiling expired with D-221' "$tmp/out" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }

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

echo "smoke verdict: both scopes judged (the integration job's flappers under their ceilings, one past it failing, an unlisted one failing); both runs judged as the list says (noisy-agent at its ordinary ceiling since D-214; a temporary ceiling refused once its decision merges); a deleted entry, a planted passing smoke, a flapper past its ceiling or unmeasured, a smoke that didn't run, and an entry missing its owner, ceiling or runs each fail"
