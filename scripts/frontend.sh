#!/usr/bin/env bash
# The frontend checks that need no server or stack: type checks for every package and app,
# the editor's unit tests (Vitest), and the Studio's Playwright suite in Chromium.
#
# Needs Node 22.17+ and Chromium for Playwright (npx -w @colophon/studio playwright install chromium).
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ ! -d node_modules ]]; then
  npm ci
fi
npm run check --workspaces --if-present
npm test --workspaces --if-present
npm run build -w @colophon/studio
npm run test:e2e -w @colophon/studio
