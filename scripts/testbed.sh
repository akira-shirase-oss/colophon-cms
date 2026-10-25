#!/usr/bin/env bash
# Builds the release server and the testbed, then runs the Playwright suite against the local
# stack (D-018). The suite starts its own server and testbed preview on ports 58091 and 58101.
#
#   scripts/testbed.sh           build and test
#   scripts/testbed.sh --dev     instead: run the testbed dev server on http://127.0.0.1:58100
#                                against a colophon-server on 127.0.0.1:58000 that you start
#
# Needs the stack (scripts/stack.sh up), the pinned Rust toolchain, Node 22.17+, and Chromium
# for Playwright (npx -w @colophon/testbed playwright install chromium).
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ ! -d node_modules ]]; then
  npm ci
fi

if [[ "${1:-}" == "--dev" ]]; then
  exec npm run dev -w @colophon/testbed
fi

scripts/check-toolchain.sh >/dev/null
cargo build --quiet --release --locked -p colophon-server
npm run check -w @colophon/testbed
npm run build -w @colophon/testbed
npm run test:e2e -w @colophon/testbed
