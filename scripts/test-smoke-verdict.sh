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
expect_pass "$list" "$ran" "assets bulk audience migrations stages noisy-agent" "$tmp/run2.tsv"
grep -q '^flap: stages failed its limit' "$tmp/out" || { echo "FAIL: no flap line:"; cat "$tmp/out"; exit 1; }
# The first run: noisy-agent's L08 6.56x is under its temporary ceiling of 8 (D-214), so it flaps, and passes.
label='the first run, under the temporary ceiling'; expect_pass "$list" "$ran" "assets audience bulk migrations noisy-agent stages" "$tmp/run1.tsv"
grep -q '^flap: noisy-agent failed its limit, l08 6.56x, under its ceiling 8x' "$tmp/out" || { echo "FAIL: no noisy-agent flap line:"; cat "$tmp/out"; exit 1; }
sed 's/^noisy-agent\t103.657\t6.56/noisy-agent\t103.657\t9.1/' "$tmp/run1.tsv" >"$tmp/over8.tsv"
label='noisy-agent past even its temporary ceiling'; want='noisy-agent is past its CEILING: l08 9.1x > 8x'
expect_fail "$list" "$ran" "assets audience bulk migrations noisy-agent stages" "$tmp/over8.tsv"
# The expiry: once D-214's row is in DECISIONS (its fix merged), the temporary ceiling is refused.
{ cat docs/DECISIONS.md; printf '| D-214 | 2026-11-20 | merged | x | y |\n'; } >"$tmp/decisions.md"
label='mutant: D-214 merged while noisy-agent keeps its temporary ceiling'; want="temporary ceiling expired with D-214"
if SMOKE_DECISIONS="$tmp/decisions.md" scripts/smoke-verdict.sh "$list" "$ran" "assets bulk" "$tmp/run2.tsv" >"$tmp/out" 2>&1; then echo "FAIL: $label accepted"; cat "$tmp/out"; exit 1; fi
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

# The integration job's scope (D-221): its smokes are integration/<endpoint>-<scenario>, read from its run_load lines.
iran=$(grep -oE 'run_load .*scripts/load\.sh [a-z][a-z-]* [a-z]+' .github/workflows/ci.yml | awk '{print "integration/" $(NF-1) "-" $NF}' | sort -u | paste -sd' ' -)
[[ $(wc -w <<<"$iran") -ge 15 ]] || { echo "FAIL: read only '$iran' from ci.yml's integration step" >&2; exit 1; }
printf 'integration/members-steady\t3.3\t\nintegration/publishing-steady\t14.1\t\nintegration/schedules-steady\t5.5\t\nintegration/tokens-steady\t2.7\t\n' >"$tmp/irun.tsv"
iverdict() { SMOKE_SCOPE=integration scripts/smoke-verdict.sh "$@" >"$tmp/out" 2>&1; }
label='integration: a green run (its scope ignores the smokes job entries)'
iverdict "$list" "$iran" "" "$tmp/irun.tsv" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
sed 's/^integration\/members-steady\t3.3/integration\/members-steady\t260/' "$tmp/irun.tsv" >"$tmp/iflap.tsv"
label='integration: members stalls to 260 ms, under its ceiling'
iverdict "$list" "$iran" "integration/members-steady" "$tmp/iflap.tsv" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
grep -q '^flap: integration/members-steady failed its limit, p99 260 ms, under its ceiling 400 ms' "$tmp/out" || { echo "FAIL: no members flap line:"; cat "$tmp/out"; exit 1; }
sed 's/^integration\/members-steady\t3.3/integration\/members-steady\t500/' "$tmp/irun.tsv" >"$tmp/iover.tsv"
label='integration mutant: members past its 2x ceiling'
if iverdict "$list" "$iran" "integration/members-steady" "$tmp/iover.tsv"; then echo "FAIL: $label accepted"; cat "$tmp/out"; exit 1; fi
grep -q 'integration/members-steady is past its CEILING: p99 500 ms > 400 ms' "$tmp/out" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }
label='integration mutant: an unlisted integration smoke fails'
if iverdict "$list" "$iran" "integration/roles-steady" "$tmp/irun.tsv"; then echo "FAIL: $label accepted"; cat "$tmp/out"; exit 1; fi
grep -q 'smoke integration/roles-steady failed and is not on' "$tmp/out" || { echo "FAIL: $label:"; cat "$tmp/out"; exit 1; }

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

echo "smoke verdict: both scopes judged (the integration job's flappers under their ceilings, one past it failing, an unlisted one failing); both runs judged as the list says (noisy-agent under its temporary ceiling, refused once D-214 merges); a deleted entry, a planted passing smoke, a flapper past its ceiling or unmeasured, a smoke that didn't run, and an entry missing its owner, ceiling or runs each fail"
