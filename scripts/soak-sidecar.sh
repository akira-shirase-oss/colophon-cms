#!/usr/bin/env bash
# P6 soak sidecar (perf/LOAD_MATRIX.md): one failure drill per hour while the load run
# goes — pause Valkey for 60 s, pause Postgres for 30 s, alternating; every sixth hour a
# session-expiry drill instead (see below). Every drill is timestamped into the log so the
# run's result can be read against them: the outage windows are the evidence, not noise to
# be explained away afterwards.
#
#   scripts/soak-sidecar.sh [hours] [logfile]
#
# defaults: 24 hours, perf/results/soak-sidecar.log
#
# The matrix's third drill — a receiver 503 window — applies to scenarios that configure
# delivery destinations; the mixed soak configures none, so there is nothing to pause.
# The session drill exists because the dev accounts' absolute session lifetime is 12 h:
# the 2026-11-09 soak outlived its sign-ins and collected 401s for its whole second half.
# mixed.js now re-authenticates on 401; expiring every live session on a schedule
# exercises that path deliberately instead of leaving it to wall-clock luck. It runs
# against the same database load.sh's server uses (COLOPHON_TEST_DATABASE or `colophon`).
# A drill that cannot be unpaused is retried loudly and the dependency's state recorded:
# a stuck-paused Postgres or Valkey would silently invalidate the whole soak.
set -euo pipefail
cd "$(dirname "$0")/.."

hours="${1:-24}"
log="${2:-perf/results/soak-sidecar.log}"
database="${COLOPHON_TEST_DATABASE:-colophon}"
compose=(docker compose -f deploy/compose.yaml)

mkdir -p "$(dirname "$log")"
echo "$(date -u +%FT%TZ) sidecar armed: $hours hourly drills -> $log" >>"$log"

for hour in $(seq 1 "$hours"); do
  sleep 3600
  if (( hour % 6 == 0 )); then
    # Session-expiry drill: every live session in the server's database dies at once —
    # the same 401 wave the 12 h absolute lifetime dealt the first soak, on demand.
    echo "$(date -u +%FT%TZ) drill $hour: expire all sessions in $database" >>"$log"
    if "${compose[@]}" exec -T postgres psql -U colophon -d "$database" -XAtqc \
      "update sessions set expires_at = now() where expires_at > now()" >>"$log" 2>&1; then
      echo "$(date -u +%FT%TZ) drill $hour: sessions expired" >>"$log"
    else
      echo "$(date -u +%FT%TZ) drill $hour: session expiry FAILED — sessions untouched" >>"$log"
    fi
    continue
  fi
  if (( hour % 2 == 1 )); then
    dep=valkey; secs=60
  else
    dep=postgres; secs=30
  fi
  echo "$(date -u +%FT%TZ) drill $hour: pause $dep ${secs}s" >>"$log"
  if ! "${compose[@]}" pause "$dep" >>"$log" 2>&1; then
    echo "$(date -u +%FT%TZ) drill $hour: pause $dep FAILED — dependency untouched" >>"$log"
    continue
  fi
  sleep "$secs"
  resumed=0
  for try in 1 2 3; do
    if "${compose[@]}" unpause "$dep" >>"$log" 2>&1; then
      resumed=1
      break
    fi
    echo "$(date -u +%FT%TZ) drill $hour: unpause $dep retry $try" >>"$log"
    sleep 5
  done
  state=$("${compose[@]}" ps "$dep" --format '{{.State}}' 2>/dev/null | head -1 || echo "unknown")
  if (( resumed == 1 )); then
    echo "$(date -u +%FT%TZ) drill $hour: $dep resumed (state: $state)" >>"$log"
  else
    echo "$(date -u +%FT%TZ) drill $hour: !!! $dep STILL PAUSED after 3 unpause tries (state: $state) — unpause it by hand" >>"$log"
  fi
done
echo "$(date -u +%FT%TZ) sidecar done" >>"$log"
