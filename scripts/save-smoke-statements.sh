#!/usr/bin/env bash
# The smokes job's pre-re-run hook (scripts/run-smoke.sh's RUN_SMOKE_BEFORE_RERUN): before a stalled smoke runs
# again, keep what Postgres counted for the stalled attempt as <stem>.stalled.statements.txt (scripts/smoke-statements.sh,
# as the job writes <smoke>.statements.txt), then reset pg_stat_statements so the re-run's statements are its own.
#
#   scripts/save-smoke-statements.sh <stem>     (the smoke's path without .record.json)
set -euo pipefail
stem=$1
cd "$(dirname "$0")/.."
pgq() { docker compose -f deploy/compose.yaml exec -T postgres psql -U colophon -d postgres -XAtq "$@"; }
scripts/smoke-statements.sh >"$stem.stalled.statements.txt" 2>&1 || true
pgq -c "SELECT pg_stat_statements_reset()" >/dev/null
