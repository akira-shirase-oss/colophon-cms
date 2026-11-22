#!/usr/bin/env bash
# r2-keys.sh: the backups have their own R2 token, unless the operator chose one token for both buckets
# (r2_backup_shares_media=yes), which is accepted with a warning and nothing else.
#
#   deploy/droplet/test-r2-keys.sh
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=deploy/droplet/r2-keys.sh
. "$here/r2-keys.sh"
fail() { echo "FAIL: $*" >&2; exit 1; }
t=$(mktemp -d); trap 'rm -rf "$t"' EXIT
media_id=0123456789abcdef0123456789abcdef; backup_id=fedcba9876543210fedcba9876543210
printf 'media-secret' >"$t/media"; printf 'backup-secret' >"$t/backup"; printf 'media-secret' >"$t/same"
mkdir "$t/install"

# Not shared: two tokens pass; the same key id or the same secret is refused.
r2_backup_check "$media_id" "$backup_id" "$t/media" "$t/backup" "$t/install" 2>"$t/err" || fail "two tokens refused: $(cat "$t/err")"
grep -q WARNING "$t/err" && fail "two tokens warned"
r2_backup_check "$media_id" "$media_id" "$t/media" "$t/backup" "$t/install" 2>"$t/err" && fail "the media key id accepted for the backups"
grep -q 'need their own R2 token' "$t/err" || fail "the key id refusal doesn't say why: $(cat "$t/err")"
grep -q 'r2_backup_shares_media=yes' "$t/err" || fail "the refusal doesn't name the way to accept one token"
r2_backup_check "$media_id" "$backup_id" "$t/media" "$t/same" "$t/install" 2>"$t/err" && fail "the media secret accepted for the backups"
r2_backup_shared "$t/install" && fail "shared without the file"

# Only an exact "yes" (whitespace aside) is the choice.
for v in no YES y 'yes please' ''; do
  printf '%s' "$v" >"$t/install/r2_backup_shares_media"
  r2_backup_shared "$t/install" && fail "'$v' read as the choice to share"
done
printf 'yes\n' >"$t/install/r2_backup_shares_media"
r2_backup_shared "$t/install" || fail "'yes' not read as the choice to share"

# Shared: the media token on both passes, with a warning; anything else is refused.
r2_backup_check "$media_id" "$media_id" "$t/media" "$t/same" "$t/install" 2>"$t/err" || fail "the shared token refused: $(cat "$t/err")"
grep -q 'WARNING: the backups use the media R2 token' "$t/err" || fail "the shared token didn't warn"
r2_backup_check "$media_id" "$backup_id" "$t/media" "$t/same" "$t/install" 2>"$t/err" && fail "shared, with another key id, accepted"
r2_backup_check "$media_id" "$media_id" "$t/media" "$t/backup" "$t/install" 2>"$t/err" && fail "shared, with another secret, accepted"
grep -q "isn't the media token's" "$t/err" || fail "the shared mismatch doesn't say why: $(cat "$t/err")"
echo "r2 keys: two tokens pass, one token passes only by the operator's choice and with a warning"
