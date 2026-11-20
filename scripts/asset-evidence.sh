#!/usr/bin/env bash
# D-218's evidence after an assets load run: where an upload's time to `usable` goes.
#
#   scripts/asset-evidence.sh SERVER_LOG [DATABASE]      (DATABASE defaults to colophon; the compose stack's postgres)
#
# From asset_jobs, every verify job's created → finished time (what a client waits, queue and work together) and the
# backlog second by second (jobs created minus jobs finished so far). From the worker's debug lines (the server run
# with LOAD_LOG=...,colophon_worker=debug): each attempt's own work time, each verify's fetch / decode / land split,
# and how many jobs each claim took against the free slots. Wait ≈ created→finished minus work.
set -euo pipefail
log=$1
database=${2:-colophon}
psql() { docker compose -f deploy/compose.yaml exec -T postgres psql -U colophon -d "$database" -XAq "$@"; }

echo "## asset_jobs: created → finished, verify jobs that succeeded (ms)"
psql -c "SELECT count(*) AS jobs,
                round(percentile_cont(0.5) WITHIN GROUP (ORDER BY ms)) AS p50,
                round(percentile_cont(0.95) WITHIN GROUP (ORDER BY ms)) AS p95,
                round(percentile_cont(0.99) WITHIN GROUP (ORDER BY ms)) AS p99,
                round(max(ms)) AS max
         FROM (SELECT extract(epoch FROM finished_at - created_at) * 1000 AS ms
               FROM asset_jobs WHERE kind = 'verify' AND status = 'succeeded') t"

echo "## asset_jobs: the backlog by second (created so far minus finished so far), its five highest"
psql -c "WITH s AS (SELECT generate_series(date_trunc('second', min(created_at)), max(coalesce(finished_at, created_at)), '1 second') AS at
                    FROM asset_jobs WHERE kind = 'verify')
         SELECT to_char(at, 'HH24:MI:SS') AS second,
                (SELECT count(*) FROM asset_jobs WHERE kind = 'verify' AND created_at < at + interval '1 second') AS created,
                (SELECT count(*) FROM asset_jobs WHERE kind = 'verify' AND finished_at < at + interval '1 second') AS finished,
                (SELECT count(*) FROM asset_jobs WHERE kind = 'verify' AND created_at < at + interval '1 second')
                - (SELECT count(*) FROM asset_jobs WHERE kind = 'verify' AND finished_at < at + interval '1 second') AS backlog
         FROM s ORDER BY backlog DESC, at LIMIT 5"

# Percentiles of one numeric field over the log lines with one message.
pct() {
  jq -R -r --arg m "$1" --arg f "$2" 'fromjson? | select(.message == $m) | .[$f] // empty' "$log" | sort -n |
    awk -v name="$1: $2" 'function q(p, i) {i = int(NR * p); if (i < NR * p) i++; return v[i < 1 ? 1 : i]}
      {v[NR] = $1} END {
      if (!NR) {print name ": none logged"; exit}
      printf "%s: n %d, p50 %s, p95 %s, p99 %s, max %s\n", name, NR, q(0.5), q(0.95), q(0.99), v[NR]}'
}
echo "## the worker's own times (ms) and claims"
pct "asset attempt done" work_ms
pct "asset verified" fetch_ms
pct "asset verified" decode_ms
pct "asset verified" land_ms
pct "asset jobs claimed" claimed
pct "asset jobs claimed" free
