#!/usr/bin/env bash
# The integration half's starters keep fixed ports (CI gives every job its own runner), so each must
# refuse to start when its port already has a listener, rather than test whatever holds it.
# scripts/testbed.sh and scripts/studio.sh run here as copies in a scratch tree whose later steps
# are stubs: the guard has to stop them before the first one, so this never touches the stack.
# load.sh's own servers go through the same scripts/port-guard.sh guard.
#
#   scripts/test-port-guard.sh
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0
scratch=$(mktemp -d)
holder=""
# shellcheck disable=SC2329 # invoked by the EXIT trap below
cleanup() {
  [[ -z "$holder" ]] || kill "$holder" 2>/dev/null || true
  rm -rf "$scratch"
}
trap cleanup EXIT

# A listener on a free port, held until this script exits; prints the port.
hold() {
  node -e "const s = require('node:net').createServer(); s.listen(0, '127.0.0.1', () => { console.log(s.address().port); }); setTimeout(() => {}, 600000);" >"$scratch/port" &
  holder=$!
  for _ in $(seq 1 50); do [[ -s "$scratch/port" ]] && break; sleep 0.1; done
  cat "$scratch/port"
}

# A tree with the real script and guard, and stubs that say "reached" for everything after it.
mkdir -p "$scratch/tree/scripts" "$scratch/tree/node_modules"
cp scripts/testbed.sh scripts/studio.sh "$scratch/tree/scripts/"
cp scripts/port-guard.sh "$scratch/tree/scripts/" 2>/dev/null || true
for stub in check-toolchain.sh migrate.sh; do
  printf '#!/usr/bin/env bash\necho "reached %s past the port guard" >&2\nexit 99\n' "$stub" >"$scratch/tree/scripts/$stub"
  chmod +x "$scratch/tree/scripts/$stub"
done

port=$(hold)

# expect <name> <script> <env assignment>: exit 1, naming the port, before any later step.
expect() {
  local out code=0
  out=$(env "$3" bash "$scratch/tree/scripts/$2" 2>&1) || code=$?
  if [[ "$code" == 1 && "$out" == *"port $port already has a listener"* && "$out" != *reached* ]]; then
    echo "ok   $1"
  else
    echo "FAIL $1: exit $code"; printf '     %s\n' "${out//$'\n'/$'\n'     }"; fail=1
  fi
}

expect "testbed.sh refuses a taken server port" testbed.sh "COLOPHON_TESTBED_SERVER_PORT=$port"
expect "testbed.sh refuses a taken testbed port" testbed.sh "COLOPHON_TESTBED_PORT=$port"
expect "studio.sh refuses a taken Studio port" studio.sh "COLOPHON_STUDIO_PORT=$port"

# Free ports pass the guard (the first stub is then reached).
free=$(node -e "const s = require('node:net').createServer(); s.listen(0, '127.0.0.1', () => { console.log(s.address().port); s.close(); });")
out=$(COLOPHON_STUDIO_PORT=$free bash "$scratch/tree/scripts/studio.sh" 2>&1) || true
if [[ "$out" == *"reached check-toolchain.sh"* ]]; then echo "ok   a free port passes the guard"; else echo "FAIL a free port: $out"; fail=1; fi

# The testbed preview may not drift to the next port either.
# shellcheck disable=SC2016 # the config's own template text, matched literally
if grep -qF -- '--port ${TESTBED_PORT} --strictPort' apps/testbed/playwright.config.ts; then
  echo "ok   the testbed preview is strict about its port"
else
  echo "FAIL the testbed preview can move to another port"; fail=1
fi

exit "$fail"
