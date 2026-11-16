#!/usr/bin/env bash
# No text under 14px anywhere in the Studio, the testbed, or the shared packages (D-057's
# reading floor). `text-xs` is remapped to 0.875rem in packages/ui's @theme, and this fails if
# anything else in source renders smaller: arbitrary text-[…] sizes, a smaller utility
# (text-2xs), or a raw font-size below 0.875rem/0.875em/14px.
#
# Usage: scripts/check-typography.sh
set -euo pipefail
cd "$(dirname "$0")/.."

found=$(
  grep -rEn \
    'font-size: *0\.[0-7][0-9]*(rem|em)|font-size: *0\.8[0-6][0-9]*(rem|em)|font-size: *0\.87[0-4][0-9]*(rem|em)|font-size: *([0-9]|1[0-3])(\.[0-9]+)?px|text-\[0\.[0-7][0-9]*(rem|em)\]|text-\[0\.8[0-6][0-9]*(rem|em)\]|text-\[0\.87[0-4][0-9]*(rem|em)\]|text-\[([0-9]|1[0-3])(\.[0-9]+)?px\]|text-2xs' \
    --include='*.svelte' --include='*.css' \
    apps/*/src packages/*/src 2>/dev/null || true
)

if [[ -n "$found" ]]; then
  echo "error: text under 14px is not allowed anywhere (the smallest is text-xs at 0.875rem):" >&2
  echo "$found" >&2
  exit 1
fi

echo "typography ok: nothing under 14px in apps/*/src or packages/*/src"
