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
npm run check --workspaces --if-present
npm test --workspaces --if-present
# What npm would ship of @colophon-cms/sdk and @colophon-cms/rich-text, installed and used from
# the tarballs (D-170): allowlist, budgets, publint, attw, Node, TypeScript, a browser bundle.
node packages/sdk/scripts/pack-check.mjs
npm run build -w @colophon/studio
npm run test:e2e -w @colophon/studio
