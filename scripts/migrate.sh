#!/usr/bin/env bash
# Applies pending migrations to the local stack's database as the migrator role
# (deploy/local.env). scripts/load.sh, scripts/testbed.sh and CI run it before starting a server.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/check-toolchain.sh >/dev/null
cargo build --quiet --release --locked -p colophon-cli
(
  set -a
  # shellcheck source=/dev/null # plain KEY=value lines
  . deploy/local.env
  set +a
  exec target/release/colophon admin migrate
)
