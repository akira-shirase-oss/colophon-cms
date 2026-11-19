#!/usr/bin/env bash
# Tests that a backup key scripts/backup-crypt.sh mints is never readable by anyone else, not even
# for a moment: the file is created with mode 600, before a byte of the secret is written into it,
# for both kinds of key (a gpg passphrase file and an age identity). With a umask of 022, a key
# written by a plain redirect and chmod-ed afterwards would exist for a while as 644.
#
# openssl and age-keygen are stand-ins that record the key file's mode at the moment they write
# the secret.
#
#   scripts/test-backup-key-mode.sh
set -euo pipefail
cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail() {
  echo "FAIL: $*" >&2
  exit 1
}
mode() { stat -c %a "$1" 2>/dev/null || stat -f %Lp "$1"; }
export -f mode

mkdir "$work/bin"
# openssl rand -base64 48 > KEY: by the time it runs, the shell has created KEY.
cat >"$work/bin/openssl" <<'EOF'
#!/usr/bin/env bash
mode "$KEY_UNDER_TEST" >"$MODE_LOG"
echo "not-a-real-passphrase"
EOF
# age-keygen -o KEY: it creates KEY itself, as the real one does, under the caller's umask.
cat >"$work/bin/age-keygen" <<'EOF'
#!/usr/bin/env bash
[[ "$1" == -o ]] || exit 2
: >"$2"
mode "$2" >"$MODE_LOG"
echo "AGE-SECRET-KEY-NOT-REAL" >"$2"
EOF
printf '#!/usr/bin/env bash\nexit 0\n' >"$work/bin/age"
chmod +x "$work/bin/"*

# mint ENC: source backup-crypt.sh with a umask that would leave a fresh file readable by others.
mint() {
  local tools=$1 key=$2
  rm -f "$key" "$work/mode"
  (
    umask 022
    export PATH="$tools:/usr/bin:/bin" MODE_LOG="$work/mode" KEY_UNDER_TEST="$key"
    export COLOPHON_BACKUP_PASSPHRASE_FILE="$key" COLOPHON_BACKUP_AGE_KEY_FILE="$key"
    # shellcheck source=scripts/backup-crypt.sh
    . scripts/backup-crypt.sh
    ensure_key create 2>/dev/null
    echo "$ENC" >"$work/enc"
  )
}

# The gpg passphrase: no age on PATH.
mkdir "$work/gpg-tools"
cp "$work/bin/openssl" "$work/gpg-tools/"
mint "$work/gpg-tools" "$work/passphrase"
[[ "$(cat "$work/enc")" == gpg ]] || fail "expected the gpg path, got $(cat "$work/enc")"
[[ "$(cat "$work/mode")" == 600 ]] || fail "the passphrase file was mode $(cat "$work/mode") while its secret was written"
[[ "$(mode "$work/passphrase")" == 600 ]] || fail "the passphrase file ended as mode $(mode "$work/passphrase")"

# The age identity.
mint "$work/bin" "$work/identity"
[[ "$(cat "$work/enc")" == age ]] || fail "expected the age path, got $(cat "$work/enc")"
[[ "$(cat "$work/mode")" == 600 ]] || fail "the age identity was mode $(cat "$work/mode") while its secret was written"
[[ "$(mode "$work/identity")" == 600 ]] || fail "the age identity ended as mode $(mode "$work/identity")"

echo "backup keys: created 600, never readable by others"
