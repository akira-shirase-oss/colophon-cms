#!/usr/bin/env bash
# Tests that a backup or restore that fails or is killed halfway leaves no plaintext behind:
#
#   - scripts/backup.sh create copies the cluster into a scratch directory inside the postgres
#     container, unencrypted, before streaming it out through the encryption. If the stream fails,
#     or the run is killed while it streams, that scratch directory is removed all the same, and
#     no half-written payload is left beside the finished backups.
#   - scripts/restore.sh's prepare stage decrypts the payload straight into the container that
#     unpacks it: no decrypted copy is ever written to the restore's directory, so a failed unpack
#     leaves none behind.
#
# docker, gpg, openssl and age are stand-ins; nothing real is backed up or restored.
#
#   scripts/test-backup-plaintext.sh
set -euo pipefail
cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail() {
  echo "FAIL: $*" >&2
  exit 1
}

mkdir "$work/bin"
# docker: `compose ... exec -T postgres <cmd>`. The base backup's stream (sh -c ... sh DIR) writes a
# little "plaintext" and then fails (BACKUP_MODE=fail) or hangs until killed (BACKUP_MODE=hang).
cat >"$work/bin/docker" <<'EOF'
#!/usr/bin/env bash
echo "docker $*" >>"$CALLS"
args=" $* "
case "$args" in
  *" exec -T postgres true "*) exit 0 ;;
  *" exec -T postgres mktemp "*) echo /tmp/colophon-backup.TEST; exit 0 ;;
  *" exec -T postgres sh -c "*)
    echo "PLAINTEXT-CLUSTER-BYTES"
    if [[ "$BACKUP_MODE" == hang ]]; then touch "$STREAMING"; sleep 30; fi
    exit 1 ;;
  *" run "*) cat >/dev/null; exit 1 ;;  # the restore's unpack, failing
esac
exit 0
EOF
# gpg: encrypts stdin to --output, or decrypts a file to stdout.
cat >"$work/bin/gpg" <<'EOF'
#!/usr/bin/env bash
out=""; prev=""
for a in "$@"; do [[ "$prev" == --output ]] && out=$a; prev=$a; done
if [[ -n "$out" ]]; then { echo ENCRYPTED; cat; } >"$out"; else echo "PLAINTEXT-FROM-DECRYPT"; fi
EOF
printf '#!/usr/bin/env bash\necho not-random\n' >"$work/bin/openssl"
chmod +x "$work/bin/"*
echo "a-test-passphrase" >"$work/passphrase"

run_backup() { # MODE: runs scripts/backup.sh create into $work/backups with the stand-ins
  rm -rf "$work/backups" "$work/calls" "$work/streaming"
  env PATH="$work/bin:/usr/bin:/bin" CALLS="$work/calls" STREAMING="$work/streaming" BACKUP_MODE="$1" \
    COLOPHON_BACKUP_PASSPHRASE_FILE="$work/passphrase" scripts/backup.sh create "$work/backups"
}

# 1. The stream fails halfway.
status=0
run_backup fail >/dev/null 2>&1 || status=$?
[[ "$status" != 0 ]] || fail "a backup whose stream failed exited 0"
grep -q 'exec -T postgres rm -rf /tmp/colophon-backup.TEST' "$work/calls" \
  || fail "a failed backup left its plaintext scratch in the container: $(cat "$work/calls")"
if find "$work/backups" -name '*.tmp' 2>/dev/null | grep -q .; then fail "a failed backup left a half-written payload"; fi

# 2. The run is killed while it streams, as Ctrl-C or a service manager does: the whole process
#    group gets the signal (job control puts the background run in a group of its own).
set -m
run_backup hang >/dev/null 2>&1 &
pid=$!
set +m
for _ in $(seq 100); do [[ -f "$work/streaming" ]] && break; sleep 0.1; done
[[ -f "$work/streaming" ]] || fail "the stand-in stream never started"
kill -TERM -- "-$pid"
wait "$pid" 2>/dev/null || true
# The script's exit handler runs after the signal; give it up to ten seconds.
for _ in $(seq 100); do grep -q 'rm -rf /tmp/colophon-backup.TEST' "$work/calls" && break; sleep 0.1; done
grep -q 'exec -T postgres rm -rf /tmp/colophon-backup.TEST' "$work/calls" \
  || fail "a killed backup left its plaintext scratch in the container: $(cat "$work/calls")"

# 3. A restore's unpack fails: the decrypted payload went straight into the unpacking container
#    and was never written to the restore's directory.
mkdir -p "$work/backup" "$work/rbin"
cat >"$work/rbin/docker" <<'STUB'
#!/usr/bin/env bash
echo "docker $*" >>"$CALLS"
case " $* " in
  *" config --format json "*) echo '{"services":{"postgres":{"image":"postgres:test"},"valkey":{"image":"valkey:test"}}}' ;;
  *" run "*) cat >"$SEEN"; exit 1 ;;
esac
exit 0
STUB
printf '#!/usr/bin/env bash\necho PLAINTEXT-FROM-DECRYPT\n' >"$work/rbin/gpg"
chmod +x "$work/rbin/"*
echo ENCRYPTED >"$work/backup/base.tar.gz.gpg"
sha=$(openssl dgst -sha256 -r "$work/backup/base.tar.gz.gpg" | cut -d' ' -f1)
jq -n --arg sha "$sha" '{encryption: {tool: "gpg"}, database: {name: "colophon"}, payload: {file: "base.tar.gz.gpg", sha256: $sha}}' \
  >"$work/backup/manifest.json"
status=0
env PATH="$work/rbin:/usr/bin:/bin" CALLS="$work/rcalls" SEEN="$work/seen" COLOPHON_BACKUP_PASSPHRASE_FILE="$work/passphrase" \
  scripts/restore.sh "$work/backup" --step prepare --into "$work/restore" >/dev/null 2>&1 || status=$?
[[ "$status" != 0 ]] || fail "a restore whose unpack failed exited 0"
grep -q PLAINTEXT-FROM-DECRYPT "$work/seen" || fail "the decrypted payload didn't reach the unpacking container"
if grep -rq PLAINTEXT-FROM-DECRYPT "$work/restore" 2>/dev/null; then
  fail "a failed restore left decrypted bytes in its directory: $(grep -rl PLAINTEXT-FROM-DECRYPT "$work/restore")"
fi

echo "backups: no plaintext left behind on failure or kill"
