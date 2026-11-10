#!/usr/bin/env bash
# P6 soak sidecar (perf/LOAD_MATRIX.md): one failure drill per hour while the load run
# goes — pause Valkey for 60 s, pause Postgres for 30 s, alternating; every sixth hour a
# session-expiry drill instead (see below). Every drill is timestamped into the log so the
# run's result can be read against them: the outage windows are the evidence, not noise to
# be explained away afterwards.
#
#   scripts/soak-sidecar.sh [hours] [logfile] [windows_jsonl]
#
# defaults: 24 hours, perf/results/soak-sidecar.log, no windows file.
# SOAK_DRILL_INTERVAL_S compresses the cadence for shorter validations (default 3600 —
# the hourly contract); SOAK_DRILL_GRACE_S sets the recovery grace (default 120).
#
# The third argument, when given, receives one JSON object per completed drill —
# {"dep","kind","start_epoch","end_epoch","start","end"} — the machine-readable form of
# the same windows. Pass it to load.sh as DRILL_WINDOWS=<path>: mixed.js then excuses
# failures inside the windows (a paused dependency refuses load by design) and the gate
# means "no errors outside them". `end` is the resume time plus GRACE_S below, covering
# requests still in flight and the pool's drain; the run's /readyz samples independently
# prove the service came back.
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
windows="${3:-}"
database="${COLOPHON_TEST_DATABASE:-colophon}"
compose=(docker compose -f deploy/compose.yaml)
# Recovery grace appended to every window's end: requests issued in a pause's last
# second can fail at resume + the request deadline, and the pool needs a moment to hand
# out healthy connections again (the 2026-11-10 1 h run's refusals ran ~60-90 s past a
# 30 s pause). 120 s covers that with margin; /readyz proves the real recovery.
grace_s="${SOAK_DRILL_GRACE_S:-120}"
interval_s="${SOAK_DRILL_INTERVAL_S:-3600}"

mkdir -p "$(dirname "$log")"
if [[ -n "$windows" ]]; then
  mkdir -p "$(dirname "$windows")"
  # Fresh file per run: a reused path must never carry windows that excuse this run.
  : >"$windows"
fi

# Appends one window record. Args: dep kind start_epoch end_epoch.
record_window() {
  [[ -z "$windows" ]] && return 0
  local dep=$1 kind=$2 start=$3 end=$4 end_iso
  end_iso=$(date -u -d "@$end" +%FT%TZ 2>/dev/null || date -u -r "$end" +%FT%TZ)
  printf '{"dep":"%s","kind":"%s","start_epoch":%s,"end_epoch":%s,"start":"%s","end":"%s"}\n' \
    "$dep" "$kind" "$start" "$end" "$(date -u -r "$start" +%FT%TZ 2>/dev/null || date -u -d "@$start" +%FT%TZ)" "$end_iso" \
    >>"$windows"
}

echo "$(date -u +%FT%TZ) sidecar armed: drills every ${interval_s}s for ${hours}h -> $log${windows:+, windows -> $windows}" >>"$log"

end_epoch=$(( $(date +%s) + hours * 3600 ))
drill_n=0
while :; do
  now=$(date +%s)
  remaining=$(( end_epoch - now ))
  (( remaining <= 0 )) && break
  sleep "$(( remaining < interval_s ? remaining : interval_s ))"
  now=$(date +%s)
  remaining=$(( end_epoch - now ))
  # A drill needs its pause plus unpause retries (~75 s worst case) inside the run;
  # starting one with less than that left lands it after k6 has already stopped.
  if (( remaining < 90 )); then
    echo "$(date -u +%FT%TZ) sidecar: ${remaining}s of run left — last drill skipped" >>"$log"
    break
  fi
  drill_n=$(( drill_n + 1 ))
  if (( drill_n % 6 == 0 )); then
    # Session-expiry drill: every live session in the server's database dies at once —
    # the same 401 wave the 12 h absolute lifetime dealt the first soak, on demand.
    echo "$(date -u +%FT%TZ) drill $drill_n: expire all sessions in $database" >>"$log"
    drill_start=$(date +%s)
    if "${compose[@]}" exec -T postgres psql -U colophon -d "$database" -XAtqc \
      "update sessions set expires_at = now() where expires_at > now()" >>"$log" 2>&1; then
      echo "$(date -u +%FT%TZ) drill $drill_n: sessions expired" >>"$log"
      record_window sessions session "$drill_start" "$(( $(date +%s) + grace_s ))"
    else
      echo "$(date -u +%FT%TZ) drill $drill_n: session expiry FAILED — sessions untouched" >>"$log"
    fi
    continue
  fi
  if (( drill_n % 2 == 1 )); then
    dep=valkey; secs=60
  else
    dep=postgres; secs=30
  fi
  echo "$(date -u +%FT%TZ) drill $drill_n: pause $dep ${secs}s" >>"$log"
  drill_start=$(date +%s)
  if ! "${compose[@]}" pause "$dep" >>"$log" 2>&1; then
    echo "$(date -u +%FT%TZ) drill $drill_n: pause $dep FAILED — dependency untouched" >>"$log"
    continue
  fi
  sleep "$secs"
  resumed=0
  for try in 1 2 3; do
    if "${compose[@]}" unpause "$dep" >>"$log" 2>&1; then
      resumed=1
      break
    fi
    echo "$(date -u +%FT%TZ) drill $drill_n: unpause $dep retry $try" >>"$log"
    sleep 5
  done
  state=$("${compose[@]}" ps "$dep" --format '{{.State}}' 2>/dev/null | head -1 || echo "unknown")
  # The window covers the pause plus the drain: end is now (resumed or not — a stuck
  # pause keeps producing failures the gate should see as outside-window once this
  # window closes) plus the recovery grace.
  record_window "$dep" pause "$drill_start" "$(( $(date +%s) + grace_s ))"
  if (( resumed == 1 )); then
    echo "$(date -u +%FT%TZ) drill $drill_n: $dep resumed (state: $state)" >>"$log"
  else
    echo "$(date -u +%FT%TZ) drill $drill_n: !!! $dep STILL PAUSED after 3 unpause tries (state: $state) — unpause it by hand" >>"$log"
  fi
done
echo "$(date -u +%FT%TZ) sidecar done after $drill_n drills" >>"$log"
