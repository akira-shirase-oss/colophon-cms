#!/usr/bin/env bash
# The smokes job's strict verdict (scripts/smoke-verdict.sh, D-209 addendum) against the real list: the run as it
# stands passes; deleting an entry fails (its smoke fails unexpected); planting a passing smoke fails ("remove it");
# a listed smoke that didn't run fails; an entry without an owner is refused.
#
#   scripts/test-smoke-verdict.sh
set -euo pipefail
cd "$(dirname "$0")/.."
list=perf/expected-smoke-failures.tsv
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
# The smokes the job runs, read from it, and the list's own failures: today's run.
ran=$(grep -oE 'smoke .*scripts/load\.sh [a-z][a-z-]*' .github/workflows/ci.yml | awk '{print $NF}' | sort -u | paste -sd' ' -)
failed=$(grep -vE '^(#|$)' "$list" | cut -f1 | paste -sd' ' -)
[[ $(wc -w <<<"$ran") -ge 20 ]] || { echo "FAIL: read only '$ran' from ci.yml" >&2; exit 1; }

verdict() { scripts/smoke-verdict.sh "$@" >"$tmp/out" 2>&1; }
expect_pass() { verdict "$@" || { echo "FAIL: $label should pass:"; cat "$tmp/out"; exit 1; }; }
expect_fail() {
  if verdict "$@"; then echo "FAIL: $label should fail:"; cat "$tmp/out"; exit 1; fi
  grep -q -- "$want" "$tmp/out" || { echo "FAIL: $label: no '$want' in:"; cat "$tmp/out"; exit 1; }
}

label='the run as it stands'; expect_pass "$list" "$ran" "$failed"
label='nothing fails (every listed smoke passed)'; want='PASSED: remove it'; expect_fail "$list" "$ran" ""

first=$(cut -f1 <<<"$(grep -vE '^(#|$)' "$list" | head -1)")
grep -v "^$first	" "$list" >"$tmp/deleted.tsv"
label="mutant: $first deleted from the list"; want="smoke $first failed and is not an expected failure"
expect_fail "$tmp/deleted.tsv" "$ran" "$failed"

{ cat "$list"; printf 'warm-baseline\tD-000\tnobody\tplanted\tnothing\n'; } >"$tmp/planted.tsv"
label='mutant: a passing smoke (warm-baseline) planted on the list'; want='smoke warm-baseline PASSED: remove it'
expect_fail "$tmp/planted.tsv" "$ran" "$failed"

{ cat "$list"; printf 'gone\tD-000\tnobody\tplanted\tnothing\n'; } >"$tmp/gone.tsv"
label='a listed smoke that did not run'; want='lists gone, which did not run'
expect_fail "$tmp/gone.tsv" "$ran" "$failed gone"

printf 'assets\tD-218\t\tx\tsomething\n' >"$tmp/ownerless.tsv"
label='an entry without an owner'; want='needs a decision, an owner'
expect_fail "$tmp/ownerless.tsv" "$ran" "assets"

echo "smoke verdict: today's run passes; a deleted entry, a planted passing smoke, a smoke that didn't run and an ownerless entry each fail"
