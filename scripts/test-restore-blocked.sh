#!/usr/bin/env bash
# Tests that a restore whose payload doesn't match its manifest's SHA-256 stops with a
# stages/prepare.blocked marker naming why (deploy/runbooks/restore.md: "prepare.blocked: payload
# SHA-256 mismatch"), so `restore.sh status` shows a blocked stage, not nothing; and that it
# decrypts and unpacks nothing. docker and gpg are stand-ins.
#
#   scripts/test-restore-blocked.sh
set -euo pipefail
cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail() {
  echo "FAIL: $*" >&2
  exit 1
}
mkdir -p "$work/bin" "$work/backup"
cat >"$work/bin/docker" <<STUB
#!/usr/bin/env bash
echo "docker \$*" >>"$work/calls"
case " \$* " in
  *" config --format json "*) echo '{"services":{"postgres":{"image":"postgres:test"},"valkey":{"image":"valkey:test"}}}' ;;
esac
exit 0
STUB
printf '#!/usr/bin/env bash\necho "gpg $*" >>"%s/calls"\necho PLAINTEXT\n' "$work" >"$work/bin/gpg"
chmod +x "$work/bin/"*
echo "a-test-passphrase" >"$work/passphrase"
echo ENCRYPTED >"$work/backup/base.tar.gz.gpg"
# The manifest records a different hash than the stored file has.
jq -n '{encryption: {tool: "gpg"}, database: {name: "colophon"}, payload: {file: "base.tar.gz.gpg", sha256: "0000000000000000000000000000000000000000000000000000000000000000"}}' \
  >"$work/backup/manifest.json"
: >"$work/calls"

status=0
env PATH="$work/bin:/usr/bin:/bin" COLOPHON_BACKUP_PASSPHRASE_FILE="$work/passphrase" \
  scripts/restore.sh "$work/backup" --step prepare --into "$work/restore" >/dev/null 2>&1 || status=$?
[[ "$status" != 0 ]] || fail "a payload that fails its SHA-256 restored"
blocked="$work/restore/stages/prepare.blocked"
[[ -f "$blocked" ]] || fail "no stages/prepare.blocked after a SHA-256 mismatch: $(ls "$work/restore/stages" 2>/dev/null)"
jq -e '.status == "blocked" and (.reason | test("SHA-256"))' "$blocked" >/dev/null || fail "the marker doesn't say why: $(cat "$blocked")"
! grep -qE '^gpg |docker .* run ' "$work/calls" || fail "it decrypted or unpacked a payload that failed its hash: $(cat "$work/calls")"
status_out=$(env PATH="$work/bin:/usr/bin:/bin" scripts/restore.sh status "$work/restore" 2>&1 || true)
grep -qi 'blocked  *prepare' <<<"$status_out" || fail "restore.sh status doesn't show the blocked stage: $status_out"

echo "restore: a payload that fails its hash blocks prepare, with the reason, and is never opened"
