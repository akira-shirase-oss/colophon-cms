#!/usr/bin/env bash
# Applies pending migrations to the local stack's database as the migrator role
# (deploy/local.env), then bootstraps the development realm's dev-owner as the installation's
# first administrator (D-043; a no-op once done). scripts/load.sh, scripts/testbed.sh and CI run
# it before starting a server.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/check-toolchain.sh >/dev/null
cargo build --quiet --release --locked -p colophon-cli
(
  # shellcheck source=scripts/local-env.sh
  . scripts/local-env.sh
  target/release/colophon admin migrate
  # dev-owner's subject is fixed in deploy/keycloak/colophon-realm.json.
  exec target/release/colophon admin bootstrap --subject 00000000-0000-4000-8000-00000000c001
)
