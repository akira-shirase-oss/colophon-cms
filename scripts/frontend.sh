#!/usr/bin/env bash
# The frontend checks that need no server or stack: type checks for every package and app,
# the editor's unit tests (Vitest), the npm packages' packaging gate, and the Studio's
# Playwright suite in Chromium.
#
# Needs Node 22.17+ and Chromium for Playwright (npx -w @colophon/studio playwright install chromium).
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ ! -d node_modules ]]; then
  npm ci
fi
# The design tokens (D-210): every text/surface pair in both themes meets its WCAG bar, the committed table
# is current, and @colophon/ui's copy of design/tokens.css is a byte copy.
node scripts/design-contrast.mjs >/dev/null
node scripts/design-contrast.mjs --check docs/design/contrast.md >/dev/null
node scripts/design-contrast.mjs --check-copy packages/ui/src/lib/tokens.css
npm run check --workspaces --if-present
npm test --workspaces --if-present
# The load scenarios' shared helpers (perf/k6/lib): the Idempotency-Key builder against the API's rule.
node --test perf/k6/lib/*.test.mjs
# What npm would ship of @colophon-cms/sdk and @colophon-cms/rich-text, installed and used from
# the tarballs (D-170): allowlist, budgets, publint, attw, Node, TypeScript, a browser bundle.
node packages/sdk/scripts/pack-check.mjs
# The SDK's load smoke: delivery reads through the packed SDK against a stub in its own process,
# beside bare fetch (perf/node/sdk-delivery.mjs; full runs are TOTAL=1000000 on a quiet machine).
# The overhead budget is loosened for a shared runner; failures and heap growth are not.
TOTAL=20000 OVERHEAD_P99_MS=25 KEEP=0 node --expose-gc perf/node/sdk-delivery.mjs
npm run build -w @colophon/studio
npm run test:e2e -w @colophon/studio
