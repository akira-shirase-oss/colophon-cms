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
printf 'audience\t580.731\t\t0\t\nmigrations\t408.022\t\t0\t\nstages\t128.171\t\t0\t\nnoisy-agent\t18.466\t1.6\t0\t\nassets\t300\t\t0\t\nbulk\t2500\t\t0\t\n' >"$tmp/run2.tsv"
# The first run (CI 44543394086): all six failed; noisy-agent's L08 ratios were 5.79 and 6.56.
printf 'audience\t747.707\t\t0\t\nmigrations\t681.471\t\t0\t\nstages\t254.202\t\t0\t\nnoisy-agent\t103.657\t6.56\t0\t\nassets\t619.375\t\t0\t\nbulk\t2876.506\t\t0\t\n' >"$tmp/run1.tsv"
# D-216's run (CI 44585574138): every smoke passed. bulk, undo and assets left the list with D-216 (perf-compare
# 44697596006, EPYC 7763: bulk p99 795–2955 → 319–327 ms, undo 866–1013 → 350–395, assets 686–836 → 436–442).
printf 'audience\t549.583\t\t0\t\nmigrations\t258.603\t\t0\t\nstages\t101.837\t\t0\t\nnoisy-agent\t10.036\t1.264\t0\t\nassets\t306.718\t\t0\t\nbulk\t188.31\t\t0\t\n' >"$tmp/run3.tsv"
# The first run's other smokes with D-216's bulk and assets, for the noisy-agent cases below.
sed -e 's/^assets\t619.375/assets\t306.718/' -e 's/^bulk\t2876.506/bulk\t188.31/' "$tmp/run1.tsv" >"$tmp/run1b.tsv"

# Every other smoke that ran, at a green p99 (D-221 lists them all as flapping, each judged by what it measured).
for run in run1 run2 run3 run1b; do
  for smoke in $ran; do grep -q "^$smoke	" "$tmp/$run.tsv" || printf '%s\t9.5\t\t0\t\n' "$smoke" >>"$tmp/$run.tsv"; done
done

verdict() { scripts/smoke-verdict.sh "$@" >"$tmp/out" 2>&1; }
expect_pass() { verdict "$@" || { echo "FAIL: $label should pass:"; cat "$tmp/out"; exit 1; }; }
expect_fail() {
  if verdict "$@"; then echo "FAIL: $label should fail:"; cat "$tmp/out"; exit 1; fi
  grep -q -- "$want" "$tmp/out" || { echo "FAIL: $label: no '$want' in:"; cat "$tmp/out"; exit 1; }
}
with() { { cat "$list"; printf '%s\n' "$1"; } >"$tmp/list.tsv"; }

# The second run is from before D-216: bulk failed it, and with D-216 bulk is off the list, so that failure fails.
label='the second run, from before D-216'; want='smoke bulk failed and is not on'
expect_fail "$list" "$ran" "assets bulk" "$tmp/run2.tsv"
label="D-216's run"; expect_pass "$list" "$ran" "" "$tmp/run3.tsv"
grep -q '^pass: migrations, p99 258.603 ms (26% of its ceiling' "$tmp/out" || { echo "FAIL: no margin line:"; cat "$tmp/out"; exit 1; }
label="D-216's run with every flapper failing its limit, under its ceiling"
expect_pass "$list" "$ran" "audience migrations stages noisy-agent" "$tmp/run3.tsv"
grep -q '^flap: stages failed its limit' "$tmp/out" || { echo "FAIL: no flap line:"; cat "$tmp/out"; exit 1; }
# The first run: noisy-agent's L08 6.56x was the D-214 defect itself, under the temporary ceiling of 8 that D-214's
# merge retired; at the ordinary ceiling of 4 (2x its limit) that run fails, as it should now that the defect is fixed.
label='the first run, past the ordinary ceiling'; want='noisy-agent is past its CEILING: l08 6.56x > 4x'
expect_fail "$list" "$ran" "audience migrations noisy-agent stages" "$tmp/run1b.tsv"
sed 's/^noisy-agent\t103.657\t6.56/noisy-agent\t103.657\t3.1/' "$tmp/run1b.tsv" >"$tmp/under4.tsv"
label='noisy-agent failing its limit under its ceiling'
expect_pass "$list" "$ran" "audience migrations noisy-agent stages" "$tmp/under4.tsv"
grep -q '^flap: noisy-agent failed its limit, l08 3.1x, under its ceiling 4x' "$tmp/out" || { echo "FAIL: no noisy-agent flap line:"; cat "$tmp/out"; exit 1; }
# The expiry: a temporary ceiling whose decision's row is in DECISIONS (its fix merged) is refused. D-214's is.
sed 's/^\(noisy-agent\t[^\t]*\t[^\t]*\t[^\t]*\t\)l08<=4\t/\1l08<=8 EXPIRES-WITH D-214\t/' "$list" >"$tmp/expired.tsv"
grep -q 'EXPIRES-WITH D-214' "$tmp/expired.tsv" || { echo "FAIL: could not plant the expired ceiling"; exit 1; }
label='mutant: D-214 merged while noisy-agent keeps its temporary ceiling'; want="temporary ceiling expired with D-214"
if scripts/smoke-verdict.sh "$tmp/expired.tsv" "$ran" "" "$tmp/run3.tsv" >"$tmp/out" 2>&1; then echo "FAIL: $label accepted"; cat "$tmp/out"; exit 1; fi
grep -q -- "$want" "$tmp/out" || { echo "FAIL: $label: no '$want' in:"; cat "$tmp/out"; exit 1; }

grep -v '^migrations	' "$list" >"$tmp/deleted.tsv"
label='mutant: migrations deleted from the list'; want='smoke migrations failed and is not on'
expect_fail "$tmp/deleted.tsv" "$ran" "migrations" "$tmp/run3.tsv"
with $'cache-down\tstrict\tD-000\tnobody\t-\tplanted\tnothing'
label='mutant: a passing smoke planted as strict'; want='smoke cache-down PASSED: remove it'
expect_fail "$tmp/list.tsv" "$ran" "" "$tmp/run3.tsv"

sed 's/^migrations\t258.603/migrations\t1200/' "$tmp/run3.tsv" >"$tmp/over.tsv"
label='mutant: a flapper past its ceiling while PASSING its outcome'; want='migrations is past its CEILING: p99 1200 ms > 1000 ms'
expect_fail "$list" "$ran" "" "$tmp/over.tsv"
# The smokes job scales its limits to the runner (D-221: P99 / scale), so a flapper's p99 ceiling scales with it:
# 2x the limit stays 2x. At scale 0.565 migrations' ceiling is 1000 / 0.565 = 1770 ms (CI 44680118124 measured
# 1178 ms there, which an unscaled 1000 ms ceiling failed). A ratio ceiling (l08) doesn't scale.
sed 's/^migrations\t258.603/migrations\t1500/' "$tmp/run3.tsv" >"$tmp/scaled.tsv"
label='a flapper over its reference ceiling but under the scaled one'
SMOKE_LIMIT_SCALE=0.565 scripts/smoke-verdict.sh "$list" "$ran" "" "$tmp/scaled.tsv" >"$tmp/out" 2>&1 \
  || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
grep -q 'migrations, p99 1500 ms (85% of its ceiling' "$tmp/out" || { echo "FAIL: $label: no scaled margin:"; cat "$tmp/out"; exit 1; }
sed 's/^migrations\t258.603/migrations\t1800/' "$tmp/run3.tsv" >"$tmp/scaled-over.tsv"
label='mutant: a flapper past its scaled ceiling'
if SMOKE_LIMIT_SCALE=0.565 scripts/smoke-verdict.sh "$list" "$ran" "" "$tmp/scaled-over.tsv" >"$tmp/out" 2>&1; then
  echo "FAIL: $label accepted"; cat "$tmp/out"; exit 1
fi
grep -q 'migrations is past its CEILING: p99 1800 ms > 1769.9 ms' "$tmp/out" || { echo "FAIL: $label: wrong reason:"; cat "$tmp/out"; exit 1; }
grep -v '^stages	' "$tmp/run3.tsv" >"$tmp/unmeasured.tsv"
label='a flapper with nothing measured'; want='stages has no p99 measured this run'
expect_fail "$list" "$ran" "" "$tmp/unmeasured.tsv"

with $'gone\tstrict\tD-000\tnobody\t-\tplanted\tnothing'
label='a listed smoke that did not run'; want='lists gone, which did not run'
expect_fail "$tmp/list.tsv" "$ran" "gone" "$tmp/run3.tsv"

# The integration job's scope (D-221): its smokes are integration/<endpoint>-<scenario>, read from its run_load lines.
iran=$(grep -oE 'run_load .*scripts/load\.sh [a-z][a-z-]* [a-z]+' .github/workflows/ci.yml | awk '{print "integration/" $(NF-1) "-" $NF}' | sort -u | paste -sd' ' -)
[[ $(wc -w <<<"$iran") -ge 15 ]] || { echo "FAIL: read only '$iran' from ci.yml's integration step" >&2; exit 1; }
# A green run: every integration smoke at its p99 from CI 44561341789's step (the steady ones all 1–21 ms).
for smoke in $iran; do printf '%s\t%s\t\t0\t\n' "$smoke" 9.5; done >"$tmp/irun.tsv"
iverdict() { SMOKE_SCOPE=integration scripts/smoke-verdict.sh "$@" >"$tmp/out" 2>&1; }
label='integration: a green run (its scope ignores the smokes job entries)'
iverdict "$list" "$iran" "" "$tmp/irun.tsv" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
# D-221 is closed: runner speed is normalised (rates and limits scaled to the runner), so a steady smoke that still
# fails its limit fails the job; nothing tolerates it.
sed 's/^integration\/entries-steady\t9.5/integration\/entries-steady\t900/' "$tmp/irun.tsv" >"$tmp/islow.tsv"
label='integration: entries failing its (scaled) limit fails, no tolerance listed'
if iverdict "$list" "$iran" "integration/entries-steady" "$tmp/islow.tsv"; then echo "FAIL: $label accepted"; cat "$tmp/out"; exit 1; fi
grep -q 'smoke integration/entries-steady failed and is not on' "$tmp/out" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
# tokens, schedules and publishing steady are listed flapping under D-223 until its phase 2 (write smokes run under
# their path's capacity): the 9V74's measured p99s (CI 44694257974) are tolerated, past each ceiling still fails.
for case in "tokens 529.14 750" "schedules 505.53 750" "publishing 781.17 1100"; do
  read -r name measured past <<<"$case"
  sed "s/^integration\/$name-steady\t9.5/integration\/$name-steady\t$measured/" "$tmp/irun.tsv" >"$tmp/iwrite.tsv"
  label="integration: $name flapping at its measured $measured ms, under its D-223 ceiling"
  iverdict "$list" "$iran" "integration/$name-steady" "$tmp/iwrite.tsv" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
  grep -q "flap: integration/$name-steady failed its limit" "$tmp/out" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
  sed "s/^integration\/$name-steady\t9.5/integration\/$name-steady\t$past/" "$tmp/irun.tsv" >"$tmp/iwrite.tsv"
  label="integration: $name past its D-223 ceiling ($past ms) fails"
  if iverdict "$list" "$iran" "integration/$name-steady" "$tmp/iwrite.tsv"; then echo "FAIL: $label accepted"; cat "$tmp/out"; exit 1; fi
done
# members-steady is listed flapping under D-223 (the member-change queue behind slow COMMITs), with a 1,500 ms ceiling
# that expires with D-223: CI 44673105548's 1314 ms is tolerated, anything past the ceiling still fails.
sed 's/^integration\/members-steady\t9.5/integration\/members-steady\t1313.778/' "$tmp/irun.tsv" >"$tmp/imembers.tsv"
label='integration: members flapping at its measured 1314 ms, under its D-223 ceiling'
iverdict "$list" "$iran" "integration/members-steady" "$tmp/imembers.tsv" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
grep -q 'flap: integration/members-steady failed its limit' "$tmp/out" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
sed 's/^integration\/members-steady\t9.5/integration\/members-steady\t1600/' "$tmp/irun.tsv" >"$tmp/iceiling.tsv"
label='integration: members past its D-223 ceiling fails'
if iverdict "$list" "$iran" "integration/members-steady" "$tmp/iceiling.tsv"; then echo "FAIL: $label accepted"; cat "$tmp/out"; exit 1; fi
label='integration mutant: an unlisted (strict) integration smoke fails'
if iverdict "$list" "$iran" "integration/health-throughput" "$tmp/irun.tsv"; then echo "FAIL: $label accepted"; cat "$tmp/out"; exit 1; fi
grep -q 'smoke integration/health-throughput failed and is not on' "$tmp/out" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
awk -F'\t' 'BEGIN {OFS = FS} $1 == "integration/members-steady" {$2 = 260; $4 = 4; $5 = "checks_failed=4"} {print}' "$tmp/irun.tsv" >"$tmp/ihard.tsv"
label='integration hard: members flapping under its ceiling but with failed checks'
if iverdict "$list" "$iran" "integration/members-steady" "$tmp/ihard.tsv"; then echo "FAIL: $label accepted"; cat "$tmp/out"; exit 1; fi
grep -q 'smoke integration/members-steady has hard failures (4: checks_failed=4)' "$tmp/out" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
# D-221's temporary ceilings are gone with it: one planted back is refused, its row no longer being Open.
with $'integration/publishing-steady\tflapping\tD-221\tlearning-95\tp99<=400 EXPIRES-WITH D-221\tplanted → back\tnothing'
label='integration mutant: a D-221 tolerance planted back after D-221 closed'
if SMOKE_SCOPE=integration scripts/smoke-verdict.sh "$tmp/list.tsv" "$iran" "" "$tmp/irun.tsv" >"$tmp/out" 2>&1; then echo "FAIL: $label accepted"; cat "$tmp/out"; exit 1; fi
grep -q 'temporary ceiling expired with D-221' "$tmp/out" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
# undo's temporary ceiling expired with D-216, whose row this branch adds: planted back, it is refused.
with $'undo\tflapping\tD-216\tlearning-a4\tp99<=1400 EXPIRES-WITH D-216\tp99 2291 → 395 ms\tnothing'
label='mutant: D-216 merged while undo keeps its temporary ceiling'
if scripts/smoke-verdict.sh "$tmp/list.tsv" "$ran" "" "$tmp/run3.tsv" >"$tmp/out" 2>&1; then echo "FAIL: $label accepted"; cat "$tmp/out"; exit 1; fi
grep -q 'temporary ceiling expired with D-216' "$tmp/out" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
# Under half the reference speed latency isn't judged (SMOKE_LATENCY_JUDGED=no), but hard failures still are.
label='not judged: smokes failing their limits on a too-slow runner pass the verdict'
SMOKE_LATENCY_JUDGED=no scripts/smoke-verdict.sh "$list" "$ran" "assets audience bulk collaboration live" "$tmp/run2.tsv" >"$tmp/out" 2>&1 || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
grep -q 'latency not judged on this runner' "$tmp/out" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
awk -F'\t' 'BEGIN {OFS = FS} $1 == "live" {$4 = 3; $5 = "requests_failed=3"} {print}' "$tmp/run2.tsv" >"$tmp/slowhard.tsv"
label='not judged: a hard failure on a too-slow runner still fails'
if SMOKE_LATENCY_JUDGED=no scripts/smoke-verdict.sh "$list" "$ran" "assets bulk live" "$tmp/slowhard.tsv" >"$tmp/out" 2>&1; then echo "FAIL: $label accepted"; cat "$tmp/out"; exit 1; fi
grep -q 'smoke live has hard failures (3: requests_failed=3)' "$tmp/out" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }

# Hard failures (correctness) are never tolerated, whatever the list or a ceiling says.
hard() { awk -F'\t' -v s="$1" -v h="$2" -v d="$3" 'BEGIN {OFS = FS} $1 == s {$4 = h; $5 = d} {print}' "$tmp/run3.tsv" >"$tmp/hard.tsv"; }
hard migrations 1 'checks_failed=1; release names it: 1 of 400'
label='hard: a flapper under its ceiling with a failed check'; want='smoke migrations has hard failures (1: checks_failed=1; release names it: 1 of 400)'
expect_fail "$list" "$ran" "" "$tmp/hard.tsv"
hard assets 3598 'upload_refused=3598; upload 202: 3598 of 4096'
with $'assets\tstrict\tD-000\tnobody\t-\tplanted\tnothing'
label='hard: a strict expected failure that is also a correctness failure'; want='smoke assets has hard failures (3598'
expect_fail "$tmp/list.tsv" "$ran" "assets" "$tmp/hard.tsv"
hard cache-down 2 'requests_failed=2'
label='hard: an unlisted passing smoke (an outage drill) with real request errors'; want='smoke cache-down has hard failures (2: requests_failed=2)'
expect_fail "$list" "$ran" "" "$tmp/hard.tsv"
awk -F'\t' 'BEGIN {OFS = FS} $1 == "stages" {$4 = ""} {print}' "$tmp/run3.tsv" >"$tmp/hard.tsv"
label='hard: no count measured for a smoke that ran'; want='smoke stages has no hard-failure count measured'
expect_fail "$list" "$ran" "" "$tmp/hard.tsv"
label='hard: every count 0, the outage drills included — today'"'"'s verdict unchanged'
expect_pass "$list" "$ran" "" "$tmp/run3.tsv"

refuse() {
  printf '%s\n' "$1" >"$tmp/bad.tsv"
  if verdict "$tmp/bad.tsv" "$ran" "assets" "$tmp/run3.tsv"; then echo "FAIL: $label accepted"; exit 1; fi
  grep -q -- "$want" "$tmp/out" || { echo "FAIL: $label: no '$want' in:"; cat "$tmp/out"; exit 1; }
}
label='an entry without an owner'; want='needs a decision, an owner'
refuse $'assets\tstrict\tD-218\t\t-\tx\tsomething'
label='a flapper without a ceiling'; want='needs a ceiling'
refuse $'stages\tflapping\tD-209\tlearning-95\t-\tp99 254 → 128\tsomething'
label="a flapper with one run's numbers"; want="needs both runs' measurements"
refuse $'stages\tflapping\tD-209\tlearning-95\tp99<=400\tp99 254 ms\tsomething'

echo "smoke verdict: hard failures fail every smoke (a flapper, a strict expected failure, an unlisted drill, an unmeasured one) and all-zero leaves the verdict as it was; both scopes judged (the integration job's flappers under their ceilings, one past it failing, an unlisted one failing); both runs judged as the list says (noisy-agent at its ordinary ceiling since D-214; a temporary ceiling refused once its decision merges); a deleted entry, a planted passing smoke, a flapper past its ceiling or unmeasured, a smoke that didn't run, and an entry missing its owner, ceiling or runs each fail"
