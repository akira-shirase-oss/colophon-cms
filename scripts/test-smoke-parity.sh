#!/usr/bin/env bash
# Every load smoke scripts/check.sh runs also runs in CI (D-209). Builders run only the quick half
# locally (format, lint, unit tests, frontend), and CI runs the full check, so a scenario that
# check.sh lists but no CI job runs runs nowhere — the outage drills cache-down and
# postgres-trouble did, on 2026-11-18. Fails naming each one missing from .github/workflows/ci.yml.
#
#   scripts/test-smoke-parity.sh
set -euo pipefail
cd "$(dirname "$0")/.."
endpoints() { grep -oE 'scripts/load\.sh [a-z][a-z-]*' "$1" | awk '{print $2}' | sort -u; }
local_list=$(endpoints scripts/check.sh)
ci_list=$(endpoints .github/workflows/ci.yml)
[[ -n "$local_list" ]] || { echo "FAIL: no load.sh scenario found in scripts/check.sh" >&2; exit 1; }
missing=$(comm -23 <(echo "$local_list") <(echo "$ci_list"))
if [[ -n "$missing" ]]; then
  echo "FAIL: check.sh runs $(wc -l <<<"$missing" | tr -d ' ') load smoke(s) no CI job runs: $(paste -sd' ' - <<<"$missing")" >&2
  exit 1
fi
echo "smoke parity: all $(wc -l <<<"$local_list" | tr -d ' ') of check.sh's load smokes run in CI"
