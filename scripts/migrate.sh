#!/usr/bin/env bash
# Applies pending migrations to the local stack's database as the migrator role
# (deploy/local.env), then provisions the built-in dev accounts — dev-owner@colophon.test
# (administrator) and dev-editor@colophon.test, password colophon-dev (D-146; idempotent).
# scripts/load.sh, scripts/testbed.sh and CI run it before starting a server.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/check-toolchain.sh >/dev/null
cargo build --quiet --release --locked -p colophon-cli
(
  # shellcheck source=scripts/local-env.sh
  . scripts/local-env.sh
  target/release/colophon admin migrate
  # dev-owner and dev-editor are built-in password accounts (@colophon.test / colophon-dev) —
  # provisioned idempotently, no identity provider needed (D-146); only dev-owner is an admin.
  target/release/colophon admin account --email dev-owner@colophon.test --password colophon-dev --display-name "Dev Owner" --admin
  exec target/release/colophon admin account --email dev-editor@colophon.test --password colophon-dev --display-name "Dev Editor"
)
