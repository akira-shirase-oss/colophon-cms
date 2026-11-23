#!/usr/bin/env bash
# Every load smoke scripts/check.sh runs also runs in CI (D-209). Builders run only the quick half
# locally (format, lint, unit tests, frontend), and CI runs the full check, so a scenario that
# check.sh lists but no CI job runs runs nowhere — the outage drills cache-down and
# postgres-trouble did, on 2026-11-18. Fails naming each one missing from .github/workflows/ci.yml.
#
#   scripts/test-smoke-parity.sh
set -euo pipefail
cd "$(dirname "$0")/.."
# A scenario's mix (which operation, which target) comes from the scenario-wide iteration index,
# exec.scenario.iterationInTest, never __ITER: __ITER counts per VU, so with many VUs and few
# iterations each, slots past the first few never run (members at 20/s for 5 s made no writes at
# all), and a write share that depends on rate x duration / VUs under-measures writes. __ITER stays
# fine as part of a key or label (`${__VU}-${__ITER}`, an argument); in arithmetic or a comparison
# it's a mix selector, and fails here.
# `typeof __ITER` is the setup test (__ITER exists only in iterations), not a mix, so it's removed before matching.
mixed=$(awk '{ l = $0; gsub(/typeof __ITER/, "", l)
  if (l ~ /__ITER[[:space:]]*([-+*\/%<>]|[=!]=)|[-+*\/%][[:space:]]*__ITER([^A-Za-z0-9_]|$)/) print FILENAME ":" FNR ": " $0 }' \
  perf/k6/*.js perf/*.mjs 2>/dev/null || true)
if [[ -n "$mixed" ]]; then
  echo "FAIL: $(wc -l <<<"$mixed" | tr -d ' ') load-scenario line(s) choose their mix from __ITER, not exec.scenario.iterationInTest:" >&2
  echo "$mixed" >&2
  exit 1
fi
# Every Idempotency-Key a scenario sends is built by perf/k6/lib/idempotency-key.js, which refuses one outside the
# API's rule (16 to 128 letters, digits, '-' or '_') at the line that built it: hand-built ones fell under 16
# characters at small VU and iteration numbers and were refused 400 mid-run (changesets, CI 44632793507). A header
# value written as a literal, or a key helper returning a bare `k6-...` template, fails here.
handbuilt=$(grep -nE "'Idempotency-Key'[[:space:]]*:[[:space:]]*[\`'\"]|(return|=>)[[:space:]]+\`k6-" perf/k6/*.js || true)
if [[ -n "$handbuilt" ]]; then
  echo "FAIL: $(wc -l <<<"$handbuilt" | tr -d ' ') Idempotency-Key(s) built by hand, not through perf/k6/lib/idempotency-key.js:" >&2
  echo "$handbuilt" >&2
  exit 1
fi
# A key held in a name is checked at its definition: once its builder calls are removed, no template may remain, so
# a key derived by hand from a built one (undo's replay) or one built without the VU (bulk) fails here.
node scripts/check-idempotency-keys.mjs perf/k6/*.js >&2 \
  || { echo "FAIL: an Idempotency-Key above is built by hand, not by perf/k6/lib's builders" >&2; exit 1; }
# The scenario-wide index can give one VU the same slot twice, so a scenario that keeps per-VU state
# (a module-level `let`) must not depend on its slots coming in order: schedules did (schedule in
# slot 8, cancel in 9) and got 767 409s in CI 44632793507. Each such variable is marked on the line
# above with `// per-VU state: <why any slot order is valid>`, so the question is answered for it.
unmarked=$(for f in perf/k6/*.js; do
  grep -q 'iterationInTest' "$f" || continue
  awk -v f="$f" '/^(let|var) [A-Za-z_]/ && prev !~ /^\/\/ per-VU state: / {print f ":" NR ": " $0} {prev = $0}' "$f"
done)
if [[ -n "$unmarked" ]]; then
  echo "FAIL: per-VU state in a scenario whose mix comes from the scenario-wide iteration, not marked '// per-VU state: <why any slot order is valid>':" >&2
  echo "$unmarked" >&2
  exit 1
fi
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
