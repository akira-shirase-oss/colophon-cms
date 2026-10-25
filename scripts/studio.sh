#!/usr/bin/env bash
# Builds the release server and the Studio, then runs the Studio's Playwright suite against
# them and the local stack: the server serves the built Studio at /studio on port 58093.
#
# Needs the stack (scripts/stack.sh up), the pinned Rust toolchain, Node 22.17+, and Chromium
# for Playwright (npx -w @colophon/studio playwright install chromium).
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ ! -d node_modules ]]; then
  npm ci
fi

scripts/check-toolchain.sh >/dev/null
scripts/migrate.sh
cargo build --quiet --release --locked -p colophon-server
npm run build -w @colophon/studio
npx -w @colophon/studio playwright test -c playwright.server.config.ts
