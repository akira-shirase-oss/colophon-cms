#!/usr/bin/env bash
# scripts/retry.sh retries a failing command, stops at the first success, gives up after N tries
# with the last status, and refuses a bad count.
set -euo pipefail
cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
export RETRY_DELAY=0

# A command that fails until its Nth call.
cat >"$work/flaky" <<'SH'
#!/usr/bin/env bash
n=$(( $(cat "$1" 2>/dev/null || echo 0) + 1 )); echo "$n" >"$1"
(( n >= $2 )) || exit 7
SH
chmod +x "$work/flaky"

fail() { echo "test-retry: $*" >&2; exit 1; }

scripts/retry.sh 3 "$work/flaky" "$work/a" 3 2>/dev/null || fail "a command that succeeds on its third call failed under 3 tries"
[[ $(cat "$work/a") == 3 ]] || fail "expected 3 calls, got $(cat "$work/a")"

scripts/retry.sh 3 "$work/flaky" "$work/b" 1 2>/dev/null || fail "a command that succeeds at once failed"
[[ $(cat "$work/b") == 1 ]] || fail "a success was run again ($(cat "$work/b") calls)"

status=0; scripts/retry.sh 2 "$work/flaky" "$work/c" 5 2>/dev/null || status=$?
[[ $status == 7 ]] || fail "giving up should return the last status 7, got $status"
[[ $(cat "$work/c") == 2 ]] || fail "expected 2 calls before giving up, got $(cat "$work/c")"

status=0; scripts/retry.sh 0 true 2>/dev/null || status=$?
[[ $status == 2 ]] || fail "a count of 0 should be refused with 2, got $status"

echo "test-retry: ok"
