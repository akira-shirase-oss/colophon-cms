#!/usr/bin/env bash
# The provider-independent backup (deploy/droplet/DEPLOY.md, step 10): Neon's point-in-time
# restore covers a bad write, not losing the Neon account or project. Daily, this dumps the whole
# `colophon` database with pg_dump -Fc over sslmode=verify-full, encrypts it with age to the
# recipient in /etc/colophon/backup-age-recipient (the identity lives on the droplet, with a copy
# the maintainer keeps offline), and puts it in the R2 backups bucket under logical/. Then it prunes:
# among logical/ dumps, the newest of each of the 14 newest days and of each of the 8 newest ISO
# weeks stay; the newest dump always stays; nothing else does.
#
#   backup-logical.sh run            # dump, encrypt, upload, prune (the timer's command)
#   backup-logical.sh prune [--dry]  # the retention alone; --dry lists without deleting
#   backup-logical.sh latest         # the newest dump's name and age in hours (smoke.sh reads it)
#
# Reads, as root or through systemd credentials:
#   $CREDENTIALS_DIRECTORY/migration_database_url (or /etc/colophon/credentials/…)
#   $CREDENTIALS_DIRECTORY/s3_secret_key, and COLOPHON_S3_ENDPOINT / COLOPHON_S3_ACCESS_KEY from
#   /etc/colophon/colophon.env; BACKUP_BUCKET (default colophon-backups).
# Needs pg_dump 18 (postgresql-client-18), age and rclone.
set -euo pipefail
KEEP_DAILY="${KEEP_DAILY:-14}"
KEEP_WEEKLY="${KEEP_WEEKLY:-8}"
LOCK_DAYS="${LOCK_DAYS:-7}"   # the backups bucket's lock (CREATE.md row 5)
bucket="${BACKUP_BUCKET:-colophon-backups}"
creds="${CREDENTIALS_DIRECTORY:-/etc/colophon/credentials}"
env_file="${COLOPHON_ENV_FILE:-/etc/colophon/colophon.env}"
recipient_file="${BACKUP_AGE_RECIPIENT_FILE:-/etc/colophon/backup-age-recipient}"

setting() { sed -n "s/^$1=//p" "$env_file" | tail -n 1; }

# rclone's remote, from environment only (no config file with a secret in it).
r2() {
  RCLONE_CONFIG_R2_TYPE=s3 RCLONE_CONFIG_R2_PROVIDER=Cloudflare \
    RCLONE_CONFIG_R2_ENDPOINT="$(setting COLOPHON_S3_ENDPOINT)" \
    RCLONE_CONFIG_R2_ACCESS_KEY_ID="$(setting COLOPHON_S3_ACCESS_KEY)" \
    RCLONE_CONFIG_R2_SECRET_ACCESS_KEY="$(tr -d '\n' <"$creds/s3_secret_key")" \
    RCLONE_CONFIG_R2_NO_CHECK_BUCKET=true \
    rclone "$@"
}

# The dumps under logical/, oldest first (names sort by time: <UTC stamp>.dump.age).
dumps() { r2 lsf --files-only "r2:$bucket/logical/" | grep -E '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{6}Z\.dump\.age$' | sort || true; }

# iso_week <YYYY-MM-DD> → 2026-W47 (jq's libc strftime: the same on Linux and macOS).
iso_week() { jq -rn --arg d "$1" '$d | strptime("%Y-%m-%d") | strftime("%G-W%V")'; }

prune() {
  local mode=${1:-apply} name day week days="" weeks="" ndays=0 nweeks=0 first=1
  local -a keep=() drop=()
  while IFS= read -r name; do
    [[ -z "$name" ]] && continue
    day=${name:0:10}; week=$(iso_week "$day")
    local why=""
    if (( first )); then why="newest"; first=0; fi
    if [[ " $days " != *" $day "* ]] && (( ndays < KEEP_DAILY )); then days+=" $day"; ndays=$((ndays + 1)); why+=" daily"; fi
    if [[ " $weeks " != *" $week "* ]] && (( nweeks < KEEP_WEEKLY )); then weeks+=" $week"; nweeks=$((nweeks + 1)); why+=" weekly"; fi
    if [[ -n "$why" ]]; then keep+=("$name"); else drop+=("$name"); fi
  done < <(dumps | sort -r)
  for name in ${keep[@]+"${keep[@]}"}; do echo "keep   $name"; done
  # The backups bucket carries a 7-day lock (CREATE.md), so a stolen key can't wipe recent dumps:
  # a refusal that names the lock, for a dump under LOCK_DAYS old, is expected and logged; any
  # other failure fails the run, after every deletion has been tried.
  local out failed=0 age
  for name in ${drop[@]+"${drop[@]}"}; do
    if [[ "$mode" == dry ]]; then echo "prune  $name (dry run)"; continue; fi
    if out=$(r2 deletefile "r2:$bucket/logical/$name" 2>&1); then echo "pruned $name"; continue; fi
    age=$(( ($(date -u +%s) - $(stamp_epoch "$name")) / 86400 ))
    if (( age < LOCK_DAYS )) && grep -qiE 'object.?lock|locked|retention|WORM' <<<"$out"; then
      echo "locked $name ($age d old; the bucket lock keeps it until $LOCK_DAYS d)"
    else
      echo "error: couldn't delete $name ($age d old): $(tail -1 <<<"$out")" >&2
      failed=1
    fi
  done
  return $failed
}

# stamp_epoch <name>: the dump's UTC stamp as seconds.
stamp_epoch() { local stamp=${1%.dump.age}; jq -rn --arg s "$stamp" '$s | strptime("%Y-%m-%dT%H%M%SZ") | mktime'; }

latest() {
  local name stamp epoch
  name=$(dumps | tail -n 1)
  [[ -n "$name" ]] || { echo "none"; return 1; }
  epoch=$(stamp_epoch "$name")
  echo "$name $(( ($(date -u +%s) - epoch) / 3600 ))"
}

run() {
  [[ -s "$recipient_file" ]] || { echo "error: no age recipient in $recipient_file (DEPLOY.md step 10)" >&2; exit 1; }
  local url stamp
  url=$(tr -d '\n' <"$creds/migration_database_url")
  [[ "$url" == *sslmode=verify-full* ]] || { echo "error: the database URL must use sslmode=verify-full" >&2; exit 1; }
  stamp=$(date -u +%Y-%m-%dT%H%M%SZ)
  work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
  # The URL holds the password, so it reaches pg_dump as libpq's environment (readable by root
  # only), never argv. And the dump never touches the disk unencrypted: it streams into age.
  [[ "$url" =~ ^postgres(ql)?://([^:@/]+):([^@/]+)@([^:/?]+)(:([0-9]+))?/([^?]+)\?(.*)$ ]] \
    || { echo "error: can't read the database URL's parts" >&2; exit 1; }
  local user=${BASH_REMATCH[2]} pass=${BASH_REMATCH[3]} host=${BASH_REMATCH[4]} port=${BASH_REMATCH[6]:-5432} db=${BASH_REMATCH[7]}
  pass=$(printf '%b' "${pass//%/\\x}")   # percent-decoded
  PGHOST=$host PGPORT=$port PGUSER=$user PGPASSWORD=$pass PGDATABASE=$db PGSSLMODE=verify-full \
    PGSSLROOTCERT=system pg_dump --format=custom --no-owner \
    | age --encrypt --recipients-file "$recipient_file" >"$work/$stamp.dump.age"
  [[ -s "$work/$stamp.dump.age" ]] || { echo "error: the dump is empty" >&2; exit 1; }
  r2 copyto "$work/$stamp.dump.age" "r2:$bucket/logical/$stamp.dump.age"
  echo "backup-logical: uploaded logical/$stamp.dump.age ($(wc -c <"$work/$stamp.dump.age" | tr -d ' ') bytes)"
  prune apply
}

case "${1:-}" in
  run) run ;;
  prune) if [[ "${2:-}" == --dry ]]; then prune dry; else prune apply; fi ;;
  latest) latest ;;
  *) echo "usage: backup-logical.sh run | prune [--dry] | latest" >&2; exit 2 ;;
esac
