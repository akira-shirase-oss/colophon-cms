#!/usr/bin/env bash
# The smokes job's pre-re-run hook (scripts/run-smoke.sh's RUN_SMOKE_BEFORE_RERUN): before a stalled smoke runs
# again, keep what Postgres counted for the stalled attempt as <stem>.stalled.statements.txt, the same query the
# job writes <smoke>.statements.txt with, then reset pg_stat_statements so the re-run's statements are its own.
#
#   scripts/save-smoke-statements.sh <stem>     (the smoke's path without .record.json)
set -euo pipefail
stem=$1
cd "$(dirname "$0")/.."
pgq() { docker compose -f deploy/compose.yaml exec -T postgres psql -U colophon -d postgres -XAtq "$@"; }
pgq -c "SELECT round(total_exec_time::numeric, 1) AS total_ms, calls, round(mean_exec_time::numeric, 3) AS mean_ms, rows, left(regexp_replace(query, '\\s+', ' ', 'g'), 160) FROM pg_stat_statements ORDER BY total_exec_time DESC LIMIT 10" \
  >"$stem.stalled.statements.txt" 2>&1 || true
pgq -c "SELECT pg_stat_statements_reset()" >/dev/null
