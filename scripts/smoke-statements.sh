#!/usr/bin/env bash
# What Postgres counted for one smoke's window (pg_stat_statements, reset at its start), on stdout: the top ten
# statements by total time, with the shared buffers each read. Buffers per call tell a statement that walks more
# rows than it returns (dead index entries an open snapshot keeps unmarked) from one that only waits for a CPU on
# a saturated runner: the same time, very different reads. The smokes job writes it as <smoke>.statements.txt and
# scripts/save-smoke-statements.sh, before a stalled smoke's re-run, as <smoke>.stalled.statements.txt.
#
#   scripts/smoke-statements.sh
set -euo pipefail
cd "$(dirname "$0")/.."
docker compose -f deploy/compose.yaml exec -T postgres psql -U colophon -d postgres -XAtq -c \
  "SELECT round(total_exec_time::numeric, 1) AS total_ms, calls, round(mean_exec_time::numeric, 3) AS mean_ms, rows,
          shared_blks_hit, shared_blks_read,
          round((shared_blks_hit + shared_blks_read)::numeric / nullif(calls, 0), 1) AS blks_per_call,
          left(regexp_replace(query, '\\s+', ' ', 'g'), 160)
   FROM pg_stat_statements ORDER BY total_exec_time DESC LIMIT 10"
