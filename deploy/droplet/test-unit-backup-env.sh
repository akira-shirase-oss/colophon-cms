#!/usr/bin/env bash
# CI only: the droplet's two backup units, exactly as shipped, on a real systemd (a GitHub runner
# VM), with stand-ins for the programs they start that print what they were given:
#   - colophon-ledger-export.service writes to colophon-backups with the backups' own key id and
#     secret, though colophon.env names colophon-media and the media key (EnvironmentFile=
#     overrides Environment=, which once sent the ledger to the media bucket);
#   - colophon-backup-logical.service is handed the backups' secret, and neither unit the media one;
#   - without COLOPHON_BACKUP_S3_ACCESS_KEY, the ledger export refuses to start its program;
#   - the server's unit loads the media secret alone.
#
#   sudo CI=true deploy/droplet/test-unit-backup-env.sh
set -euo pipefail
[[ "${CI:-}" == true ]] || { echo "refusing: this installs system-wide units; it runs in CI only" >&2; exit 2; }
[[ $EUID -eq 0 ]] || { echo "run with sudo" >&2; exit 2; }
cd "$(dirname "$0")"
fail() { echo "FAIL: $*" >&2; exit 1; }
units=(colophon-ledger-export.service colophon-backup-logical.service)
journal_file=$(mktemp)
cleanup() {
  rm -f "$journal_file"
  for u in "${units[@]}"; do rm -f "/etc/systemd/system/$u"; done
  systemctl daemon-reload || true
}
trap cleanup EXIT

install -d -m 0755 /etc/colophon /usr/local/bin
install -d -m 0700 /etc/colophon/credentials
umask 077
printf 'postgres://o:p@db.example.com/colophon?sslmode=verify-full' >/etc/colophon/credentials/migration_database_url
printf 'media-secret' >/etc/colophon/credentials/s3_secret_key
printf 'backup-secret' >/etc/colophon/credentials/backup_s3_secret_key
chmod 0400 /etc/colophon/credentials/*
umask 022
env_file() {
  printf 'COLOPHON_S3_ENDPOINT=https://acct.r2.cloudflarestorage.com\nCOLOPHON_S3_BUCKET=colophon-media\nCOLOPHON_S3_ACCESS_KEY=MEDIAKEY\n' >/etc/colophon/colophon.env
  [[ "${1:-}" == without ]] || echo 'COLOPHON_BACKUP_S3_ACCESS_KEY=BACKUPKEY' >>/etc/colophon/colophon.env
}

# The stand-ins: what they were started with, on stdout (the journal).
cat >/usr/local/bin/colophon <<'STUB'
#!/bin/sh
echo "seen: bucket=$COLOPHON_S3_BUCKET key=$COLOPHON_S3_ACCESS_KEY secret=$(cat "$COLOPHON_S3_SECRET_KEY_FILE") creds=$(ls "$CREDENTIALS_DIRECTORY" | tr '\n' ,) args=$*"
STUB
cat >/usr/local/bin/colophon-backup-logical <<'STUB'
#!/bin/sh
echo "seen: creds=$(ls "$CREDENTIALS_DIRECTORY" | tr '\n' ,)"
STUB
chmod 0755 /usr/local/bin/colophon /usr/local/bin/colophon-backup-logical
for u in "${units[@]}"; do install -m 0644 "$u" /etc/systemd/system/; done
systemctl daemon-reload
# seen <unit>: start it once (oneshot) and print its stand-in's line from THIS start only: the
# journal after a cursor taken just before (a second-resolution --since caught the previous start's
# line, and a finished oneshot unit no longer reports its InvocationID). The start's whole journal
# goes to $journal_file: seen runs in a command substitution, so a variable wouldn't survive it.
seen() {
  local cursor
  cursor=$(journalctl -n 1 -o cat --show-cursor --no-pager 2>/dev/null | sed -n 's/^-- cursor: //p' | tail -1)
  systemctl start "$1" || true
  if [[ -n "$cursor" ]]; then journalctl -u "$1" --after-cursor="$cursor" --no-pager -o cat >"$journal_file"
  else journalctl -u "$1" --no-pager -o cat >"$journal_file"; fi
  grep '^seen:' "$journal_file" | tail -1 || true
}

env_file
got=$(seen colophon-ledger-export.service)
[[ "$got" == "seen: bucket=colophon-backups key=BACKUPKEY secret=backup-secret creds=backup_s3_secret_key,migration_database_url, args=admin ledger-export --s3-key deletion-ledger/"*".jsonl --json" ]] \
  || fail "the ledger export was started with: ${got:-nothing} ($(journalctl -u colophon-ledger-export --no-pager -n 5 -o cat))"
echo "  ok    the ledger export: colophon-backups, the backups' key id and secret, never the media ones ($got)"

got=$(seen colophon-backup-logical.service)
[[ "$got" == "seen: creds=backup_s3_secret_key,migration_database_url," ]] || fail "the logical backup's credentials: ${got:-nothing}"
echo "  ok    the logical backup is handed the backups' secret, not the media one"

env_file without
got=$(seen colophon-ledger-export.service)
[[ -z "$got" ]] || fail "the ledger export ran without COLOPHON_BACKUP_S3_ACCESS_KEY: $got"
grep -q 'no COLOPHON_BACKUP_S3_ACCESS_KEY' "$journal_file" \
  || fail "no refusal in the journal: $(tail -5 "$journal_file")"
echo "  ok    without COLOPHON_BACKUP_S3_ACCESS_KEY the ledger export refuses to start"

grep -q 'backup_s3_secret_key' ../production/colophon.service && fail "the server's unit loads the backups' secret"
grep -q '^LoadCredential=s3_secret_key:' ../production/colophon.service || fail "the server's unit lost its media secret"
echo "  ok    the server's unit loads the media secret alone"
echo "backup units: the backups' own R2 key and bucket reach both units; the media key reaches neither"
