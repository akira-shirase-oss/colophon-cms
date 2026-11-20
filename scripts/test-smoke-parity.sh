#!/usr/bin/env bash
# Every load smoke scripts/check.sh runs also runs in CI (D-209). Builders run only the quick half
# locally (format, lint, unit tests, frontend), and CI runs the full check, so a scenario that
# check.sh lists but no CI job runs runs nowhere — the outage drills cache-down and
# postgres-trouble did, on 2026-11-18. Fails naming each one missing from .github/workflows/ci.yml.
#
#   scripts/test-smoke-parity.sh
set -euo pipefail
cd "$(dirname "$0")/.."
# The scenarios a text names as `scripts/load.sh <endpoint>`; nothing found is an empty list, not an exit.
endpoints() { { grep -oE 'scripts/load\.sh [a-z][a-z-]*' || true; } | awk '{print $2}' | sort -u; }
# check.sh's list is read from the plan it runs (its smokes are data, SMOKES=(...), not literal commands), as
# the stack's owner sees it: on a shared stack it leaves the two that pause services out, to CI.
local_list=$(COLOPHON_STACK_OWNER=1 scripts/check.sh --integration-plan | endpoints)
ci_list=$(endpoints <.github/workflows/ci.yml)
[[ -n "$local_list" ]] || { echo "FAIL: no load.sh scenario found in scripts/check.sh" >&2; exit 1; }
missing=$(comm -23 <(echo "$local_list") <(echo "$ci_list"))
if [[ -n "$missing" ]]; then
  echo "FAIL: check.sh runs $(wc -l <<<"$missing" | tr -d ' ') load smoke(s) no CI job runs: $(paste -sd' ' - <<<"$missing")" >&2
  exit 1
fi
# Every smoke has exactly one kind in perf/smoke-kinds.tsv (D-223: write smokes will scale by the write probe, read
# smokes by health), so none can silently take the wrong scale; and its limiters are perf/limiter-smokes.tsv's.
kinds=perf/smoke-kinds.tsv
kinded=$(grep -vE '^(#|$)' "$kinds" | cut -f1 | sort)
dupes=$(uniq -d <<<"$kinded")
[[ -z "$dupes" ]] || { echo "FAIL: $kinds lists a smoke twice: $(paste -sd' ' - <<<"$dupes")" >&2; exit 1; }
unkinded=$(comm -23 <(echo "$local_list") <(echo "$kinded"))
[[ -z "$unkinded" ]] || { echo "FAIL: check.sh runs smoke(s) with no kind in $kinds: $(paste -sd' ' - <<<"$unkinded")" >&2; exit 1; }
stray=$(comm -13 <(echo "$local_list") <(echo "$kinded"))
[[ -z "$stray" ]] || { echo "FAIL: $kinds lists smoke(s) check.sh doesn't run: $(paste -sd' ' - <<<"$stray")" >&2; exit 1; }
badkind=$(grep -vE '^(#|$)' "$kinds" | awk -F'\t' '$2 !~ /^(read|write|drill|limiter)$/ {print $1}')
[[ -z "$badkind" ]] || { echo "FAIL: $kinds: a kind is read, write, drill or limiter: $(paste -sd' ' - <<<"$badkind")" >&2; exit 1; }
klimiters=$(grep -vE '^(#|$)' "$kinds" | awk -F'\t' '$2 == "limiter" {print $1}' | sort)
[[ "$klimiters" == "$(grep -vE '^(#|$)' perf/limiter-smokes.tsv | cut -f1 | sort)" ]] \
  || { echo "FAIL: $kinds's limiters aren't perf/limiter-smokes.tsv's" >&2; exit 1; }
# A limiter smoke (perf/limiter-smokes.tsv) exists to exceed a fixed limit, so CI may never scale its rate to the
# runner: spike at a scaled 173/s spiked to 865/s, under the 1000/s delivery limit, and measured no refusal (CI
# 44620651187). Each listed smoke must be a real one that CI runs, and no CI line of it may use rate().
limiters=$(grep -vE '^(#|$)' perf/limiter-smokes.tsv | cut -f1)
[[ -n "$limiters" ]] || { echo "FAIL: perf/limiter-smokes.tsv lists no limiter smoke" >&2; exit 1; }
bad=""
while read -r name; do
  grep -qx "$name" <<<"$local_list" || bad+=" $name (not a check.sh smoke)"
  lines=$(grep -E "scripts/load\.sh $name( |$)" .github/workflows/ci.yml || true)
  [[ -n "$lines" ]] || { bad+=" $name (no CI line)"; continue; }
  # shellcheck disable=SC2016 # the literal text `$(rate ` in ci.yml, not an expansion
  if grep -qF '$(rate ' <<<"$lines"; then bad+=" $name (its rate is scaled with rate())"; fi
done <<<"$limiters"
if [[ -n "$bad" ]]; then
  echo "FAIL: a limiter smoke must run at its fixed rate in CI:$bad" >&2
  exit 1
fi
echo "smoke parity: all $(wc -l <<<"$local_list" | tr -d ' ') of check.sh's load smokes run in CI; $(wc -l <<<"$limiters" | tr -d ' ') limiter smokes unscaled"
