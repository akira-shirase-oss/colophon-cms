#!/usr/bin/env bash
# Tests scripts/sweep-test-databases.sh with a stand-in docker: on a stack that isn't the run's own
# it refuses before touching anything (D-187); on its own stack it drops exactly the leftover
# colophon_test_* and colophon_setup_* databases the server listed, and never another name, even
# one the server (wrongly) listed.
#
#   scripts/test-sweep-test-databases.sh
set -euo pipefail
cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail() {
  echo "FAIL: $*" >&2
  exit 1
}
mkdir "$work/bin"
cat >"$work/bin/docker" <<STUB
#!/usr/bin/env bash
echo "docker \$*" >>"$work/calls"
case "\$*" in
  *"SELECT d.datname"*)
    printf '%s\n' colophon_test_0123456789abcdef0123456789abcdef colophon_setup_fedcba9876543210fedcba9876543210 colophon colophon_test_short ;;
esac
exit 0
STUB
chmod +x "$work/bin/docker"
shared() { env -u CI -u COMPOSE_PROJECT_NAME -u COLOPHON_STACK_OWNER PATH="$work/bin:$PATH" "$@"; }

: >"$work/calls"
status=0
out=$(shared scripts/sweep-test-databases.sh 2>&1) || status=$?
[[ "$status" == 2 ]] || fail "on a shared stack: exit $status, wanted 2 ($out)"
[[ ! -s "$work/calls" ]] || fail "on a shared stack it called docker: $(cat "$work/calls")"

: >"$work/calls"
out=$(shared env CI=true scripts/sweep-test-databases.sh)
grep -q 'DROP DATABASE IF EXISTS colophon_test_0123456789abcdef0123456789abcdef$' "$work/calls" || fail "didn't drop the leftover test database: $(cat "$work/calls")"
grep -q 'DROP DATABASE IF EXISTS colophon_setup_fedcba9876543210fedcba9876543210$' "$work/calls" || fail "didn't drop the leftover setup database"
! grep -qE 'DROP DATABASE IF EXISTS (colophon|colophon_test_short)$' "$work/calls" || fail "dropped a database it must never touch: $(grep DROP "$work/calls")"
! grep -q 'WITH (FORCE)' "$work/calls" || fail "forced a drop"
grep -q 'dropped 2 leftover' <<<"$out" || fail "the summary: $out"

echo "sweep: refuses on a shared stack; drops exactly the leftovers on its own"
