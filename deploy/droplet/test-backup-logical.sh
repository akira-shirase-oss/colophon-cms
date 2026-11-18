#!/usr/bin/env bash
# Tests deploy/droplet/backup-logical.sh with stand-in rclone, pg_dump and age that record what
# they were given: the database password reaches pg_dump only through its environment (never
# argv), over verify-full; a URL without verify-full is refused before any dump; the upload lands
# under logical/; retention keeps the newest of the 14 newest days and of the 8 newest ISO weeks
# and the newest dump, and deletes the rest; `latest` names the newest dump and its age.
#
#   deploy/droplet/test-backup-logical.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail() {
  echo "FAIL: $*" >&2
  exit 1
}
mkdir -p "$work/bin" "$work/creds"
cat >"$work/bin/rclone" <<STUB
#!/usr/bin/env bash
echo "rclone \$*" >>"$work/calls"
case "\$1" in
  lsf) cat "$work/listing" 2>/dev/null ;;
  copyto) echo "\$2" >>"$work/uploaded"; basename "\$2" >>"$work/listing" ;;
  deletefile)
    n=\$(basename "\$2")
    # The bucket lock (CREATE.md): a dump under LOCK_STUB_DAYS old can't be deleted. FAIL_DELETE names
    # one dump whose deletion fails for another reason.
    if [[ -n "\${FAIL_DELETE:-}" && "\$n" == "\$FAIL_DELETE" ]]; then echo "ERROR : AccessDenied: Access Denied" >&2; exit 1; fi
    if [[ -n "\${LOCK_STUB_DAYS:-}" ]]; then
      age=\$(( (\$(date -u +%s) - \$(jq -rn --arg s "\${n%.dump.age}" '\$s | strptime("%Y-%m-%dT%H%M%SZ") | mktime')) / 86400 ))
      if (( age < LOCK_STUB_DAYS )); then echo "ERROR : ObjectLocked: The object is locked by the bucket's retention rule" >&2; exit 1; fi
    fi
    grep -vx "\$n" "$work/listing" >"$work/listing.new" || true; mv "$work/listing.new" "$work/listing" ;;
esac
STUB
cat >"$work/bin/pg_dump" <<STUB
#!/usr/bin/env bash
echo "argv: \$*" >>"$work/pg_dump"
echo "env: PGPASSWORD=\${PGPASSWORD:-} PGSSLMODE=\${PGSSLMODE:-} PGHOST=\${PGHOST:-} PGUSER=\${PGUSER:-} PGDATABASE=\${PGDATABASE:-}" >>"$work/pg_dump"
printf 'PGDMP-fake'
STUB
printf '#!/usr/bin/env bash\necho "age $*" >>"%s/calls"\nprintf "AGE:"; cat\n' "$work" >"$work/bin/age"
chmod +x "$work/bin/"*
printf 'COLOPHON_S3_ENDPOINT=https://acct.r2.cloudflarestorage.com\nCOLOPHON_S3_ACCESS_KEY=AKID\n' >"$work/colophon.env"
echo 'r2-secret' >"$work/creds/s3_secret_key"
echo 'age1recipientexample' >"$work/recipient"
backup() { env PATH="$work/bin:$PATH" CREDENTIALS_DIRECTORY="$work/creds" COLOPHON_ENV_FILE="$work/colophon.env" \
  BACKUP_AGE_RECIPIENT_FILE="$work/recipient" deploy/droplet/backup-logical.sh "$@"; }

# run: the password only in pg_dump's environment, verify-full, encrypted, uploaded under logical/.
echo 'postgres://owner:s3cr%40t@ep-x.us-east-2.aws.neon.tech/colophon?sslmode=verify-full' >"$work/creds/migration_database_url"
backup run >/dev/null
grep '^argv:' "$work/pg_dump" | grep -q 's3cr' && fail "the password reached pg_dump's argv: $(cat "$work/pg_dump")"
grep -q 'PGPASSWORD=s3cr@t PGSSLMODE=verify-full PGHOST=ep-x.us-east-2.aws.neon.tech PGUSER=owner PGDATABASE=colophon' "$work/pg_dump" \
  || fail "pg_dump's environment: $(cat "$work/pg_dump")"
grep -qE '\.dump\.age$' "$work/uploaded" || fail "nothing uploaded: $(cat "$work/calls")"
grep -qE 'rclone copyto .* r2:colophon-backups/logical/[0-9TZ-]+\.dump\.age' "$work/calls" || fail "the upload's key: $(cat "$work/calls")"
grep -q 'age --encrypt --recipients-file' "$work/calls" || fail "not encrypted to the recipient file"
grep -q 'PGDMP' "$(head -1 "$work/uploaded")" 2>/dev/null && fail "an unencrypted dump was uploaded"

# A URL without verify-full: refused before any dump.
: >"$work/pg_dump"
echo 'postgres://owner:pw@ep-x.neon.tech/colophon?sslmode=require' >"$work/creds/migration_database_url"
backup run >/dev/null 2>&1 && fail "a URL without verify-full was dumped"
[[ ! -s "$work/pg_dump" ]] || fail "pg_dump ran for a URL without verify-full"

# Retention: 40 daily dumps ending today. Kept: the 14 newest days, plus the newest of each of the
# 8 newest ISO weeks (most already among the 14 days); every other one is deleted.
: >"$work/listing"
for i in $(seq 39 -1 0); do
  jq -rn --argjson i "$i" '(now - $i*86400) | strftime("%Y-%m-%dT031700Z.dump.age")' >>"$work/listing"
done
expected=$(sort -r "$work/listing" | awk '
  { d=substr($0,1,10); cmd="date -u -j -f %Y-%m-%d " d " +%G-W%V 2>/dev/null || date -u -d " d " +%G-W%V"; cmd | getline w; close(cmd);
    keep=0; if (!(d in days) && nd<14) {days[d]=1; nd++; keep=1} if (!(w in weeks) && nw<8) {weeks[w]=1; nw++; keep=1}
    if (NR==1) keep=1; if (keep) print }' | sort)
backup prune >/dev/null
kept=$(sort "$work/listing")
[[ "$kept" == "$expected" ]] || fail "retention kept $(wc -l <<<"$kept") dumps, wanted $(wc -l <<<"$expected"): $(diff <(echo "$kept") <(echo "$expected") | head)"
(( $(wc -l <<<"$kept") >= 14 && $(wc -l <<<"$kept") <= 22 )) || fail "retention kept $(wc -l <<<"$kept")"

# The bucket lock: pruning harder than the lock allows (2 days kept), the young dumps' refusals are
# expected and logged, the old ones go, and the run passes.
listing_for() { : >"$work/listing"; for i in $(seq 12 -1 0); do jq -rn --argjson i "$i" '(now - $i*86400 - 60) | strftime("%Y-%m-%dT%H%M%SZ.dump.age")' >>"$work/listing"; done; }
listing_for
out=$(env LOCK_STUB_DAYS=7 KEEP_DAILY=2 KEEP_WEEKLY=1 PATH="$work/bin:$PATH" CREDENTIALS_DIRECTORY="$work/creds" COLOPHON_ENV_FILE="$work/colophon.env" \
  BACKUP_AGE_RECIPIENT_FILE="$work/recipient" deploy/droplet/backup-logical.sh prune) || fail "a lock refusal for a young dump failed the prune: $out"
grep -q '^locked ' <<<"$out" || fail "no dump was reported locked: $out"
grep -q '^pruned ' <<<"$out" || fail "the old dumps weren't pruned: $out"
(( $(grep -c '^locked ' <<<"$out") <= 7 )) || fail "more than 7 days of dumps reported locked: $out"

# Any other refusal fails the prune: an old dump the bucket won't delete for another reason.
listing_for
victim=$(head -1 "$work/listing")
if env FAIL_DELETE="$victim" KEEP_DAILY=2 KEEP_WEEKLY=1 PATH="$work/bin:$PATH" CREDENTIALS_DIRECTORY="$work/creds" COLOPHON_ENV_FILE="$work/colophon.env" \
  BACKUP_AGE_RECIPIENT_FILE="$work/recipient" deploy/droplet/backup-logical.sh prune >/dev/null 2>&1; then
  fail "a delete refused for another reason (AccessDenied) didn't fail the prune"
fi
# And a lock refusal for a dump OLDER than the lock is an error too (the lock shouldn't hold it).
listing_for
if env LOCK_STUB_DAYS=30 LOCK_DAYS=7 KEEP_DAILY=2 KEEP_WEEKLY=1 PATH="$work/bin:$PATH" CREDENTIALS_DIRECTORY="$work/creds" COLOPHON_ENV_FILE="$work/colophon.env" \
  BACKUP_AGE_RECIPIENT_FILE="$work/recipient" deploy/droplet/backup-logical.sh prune >/dev/null 2>&1; then
  fail "a lock refusal for a dump older than the lock didn't fail the prune"
fi

# latest: the newest dump, and its age in hours.
out=$(backup latest)
[[ "$out" == "$(sort "$work/listing" | tail -1) "* ]] || fail "latest: $out"

echo "backup-logical: password only in the environment, verify-full or nothing, encrypted upload, 14 daily + 8 weekly kept, the bucket lock's refusals expected and any other failing"
