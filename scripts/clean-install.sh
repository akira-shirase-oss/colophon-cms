#!/usr/bin/env bash
# The independent-install gate's mechanical half (D-132). END_GOAL's v0.1 list asks for one
# person other than the maintainer to install Colophon "from the README alone"; this script
# is the rehearsal of that claim: a `git clone` of this repository into a temporary
# directory, then the README's own commands, step by step, until /readyz answers. The gate
# itself stays human (docs/usability-protocol.md); this is the part a machine can check.
#
#   scripts/clean-install.sh
#
# Wherever the README's instructions do not work verbatim the script fails loudly and names
# the step that broke; that list is the friction record the gate is graded on. The dated
# result lands in perf/results/<stamp>-clean-install-<machine>.json like the other drills.
#
# Isolation (D-132): the clone's stack runs under a second compose project through
# COMPOSE_PROJECT_NAME on the compose file's own COLOPHON_*_PORT knobs, and the clone's
# deploy/local.env is patched to match. The development stack is never touched: same
# project name would adopt its volumes (a warm database is not a clean install), and same
# ports would collide with it. Fresh volumes give the rehearsal what a newcomer gets: an
# empty database and an unmigrated schema — sign-in is the built-in password card (D-146),
# so no identity provider is part of the rehearsal.
#
# Needs everything the README lists: git, Docker with Compose, the pinned Rust toolchain
# through rustup's proxies, Node 22.17+ for the Studio half, and curl/jq/openssl for the
# stack's own checks. Runs on macOS's bash 3.2.
set -euo pipefail
cd "$(dirname "$0")/.."

# machine_name: the perf/results convention ("apple-m2-pro-12c"), same recipe as
# scripts/backup-crypt.sh's; inlined so this script sources no encryption helpers.
machine_name() {
  local cpu cores
  if [[ "$(uname -s)" == "Darwin" ]]; then
    cpu=$(sysctl -n machdep.cpu.brand_string); cores=$(sysctl -n hw.ncpu)
  else
    cpu=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//'); cores=$(nproc)
  fi
  printf '%s-%sc' "$cpu" "$cores" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '-' | sed 's/-$//'
}

# --- Isolation settings (D-132): none of these exists on a newcomer's machine ------------

# A second compose project: the development stack's containers, volumes and database stay
# untouched, and the rehearsal's volumes start empty: the cold install the gate is about.
readonly PROJECT="colophon-clean-install"
export COMPOSE_PROJECT_NAME="$PROJECT"

# The compose file's own port knobs (deploy/compose.yaml), moved off the development
# stack's ports; pre-set values are honoured so a port-conflicted machine can move again.
export COLOPHON_PG_PORT="${COLOPHON_PG_PORT:-55532}"
export COLOPHON_VALKEY_PORT="${COLOPHON_VALKEY_PORT:-56479}"
export COLOPHON_S3_PORT="${COLOPHON_S3_PORT:-54000}"
export COLOPHON_S3_ADMIN_PORT="${COLOPHON_S3_ADMIN_PORT:-54003}"
# The server: any port works now that sign-in needs no registered callback (D-146).
readonly SERVER_PORT=58090
readonly SERVER_ORIGIN="http://127.0.0.1:$SERVER_PORT"
# colophon doctor and login default to :58000; COLOPHON_SERVER is the CLI's own knob for
# aiming them elsewhere, the same consequence of a moved port as the local.env patch.
export COLOPHON_SERVER="$SERVER_ORIGIN"

work=$(mktemp -d "${TMPDIR:-/tmp}/colophon-clean-install.XXXXXX")
CLONE="$work/repo"
mkdir -p "$work/logs"
SERVER_PID=""

cleanup() {
  if [[ -n "$SERVER_PID" ]]; then kill "$SERVER_PID" 2>/dev/null || true; fi
  if [[ -d "$CLONE/deploy" ]]; then
    (cd "$CLONE" && docker compose -f deploy/compose.yaml down --volumes --timeout 10 \
      >/dev/null 2>&1) || true
  fi
  rm -rf "$work"
}
trap cleanup EXIT

checks=0
failures=0
steps_json=""
friction_json=""
aborted=""

pass() { checks=$((checks + 1)); printf '  ok    %s\n' "$1"; }
fail() { checks=$((checks + 1)); failures=$((failures + 1)); printf '  FAIL  %s\n' "$1"; }

# expect <what> <wanted> <got>
expect() {
  if [[ "$3" == "$2" ]]; then pass "$1"; else fail "$1: expected '$2', got '$3'"; fi
}

# friction <step> <what a newcomer hits> <fixed|open>
friction() {
  friction_json+=$(jq -n --arg step "$1" --arg detail "$2" --arg state "$3" \
    '{step: $step, detail: $detail, state: $state}')
  friction_json+=","
  printf '  rub   %s: %s [%s]\n' "$1" "$2" "$3"
}

# record <name> <source> <readme> <command> <seconds> <exit code>
record() {
  steps_json+=$(jq -n --arg name "$1" --arg source "$2" --arg readme "$3" --arg cmd "$4" \
    --argjson seconds "$5" --argjson code "$6" \
    '{name: $name, source: $source, readme: $readme, command: $cmd,
      seconds: $seconds, exit_code: $code,
      status: (if $code == 0 then "ok" else "failed" end)}')
  steps_json+=","
}

# step <name> <what the README says> <command evaluated inside the clone>
# A step is the README tried verbatim; a non-zero exit is friction, printed with the log's
# tail so the breakage itself is the record. Returns the step's exit code.
step() {
  local name=$1 readme=$2 cmd=$3
  local log="$work/logs/$1.log" rc=0 t0=$SECONDS
  printf '==> %s\n    README: %s\n' "$name" "$readme"
  (cd "$CLONE" && eval "$cmd") >"$log" 2>&1 || rc=$?
  local secs=$((SECONDS - t0))
  if [[ $rc == 0 ]]; then
    printf '    done (%ss)\n' "$secs"
  else
    printf '    FAILED exit %s (%ss): last lines of the step output:\n' "$rc" "$secs"
    tail -n 25 "$log" | sed 's/^/    | /'
    friction "$name" "the README's instruction failed verbatim (exit $rc)" "open"
  fi
  record "$name" "readme" "$readme" "$cmd" "$secs" "$rc"
  return "$rc"
}

# start_server <extra env as NAME=value words>: the README's `cargo run --release -p
# colophon-server`, split so the rehearsal can stop it afterwards: the binary it would
# exec is run directly, against the same sourced deploy/local.env.
start_server() {
  (
    cd "$CLONE"
    set -a
    # shellcheck disable=SC1091 # the clone's deploy/local.env, patched to the moved ports
    . deploy/local.env
    set +a
    exec env "$@" target/release/colophon-server
  ) >>"$work/logs/server.log" 2>&1 &
  SERVER_PID=$!
}

# wait_live: /livez inside 60 s (the process is already built; startup is probes and pools).
wait_live() {
  local _
  for _ in $(seq 1 120); do
    if curl -fsS -o /dev/null "$SERVER_ORIGIN/livez" 2>/dev/null; then return 0; fi
    sleep 0.5
  done
  return 1
}

# --- Prerequisites, checked against what the README actually lists ----------------------

for tool in git docker cargo rustc node npm curl jq openssl; do
  command -v "$tool" >/dev/null 2>&1 || { echo "error: $tool is required" >&2; exit 1; }
done
docker compose version >/dev/null 2>&1 || { echo "error: docker compose is required" >&2; exit 1; }

# The README's own caveat: another cargo on PATH (Homebrew's) ignores rust-toolchain.toml's
# pin. Apply the fix it documents, rustup's proxies first, where they exist.
if [[ -d "$HOME/.cargo/bin" ]]; then export PATH="$HOME/.cargo/bin:$PATH"; fi
if command -v brew >/dev/null 2>&1; then
  rustup_prefix=$(brew --prefix rustup 2>/dev/null || true)
  if [[ -n "$rustup_prefix" && -d "$rustup_prefix/bin" ]]; then
    export PATH="$rustup_prefix/bin:$PATH"
  fi
fi

# A port the rehearsal wants must be free; the message names the knob that moves it.
port_taken() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }
for port in "$COLOPHON_PG_PORT" "$COLOPHON_VALKEY_PORT" "$COLOPHON_S3_PORT" \
            "$COLOPHON_S3_ADMIN_PORT" "$SERVER_PORT"; do
  if port_taken "$port"; then
    echo "error: 127.0.0.1:$port is already listening; move the rehearsal's port with the" >&2
    echo "       COLOPHON_*_PORT variables (deploy/compose.yaml) and rerun." >&2
    exit 1
  fi
done

echo "clean-install rehearsal: clone $PWD, README commands only, stack under project $PROJECT"
echo "isolation ports: pg=$COLOPHON_PG_PORT valkey=$COLOPHON_VALKEY_PORT" \
     "s3=$COLOPHON_S3_PORT/$COLOPHON_S3_ADMIN_PORT server=$SERVER_PORT"

# --- The clone: what a newcomer gets ----------------------------------------------------

t0=$SECONDS
git clone --quiet "$PWD" "$CLONE"
# Plus the working tree's uncommitted state, so the rehearsal tests the README as it
# stands in this checkout, not as the last commit left it. Secrets stay out: the overlay
# is git's own list of tracked-modified and untracked-but-not-ignored files.
overlaid=0
while IFS= read -r -d '' f; do
  if [[ -f "$f" ]]; then
    mkdir -p "$CLONE/$(dirname "$f")"
    cp "$f" "$CLONE/$f"
    overlaid=$((overlaid + 1))
  fi
done < <(git ls-files -mz -mo --exclude-standard)
record "clone" "isolation" \
  "git clone <the repository>; the overlay adds this checkout's uncommitted changes" \
  "git clone $PWD $CLONE (+ $overlaid working-tree files)" "$((SECONDS - t0))" 0
echo "==> clone: $PWD (+ $overlaid uncommitted files overlaid)"

# Move the clone's ports to the rehearsal's set: the compose file's COLOPHON_*_PORT knobs
# for the stack, and deploy/local.env for the server, the same edit the README's deploy
# notes tell a port-conflicted installer to make.
t0=$SECONDS
sed -i.bak \
  -e "s|:55432|:$COLOPHON_PG_PORT|g" \
  -e "s|:56379|:$COLOPHON_VALKEY_PORT|g" \
  -e "s|:53900|:$COLOPHON_S3_PORT|g" \
  -e "s|:58000|:$SERVER_PORT|g" \
  "$CLONE/deploy/local.env" && rm "$CLONE/deploy/local.env.bak"
# Nothing from an earlier rehearsal may still be standing.
(cd "$CLONE" && docker compose -f deploy/compose.yaml down --volumes >/dev/null 2>&1) || true
record "ports" "isolation" \
  "ports moved through deploy/compose.yaml's COLOPHON_*_PORT variables" \
  "COLOPHON_{PG,VALKEY,S3,S3_ADMIN}_PORT + deploy/local.env patched to match" \
  "$((SECONDS - t0))" 0
echo "==> ports: the clone's compose project runs on the rehearsal's set"

# The audit: gaps this rehearsal found in the README, each re-checked against the clone's
# copy so the record reopens on its own if the text regresses.
# audit <step> <file> <pattern> <what a newcomer hits>
audit() {
  if grep -q "$3" "$CLONE/$2"; then
    friction "$1" "$4" "fixed"
  else
    friction "$1" "$4" "open"
  fi
}
audit "get-the-source" "README.md" "git clone" \
  "the README never says how to get the repository"
audit "stack-tools" "README.md" "jq" \
  "scripts/stack.sh needs curl, jq and openssl; the README's requirements name none of them"
audit "studio-deps" "README.md" "npm ci" \
  "a fresh clone's Studio build needs npm ci first; the README does not say so"
audit "moved-ports" "deploy/README.md" "local.env" \
  "the port-move note does not say deploy/local.env carries the same addresses"

# The pinned toolchain is a listed requirement; check it in the clone the way
# scripts/migrate.sh will.
t0=$SECONDS
rc=0
(cd "$CLONE" && scripts/check-toolchain.sh) >"$work/logs/toolchain.log" 2>&1 || rc=$?
if [[ $rc != 0 ]]; then
  echo "==> toolchain: FAILED: $(tail -n 3 "$work/logs/toolchain.log")"
  friction "toolchain" "the pinned toolchain is not what PATH provides" "open"
fi
record "toolchain" "isolation" \
  "rustup installs the pinned toolchain from rust-toolchain.toml (README: Requirements)" \
  "scripts/check-toolchain.sh" "$((SECONDS - t0))" "$rc"
if [[ $rc != 0 ]]; then aborted="toolchain"; fi

# --- The README, verbatim ---------------------------------------------------------------

if [[ -z "$aborted" ]]; then
  step "stack-up" "scripts/stack.sh up" "scripts/stack.sh up" || aborted="stack-up"
fi

if [[ -z "$aborted" ]]; then
  step "migrate" "scripts/migrate.sh" "scripts/migrate.sh" || aborted="migrate"
fi

if [[ -z "$aborted" ]]; then
  # `set -a; . deploy/local.env; set +a` then `cargo run --release -p colophon-server`.
  # Built here so the start below is instant; the run half is start_server (same binary).
  step "server-build" \
    "set -a; . deploy/local.env; set +a; cargo run --release -p colophon-server" \
    "cargo build --quiet --release --locked -p colophon-server" || aborted="server-build"
fi

if [[ -z "$aborted" ]]; then
  start_server
  if wait_live; then
    pass "the server answers /livez (the README's cargo run line)"
  else
    fail "the server never answered /livez (log: $work/logs/server.log)"
    friction "server-start" "the server did not come up on the README's env" "open"
    aborted="server-start"
  fi
fi

if [[ -z "$aborted" ]]; then
  readyz=$(curl -fsS "$SERVER_ORIGIN/readyz" 2>/dev/null || true)
  readyz_status=$(printf '%s' "$readyz" | jq -r '.status // empty' 2>/dev/null)
  if [[ "$readyz_status" == ready || "$readyz_status" == degraded ]]; then
    pass "/readyz answers 200 ($readyz_status): the gate's endpoint"
  else
    fail "/readyz answered '$readyz_status' ($readyz)"
    aborted="readyz"
  fi

  version_body=$(curl -fsS "$SERVER_ORIGIN/version" 2>/dev/null || true)
  served_commit=$(printf '%s' "$version_body" | jq -r '.commit // empty' 2>/dev/null)
  expect "/version reports the checkout's commit" "$(git rev-parse HEAD)" "$served_commit"
fi

if [[ -z "$aborted" ]]; then
  step "doctor" "cargo run --release -p colophon-cli -- doctor" \
    "set -a; . deploy/local.env; set +a; cargo run --quiet --release -p colophon-cli -- doctor" \
    || aborted="doctor"
fi

if [[ -z "$aborted" ]]; then
  # The README's sign-in works without any provider (D-146): migrate.sh provisioned
  # dev-owner@colophon.test, and /auth/signin answers 204 with the session cookie.
  signed_in=$(curl -sS -o /dev/null -w '%{http_code}' \
    -H 'content-type: application/json' \
    -d '{"email":"dev-owner@colophon.test","password":"colophon-dev"}' \
    "$SERVER_ORIGIN/auth/signin" 2>/dev/null || true)
  expect "the password card signs dev-owner in" "204" "$signed_in"
fi

# The README's Studio block: npm ci (recorded friction before this line existed), the
# build, then the server restarted with COLOPHON_STUDIO_DIR pointing at it.
if [[ -z "$aborted" ]]; then
  step "npm-ci" "npm ci" "npm ci" || aborted="npm-ci"
fi
if [[ -z "$aborted" ]]; then
  step "studio-build" "npm run build -w @colophon/studio" \
    "npm run build -w @colophon/studio" || aborted="studio-build"
fi

if [[ -z "$aborted" ]]; then
  kill "$SERVER_PID" 2>/dev/null || true
  wait "$SERVER_PID" 2>/dev/null || true
  SERVER_PID=""
  start_server "COLOPHON_STUDIO_DIR=$CLONE/apps/studio/build"
  if wait_live; then
    studio=$(curl -fsS -o /dev/null -w '%{http_code}' "$SERVER_ORIGIN/studio/" 2>/dev/null || true)
    expect "the Studio answers on /studio/ (COLOPHON_STUDIO_DIR set, as the README shows)" \
      "200" "$studio"
  else
    fail "the server with COLOPHON_STUDIO_DIR never answered /livez"
    aborted="studio-serve"
  fi
fi

# --- The record --------------------------------------------------------------------------

if [[ -n "$aborted" ]]; then
  friction "$aborted" "the README path stopped here; every later step was skipped" "open"
fi

stamp=$(date -u +%Y-%m-%dT%H%M%SZ)
machine=$(machine_name)
file="perf/results/$stamp-clean-install-$machine.json"
jq -n --arg stamp "$stamp" --arg commit "$(git rev-parse HEAD)" \
  --argjson dirty "$overlaid" --arg machine "$machine" \
  --arg project "$PROJECT" \
  --arg gate "one person other than the maintainer installs Colophon from the README alone (END_GOAL v0.1)" \
  --arg what "the mechanical rehearsal: the README's own commands in a fresh clone; the gate itself stays human" \
  --argjson ports "{\"pg\": $COLOPHON_PG_PORT, \"valkey\": $COLOPHON_VALKEY_PORT, \"s3\": $COLOPHON_S3_PORT, \"s3_admin\": $COLOPHON_S3_ADMIN_PORT, \"server\": $SERVER_PORT}" \
  --arg aborted "$aborted" \
  --argjson steps "[${steps_json%,}]" \
  --argjson friction "[${friction_json%,}]" \
  --argjson checks "$checks" --argjson failures "$failures" \
  '{
    scenario: "clean-install",
    date: $stamp,
    commit: $commit,
    working_tree_files_overlaid: $dirty,
    machine: $machine,
    gate: $gate,
    what_this_is: $what,
    isolation: {
      compose_project: $project,
      note: "fresh volumes and moved ports; the development stack is never touched",
      ports: $ports
    },
    steps: $steps,
    friction: $friction,
    aborted_at: (if $aborted == "" then null else $aborted end),
    not_rehearsed: [
      "scripts/check.sh (the contributor suite, not the install path)",
      "colophon login (needs a human at a browser to approve the device)",
      "a machine with no Rust or Node caches at all"
    ],
    checks: $checks,
    failures: $failures,
    verdict: (if $failures == 0 and $aborted == ""
      then "readme-path-verified" else "failed" end)
  }' >"$file"
echo "result: $file"
if (( failures != 0 )) || [[ -n "$aborted" ]]; then
  echo "FAIL: the README path did not hold (aborted at '${aborted:-end}', $failures of $checks checks)" >&2
  exit 1
fi
echo "PASS: the README alone reached /readyz and the Studio ($checks checks)"
