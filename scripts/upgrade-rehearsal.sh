#!/usr/bin/env bash
# The upgrade runbook's rehearsed half (deploy/runbooks/upgrade.md; D-130): against the real
# local stack and a scratch database it proves
#   - the live database reports `migrate-status` current (the pre-flight baseline);
#   - a never-migrated database reports `behind` with every migration pending, then `admin
#     migrate` brings it to the same applied set as the live database (binary owns the set);
#   - a database carrying a migration this binary doesn't know is refused at exit 4 — the
#     rollback gate (the schema is newer than the code; never swap that binary back);
#   - the serving smoke checks: /livez, /readyz, /version with the deployment identity.
#
# What it deliberately does NOT rehearse, because the honest answers live in the runbook:
# the binary swap on a live system, a staged roll across servers, and the support-window
# cleanup. Record: perf/results/<stamp>-upgrade-rehearsal-<machine>.json.
#
# Needs the local stack up (scripts/stack.sh up) plus cargo, jq and curl. Runs on macOS's
# bash 3.2; creates and drops one scratch database named colophon_ur_<hex>.
set -euo pipefail
cd "$(dirname "$0")/.."

. scripts/backup-crypt.sh   # machine_name only

compose() { docker compose -f deploy/compose.yaml "$@"; }

checks=0
failures=0
pass() { checks=$((checks + 1)); printf '  ok    %s\n' "$1"; }
fail() { checks=$((checks + 1)); failures=$((failures + 1)); printf '  FAIL  %s\n' "$1"; }

note() { printf '%s\n' "$1"; }
section() { printf '%s\n' "$1"; }

# The address the rehearsal's own server binds — off the stack's 58000 on purpose, so a
# running development server is never disturbed.
REHEARSAL_ADDR="${COLOPHON_REHEARSAL_ADDR:-127.0.0.1:58098}"
SERVER_PID=""
SCRATCH_DB=""
cleanup() {
  if [[ -n "$SERVER_PID" ]]; then kill "$SERVER_PID" 2>/dev/null || true; fi
  if [[ -n "$SCRATCH_DB" ]]; then
    compose exec -T postgres psql -U colophon -d postgres -XAtq \
      -c "DROP DATABASE IF EXISTS $SCRATCH_DB WITH (FORCE)" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

for tool in docker jq curl cargo; do
  command -v "$tool" >/dev/null || { echo "error: $tool is required" >&2; exit 1; }
done
compose exec -T postgres true 2>/dev/null || {
  echo "error: the postgres container is not running (scripts/stack.sh up)" >&2; exit 1; }

section "build"
cargo build --quiet --release --locked -p colophon-cli -p colophon-server
BIN=target/release/colophon
pass "the release binaries build (--locked)"

# The migrator URL pattern from deploy/local.env, for the live database and the scratch one.
set -a; . scripts/local-env.sh; set +a
live_url=${COLOPHON_MIGRATION_DATABASE_URL%/colophon}/colophon

section "pre-flight: the live database is current"
live=$(mktemp "${TMPDIR:-/tmp}/colophon-ur-live.XXXXXX")
"$BIN" admin migrate-status --database-url "$live_url" --json >"$live"
status=$(jq -r '.data.status' "$live")
if [[ "$status" == "current" ]]; then
  pass "live migrate-status: current ($(jq -r '.data.applied | length' "$live") applied)"
else
  fail "live migrate-status: $status — run 'admin migrate' before upgrading"
fi
live_applied=$(jq -c '.data.applied' "$live")

section "scratch: behind → migrate → current, against the same binary"
SCRATCH_DB="colophon_ur_$(openssl rand -hex 8)"
scratch_url=${COLOPHON_MIGRATION_DATABASE_URL%/colophon}/$SCRATCH_DB
compose exec -T postgres psql -U colophon -d postgres -XAtq \
  -c "CREATE DATABASE $SCRATCH_DB" >/dev/null

out=$(mktemp "${TMPDIR:-/tmp}/colophon-ur-status.XXXXXX")
"$BIN" admin migrate-status --database-url "$scratch_url" --json >"$out"
if [[ "$(jq -r '.data.status' "$out")" == behind ]] \
   && [[ "$(jq -r '.data.applied | length' "$out")" == 0 ]] \
   && [[ "$(jq -r '.data.pending | length' "$out")" -gt 0 ]]; then
  pass "a never-migrated database reports behind with $(jq -r '.data.pending | length' "$out") pending"
else
  fail "expected behind with pending migrations: $(cat "$out")"
fi

"$BIN" admin migrate --database-url "$scratch_url" --json >"$out"
"$BIN" admin migrate-status --database-url "$scratch_url" --json >"$out"
if [[ "$(jq -r '.data.status' "$out")" == current ]] \
   && [[ "$(jq -c '.data.applied' "$out")" == "$live_applied" ]]; then
  pass "admin migrate brings the scratch database to the live database's applied set"
else
  fail "after migrate: $(jq -c '.data' "$out")"
fi

section "the rollback gate: a schema newer than the binary is refused (exit 4)"
compose exec -T postgres psql -U colophon -d "$SCRATCH_DB" -XAtq -v ON_ERROR_STOP=1 -c \
  "INSERT INTO _sqlx_migrations
      (version, description, installed_on, success, checksum, execution_time)
   VALUES (9999, 'unknown-to-this-binary', now(), true, '\\x00', 0)" >/dev/null
rc=0
"$BIN" admin migrate-status --database-url "$scratch_url" --json >"$out" || rc=$?
if [[ "$rc" == 4 ]] && [[ "$(jq -r '.data.status' "$out")" == unknown ]]; then
  pass "an unknown migration is refused at exit 4 — the old binary must not come back"
else
  fail "expected exit 4 + status unknown, got exit $rc: $(cat "$out")"
fi
# Clean the planted row before the server-smoke half points at nothing — scratch dies anyway.
compose exec -T postgres psql -U colophon -d "$SCRATCH_DB" -XAtq \
  -c "DELETE FROM _sqlx_migrations WHERE version = 9999" >/dev/null

section "serving smoke checks on a rehearsal server ($REHEARSAL_ADDR)"
COLOPHON_LISTEN_ADDR=$REHEARSAL_ADDR COLOPHON_LOG=warn \
  target/release/colophon-server >"${TMPDIR:-/tmp}/colophon-ur-server.log" 2>&1 &
SERVER_PID=$!
ready=0
for _ in $(seq 1 40); do
  curl -fsS -o /dev/null "http://$REHEARSAL_ADDR/livez" 2>/dev/null && { ready=1; break; }
  sleep 0.5
done
if [[ "$ready" == 1 ]]; then
  pass "/livez answers 200 on the rehearsal server"
else
  fail "/livez never answered (server log: ${TMPDIR:-/tmp}/colophon-ur-server.log)"
fi

readyz=$(curl -fsS "http://$REHEARSAL_ADDR/readyz" 2>/dev/null || true)
readyz_status=$(printf '%s' "$readyz" | jq -r '.status // empty' 2>/dev/null)
if [[ "$readyz_status" == ready || "$readyz_status" == degraded ]]; then
  pass "/readyz answers 200 ($readyz_status)"
else
  fail "/readyz answered '$readyz_status' ($readyz)"
fi

version_body=$(curl -fsS "http://$REHEARSAL_ADDR/version" 2>/dev/null || true)
served_commit=$(printf '%s' "$version_body" | jq -r '.commit // empty' 2>/dev/null)
built_commit=$(git rev-parse HEAD)
if [[ -n "$served_commit" && "$served_commit" == "$built_commit" ]]; then
  pass "/version reports the deployed commit (${served_commit:0:12})"
else
  fail "/version commit $served_commit != built $built_commit — the served binary isn't this build"
fi

kill "$SERVER_PID" 2>/dev/null || true
wait "$SERVER_PID" 2>/dev/null || true
SERVER_PID=""

stamp=$(date -u +%Y-%m-%dT%H%M%SZ)
machine=$(machine_name)
file="perf/results/$stamp-upgrade-rehearsal-$machine.json"
jq -n --arg stamp "$stamp" --arg commit "$(git rev-parse HEAD)" \
  --argjson dirty "$(git status --porcelain | grep -c . || true)" \
  --arg machine "$machine" --arg scratch "$SCRATCH_DB" \
  --argjson checks "$checks" --argjson failures "$failures" \
  --slurpfile live "$live" \
  '{
    scenario: "upgrade-rehearsal", date: $stamp, commit: $commit,
    dirty_files: $dirty, machine: $machine,
    rehearsed: [
      "live admin migrate-status (the pre-flight baseline)",
      "scratch database: behind → admin migrate → current, same applied set as live",
      "unknown migration → exit 4: the rollback gate against a newer schema",
      "serving smoke: /livez, /readyz, /version deployment identity on a rehearsal port"
    ],
    not_rehearsed: [
      "the binary swap on a live system (runbook step 5)",
      "a staged roll across several servers",
      "support-window field removal after backfill"
    ],
    live_migrate_status: $live[0].data,
    scratch_database: $scratch,
    checks: $checks, failures: $failures,
    verdict: (if $failures == 0 then "runbook-halves-proven" else "failed" end)
  }' >"$file"
echo "result: $file"
if (( failures != 0 )); then echo "FAIL: $failures of $checks checks" >&2; exit 1; fi
echo "PASS: upgrade runbook's local halves proven ($checks checks)"
