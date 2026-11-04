#!/usr/bin/env bash
# The restore procedure (module 18 "Restore procedure", spec §33; O2, O3; F13/F14; D-126):
# bring a captured backup back on CLEAN infrastructure, in six ordered stages, each a marker
# the next stage checks — a half-done restore is inspectable, never silently rerun.
#
#   scripts/restore.sh BACKUP_DIR [options]          run every stage still pending
#   scripts/restore.sh BACKUP_DIR --step STAGE       run exactly one stage
#   scripts/restore.sh BACKUP_DIR --from STAGE       run that stage and the ones after it
#   scripts/restore.sh status DIR                    print the stage markers
#   scripts/restore.sh teardown DIR [--purge]        drop the scratch containers/volume
#   scripts/restore.sh drill [--keep]                the full P6 drill with evidence in
#                                                    perf/results/<stamp>-restore-drill-<machine>.json
#
# Options: --into DIR (workspace, default restores/<backup-stamp>), --ledger FILE (a
# deletion-ledger export; repeatable — the interval must reach back to the backup's own
# watermark), --resume-jobs (reconcile returns held jobs to pending), --force (rerun a
# completed stage), --keep (drill leaves the scratch stack up).
#
# The stages (each refuses to run before the previous stage's marker says done):
#   1 prepare    decrypt base.tar.gz.<enc> into a fresh scratch volume — clean infrastructure,
#                never the live stack's directories or ports.
#   2 verify     pg_verifybackup on the restored bytes before postgres first starts, then the
#                schema check: the restored database's migrations against the manifest and this
#                binary's set — a backup newer than the code is refused.
#   3 epoch      `admin restore epoch`: every environment's epoch advances, exactly once —
#                the recovery epoch (D4) that O2 demands before anything serves.
#   4 ledger     `admin ledger-replay` for every export file that continues the backup's own
#                ledger watermark. A gap the files don't cover, or a row that can't replay,
#                blocks the restore — O2.
#   5 reconcile  `admin restore hold` freezes every unfinished job ('held' can't be claimed,
#                retried or expired), `admin restore jobs` lists what the operator reconciles;
#                --resume-jobs is the explicit enable (O3: nothing dispatches blind).
#   6 serve      only after all five markers: start colophon-server --role api on the scratch
#                database and wait for /readyz — the gate that finally serves.
#
# The scratch stack is a docker container + named volume on its own port, plus a scratch
# valkey for the server's cache: nothing the restore creates collides with the live stack,
# and nothing it reads back (WAL archive included) is ever written — O5.
#
# Runs on macOS's bash 3.2; needs docker, jq, openssl, curl and the encrypting tool the
# backup was made with. Secrets stay in their files (scripts/backup-crypt.sh); the database
# URLs the admin commands need ride in the environment, never in an argument.
set -euo pipefail
cd "$(dirname "$0")/.."

. scripts/backup-crypt.sh

STAGES="prepare verify epoch ledger reconcile serve"

# Scratch addressing: everything lands on 127.0.0.1, on ports the compose stack doesn't use.
# If a chosen port is already bound the script bumps it until it finds a free one.
PG_PORT="${COLOPHON_RESTORE_PG_PORT:-55439}"
HTTP_PORT="${COLOPHON_RESTORE_HTTP_PORT:-58100}"
VALKEY_PORT="${COLOPHON_RESTORE_VALKEY_PORT:-56380}"
ROOT="${COLOPHON_RESTORE_ROOT:-restores}"

DB=""
COLOPHON_BIN=target/release/colophon
SERVER_BIN=target/release/colophon-server

compose() { docker compose -f deploy/compose.yaml "$@"; }

# image <service>: the pinned image a compose service runs, so the scratch uses the same
# binaries the backup came from.
image() {
  docker compose -f deploy/compose.yaml config --format json \
    | jq -r --arg s "$1" '.services[$s].image'
}

die() { echo "error: $*" >&2; exit "${2:-2}"; }

say() { printf '  %-9s %s\n' "$1" "$2"; }

# --- markers -----------------------------------------------------------------------------------
#
# stages/<name>.done is the record that a stage finished (its JSON is the stage's result:
# epochs advanced, jobs held, replay watermarks). stages/<name>.blocked records a stage that
# ran and refused — inspect it, fix the cause, rerun with --step. Nothing here runs a done
# stage again without --force: a doubled epoch or a re-held outbox is exactly the silent
# rerun the markers exist to prevent.

DIR=""
stage_file() { echo "$DIR/stages/$1.$2"; }

stage_status() { # done | blocked | pending
  if [[ -f "$(stage_file "$1" "done")" ]]; then echo "done"
  elif [[ -f "$(stage_file "$1" "blocked")" ]]; then echo "blocked"
  else echo "pending"; fi
}

prior_stages() { # print the stages before $1
  local s
  for s in $STAGES; do
    [[ "$s" == "$1" ]] && break
    echo "$s"
  done
}

require_prior() { # <stage>: every earlier stage must be done
  local s status
  for s in $(prior_stages "$1"); do
    status=$(stage_status "$s")
    [[ "$status" == "done" ]] || die "stage $1 cannot run: stage $s is $status — the procedure is ordered"
  done
}

mark_done() { # <stage> <extra-json-object-fields>: write the marker, stamp it
  local stage=$1 extra=$2
  [[ -n "$extra" ]] || extra='{}'
  rm -f "$(stage_file "$stage" "blocked")"
  jq -n --arg stage "$stage" --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --argjson extra "$extra" \
    '{stage: $stage, status: "done", at: $at} + $extra' >"$(stage_file "$stage" "done")"
  say "$stage:" "done"
}

mark_blocked() { # <stage> <reason>
  local stage=$1 reason=$2
  jq -n --arg stage "$stage" --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --arg reason "$reason" \
    '{stage: $stage, status: "blocked", at: $at, reason: $reason}' >"$(stage_file "$stage" blocked)"
  # stderr: a block inside a command substitution must still be visible.
  printf '  %-9s BLOCKED — %s\n' "$stage:" "$reason" >&2
}

run_stage() { # <stage>: gate on markers, dispatch to stage_<name>
  local stage=$1
  require_prior "$stage"
  if [[ "$(stage_status "$stage" )" == "done" && "$FORCE" != 1 ]]; then
    die "stage $stage is already done ($(stage_file "$stage" "done")) — inspect it, or --force to rerun"
  fi
  "stage_$stage" "$stage"
}

# --- scratch plumbing --------------------------------------------------------------------------

# free_port <port>: the port if it is free on 127.0.0.1, else the next free one — so a restore
# run alongside another restore (or an occupied port) still comes up.
free_port() {
  local p=$1
  while (echo >"/dev/tcp/127.0.0.1/$p") 2>/dev/null; do p=$((p + 1)); done
  echo "$p"
}

# psql_scratch <sql>: a statement on the restored database, inside its container.
psql_scratch() {
  docker exec "$CONTAINER" psql -U colophon -d "$DB" -XAtq -v ON_ERROR_STOP=1 -c "$1"
}

BINARIES_READY=0
ensure_binaries() {
  if [[ ! -x "$COLOPHON_BIN" ]] || [[ ! -x "$SERVER_BIN" ]]; then
    cargo build --quiet --release -p colophon-cli -p colophon-server
  fi
  BINARIES_READY=1
}

# admin <args...>: the migrator's CLI on the scratch database; the URL rides in the
# environment so the credential never appears in the process list.
admin() {
  (( BINARIES_READY )) || ensure_binaries
  COLOPHON_MIGRATION_DATABASE_URL="postgres://colophon:colophon-dev@127.0.0.1:$PG_PORT/$DB" \
    "$COLOPHON_BIN" admin "$@"
}

wait_pg() { # the scratch postgres answers within 90s (it may be replaying the backup's WAL)
  for _ in $(seq 1 90); do
    if docker exec "$CONTAINER" pg_isready -U colophon -d "$DB" >/dev/null 2>&1; then return 0; fi
    sleep 1
  done
  return 1
}

wait_valkey() {
  for _ in $(seq 1 30); do
    if docker exec "$VALKEY_CONTAINER" \
        valkey-cli -a colophon-dev --no-auth-warning ping 2>/dev/null | grep -q PONG; then
      return 0
    fi
    sleep 1
  done
  return 1
}

# --- the six stages ----------------------------------------------------------------------------

stage_prepare() {
  local manifest="$BACKUP_DIR/manifest.json"
  [[ -f "$manifest" ]] || die "$BACKUP_DIR holds no manifest.json — not a backup directory"
  # The encrypting tool is whatever made this backup, not whatever this machine prefers now.
  ENC=$(jq -r '.encryption.tool' "$manifest")
  [[ "$ENC" == age || "$ENC" == gpg ]] || die "manifest names unknown encryption tool '$ENC'"
  DB=$(jq -r '.database.name' "$manifest")
  [[ "$DB" =~ ^[a-z][a-z0-9_]{0,62}$ ]] || die "manifest's database name '$DB' is not usable"
  ensure_key verify

  local payload
  payload="$BACKUP_DIR/$(jq -r '.payload.file' "$manifest")"
  [[ -f "$payload" ]] || die "$payload is missing"
  local recorded actual
  recorded=$(jq -r '.payload.sha256' "$manifest")
  actual=$(openssl dgst -sha256 -r "$payload" | cut -d' ' -f1)
  [[ "$actual" == "$recorded" ]] || die "$payload fails the manifest's SHA-256"
  say "$1:" "payload SHA-256 matches the manifest"

  PG_PORT=$(free_port "$PG_PORT")
  jq --argjson pg "$PG_PORT" '.pg_port = $pg' "$DIR/state.json" >"$DIR/state.json.tmp" \
    && mv "$DIR/state.json.tmp" "$DIR/state.json"

  # Decrypt through the key file (never a flag) and unpack the data directory into the fresh
  # volume. The tar's single top-level `base/` is flattened into PGDATA's place. A previous
  # interrupted prepare may have left its volume half-populated — start it clean.
  decrypt "$payload" >"$DIR/payload.tar.gz"
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
  docker volume rm "$VOLUME" >/dev/null 2>&1 || true
  docker volume create "$VOLUME" >/dev/null
  docker run --rm -i -v "$VOLUME:/var/lib/postgresql" --entrypoint sh "$(image postgres)" -c \
    'set -eu; d=${PGDATA:?}; mkdir -p "$d";
     tar -xzf - -C "$d" --strip-components=1;
     chown -R postgres:postgres "$d"; chmod 700 "$d"' <"$DIR/payload.tar.gz"
  rm -f "$DIR/payload.tar.gz"
  jq --arg db "$DB" '.database = $db' "$DIR/state.json" >"$DIR/state.json.tmp" \
    && mv "$DIR/state.json.tmp" "$DIR/state.json"
  mark_done prepare "$(jq -n --arg vol "$VOLUME" --arg db "$DB" \
    '{volume: $vol, database: $db}')"
}

stage_verify() {
  # PostgreSQL's own check on the restored bytes — before the cluster first starts, while the
  # files still are exactly what the backup held (a started cluster rewrites them).
  docker run --rm -v "$VOLUME:/var/lib/postgresql" --entrypoint sh "$(image postgres)" -c \
    'pg_verifybackup -q "$PGDATA"' || { mark_blocked verify "pg_verifybackup rejected the restored files"; return 1; }
  say "$1:" "pg_verifybackup passed on the restored data directory"

  docker run -d --name "$CONTAINER" -v "$VOLUME:/var/lib/postgresql" \
    -p "127.0.0.1:$PG_PORT:5432" "$(image postgres)" >/dev/null
  if ! wait_pg; then
    docker logs "$CONTAINER" 2>&1 | tail -20 >&2 || true
    mark_blocked verify "the scratch postgres never became ready (logs above)"
    return 1
  fi
  say "$1:" "scratch postgres up on 127.0.0.1:$PG_PORT"

  local check rc=0
  check=$(admin restore verify --manifest "$BACKUP_DIR/manifest.json" --json 2>"$DIR/log/verify.err") || rc=$?
  printf '%s' "$check" >"$DIR/log/verify.json" 2>/dev/null || true
  if (( rc != 0 )); then
    mark_blocked verify "schema check refused this backup ($(printf '%s' "$check" | jq -r '.data.status // .error // "error"' 2>/dev/null))"
    return 1
  fi
  mark_done verify "$(printf '%s' "$check" | jq '{schema: .data.status, manifest_match: .data.manifest_match, pending: .data.pending}')"
}

stage_epoch() {
  local out rc=0
  out=$(admin restore epoch --json) || rc=$?
  (( rc == 0 )) || { mark_blocked epoch "the epoch advance failed"; return 1; }
  mark_done epoch "$(printf '%s' "$out" | jq '{environments: .data.environments}')"
}

stage_ledger() {
  # The backup's own ledger position, from the restored database itself: the ids it already
  # holds were committed with the deletions they record, so the export's required interval is
  # exactly the rows the restored ledger lacks. (max(id) alone won't do — ids drawn out of
  # order, as when a replay fills old gaps, would hide real post-backup rows beneath it.)
  local high restored_ids="[]"
  if [[ -n "$(psql_scratch "select coalesce(to_regclass('deletion_ledger')::text, '')")" ]]; then
    high=$(psql_scratch "select coalesce(max(id), 0) from deletion_ledger")
    restored_ids=$(psql_scratch "select coalesce(jsonb_agg(id order by id), '[]'::jsonb) from deletion_ledger")
  else
    high=0
  fi
  say "$1:" "restored ledger watermark is $high"

  if ((${#LEDGERS[@]} == 0)); then
    mark_blocked ledger "no --ledger export files given — the interval from watermark $high can't be reconstructed"
    return 1
  fi

  # Every file must start with a well-formed export header before any replay happens.
  local file
  for file in "${LEDGERS[@]}"; do
    head -n 1 "$file" | jq -e \
      '.format == "colophon/deletion-ledger" and .version == 1 and
       (.after | type) == "number" and (.high_water | type) == "number"' >/dev/null \
      || { mark_blocked ledger "$file's first line isn't a deletion-ledger export header"; return 1; }
  done

  # Coverage: files ordered by `after`; a file that starts above the coverage the earlier
  # ones built leaves an unreconstructed interval, and O2 keeps serving off.
  local coverage=$high order after hw used_any
  order=$(for file in "${LEDGERS[@]}"; do
      head -n 1 "$file" | jq -r --arg f "$file" '"\(.after)\t\($f)"'
    done | sort -n | cut -f2-)

  local results="[]" n=0
  while IFS= read -r file; do
    read -r after hw <<<"$(head -n 1 "$file" | jq -r '"\(.after) \(.high_water)"')"
    if (( after > coverage )); then
      mark_blocked ledger "$file starts at id $after but coverage only reaches $coverage — the interval between them is missing"
      return 1
    fi
    # Replay only the rows the restored ledger lacks: the ones it holds were committed with
    # the deletions they record — the backup already holds their effects, and re-running them
    # can only trip over stale history, never add truth.
    local filtered="$DIR/log/interval-$n.jsonl" out rc=0
    n=$((n + 1))
    jq -c --argjson have "$restored_ids" \
      'select(input_line_number == 1 or .id as $i | ($have | index($i)) == null)' \
      "$file" >"$filtered"
    out=$(admin ledger-replay --input "$filtered" --json) || rc=$?
    printf '%s' "$out" >>"$DIR/log/ledger.jsonl"; printf '\n' >>"$DIR/log/ledger.jsonl"
    if (( rc != 0 )); then
      mark_blocked ledger "replay of $file stopped with conflicts ($(printf '%s' "$out" | jq -r '.data.conflicts | length' 2>/dev/null || echo '?')) — see stages/ledger.blocked and log/ledger.jsonl"
      return 1
    fi
    (( hw > coverage )) && coverage=$hw
    used_any=1
    results=$(printf '%s' "$results" | jq --arg file "$file" --argjson out "$out" \
      '. + [{file: $file, high_water: $out.data.high_water, applied: $out.data.applied, skipped: $out.data.skipped}]')
  done <<<"$order"

  [[ -n "$used_any" ]] || { mark_blocked ledger "no export files to replay"; return 1; }
  mark_done ledger "$(jq -n --argjson through "$coverage" --argjson files "$results" \
    '{replayed_through: $through, files: $files}')"
}

stage_reconcile() {
  local held jobs rc=0
  held=$(admin restore hold --json) || rc=$?
  (( rc == 0 )) || { mark_blocked reconcile "holding the outbox failed"; return 1; }
  jobs=$(admin restore jobs --json) || rc=$?
  (( rc == 0 )) || { mark_blocked reconcile "the outbox inventory failed"; return 1; }
  say "$1:" "$(printf '%s' "$held" | jq -r \
    '"\(.data.jobs | length) jobs held (\(.data.in_flight_at_backup) were running at backup time)"')"

  local resumed="false" extra
  if [[ "$RESUME_JOBS" == 1 ]]; then
    local out
    out=$(admin restore resume --json) || rc=$?
    (( rc == 0 )) || { mark_blocked reconcile "resuming held jobs failed"; return 1; }
    resumed="true"
    say "$1:" "resumed $(printf '%s' "$out" | jq -r '.data.jobs') held jobs (--resume-jobs)"
  fi
  extra=$(jq -n --argjson held "$held" --argjson inventory "$jobs" --argjson resumed "$resumed" \
    '{held: $held.data.jobs | length, in_flight_at_backup: $held.data.in_flight_at_backup,
      resumed: $resumed, inventory: $inventory.data}')
  mark_done reconcile "$extra"
}

stage_serve() {
  ensure_binaries
  VALKEY_PORT=$(free_port "$VALKEY_PORT")
  HTTP_PORT=$(free_port "$HTTP_PORT")
  # A scratch cache of its own: the live valkey's generation-keyed entries belong to the live
  # cluster's timeline, and a restore must never read another timeline's cache.
  docker rm -f "$VALKEY_CONTAINER" >/dev/null 2>&1 || true
  docker run -d --name "$VALKEY_CONTAINER" \
    -p "127.0.0.1:$VALKEY_PORT:6379" "$(image valkey)" \
    valkey-server --requirepass colophon-dev --save '' --appendonly no >/dev/null
  wait_valkey || { mark_blocked serve "the scratch valkey never answered"; return 1; }

  # shellcheck source=/dev/null # plain KEY=value lines
  set -a; . deploy/local.env; set +a
  COLOPHON_ROLE=api \
  COLOPHON_LISTEN_ADDR="127.0.0.1:$HTTP_PORT" \
  COLOPHON_DATABASE_URL="postgres://colophon_app:colophon-dev@127.0.0.1:$PG_PORT/$DB" \
  COLOPHON_MIGRATION_DATABASE_URL="postgres://colophon:colophon-dev@127.0.0.1:$PG_PORT/$DB" \
  COLOPHON_CACHE_URL="redis://:colophon-dev@127.0.0.1:$VALKEY_PORT" \
  COLOPHON_PUBLIC_URL="http://localhost:$HTTP_PORT" \
    "$SERVER_BIN" >"$DIR/log/server.log" 2>&1 &
  local pid=$!
  local ready=""
  for _ in $(seq 1 60); do
    if curl -fsS -o /dev/null -m 2 "http://127.0.0.1:$HTTP_PORT/readyz" 2>/dev/null; then ready=1; break; fi
    kill -0 "$pid" 2>/dev/null || break
    sleep 1
  done
  if [[ -z "$ready" ]]; then
    tail -30 "$DIR/log/server.log" >&2 || true
    mark_blocked serve "colophon-server never answered /readyz (log/server.log above)"
    return 1
  fi
  mark_done serve "$(jq -n --argjson pid "$pid" --arg url "http://localhost:$HTTP_PORT" \
    --arg listen "127.0.0.1:$HTTP_PORT" \
    '{pid: $pid, url: $url, listen: $listen, role: "api"}')"
  say serve: "serving the restored database at http://localhost:$HTTP_PORT (pid $pid)"
}

# --- status / teardown --------------------------------------------------------------------------

status() {
  [[ -f "$DIR/state.json" ]] || die "$DIR is not a restore workspace"
  jq -r '"restore of \(.backup) → \(.database) on scratch port \(.pg_port)\n"' "$DIR/state.json"
  local s
  for s in $STAGES; do
    case "$(stage_status "$s")" in
      done)    printf '  done     %s  (%s)\n' "$s" "$(jq -r .at "$(stage_file "$s" "done")")" ;;
      blocked) printf '  BLOCKED  %s  %s\n' "$s" "$(jq -r .reason "$(stage_file "$s" "blocked")")" ;;
      *)       printf '  pending  %s\n' "$s" ;;
    esac
  done
  if [[ -f "$(stage_file serve "done")" ]]; then
    local pid
    pid=$(jq -r .pid "$(stage_file serve "done")")
    if kill -0 "$pid" 2>/dev/null; then
      jq -r '"  serving  \(.url) (pid \(.pid))"' "$(stage_file serve "done")"
    else
      jq -r '"  served   \(.url) (pid \(.pid) is gone — torn down or exited)"' \
        "$(stage_file serve "done")"
    fi
  fi
}

teardown() {
  local purge=${1:-}
  if [[ -f "$(stage_file serve "done")" ]]; then
    local pid
    pid=$(jq -r .pid "$(stage_file serve "done")")
    kill "$pid" 2>/dev/null || true
  fi
  docker rm -f "$CONTAINER" "$VALKEY_CONTAINER" >/dev/null 2>&1 || true
  docker volume rm "$VOLUME" >/dev/null 2>&1 || true
  [[ "$purge" == "--purge" ]] && rm -rf "$DIR"
  echo "torn down: $CONTAINER $VALKEY_CONTAINER $VOLUME${purge:+ ($DIR removed)}"
}

# --- argument parsing / dispatch ---------------------------------------------------------------

usage() {
  sed -n '2,40p' "$0" | sed 's/^# \{0,1\}//'
  exit "${1:-2}"
}

case "${1:-}" in
  status|teardown)
    CMD=$1; DIR=${2:?usage: scripts/restore.sh $CMD <restore-dir>}
    [[ -d "$DIR" ]] || die "$DIR does not exist"
    DB=$(jq -r .database "$DIR/state.json" 2>/dev/null || echo "")
    STAMP=$(basename "$DIR")
    CONTAINER="colophon-restore-$STAMP"; VOLUME="$CONTAINER"; VALKEY_CONTAINER="$CONTAINER-valkey"
    if [[ "$CMD" == status ]]; then status; else teardown "${3:-}"; fi
    exit 0
    ;;
  drill) CMD=drill; shift ;;
  -h|--help) usage 0 ;;
  *) CMD=run ;;
esac

BACKUP_DIR=""; STEP=""; FROM=""; INTO=""; RESUME_JOBS=0; FORCE=0; KEEP=0
LEDGERS=()
while (($#)); do
  case "$1" in
    --step) STEP=${2:?--step needs a stage}; shift ;;
    --from) FROM=${2:?--from needs a stage}; shift ;;
    --into) INTO=${2:?--into needs a directory}; shift ;;
    --ledger) LEDGERS+=("${2:?--ledger needs a file}"); shift ;;
    --resume-jobs) RESUME_JOBS=1 ;;
    --force) FORCE=1 ;;
    --keep) KEEP=1 ;;
    -h|--help) usage 0 ;;
    -*) die "unknown flag $1" ;;
    *) if [[ -z "$BACKUP_DIR" ]]; then BACKUP_DIR=$1; else die "unexpected argument $1"; fi ;;
  esac
  shift
done

[[ "$CMD" == drill ]] || {
  [[ -n "$BACKUP_DIR" && -d "$BACKUP_DIR" ]] || die "usage: scripts/restore.sh <backup-dir> [options]"
}

# The workspace: one directory per restore, holding the stage markers, the stage logs and
# state.json — the "restore manifest" the runbook's records section asks for.
new_workspace() {
  local base
  base=$(basename "$BACKUP_DIR")
  DIR="${INTO:-$ROOT/$base}"
  if [[ ! -d "$DIR" ]]; then
    mkdir -p "$DIR/stages" "$DIR/log"
    jq -n --arg backup "$BACKUP_DIR" --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
      --argjson pg "$PG_PORT" --argjson http "$HTTP_PORT" --argjson valkey "$VALKEY_PORT" \
      '{backup: $backup, created_at: $at, database: "", pg_port: $pg, http_port: $http,
        valkey_port: $valkey, stage_order: ["prepare","verify","epoch","ledger","reconcile","serve"]}' \
      >"$DIR/state.json"
  fi
  STAMP=$(basename "$DIR")
  CONTAINER="colophon-restore-$STAMP"; VOLUME="$CONTAINER"; VALKEY_CONTAINER="$CONTAINER-valkey"
  if [[ -z "$DB" ]]; then DB=$(jq -r .database "$DIR/state.json"); fi
}

run_stages() {
  local s wanted="false" ran=""
  for s in $STAGES; do
    if [[ -n "$STEP" ]]; then
      [[ "$s" == "$STEP" ]] && { run_stage "$s"; ran=1; }
    elif [[ -n "$FROM" ]]; then
      [[ "$s" == "$FROM" ]] && wanted="true"
      [[ "$wanted" == true ]] && run_stage "$s"
    else
      run_stage "$s"
    fi
  done
  # `if` guards, not `&&` tails: a false [[ on the function's last line would return 1 and
  # `set -e` would kill a run whose stages all passed.
  if [[ -n "$STEP" && -z "$ran" ]]; then die "no stage named $STEP (stages: $STAGES)"; fi
  if [[ -n "$FROM" && "$wanted" != true ]]; then die "no stage named $FROM (stages: $STAGES)"; fi
}

# --- the drill ---------------------------------------------------------------------------------
#
# Evidence the procedure works on this checkout: seed an in-flight job + a victim entry in the
# live stack, capture a backup, then change the live state (delete the victim, edit a second
# entry, export the ledger) and restore. The restored scratch database must show: a new epoch,
# the victim deleted by replay, the edit absent (the backup predates it), the in-flight job
# held — and /readyz serving. Every fact is written to the result file.

drill() {
  for tool in docker jq openssl curl "$ENC"; do
    command -v "$tool" >/dev/null || die "$tool is required"
  done
  compose exec -T postgres true 2>/dev/null \
    || die "the local stack isn't running (scripts/stack.sh up) — the drill seeds and captures there"
  ensure_binaries

  local live_db="${COLOPHON_TEST_DATABASE:-colophon}" t0=$SECONDS
  local stamp backups
  stamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  backups="$ROOT/drill-$stamp/backups"
  mkdir -p "$backups"

  # --- fixture: one project with an environment, a destination, a release, a deletable entry.
  local fixture
  fixture=$(compose exec -T postgres psql -U colophon -d "$live_db" -XAtq -v ON_ERROR_STOP=1 <<'SQL'
WITH candidate AS (
  SELECT e.project_id, e.environment_id, e.id AS entry_id, e.collection,
         e.draft_revision_id, e.draft_title
  FROM entries e
  WHERE e.deleted_at IS NULL
    AND NOT EXISTS (SELECT 1 FROM publications p
                    WHERE p.entry_id = e.id AND p.valid_to IS NULL
                      AND p.project_id = e.project_id AND p.environment_id = e.environment_id)
    AND NOT EXISTS (SELECT 1 FROM revision_references r
                    WHERE r.entry_id = e.id
                      AND r.project_id = e.project_id AND r.environment_id = e.environment_id)
    AND NOT EXISTS (SELECT 1 FROM changeset_items i
                    WHERE i.entry_id = e.id
                      AND i.project_id = e.project_id AND i.environment_id = e.environment_id
                      AND NOT EXISTS (SELECT 1 FROM releases rel
                                      WHERE rel.changeset_id = i.changeset_id
                                        AND rel.project_id = i.project_id
                                        AND rel.environment_id = i.environment_id))
    AND EXISTS (SELECT 1 FROM destinations d
                WHERE d.project_id = e.project_id AND d.environment_id = e.environment_id)
    AND EXISTS (SELECT 1 FROM releases r2
                WHERE r2.project_id = e.project_id AND r2.environment_id = e.environment_id)
    AND EXISTS (SELECT 1 FROM entries e2
                WHERE e2.project_id = e.project_id AND e2.environment_id = e.environment_id
                  AND e2.id <> e.id AND e2.deleted_at IS NULL
                  AND e2.draft_title NOT LIKE '%edited after backup')
  ORDER BY e.created_at DESC LIMIT 1
)
SELECT jsonb_build_object(
  'project_id', project_id, 'environment_id', environment_id,
  'victim_id', entry_id, 'victim_collection', collection,
  'victim_revision', draft_revision_id, 'victim_title', draft_title,
  'second_id', (SELECT e2.id FROM entries e2
                WHERE e2.project_id = c.project_id AND e2.environment_id = c.environment_id
                  AND e2.id <> c.entry_id AND e2.deleted_at IS NULL
                  AND e2.draft_title NOT LIKE '%edited after backup' LIMIT 1),
  'destination_id', (SELECT d.id FROM destinations d
                     WHERE d.project_id = c.project_id AND d.environment_id = c.environment_id LIMIT 1),
  'release_generation', (SELECT max(r.generation) FROM releases r
                         WHERE r.project_id = c.project_id AND r.environment_id = c.environment_id),
  'epoch_before', (SELECT env.epoch FROM environments env
                   WHERE env.project_id = c.project_id AND env.id = c.environment_id))
FROM candidate c
SQL
)
  [[ -n "$fixture" && "$fixture" != "null" ]] \
    || die "no suitable fixture in $live_db (need an entry with a destination + release)"
  say drill: "fixture: $(printf '%s' "$fixture" | jq -c \
    '{entry: .victim_id, collection: .victim_collection}')"
  printf '%s' "$fixture" >"$ROOT/drill-$stamp/fixture.json"

  local pid eid victim second gen epoch_before identity
  pid=$(jq -r .project_id <<<"$fixture"); eid=$(jq -r .environment_id <<<"$fixture")
  victim=$(jq -r .victim_id <<<"$fixture"); second=$(jq -r .second_id <<<"$fixture")
  gen=$(jq -r .release_generation <<<"$fixture")
  epoch_before=$(jq -r .epoch_before <<<"$fixture")
  identity=$(compose exec -T postgres psql -U colophon -d "$live_db" -XAtq -c \
    "select id from identities order by created_at limit 1")

  # A fresh destination: the outbox already holds a job for every real destination on this
  # generation, and the idempotency key must be unique.
  local dest
  dest=$(compose exec -T postgres psql -U colophon -d "$live_db" -XAtq -v ON_ERROR_STOP=1 -c \
    "INSERT INTO destinations (project_id, environment_id, id, kind, name, url, created_by)
     VALUES ('$pid', '$eid', gen_random_uuid(), 'webhook', 'restore-drill',
             'https://drill.invalid/webhook', '$identity')
     RETURNING id")

  # An in-flight job inside the backup: claimed by a worker that won't exist after restore.
  local job_id
  job_id=$(compose exec -T postgres psql -U colophon -d "$live_db" -XAtq -v ON_ERROR_STOP=1 -c \
    "INSERT INTO jobs (project_id, environment_id, id, destination_id, generation,
                       idempotency_key, status, lease_owner, lease_expires_at, deadline_at)
     VALUES ('$pid', '$eid', gen_random_uuid(), '$dest', $gen,
             'colophon:$pid:$eid:$gen:$dest', 'running', 'drill-worker-gone',
             now() + interval '15 minutes', now() + interval '1 hour')
     RETURNING id")
  say drill: "seeded in-flight job $job_id (running, leased to a worker the restore won't have)"

  # --- capture.
  local backup_dir
  COLOPHON_TEST_DATABASE="$live_db" scripts/backup.sh create "$backups" >/dev/null
  for backup_dir in "$backups"/*/; do break; done
  backup_dir=${backup_dir%/}
  local capture_seconds=$(( SECONDS - t0 ))
  say drill: "captured $backup_dir in ${capture_seconds}s"

  # --- post-backup truth: delete the victim through the real path, edit a second entry,
  #     export the ledger off-host.
  local deleted
  deleted=$(compose exec -T postgres psql -U colophon -d "$live_db" -XAtq -v ON_ERROR_STOP=1 <<SQL
SET ROLE colophon_app;
SET app.identity_id = '$identity';
SET app.project_id = '$pid';
SELECT outcome FROM colophon_delete_entry(
  '$pid', '$eid', '$victim',
  '$(jq -r .victim_collection <<<"$fixture")', '$(jq -r .victim_revision <<<"$fixture")',
  gen_random_uuid());
SQL
)
  [[ "$deleted" == "deleted" ]] || die "the drill's deletion returned '$deleted', not deleted"
  say drill: "deleted the victim entry on the live stack (ledger row recorded)"

  compose exec -T postgres psql -U colophon -d "$live_db" -XAtq -v ON_ERROR_STOP=1 -c \
    "UPDATE entries SET draft_title = draft_title || ' — edited after backup', updated_at = now()
     WHERE project_id = '$pid' AND environment_id = '$eid' AND id = '$second'" >/dev/null
  say drill: "edited a second entry's title after the backup"

  local ledger_file="$ROOT/drill-$stamp/ledger.jsonl" hw
  COLOPHON_MIGRATION_DATABASE_URL="postgres://colophon:colophon-dev@127.0.0.1:${COLOPHON_PG_PORT:-55432}/$live_db" \
    "$COLOPHON_BIN" admin ledger-export --out "$ledger_file" --json >/dev/null
  hw=$(head -n1 "$ledger_file" | jq -r .high_water)
  LEDGERS=("$ledger_file")
  say drill: "exported the ledger through high-water $hw"

  # --- restore it, all six stages, then assert.
  local r0=$SECONDS
  BACKUP_DIR=$backup_dir
  INTO="$ROOT/drill-$stamp/restore"
  new_workspace
  run_stages
  local restore_seconds=$(( SECONDS - r0 ))

  # check_eq <expected> <actual> <pass note> <fail note>: one recorded pass or fail per
  # check (ok/fails are drill's locals — bash's dynamic scoping reaches them).
  local ok=0 fails=0
  check_eq() {
    if [[ "$1" == "$2" ]]; then
      say check: "$3"; ok=$((ok + 1))
    else
      say check: "FAIL $4"; fails=$((fails + 1))
    fi
  }

  local epoch_after victim_gone job_state title_kept ready=false
  epoch_after=$(psql_scratch "select epoch from environments where project_id='$pid' and id='$eid'")
  check_eq "$((epoch_before + 1))" "$epoch_after" \
    "epoch $epoch_before → $epoch_after (advanced exactly once)" \
    "epoch is $epoch_after, expected $((epoch_before + 1))"

  victim_gone=$(psql_scratch "select deleted_at is not null from entries where id='$victim'")
  check_eq t "$victim_gone" \
    "the post-backup deletion is replayed (victim tombstoned)" \
    "victim still live"

  job_state=$(psql_scratch "select status from jobs where id='$job_id'")
  check_eq held "$job_state" \
    "the in-flight job is held — not claimed, not dispatched" \
    "job is $job_state"

  title_kept=$(psql_scratch \
    "select draft_title not like '%edited after backup' from entries where id='$second'")
  check_eq t "$title_kept" \
    "the post-backup edit is correctly absent (restore = backup state)" \
    "the post-backup edit leaked into the restore"

  if curl -fsS -o /dev/null -m 5 "http://127.0.0.1:$HTTP_PORT/readyz"; then ready=true; fi
  check_eq true "$ready" \
    "/readyz serves on the scratch stack" \
    "/readyz"

  local machine file total=$(( SECONDS - t0 ))
  machine=$(machine_name)
  file="perf/results/${stamp//:/}-restore-drill-$machine.json"
  jq -n \
    --arg stamp "$stamp" --arg commit "$(git rev-parse HEAD)" \
    --argjson dirty "$(git status --porcelain | grep -c . || true)" \
    --arg machine "$machine" --arg backup "$backup_dir" --arg workspace "$DIR" \
    --arg database "$live_db" --argjson fixture "$fixture" --arg job "$job_id" \
    --argjson ledger_hw "$hw" \
    --argjson capture "$capture_seconds" --argjson restore "$restore_seconds" \
    --argjson total "$total" --argjson checks "$ok" --argjson failures "$fails" \
    --argjson ready "$ready" \
    '{
      scenario: "restore-drill", date: $stamp, commit: $commit, dirty_files: $dirty,
      machine: $machine, backup_dir: $backup, restore_dir: $workspace, database: $database,
      fixture: $fixture, in_flight_job: $job, ledger_high_water: $ledger_hw,
      seconds: {capture: $capture, restore: $restore, capture_to_verified_serving: $total},
      checks_passed: $checks, checks_failed: $failures, serving: $ready,
      stages: ["prepare","verify","epoch","ledger","reconcile","serve"],
      verdict: (if $failures == 0 then "restored-and-verified" else "failed" end)
    }' >"$file"
  say drill: "result: $file (capture→verified-serving ${total}s)"

  [[ "$KEEP" == 1 ]] || teardown
  (( fails == 0 )) || { echo "FAIL: $fails drill checks" >&2; exit 1; }
  echo "PASS: restore drilled end to end — new epoch, ledger replayed, jobs held, serving"
}

# --- dispatch -----------------------------------------------------------------------------------

if [[ "$CMD" == drill ]]; then
  drill
else
  DB=""
  new_workspace
  # The database name comes from the manifest once prepare has run; until then it rides in
  # state.json — status/teardown on an interrupted workspace still work.
  run_stages
fi
