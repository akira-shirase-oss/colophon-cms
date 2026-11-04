#!/usr/bin/env bash
# Encrypted base backups of the local stack's PostgreSQL cluster (D-125; module 18, spec §32).
#
#   scripts/backup.sh create [ROOT]   capture a base backup into ROOT/<utc-stamp>/ (default backups/)
#   scripts/backup.sh verify DIR      decrypt and integrity-check a captured backup directory
#   scripts/backup.sh drill [ROOT]    create + verify + write perf/results/<stamp>-backup-drill-<machine>.json
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

# --- encryption --------------------------------------------------------------------------------

ENC=""
if command -v age >/dev/null 2>&1 && command -v age-keygen >/dev/null 2>&1; then ENC=age; else ENC=gpg; fi

# secret_ref: what manifest.json records — where the key came from, never the key itself.
secret_ref=""
key_file=""
ensure_key() { # create=1 lets a first run mint the local dev secret; verify must find one.
  if [[ "$ENC" == age ]]; then
    key_file="${COLOPHON_BACKUP_AGE_KEY_FILE:-deploy/backup-age-key.local}"
    if [[ ! -f "$key_file" ]]; then
      if [[ "${1:-}" != "create" ]]; then
        echo "error: no age identity at $key_file (set COLOPHON_BACKUP_AGE_KEY_FILE)" >&2; exit 1
      fi
      age-keygen -o "$key_file" 2>/dev/null
      chmod 600 "$key_file"
      echo "note: generated $key_file (gitignored, mode 600) — losing it loses every backup" >&2
    fi
    secret_ref="age identity ${COLOPHON_BACKUP_AGE_KEY_FILE:+COLOPHON_BACKUP_AGE_KEY_FILE=}$key_file"
  else
    key_file="${COLOPHON_BACKUP_PASSPHRASE_FILE:-deploy/backup-passphrase.local}"
    if [[ ! -f "$key_file" ]]; then
      if [[ "${1:-}" != "create" ]]; then
        echo "error: no passphrase file at $key_file (set COLOPHON_BACKUP_PASSPHRASE_FILE)" >&2; exit 1
      fi
      openssl rand -base64 48 >"$key_file"
      chmod 600 "$key_file"
      echo "note: generated $key_file (gitignored, mode 600) — losing it loses every backup" >&2
    fi
    secret_ref="gpg passphrase file ${COLOPHON_BACKUP_PASSPHRASE_FILE:+COLOPHON_BACKUP_PASSPHRASE_FILE=}$key_file"
  fi
}

# encrypt <out>: encrypt stdin to <out>. The passphrase stays in its file — nothing on the
# command line, so `ps` never sees it.
encrypt() {
  if [[ "$ENC" == age ]]; then
    age -e -R "$(age-keygen -y "$key_file")" -o "$1"
  else
    gpg --batch --yes --pinentry-mode loopback --passphrase-file "$key_file" \
      --cipher-algo AES256 --symmetric --output "$1"
  fi
}

# decrypt <file>: plaintext on stdout.
decrypt() {
  if [[ "$ENC" == age ]]; then
    age -d -i "$key_file" "$1"
  else
    gpg --batch --yes --pinentry-mode loopback --passphrase-file "$key_file" --decrypt "$1"
  fi
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

pg_verifybackup="skipped (stack not running)"
verify_backup() { # <dir>: integrity checks; exits 1 on any failure.
  local dir=$1 enc_file scratch
  [[ -d "$dir" ]] || { echo "error: $dir is not a backup directory" >&2; exit 2; }
  ensure_key verify

  if [[ -f "$dir/manifest.json" ]] && jq -e '
      .created_at and .git.commit and .postgres.version and .database.name and
      .encryption.key_reference and .payload.file and .payload.sha256 and
      (.migrations | type == "array") and has("deletion_ledger")' "$dir/manifest.json" >/dev/null; then
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

machine_name() {
  local cpu cores
  if [[ "$(uname -s)" == "Darwin" ]]; then
    cpu=$(sysctl -n machdep.cpu.brand_string); cores=$(sysctl -n hw.ncpu)
  else
    cpu=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//'); cores=$(nproc)
  fi
  printf '%s-%sc' "$cpu" "$cores" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '-' | sed 's/-$//'
}

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
  *)
    echo "usage: scripts/backup.sh create [root] | verify <dir> | drill [root]" >&2
    exit 2
    ;;
esac
