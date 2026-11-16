#!/usr/bin/env bash
# Encrypted base backups of the local stack's PostgreSQL cluster (D-125; module 18, spec §32).
#
#   scripts/backup.sh create [ROOT]   capture a base backup into ROOT/<utc-stamp>/ (default backups/)
#   scripts/backup.sh verify DIR      decrypt and integrity-check a captured backup directory
#   scripts/backup.sh drill [ROOT]    create + verify + write perf/results/<stamp>-backup-drill-<machine>.json
#   scripts/backup.sh daily [ROOT]    the scheduled job: create + verify + ledger export +
#                                     retention --apply, with an audit line in ROOT/daily.jsonl
#   scripts/backup.sh retention [ROOT] [--apply]
#                                     plan (default) or apply the keep policy — newest
#                                     verified + 7 capture days + 4 ISO weeks; a directory
#                                     that doesn't verify is never pruned
#   scripts/backup.sh retention-drill prove the policy on synthetic backups in a temp root
#
# Scheduling is the platform's own: `daily` is the one-shot unit cron, launchd or a systemd
# timer runs — there is no daemon to keep alive. On cron(8):
#   17 3 * * *  cd /path/to/checkout && scripts/backup.sh daily >> backups/daily.log 2>&1
#
# Each <stamp>/ holds base.tar.gz.<enc> — the whole cluster copied by pg_basebackup as the
# colophon_backup role (REPLICATION, no table rights), gzip-compressed and symmetrically
# encrypted — next to manifest.json, which records what a restore needs to know: the checkout's
# commit, the Postgres and pg_basebackup versions, the applied migrations, the encryption-key
# reference (never the secret), the database's size and the deletion ledger's high-water mark.
#
# Encryption is gpg --symmetric (AES-256) here because age is not installed; when age and
# age-keygen are on PATH the same commands use an age identity file instead. Either way the
# secret comes from a file, never a flag: COLOPHON_BACKUP_PASSPHRASE_FILE or
# COLOPHON_BACKUP_AGE_KEY_FILE wins, else deploy/backup-passphrase.local /
# deploy/backup-age-key.local (created on first create, gitignored, mode 600).
#
# A backup only counts once verify passes: the drill decrypts a copy and pg_verifybackup
# re-checks every file against PostgreSQL's own backup manifest. "A backup existing is not
# proof of a working restore" — the restore drill onto clean infrastructure is a later slice.
#
# Runs on macOS's bash 3.2; needs docker, jq, openssl and the encrypting tool.
set -euo pipefail
cd "$(dirname "$0")/.."

# ENC, ensure_key, encrypt, decrypt and machine_name live in the library restore.sh shares.
. scripts/backup-crypt.sh

compose() { docker compose -f deploy/compose.yaml "$@"; }

# The application database the manifest reports on. The backup itself is cluster-wide —
# pg_basebackup copies the whole data directory, which is what WAL replay restores.
db="${COLOPHON_BACKUP_DATABASE:-${COLOPHON_TEST_DATABASE:-colophon}}"
if [[ ! "$db" =~ ^[a-z][a-z0-9_]{0,62}$ ]]; then
  echo "error: database name '$db' is not a plain database name" >&2
  exit 2
fi

checks=0
failures=0
pass() { checks=$((checks + 1)); printf '  ok    %s\n' "$1"; }
fail() { checks=$((checks + 1)); failures=$((failures + 1)); printf '  FAIL  %s\n' "$1"; }

# psql_admin <sql>: a manifest/metadata query as the migrator, inside the container.
psql_admin() {
  compose exec -T postgres psql -U colophon -d "$db" -XAtq -v ON_ERROR_STOP=1 -c "$1"
}

# wal_files: archived segment count (dot-files are the archiver's temp names).
wal_files() {
  compose exec -T postgres sh -c \
    "find /wal-archive -type f ! -name '.*' 2>/dev/null | wc -l" | tr -d '[:space:]'
}

# --- create ------------------------------------------------------------------------------------

# create_backup sets BACKUP_DIR rather than printing it: macOS's bash 3.2 silently ignores
# set -e inside a $(...) command substitution, which would let a half-failed capture report a
# directory anyway. Called directly, a failure aborts the run.
BACKUP_DIR=""
create_backup() { # <root>
  local root=$1 stamp dir rdir t0 seconds bytes sha payload
  compose exec -T postgres true 2>/dev/null || {
    echo "error: the postgres container is not running (scripts/stack.sh up)" >&2; exit 1; }
  ensure_key create

  stamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  dir="$root/${stamp//:/}"
  mkdir -p "$dir"
  payload="base.tar.gz.$([[ "$ENC" == age ]] && echo age || echo gpg)"
  t0=$SECONDS

  # Copy the cluster inside the container (the host has no pg_basebackup), verify the copy
  # against PostgreSQL's own manifest before it counts, then stream it out through the
  # encrypting tool — plaintext never lands on the host, and the container scratch is removed
  # once the stream is captured.
  rdir=$(compose exec -T postgres mktemp -d /tmp/colophon-backup.XXXXXX | tr -d '[:space:]')
  # Encrypt under a temp name and rename on success, so a failed run can never leave a file
  # that looks like a finished backup.
  # shellcheck disable=SC2016 # $d and $1 expand inside the container's sh, not here
  compose exec -T postgres sh -c '
    set -eu
    d=$1
    pg_basebackup -w -h 127.0.0.1 -U colophon_backup -D "$d/base" -F p -X stream -c fast
    pg_verifybackup -q "$d/base"
    tar -C "$d" -czf - base
  ' sh "$rdir" | encrypt "$dir/.$payload.tmp"
  mv "$dir/.$payload.tmp" "$dir/$payload"
  compose exec -T postgres rm -rf "$rdir"
  seconds=$(( SECONDS - t0 ))

  bytes=$(wc -c <"$dir/$payload" | tr -d '[:space:]')
  sha=$(openssl dgst -sha256 -r "$dir/$payload" | cut -d' ' -f1)

  # The manifest: everything a restore needs to know about this backup — which checkout,
  # which Postgres, which migrations, which key reference, and the deletion-ledger mark the
  # restore procedure replays from (null until that slice lands).
  local pg_version bb_version db_size cluster_size schemas migrations ledger wal_count wal_newest wal_bytes
  pg_version=$(psql_admin "show server_version")
  bb_version=$(compose exec -T postgres pg_basebackup --version)
  db_size=$(psql_admin "select pg_database_size('$db')")
  cluster_size=$(psql_admin "select sum(pg_database_size(oid)) from pg_database")
  schemas=$(psql_admin "select coalesce(jsonb_agg(schema_name order by schema_name), '[]'::jsonb)
    from information_schema.schemata
    where schema_name not like 'pg\_%' and schema_name <> 'information_schema'")
  # to_regclass guards must run in a query of their own: a CASE branch still names the table,
  # and PostgreSQL parses the whole statement before deciding which branch to take.
  if [[ -n "$(psql_admin "select coalesce(to_regclass('_sqlx_migrations')::text, '')")" ]]; then
    migrations=$(psql_admin "select coalesce(jsonb_agg(
      jsonb_build_object('version', m.version, 'description', m.description)
      order by m.version), '[]'::jsonb) from _sqlx_migrations m where m.success")
  else
    migrations="[]"
  fi
  if [[ -n "$(psql_admin "select coalesce(to_regclass('deletion_ledger')::text, '')")" ]]; then
    ledger=$(psql_admin "select jsonb_build_object('present', true, 'rows',
      (select count(*) from deletion_ledger))")
  else
    ledger="null"
  fi
  # A manifest that doesn't describe the backup must never be written: die unless the pieces
  # it is built from really are JSON of the expected shape.
  printf '%s' "$migrations" | jq -e 'type == "array"' >/dev/null
  printf '%s' "$ledger" | jq -e '. == null or .present == true' >/dev/null
  wal_count=$(wal_files)
  wal_newest=$(compose exec -T postgres sh -c \
    "ls -1t /wal-archive 2>/dev/null | grep -v '^\.' | head -n 1" | tr -d '[:space:]' || true)
  wal_bytes=$(compose exec -T postgres sh -c \
    "du -sb /wal-archive 2>/dev/null | cut -f1" | tr -d '[:space:]' || echo 0)

  jq -n \
    --arg created_at "$stamp" --arg commit "$(git rev-parse HEAD)" \
    --argjson dirty "$(git status --porcelain | grep -c . || true)" \
    --arg db "$db" --argjson db_size "$db_size" --argjson cluster_size "$cluster_size" \
    --arg pg_version "$pg_version" --arg bb_version "$bb_version" \
    --argjson schemas "$schemas" --argjson migrations "$migrations" --argjson ledger "$ledger" \
    --arg enc "$ENC" --arg key_ref "$secret_ref" --arg payload "$payload" \
    --argjson bytes "$bytes" --arg sha256 "$sha" --argjson capture_seconds "$seconds" \
    --argjson wal_files "${wal_count:-0}" --arg wal_newest "${wal_newest:-}" \
    --argjson wal_bytes "${wal_bytes:-0}" \
    '{
      format: 1, created_at: $created_at,
      git: {commit: $commit, dirty_files: $dirty},
      postgres: {version: $pg_version, pg_basebackup: $bb_version},
      scope: "cluster",
      database: {name: $db, size_bytes: $db_size},
      cluster_size_bytes: $cluster_size,
      schemas: $schemas, migrations: $migrations, deletion_ledger: $ledger,
      encryption: {tool: $enc, mode: "symmetric", key_reference: $key_ref, file: $payload},
      payload: {file: $payload, bytes: $bytes, sha256: $sha256},
      wal_archive: {location: "docker volume colophon_postgres-wal-archive (/wal-archive)",
                    files: $wal_files, bytes: $wal_bytes, newest: $wal_newest},
      capture_seconds: $capture_seconds,
      verified_at_capture: true
    }' >"$dir/manifest.json"

  BACKUP_DIR="$dir"
}

# --- verify ------------------------------------------------------------------------------------

# manifest_fields <manifest.json>: every field a restore needs — the check `verify` reports and
# `retention` requires before it will call a directory a backup at all.
manifest_fields() {
  jq -e '
    .created_at and .git.commit and .postgres.version and .database.name and
    .encryption.key_reference and .payload.file and .payload.sha256 and
    (.migrations | type == "array") and has("deletion_ledger")' "$1" >/dev/null 2>&1
}

pg_verifybackup="skipped (stack not running)"
verify_backup() { # <dir>: integrity checks; exits 1 on any failure.
  local dir=$1 enc_file scratch
  [[ -d "$dir" ]] || { echo "error: $dir is not a backup directory" >&2; exit 2; }
  ensure_key verify

  if [[ -f "$dir/manifest.json" ]] && manifest_fields "$dir/manifest.json"; then
    pass "manifest.json parses with the fields a restore needs"
  else
    fail "manifest.json parses with the fields a restore needs"; return 1
  fi
  enc_file="$dir/$(jq -r '.payload.file' "$dir/manifest.json")"
  if [[ -f "$enc_file" ]] && [[ "$(openssl dgst -sha256 -r "$enc_file" | cut -d' ' -f1)" \
      == "$(jq -r '.payload.sha256' "$dir/manifest.json")" ]]; then
    pass "the encrypted payload's SHA-256 matches the manifest"
  else
    fail "the encrypted payload's SHA-256 matches the manifest"
  fi

  scratch=$(mktemp -d "${TMPDIR:-/tmp}/colophon-backup-verify.XXXXXX")
  VERIFY_SCRATCH="$scratch"
  # Keep the decrypted tarball: the host's checks below and pg_verifybackup's deep check both
  # consume these exact bytes, so the encrypted artifact itself is what gets verified — never
  # a re-tar (a macOS re-tar would inject ._ AppleDouble members and deservedly fail).
  if decrypt "$enc_file" >"$scratch/payload.tar.gz"; then
    pass "decrypts with the referenced key"
  else
    fail "decrypts with the referenced key"; return 1
  fi
  if tar -xzf "$scratch/payload.tar.gz" -C "$scratch" && [[ -f "$scratch/base/PG_VERSION" ]]; then
    pass "untars to a data directory (PG_VERSION $(cat "$scratch/base/PG_VERSION"))"
  else
    fail "untars to a data directory"; return 1
  fi
  for f in global/pg_control postgresql.conf backup_manifest; do
    if [[ -s "$scratch/base/$f" ]]; then pass "base/$f is present"; else fail "base/$f is present"; fi
  done
  if jq -e '."PostgreSQL-Backup-Manifest-Version" and ((.Files | length) > 0) and
      ((."WAL-Ranges" | length) > 0)' "$scratch/base/backup_manifest" >/dev/null 2>&1; then
    pass "PostgreSQL's backup manifest parses, with files and a WAL range"
  else
    fail "PostgreSQL's backup manifest parses, with files and a WAL range"
  fi
  if [[ -n "$(find "$scratch/base/pg_wal" -type f ! -name '.*' 2>/dev/null | head -n 1)" ]]; then
    pass "the streamed WAL needed for consistency is inside the backup"
  else
    fail "the streamed WAL needed for consistency is inside the backup"
  fi

  # Deep check: re-run PostgreSQL's checksum verification on the decrypted payload bytes,
  # inside the container that owns pg_verifybackup. Skipped only when the stack isn't running.
  if compose exec -T postgres true 2>/dev/null; then
    # shellcheck disable=SC2016 # $d expands inside the container's sh, not here
    if compose exec -T postgres sh -c '
      d=$(mktemp -d /tmp/colophon-verify.XXXXXX)
      trap "rm -rf $d" EXIT
      tar -xzf - -C "$d"
      pg_verifybackup -q "$d/base"
    ' <"$scratch/payload.tar.gz"; then
      pass "pg_verifybackup re-checks every file against the manifest"
      pg_verifybackup="passed"
    else
      fail "pg_verifybackup re-checks every file against the manifest"
      pg_verifybackup="failed"
    fi
  fi
  [[ "$failures" == 0 ]]
}

# --- drill -------------------------------------------------------------------------------------

drill() { # <root>
  local root=$1 t0 dir v0 verify_seconds
  for tool in docker jq openssl "$ENC"; do
    command -v "$tool" >/dev/null || { echo "error: $tool is required" >&2; exit 1; }
  done

  t0=$SECONDS
  create_backup "$root"
  dir=$BACKUP_DIR
  local create_seconds=$(( SECONDS - t0 ))
  echo "$dir"

  v0=$SECONDS
  local verify_rc=0
  verify_backup "$dir" || verify_rc=1
  verify_seconds=$(( SECONDS - v0 ))

  local stamp machine file
  stamp=$(basename "$dir")
  machine=$(machine_name)
  file="perf/results/$stamp-backup-drill-$machine.json"
  jq -n \
    --arg stamp "$stamp" --arg commit "$(git rev-parse HEAD)" \
    --argjson dirty "$(git status --porcelain | grep -c . || true)" \
    --arg machine "$machine" --arg dir "$dir" --arg db "$db" \
    --argjson create_seconds "$create_seconds" --argjson verify_seconds "$verify_seconds" \
    --argjson checks "$checks" --argjson failures "$failures" --arg pvb "$pg_verifybackup" \
    --slurpfile manifest "$dir/manifest.json" \
    '{
      scenario: "backup-drill", date: $stamp, commit: $commit, dirty_files: $dirty,
      machine: $machine, backup_dir: $dir,
      backup_seconds: $create_seconds, verify_seconds: $verify_seconds,
      backup_bytes: $manifest[0].payload.bytes, backup_sha256: $manifest[0].payload.sha256,
      cluster_size_bytes: $manifest[0].cluster_size_bytes,
      database: $manifest[0].database, encryption: $manifest[0].encryption,
      wal_archive: $manifest[0].wal_archive, migrations: ($manifest[0].migrations | length),
      deletion_ledger: $manifest[0].deletion_ledger,
      pg_verifybackup_at_capture: $manifest[0].verified_at_capture,
      pg_verifybackup_after_decrypt: $pvb,
      verify_checks: $checks, verify_failures: $failures,
      verdict: (if $failures == 0 then "captured-and-verified" else "failed" end),
      note: "capture + integrity only: the restore drill onto clean infrastructure is a later P6 slice"
    }' >"$file"
  echo "result: $file"
  if (( verify_rc != 0 )); then
    echo "FAIL: $failures of $checks checks" >&2; exit 1
  fi
  echo "PASS: backup captured and verified ($create_seconds s + $verify_seconds s)"
}

# --- retention ----------------------------------------------------------------------------------

# The keep policy (module 18 "Backups"; D-130): among verified backups only, keep the newest of
# each of the newest KEEP_DAILY distinct capture days and of each of the newest KEEP_WEEKS
# distinct ISO weeks — counted over the days and weeks backups actually exist for, so a stopped
# schedule keeps more of its old backups, never fewer. The newest verified backup is kept
# whatever the buckets say (the floor), and a directory that does not verify is never pruned by
# this job: "held, unverified" is a report for a person, because deleting what a script cannot
# prove is a backup is how a floor becomes an empty directory.
KEEP_DAILY=7
KEEP_WEEKS=4

# iso_week <YYYY-MM-DD> → 2026-W44, through jq's libc strftime — portable; jq is required anyway.
iso_week() { jq -rn --arg d "$1" '$d | strptime("%Y-%m-%d") | strftime("%G-W%V")'; }

# retention_ok <dir>: the bar for treating a directory as a backup — the manifest's fields, the
# payload's SHA-256 against them, and a payload that decrypts to a tree holding
# base/PG_VERSION. Deliberately lighter than `verify`: no pg_verifybackup (the deep audit is
# capture's and drill's job, and capture already ran it) and no docker, so it works the same on
# any host holding the key file. Every candidate pays the decrypt each run — minutes on a big
# backup set, which a nightly job can afford and honesty requires.
retention_ok() {
  local dir=$1 payload
  [[ -f "$dir/manifest.json" ]] || return 1
  manifest_fields "$dir/manifest.json" || return 1
  payload="$dir/$(jq -r '.payload.file' "$dir/manifest.json")"
  [[ -f "$payload" ]] || return 1
  [[ "$(openssl dgst -sha256 -r "$payload" | cut -d' ' -f1)" \
     == "$(jq -r '.payload.sha256' "$dir/manifest.json")" ]] || return 1
  decrypt "$payload" 2>/dev/null | tar -xOzf - base/PG_VERSION >/dev/null 2>&1
}

# retention_run <root> <dry|apply>: print the decision for every backup-shaped directory, and
# with `apply` remove the surplus. Anything that is not a <UTC-stamp> directory is ignored;
# anything that is but does not verify is held, never deleted. Sets RET_* for `daily`'s audit.
RET_KEPT=0 RET_PRUNED=0 RET_HELD=0 RET_SKIPPED=0
retention_run() {
  local root=$1 mode=${2:-dry}
  [[ -d "$root" ]] || { echo "error: $root is not a directory" >&2; exit 2; }
  ensure_key verify

  # Verified and unverified <UTC-stamp> directories; the glob sorts, and the stamp format sorts
  # chronologically, so each list is oldest→newest already.
  local -a verified=() unverified=()
  local dir base skipped=0
  for dir in "$root"/*/; do
    [[ -d "${dir%/}" ]] || continue   # the unglobbed literal when the root holds no directories
    dir=${dir%/}
    base=$(basename "$dir")
    [[ "$base" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{6}Z$ ]] || { skipped=$((skipped + 1)); continue; }
    if retention_ok "$dir"; then verified+=("$dir"); else unverified+=("$dir"); fi
  done

  # Newest first: the first directory seen for a day or week is that bucket's newest backup.
  local -a ordered=()
  if ((${#verified[@]})); then
    while IFS= read -r line; do ordered+=("$line"); done \
      < <(printf '%s\n' "${verified[@]}" | sort -r)
  fi

  local days="" weeks="" floor="" reason day week ndays=0 nweeks=0
  local -a keep_dir=() keep_why=() prune_dir=()
  for dir in ${ordered[@]+"${ordered[@]}"}; do
    base=$(basename "$dir"); day=${base:0:10}; week=$(iso_week "$day")
    reason=""
    if [[ -z "$floor" ]]; then
      floor=$dir
      reason="newest verified — the floor, always kept"
    fi
    if [[ " $days " != *" $day "* ]] && (( ndays < KEEP_DAILY )); then
      days="$days $day"; ndays=$((ndays + 1))
      reason="${reason:+$reason; }daily $day"
    fi
    if [[ " $weeks " != *" $week "* ]] && (( nweeks < KEEP_WEEKS )); then
      weeks="$weeks $week"; nweeks=$((nweeks + 1))
      reason="${reason:+$reason; }weekly $week"
    fi
    if [[ -n "$reason" ]]; then
      keep_dir+=("$dir"); keep_why+=("$reason")
    else
      prune_dir+=("$dir")
    fi
  done

  echo "policy: newest verified + newest of $KEEP_DAILY capture days + newest of" \
    "$KEEP_WEEKS ISO weeks; verified backups only"
  local i
  for ((i = 0; i < ${#keep_dir[@]}; i++)); do
    printf '  keep    %s  (%s)\n' "$(basename "${keep_dir[$i]}")" "${keep_why[$i]}"
  done
  for ((i = 0; i < ${#prune_dir[@]}; i++)); do
    if [[ "$mode" == apply ]]; then
      rm -rf "${prune_dir[$i]}"
      printf '  pruned  %s\n' "$(basename "${prune_dir[$i]}")"
    else
      printf '  prune   %s  (dry run — pass --apply to delete)\n' "$(basename "${prune_dir[$i]}")"
    fi
  done
  for ((i = 0; i < ${#unverified[@]}; i++)); do
    printf '  hold    %s  unverified — kept; investigate by hand\n' \
      "$(basename "${unverified[$i]}")"
  done
  if (( skipped > 0 )); then
    printf '  skip    %d non-backup %s\n' "$skipped" \
      "$([[ $skipped == 1 ]] && echo directory || echo directories)"
  fi
  RET_KEPT=${#keep_dir[@]} RET_PRUNED=${#prune_dir[@]}
  RET_HELD=${#unverified[@]} RET_SKIPPED=$skipped
  if [[ "$mode" != apply ]] && (( ${#prune_dir[@]} > 0 )); then
    echo "dry run: $((${#prune_dir[@]})) director$([[ ${#prune_dir[@]} == 1 ]] \
      && echo 'y would be pruned' || echo 'ies would be pruned') — pass --apply"
  fi
}

# --- daily -------------------------------------------------------------------------------------

# daily <root>: the scheduled run — create, verify, the off-host ledger export restore step 4
# replays (carried inside the backup's own directory), then retention applied, and one audit
# line appended to <root>/daily.jsonl. Any step failing fails the run loudly — that failure is
# the schedule's alarm, and a half-finished capture is a .tmp dot-file retention will hold,
# never count.
daily() {
  local root=$1 t0=$SECONDS dir v0 create_s verify_s high_water bin
  create_backup "$root"   # exits loudly on a failed capture
  dir=$BACKUP_DIR
  create_s=$(( SECONDS - t0 ))

  v0=$SECONDS
  verify_backup "$dir" || { echo "daily FAIL: $dir did not verify" >&2; exit 1; }
  verify_s=$(( SECONDS - v0 ))

  # The ledger export is part of "a backup you can restore from" (O2, spec §33 step 4): its
  # high-water mark is where post-backup deletions continue from, and keeping it inside the
  # backup's directory means a restore needs nothing outside it. Written by the CLI, as the
  # migrator — the same URLs scripts/migrate.sh uses.
  bin=${COLOPHON_BIN:-target/release/colophon}
  [[ -x "$bin" ]] || {
    echo "error: $bin missing — cargo build --release -p colophon-cli" >&2; exit 1; }
  if [[ -z "${COLOPHON_MIGRATION_DATABASE_URL:-}" ]]; then
    # local-env.sh honours COLOPHON_TEST_DATABASE exactly as $db above does.
    set -a; . scripts/local-env.sh; set +a
  fi
  "$bin" admin ledger-export --out "$dir/ledger-export.jsonl" --json >/dev/null
  high_water=$(head -n1 "$dir/ledger-export.jsonl" | jq -r '.high_water // 0')

  retention_run "$root" apply

  jq -nc --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --arg dir "$dir" \
    --argjson create_s "$create_s" --argjson verify_s "$verify_s" \
    --argjson ledger_hw "${high_water:-0}" \
    --argjson kept "$RET_KEPT" --argjson pruned "$RET_PRUNED" \
    --argjson held "$RET_HELD" --argjson skipped "$RET_SKIPPED" \
    '{at: $at, dir: $dir, create_seconds: $create_s, verify_seconds: $verify_s,
      ledger_high_water: $ledger_hw,
      retention: {kept: $kept, pruned: $pruned, held_unverified: $held,
                  skipped: $skipped}}' >>"$root/daily.jsonl"
  echo "daily ok: $dir (create ${create_s}s, verify ${verify_s}s, ledger @$high_water;" \
    "kept $RET_KEPT, pruned $RET_PRUNED, held $RET_HELD)"
}

# --- retention drill ----------------------------------------------------------------------------

# retention_drill: proves the policy on synthetic backups in a temp root — real encrypted
# payloads through the same encrypt/decrypt path, stamp names fanned across days and weeks,
# three unverified flavours and non-backup entries. Asserts the dry run deletes nothing, that
# --apply leaves exactly the independently computed keep set, and that every unverified and
# non-backup entry survives. Record: perf/results/<stamp>-retention-drill-<machine>.json.
retention_drill() {
  local root tmp stamp machine file
  stamp=$(date -u +%Y-%m-%dT%H%M%SZ)
  machine=$(machine_name)
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/colophon-retention-drill.XXXXXX")
  root="$tmp/backups"
  mkdir -p "$root"
  VERIFY_SCRATCH=$tmp   # the EXIT trap removes it

  # The drill's own key inside its temp root — it never touches deploy/*.local.
  export COLOPHON_BACKUP_PASSPHRASE_FILE="$tmp/passphrase"
  export COLOPHON_BACKUP_AGE_KEY_FILE="$tmp/age-key"
  ensure_key create

  # mkstamp <days-ago> <HHMMSS> → a stamp dir name that many UTC days back at that time (UTC
  # noon math, so a same-day pair can never straddle midnight).
  mkstamp() {
    jq -rn --argjson days "$1" --arg hms "$2" '
      ((now | strftime("%Y-%m-%d")) + "T12:00:00Z"
        | strptime("%Y-%m-%dT%H:%M:%SZ") | mktime) - ($days * 86400)
      | strftime("%Y-%m-%dT") + $hms + "Z"'
  }

  # mkbackup <dirname> <good|nomanifest|badsha|undecryptable>: a synthetic backup directory —
  # a real encrypted payload through the same encrypt() path, plus a manifest in the real
  # format; the broken shapes are the failure modes retention must refuse to prune.
  mkbackup() {
    local d="$root/$1" shape=$2 encf="base.tar.gz.$ENC" created sha
    created="${1:0:11}${1:11:2}:${1:13:2}:${1:15:2}Z"
    mkdir -p "$d/base/global" "$d/base/pg_wal"
    echo 18 >"$d/base/PG_VERSION"
    : >"$d/base/global/pg_control"; : >"$d/base/postgresql.conf"
    tar -C "$d" -czf - base | encrypt "$d/.$encf.tmp"
    mv "$d/.$encf.tmp" "$d/$encf"
    rm -rf "$d/base"
    if [[ "$shape" == undecryptable ]]; then
      # The manifest's SHA-256 honestly matches these bytes — which are not a backup at all.
      printf 'intact bytes, wrong content' >"$d/$encf"
    fi
    if [[ "$shape" == nomanifest ]]; then
      return   # a finished-looking payload and no manifest: a capture that never reported
    fi
    sha=$(openssl dgst -sha256 -r "$d/$encf" | cut -d' ' -f1)
    if [[ "$shape" == badsha ]]; then
      # Rotate every hex digit one step: always a different hash, never a coincidence.
      sha=$(printf '%s' "$sha" | tr '0123456789abcdef' '123456789abcdef0')
    fi
    jq -n --arg created "$created" --arg enc "$ENC" --arg encf "$encf" --arg sha "$sha" \
      --argjson bytes "$(wc -c <"$d/$encf" | tr -d '[:space:]')" \
      '{format: 1, created_at: $created,
        git: {commit: "retention-drill", dirty_files: 0},
        postgres: {version: "drill", pg_basebackup: "drill"}, scope: "cluster",
        database: {name: "drill", size_bytes: 1}, cluster_size_bytes: 1,
        schemas: [], migrations: [], deletion_ledger: null,
        encryption: {tool: $enc, mode: "symmetric", key_reference: "retention-drill",
                     file: $encf},
        payload: {file: $encf, bytes: $bytes, sha256: $sha},
        wal_archive: {location: "drill", files: 0, bytes: 0, newest: ""},
        capture_seconds: 0, verified_at_capture: true}' >"$d/manifest.json"
  }

  # Verified fixtures across sixteen days spanning seven weeks, plus a same-day second backup
  # (its day and week are always already kept — surplus on every calendar). good_names is
  # newest→oldest by construction.
  local -a good_names=()
  local day n
  for day in 0 1 2 3 4 5 6 7 8 9 10 14 21 28 35 42; do
    n=$(mkstamp "$day" 030000)
    mkbackup "$n" good
    good_names+=("$n")
  done
  n=$(mkstamp 3 090000)
  mkbackup "$n" good
  local dup_name=$n

  # Three ways a directory fails retention_ok — all must be held, never deleted.
  local -a held_names=()
  n=$(mkstamp 15 030000); mkbackup "$n" nomanifest;    held_names+=("$n")
  n=$(mkstamp 20 030000); mkbackup "$n" badsha;        held_names+=("$n")
  n=$(mkstamp 25 030000); mkbackup "$n" undecryptable; held_names+=("$n")

  # Entries that are not backup-shaped at all — never counted.
  echo not-a-backup >"$root/README.txt"
  mkdir -p "$root/scratch"
  echo '{}' >"$root/daily.jsonl"

  local ok=0 fails=0
  check() { # <expected> <actual> <pass note> <fail note>
    if [[ "$1" == "$2" ]]; then ok=$((ok + 1)); printf '  ok    %s\n' "$3"
    else fails=$((fails + 1)); printf '  FAIL  %s\n' "$4"; fi
  }

  # The keep set, stated independently: the same policy re-expressed in jq over the verified
  # names — two implementations agreeing is what makes the bash logic evidence. `unique` sorts,
  # so `reverse | .[0:N]` takes the newest N distinct days/weeks, matching the bash loops that
  # see the newest of each bucket first.
  local expected
  expected=$(printf '%s\n' "${good_names[@]}" "$dup_name" | sort -r | jq -Rrn '
    [inputs] as $d
    | ([$d[] | .[0:10]] | unique | reverse | .[0:7]) as $days
    | ([$d[] | .[0:10] | strptime("%Y-%m-%d") | strftime("%G-W%V")]
       | unique | reverse | .[0:4]) as $weeks
    | ([$days[] as $day | [$d[] | select(.[0:10] == $day)] | .[0]]
       + [$weeks[] as $w
          | [$d[] | select((.[0:10] | strptime("%Y-%m-%d") | strftime("%G-W%V")) == $w)] | .[0]]
       + [$d[0]])
    | unique | .[]')

  # 1. Dry run: a plan, and nothing deleted.
  local before after
  before=$(find "$root" -mindepth 1 | wc -l | tr -d '[:space:]')
  retention_run "$root" dry >"$tmp/dry-run.txt"
  cat "$tmp/dry-run.txt"
  after=$(find "$root" -mindepth 1 | wc -l | tr -d '[:space:]')
  check "$before" "$after" \
    "the dry run deleted nothing ($before entries before and after)" \
    "the dry run changed the tree ($before → $after)"
  local keep_count prunes_expected
  keep_count=$(printf '%s\n' "$expected" | grep -c .)
  prunes_expected=$(( ${#good_names[@]} + 1 - keep_count ))
  check "$prunes_expected" "$RET_PRUNED" \
    "the dry run plans exactly the surplus ($RET_PRUNED prunes for $keep_count keeps)" \
    "dry run plans $RET_PRUNED prunes; the policy says $prunes_expected"
  check "$prunes_expected" "$(grep -c '^  prune' "$tmp/dry-run.txt")" \
    "every planned prune is printed" \
    "plan shows $(grep -c '^  prune' "$tmp/dry-run.txt") prune lines, expected $prunes_expected"
  check "${#held_names[@]}" "$(grep -c '^  hold' "$tmp/dry-run.txt")" \
    "every unverified directory is printed as held" \
    "plan shows $(grep -c '^  hold' "$tmp/dry-run.txt") hold lines, expected ${#held_names[@]}"

  # 2. Apply: exactly the expected keep set survives, plus everything held or uncounted.
  retention_run "$root" apply >"$tmp/apply.txt"
  cat "$tmp/apply.txt"
  local missing=0 e
  while IFS= read -r e; do
    [[ -n "$e" && ! -d "$root/$e" ]] && { missing=$((missing + 1)); }
  done <<<"$expected"
  check 0 "$missing" \
    "every expected keep survives ($(printf '%s\n' "$expected" | grep -c .) directories)" \
    "$missing expected keep(s) were pruned"
  local held_missing=0
  for n in "${held_names[@]}"; do [[ -d "$root/$n" ]] || held_missing=$((held_missing + 1)); done
  check 0 "$held_missing" \
    "all three unverified directories are held, not pruned" \
    "$held_missing unverified director(ies) were pruned"
  local extra_missing=0
  for n in README.txt scratch daily.jsonl; do [[ -e "$root/$n" ]] || extra_missing=$((extra_missing + 1)); done
  check 0 "$extra_missing" "non-backup entries are untouched" \
    "$extra_missing non-backup entr(ies) were removed"
  check t "$(test -d "$root/${good_names[0]}" && echo t || echo f)" \
    "the newest verified backup survived" "the newest verified backup was pruned"
  # The surplus set is closed: stamped directories remaining = expected keeps + the held.
  local stamped_now expected_count
  stamped_now=$(find "$root" -mindepth 1 -maxdepth 1 -type d \
    -name '????-??-??T??????Z' | wc -l | tr -d '[:space:]')
  expected_count=$(( $(printf '%s\n' "$expected" | grep -c .) + ${#held_names[@]} ))
  check "$expected_count" "$stamped_now" \
    "exactly the keep set + held directories remain ($stamped_now stamped dirs)" \
    "$stamped_now stamped dirs remain, expected $expected_count"

  file="perf/results/$stamp-retention-drill-$machine.json"
  jq -n --arg stamp "$stamp" --arg commit "$(git rev-parse HEAD)" \
    --argjson dirty "$(git status --porcelain | grep -c . || true)" \
    --arg machine "$machine" --arg enc "$ENC" \
    --argjson keep_daily "$KEEP_DAILY" --argjson keep_weeks "$KEEP_WEEKS" \
    --argjson verified "$(( ${#good_names[@]} + 1 ))" --argjson held "${#held_names[@]}" \
    --argjson checks "$ok" --argjson failures "$fails" \
    --rawfile dry "$tmp/dry-run.txt" --rawfile apply "$tmp/apply.txt" \
    --argjson expected "$(printf '%s\n' "$expected" | jq -Rn '[inputs]')" \
    '{
      scenario: "backup-retention-drill", date: $stamp, commit: $commit,
      dirty_files: $dirty, machine: $machine, encryption: $enc,
      policy: {keep_capture_days: $keep_daily, keep_iso_weeks: $keep_weeks,
               floor: "the newest verified backup is always kept",
               unverified: "never pruned — listed for a person",
               buckets: "over days/weeks backups exist for, not the calendar"},
      fixtures: {verified: $verified, unverified: $held,
                 non_backup: 3, same_day_pair: true},
      expected_keeps: $expected,
      dry_run_output: ($dry | rtrimstr("\n") | split("\n")),
      apply_output: ($apply | rtrimstr("\n") | split("\n")),
      checks: $checks, failures: $failures,
      verdict: (if $failures == 0 then "policy-proven" else "failed" end)
    }' >"$file"
  echo "result: $file"
  if (( fails != 0 )); then echo "FAIL: $fails of $((ok + fails)) checks" >&2; exit 1; fi
  echo "PASS: retention policy proven ($((ok)) checks)"
}

# The scratch dir from a verify run, if one exists on this call.
VERIFY_SCRATCH=""
# shellcheck disable=SC2329 # invoked by the EXIT trap below
cleanup() { if [[ -n "$VERIFY_SCRATCH" ]]; then rm -rf "$VERIFY_SCRATCH"; fi; }
trap cleanup EXIT

case "${1:-}" in
  create)
    for tool in docker jq openssl "$ENC"; do
      command -v "$tool" >/dev/null || { echo "error: $tool is required" >&2; exit 1; }
    done
    create_backup "${2:-backups}"
    echo "$BACKUP_DIR"
    ;;
  verify)
    [[ -n "${2:-}" ]] || { echo "usage: scripts/backup.sh verify <dir>" >&2; exit 2; }
    # docker is optional here: without the stack the pg_verifybackup deep check is skipped.
    for tool in jq openssl "$ENC"; do
      command -v "$tool" >/dev/null || { echo "error: $tool is required" >&2; exit 1; }
    done
    verify_backup "$2" || { echo "verify failed: $failures of $checks checks" >&2; exit 1; }
    echo "verify ok: $checks checks passed"
    ;;
  drill)
    drill "${2:-backups}"
    ;;
  daily)
    for tool in docker jq openssl "$ENC"; do
      command -v "$tool" >/dev/null || { echo "error: $tool is required" >&2; exit 1; }
    done
    daily "${2:-backups}"
    ;;
  retention)
    # No docker: retention_ok's bar is deliberately container-free (see above).
    for tool in jq openssl "$ENC"; do
      command -v "$tool" >/dev/null || { echo "error: $tool is required" >&2; exit 1; }
    done
    root=backups mode=dry
    for arg in "${@:2}"; do
      case "$arg" in
        --apply) mode=apply ;;
        -*) echo "usage: scripts/backup.sh retention [root] [--apply]" >&2; exit 2 ;;
        *) root=$arg ;;
      esac
    done
    retention_run "$root" "$mode"
    ;;
  retention-drill)
    for tool in jq openssl "$ENC"; do
      command -v "$tool" >/dev/null || { echo "error: $tool is required" >&2; exit 1; }
    done
    retention_drill
    ;;
  *)
    echo "usage: scripts/backup.sh create [root] | verify <dir> | drill [root] |" >&2
    echo "       daily [root] | retention [root] [--apply] | retention-drill" >&2
    exit 2
    ;;
esac
