#!/usr/bin/env bash
# Linux hands outgoing connections local ports from 32768 to 60999, and every fixed port CI listens on
# (the stack's, load.sh's servers', the droplet credential test's) is in that range. Any socket can
# take one first and the listener's bind fails: k6 took 58090 ("address in use", twice), and a
# loopback client took 56398 before the credential test's Valkey bound it (quick start, CI
# 44868206132). So every job's first step moves outgoing ports to 20000-50999, before checkout or
# any other step can open a socket, and nothing CI runs listens on a fixed port inside that range.
# Fails naming each job whose first step isn't the guard, and each fixed listener port in the range.
#
#   scripts/test-ci-ports.sh               the repository's workflows and CI-run files
#   scripts/test-ci-ports.sh --self-test   also proves it fails on a missing, late or wrong guard and
#                                          on a planted port
set -euo pipefail
cd "$(dirname "$0")/.."

GUARD_NAME="Keep outgoing ports below the stack's"
GUARD_RUN='sudo sysctl -w net.ipv4.ip_local_port_range="20000 50999"'

# Each job of each workflow whose first step isn't the guard (by name and command), and each job on a
# runner matrix whose guard isn't limited to Linux (the setting exists only there).
unguarded() {
  awk -v name="$GUARD_NAME" -v cmd="$GUARD_RUN" '
    function verdict() {
      if (job == "") return
      if (!seen) { print file ": job " job " has no steps"; return }
      if (!(first ~ ("name: " name) && index(first, "run: " cmd))) {
        print file ": job " job " (line " line "): step 1 is not the guard"; return
      }
      if (matrix && index(first, "if: runner.os == '\''Linux'\''") == 0) {
        print file ": job " job " (line " line "): runs on a matrix, so its guard needs if: runner.os == '\''Linux'\''"
      }
    }
    # A new file: judge the last job of the one before, then start over.
    FNR == 1 { verdict(); injobs = 0; job = ""; file = FILENAME }
    /^jobs:/ { injobs = 1; next }
    !injobs { next }
    /^  [A-Za-z0-9_-]+:[[:space:]]*$/ {
      verdict(); job = $1; sub(/:$/, "", job); line = FNR; seen = 0; collecting = 0; steps = 0; matrix = 0; first = ""; next
    }
    /^    runs-on:/ && /matrix\./ { matrix = 1 }
    /^    steps:/ { steps = 1; next }
    steps && /^      - / { if (seen) { collecting = 0; steps = 0; next } seen = 1; collecting = 1; first = $0; next }
    collecting && /^        / { first = first "\n" $0; next }
    collecting { collecting = 0; steps = 0 }
    END { verdict() }
  ' "$@"
}

# Fixed ports inside 20000-50999 that CI-run files listen on or point at: host:port, -p, PORT=, --port.
# Hex digests and longer numbers don't match (no letter or digit may touch the number).
in_range_ports() {
  perl -ne '
    while (/(?::|[A-Z_]*PORT[A-Z_]*\s*=\s*["\x27]?(?:\$\{[A-Z_]+:-)?|-p\s+|--port[= ])(\d{5})(?![0-9A-Za-z])/g) {
      my $p = $1;
      if ($p >= 20000 && $p <= 50999 && !/ip_local_port_range|sha256|digest/) { print "$ARGV:$.: $p: $_"; last }
    }
    close ARGV if eof;
  ' "$@"
}

ci_files() {
  git ls-files .github/workflows scripts deploy perf/k6 perf/*.mjs perf/node perf/receiver perf/saturation \
    'apps/*/vite.config.ts' 'apps/*/playwright.config.ts' \
    | grep -vE '^perf/(results|baselines)/|\.(json|md|png|svg|lock)$|^scripts/test-ci-ports\.sh$' || true
}

check() {
  local workflows=$1; shift
  local found=0 out
  out=$(unguarded "$workflows"/*.yml)
  if [[ -n "$out" ]]; then
    echo "FAIL: a job's first step must be the outgoing-port guard ($GUARD_NAME: $GUARD_RUN):" >&2
    echo "$out" >&2
    found=1
  fi
  out=$( (($# == 0)) || in_range_ports "$@")
  if [[ -n "$out" ]]; then
    echo "FAIL: fixed port(s) inside the outgoing range 20000-50999, where any client socket can take them first:" >&2
    echo "$out" >&2
    found=1
  fi
  return "$found"
}

# shellcheck disable=SC2016 # the self-test's workflow text and planted scripts are literal, never expanded
if [[ "${1:-}" == "--self-test" ]]; then
  scratch=$(mktemp -d)
  trap 'rm -rf "$scratch"' EXIT
  guard=$'      - name: '"$GUARD_NAME"$'\n        run: '"$GUARD_RUN"
  linux_guard=$'      - name: '"$GUARD_NAME"$'\n        if: runner.os == \x27Linux\x27\n        run: '"$GUARD_RUN"
  checkout=$'      - uses: actions/checkout@v4'
  expect() { # expect pass|fail <label> <workflow body> [file contents]
    local want=$1 label=$2 body=$3 extra=${4:-} dir got
    dir=$(mktemp -d "$scratch/case.XXXX")
    mkdir -p "$dir/wf"
    printf 'on: push\njobs:\n%s\n' "$body" >"$dir/wf/ci.yml"
    # A second, guarded workflow after it, so a job that only the first file's end would judge is still judged.
    printf 'on: push\njobs:\n  z:\n    runs-on: ubuntu-latest\n    steps:\n%s\n' "$guard" >"$dir/wf/zz.yml"
    local files=()
    if [[ -n "$extra" ]]; then printf '%s\n' "$extra" >"$dir/script.sh"; files=("$dir/script.sh"); fi
    if check "$dir/wf" ${files[@]+"${files[@]}"} 2>"$dir/err"; then got=pass; else got=fail; fi
    if [[ "$got" != "$want" ]]; then echo "self-test FAILED: $label: expected $want, got $got" >&2; cat "$dir/err" >&2; exit 1; fi
    echo "self-test ok: $label ($got)"
  }
  job() { printf '  %s:\n    runs-on: %s\n    steps:\n%s\n' "$1" "$2" "$3"; }
  expect pass "guard first" "$(job a ubuntu-latest "$guard"$'\n'"$checkout")"
  expect fail "no guard" "$(job a ubuntu-latest "$checkout")"
  expect fail "guard after checkout" "$(job a ubuntu-latest "$checkout"$'\n'"$guard")"
  expect fail "one of two jobs unguarded" "$(job a ubuntu-latest "$guard")"$'\n'"$(job b ubuntu-latest "$checkout")"
  expect fail "a different range" "$(job a ubuntu-latest $'      - name: '"$GUARD_NAME"$'\n        run: sudo sysctl -w net.ipv4.ip_local_port_range="32768 60999"')"
  expect fail "matrix guard not limited to Linux" "$(job a '${{ matrix.os }}' "$guard")"
  expect pass "matrix guard limited to Linux" "$(job a '${{ matrix.os }}' "$linux_guard")"
  expect fail "planted -p port in range" "$(job a ubuntu-latest "$guard")" 'docker run -p 127.0.0.1:41234:6379 valkey'
  expect fail "planted PORT= in range" "$(job a ubuntu-latest "$guard")" 'LOAD_PORT=${LOAD_PORT:-30090}'
  expect pass "a listener above the range" "$(job a ubuntu-latest "$guard")" 'docker run -p 127.0.0.1:56398:6379 valkey'
  expect pass "a digest and a dynamic port" "$(job a ubuntu-latest "$guard")" $'image: valkey@sha256:63346cb24a61221e76bd\ndocker run -p 127.0.0.1::6379 valkey'
fi

files=()
while IFS= read -r file; do files+=("$file"); done < <(ci_files)
check .github/workflows ${files[@]+"${files[@]}"}
echo "ok: every job's first step is the outgoing-port guard, and no CI-run file holds a fixed port inside 20000-50999"
