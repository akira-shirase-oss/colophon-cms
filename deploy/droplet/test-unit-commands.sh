#!/usr/bin/env bash
# Every `colophon admin <subcommand>` the droplet's units and scripts run exists in the CLI they
# ship with: a renamed subcommand would fail a nightly timer silently (the ledger export once
# called `admin ledger export`, which the CLI has never had; the rehearsal found it).
#
#   deploy/droplet/test-unit-commands.sh <path to the colophon CLI>
set -euo pipefail
cli=$1
cd "$(dirname "$0")"
fail() { echo "FAIL: $*" >&2; exit 1; }
found=0
while IFS= read -r use; do
  found=$((found + 1))
  sub=${use#colophon admin }
  "$cli" admin "$sub" --help >/dev/null 2>&1 || fail "'$use' (in $(grep -lF "$use" ./*.service ./*.sh | paste -sd' ' -)) isn't a subcommand of this CLI"
done < <(grep -ohE 'colophon admin [a-z][a-z-]*( [a-z][a-z-]*)?' ./*.service ./*.sh | grep -v '^colophon admin --' | sed -E 's/ (--|[A-Z]).*//' | sort -u)
(( found > 0 )) || fail "no 'colophon admin' commands found to check"
echo "unit commands: all $found 'colophon admin' commands the droplet runs exist in the CLI"
