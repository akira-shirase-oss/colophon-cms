#!/usr/bin/env bash
# Every check CI runs, in the same order. Run it before pushing.
set -euo pipefail
cd "$(dirname "$0")/.."

# `scripts/check.sh --integration-plan` prints the integration half's steps for this stack and
# exits, running nothing (scripts/test-stack-guard.sh reads it).
plan_only=0
[[ "${1:-}" == --integration-plan ]] && plan_only=1

# Fail on any compiler warning without changing build fingerprints (Cargo 1.97+).
export CARGO_BUILD_WARNINGS=deny

# The same image CI uses, when ShellCheck isn't installed locally.
SHELLCHECK_IMAGE="koalaman/shellcheck:v0.11.0@sha256:61862eba1fcf09a484ebcc6feea46f1782532571a34ed51fedf90dd25f925a8d"

if (( plan_only == 0 )); then
  scripts/check-toolchain.sh
  cargo fmt --all --check
  cargo clippy --workspace --all-targets --locked -- -D warnings
  # The crash tests (tests/faults.rs) build only with the fault points compiled in.
  cargo clippy -p colophon-server --features fault-injection --all-targets --locked -- -D warnings
  cargo test --workspace --locked
  # The micro-benchmarks, each run once and unoptimized: they still build and their inputs still
  # hold. Timing them is scripts/bench.sh's job (D-074), not this script's.
  cargo test -p colophon-bench --benches --locked
  RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
  scripts/check-architecture.sh
  scripts/check-architecture.sh colophon-sim
  scripts/check-typography.sh
  # The simulator: 2,000 seeded runs of editors, lost answers and crashes (docs/modules/10-sim.md).
  cargo run --quiet --release --locked -p colophon-sim -- --seeds 2000
  cargo run --quiet --release --locked -p colophon-sim -- --scenario publishing --seeds 1000

  if command -v cargo-deny >/dev/null || [[ -x "$HOME/.cargo/bin/cargo-deny" ]]; then
    PATH="$PATH:$HOME/.cargo/bin" cargo deny check
  else
    echo "note: cargo-deny is not installed here; CI runs it" >&2
  fi

  if command -v shellcheck >/dev/null; then
    shellcheck scripts/*.sh
  elif docker info >/dev/null 2>&1; then
    docker run --rm -v "$PWD:/mnt:ro" -w /mnt "$SHELLCHECK_IMAGE" scripts/*.sh
  else
    echo "note: neither ShellCheck nor Docker is available here; CI runs ShellCheck" >&2
  fi

  # Frontend: types, unit tests and the Studio in Chromium. No stack needed.
  if command -v npm >/dev/null; then
    scripts/frontend.sh
  else
    echo "note: npm is not installed here; CI runs the frontend checks" >&2
  fi

fi

# The integration half of CI (D-018), whenever the local stack is up. QUICK=1 skips it.
#
# On a stack this run didn't make (scripts/stack-guard.sh, D-187), the steps that stop, pause or
# reconfigure its services are left to CI, by name: `scripts/stack.sh check` (role DDL, a forced
# WAL switch), the four ignored tests that stop Postgres or Valkey, and the two smokes that pause
# them. Everything else runs, in a database of the run's own (COLOPHON_TEST_DATABASE, required
# there); colophon_check is never dropped on a shared stack.
# shellcheck source=scripts/stack-guard.sh
. scripts/stack-guard.sh
STOPS_SERVICES=(
  admission::without_valkey_each_server_still_limits_on_its_own
  quotas::without_valkey_a_token_is_still_limited_never_unlimited
  cache::answers_survive_valkey_going_away_and_caching_resumes_when_it_is_back
  lifecycle::readiness_follows_real_outages_and_recoveries
)
PAUSES_SERVICES=(cache-down postgres-trouble)
# The load smokes, as `VAR=VALUE... endpoint`, all at the throughput scenario.
SMOKES=(
  "TOTAL=200000 P99_MS=50 health"
  "TOTAL=100000 P99_MS=50 rich-text"
  "TOTAL=20000 P99_MS=200 session"
  "TOTAL=20000 P99_MS=200 cli"
  "TOTAL=20000 P99_MS=200 members"
  "TOTAL=20000 P99_MS=200 roles"
  "TOTAL=20000 P99_MS=200 tokens"
  "TOTAL=20000 P99_MS=200 quotas"
  # L08's smoke: its arms are time-boxed, so DURATION (per phase) is the knob, not TOTAL.
  "DURATION=8s P99_MS=500 noisy-agent"
  "TOTAL=20000 P99_MS=200 schedules"
  "TOTAL=20000 P99_MS=200 projects"
  "TOTAL=20000 P99_MS=200 changesets"
  "TOTAL=10000 P99_MS=200 stages"
  "TOTAL=10000 P99_MS=500 migrations"
  "TOTAL=20000 P99_MS=200 collaboration"
  "TOTAL=10000 P99_MS=500 live"
  "TOTAL=50000 P99_MS=200 delivery"
  "TOTAL=10000 P99_MS=250 preview"
  # USERS bounds the audience the smoke signs up, not the traffic share it drives.
  "TOTAL=10000 USERS=64 P99_MS=700 audience"
  "TOTAL=20000 P99_MS=500 publishing"
  # The dependency-failure evidence runs (L11-L13, D-127): each pauses or kills a real
  # stack dependency mid-run, so they run here one at a time like everywhere else.
  "DURATION=10s P99_MS=500 cache-down"
  "DURATION=10s P99_MS=500 postgres-trouble"
  "DURATION=10s P99_MS=500 purge-trouble"
  # The load-matrix evidence runs (D-128): their arms are time-boxed or job-sized, so
  # DURATION, SPIKE_SECONDS, PUBLISH_SECONDS, CORPUS and TOTAL_RECORDS — not TOTAL —
  # shape their smokes. import-scale and media-load also run reduced corpora.
  "DURATION=10s P99_MS=700 smoke"
  "TOTAL=50000 P99_MS=350 warm-baseline"
  "TOTAL=5000 CORPUS=512 P99_MS=750 cold-cache"
  "DURATION=15s P99_MS=250 stampede"
  "DURATION=15s RATE=50 PUBLISH_SECONDS=5 P99_MS=700 mixed"
  "DURATION=8s RATE=300 SPIKE_SECONDS=10 P99_MS=250 spike"
  "TOTAL_RECORDS=1024 P99_MS=15000 import-scale"
  "DURATION=10s RATE=300 PUBLISH_SECONDS=5 P99_MS=400 cdn-warm"
  "DURATION=10s MEDIA_VUS=2 P99_MS=700 media-load"
  "TOTAL=10000 P99_MS=500 entries"
  "TOTAL=10000 P99_MS=500 content-types"
  "TOTAL=10000 P99_MS=500 modelling"
  "TOTAL=10000 P99_MS=500 site-starter"
  "TOTAL=10000 P99_MS=500 assets"
  "TOTAL=10000 IMAGES=20 P99_MS=700 rich-text-media"
  "TOTAL=10000 P99_MS=500 bulk"
  "TOTAL=50000 P99_MS=50 studio"
)

# The integration half's steps for this stack, one shell command a line.
integration_plan() {
  local own=1 skips="" smoke endpoint
  stack_is_ours || own=0
  if (( own == 0 )); then
    if [[ ! "${COLOPHON_TEST_DATABASE:-}" =~ ^[a-z][a-z0-9_]{0,62}$ || "$COLOPHON_TEST_DATABASE" == colophon ]]; then
      echo "error: this stack isn't this run's own (D-187): set COLOPHON_TEST_DATABASE to a database of your own (not colophon), or COLOPHON_STACK_OWNER=1 if the stack is yours" >&2
      return 2
    fi
    echo "# CI only on a shared stack (D-187): scripts/stack.sh check; tests ${STOPS_SERVICES[*]}; smokes ${PAUSES_SERVICES[*]}"
    for test in "${STOPS_SERVICES[@]}"; do skips+=" --skip $test"; done
  else
    echo "scripts/stack.sh check"
  fi
  # A fresh database for everything below, as CI has. Every run adds projects and revisions, and a
  # database that keeps them grows until saves slow down (5 GB on 2026-10-27: entries p99 52 ms
  # fresh, 184 ms there) and pickers that list 1,000 projects lose the ones a test just made.
  # COLOPHON_TEST_DATABASE names another database instead, created if it is missing.
  if [[ -z "${COLOPHON_TEST_DATABASE:-}" ]]; then
    echo "export COLOPHON_TEST_DATABASE=colophon_check"
    echo "psql_admin 'DROP DATABASE IF EXISTS colophon_check WITH (FORCE)' >/dev/null"
  elif [[ ! "$COLOPHON_TEST_DATABASE" =~ ^[a-z][a-z0-9_]{0,62}$ ]]; then
    echo "error: COLOPHON_TEST_DATABASE must be a plain database name" >&2
    return 2
  fi
  echo 'create_test_database'
  echo 'scripts/migrate.sh'
  # The database TLS rule (D-156) against a TLS-only postgres of its own with a private CA:
  # verify-full connects, a wrong name or an unknown issuer fails, require to another machine
  # is refused before connecting. Its container is its own; the stack is untouched.
  echo 'scripts/db-tls-check.sh'
  # These stop and restart stack services, so one at a time.
  echo "cargo test --workspace --locked -- --ignored --test-threads=1$skips"
  # The P5 evaluation suite (D-117): adversarial cases against the same stack, gated on
  # UNAUTHORIZED: 0 across COLOPHON_EVAL_TRIALS repetitions of every case.
  echo 'COLOPHON_EVAL_TRIALS=3 cargo test -p colophon-server --test evals --locked -- --ignored --test-threads=1'
  # F01, F02 and F03: the server killed before and after a commit, the worker after announcing (needs the fault points).
  echo 'cargo test -p colophon-server --features fault-injection --test faults --locked -- --ignored --test-threads=1'
  echo 'scripts/testbed.sh'
  echo 'scripts/studio.sh'
  for smoke in "${SMOKES[@]}"; do
    endpoint=${smoke##* }
    if (( own == 0 )) && [[ " ${PAUSES_SERVICES[*]} " == *" $endpoint "* ]]; then continue; fi
    echo "PATH=\"\$PATH:\$HOME/.local/bin\" ${smoke% *} KEEP=0 scripts/load.sh $endpoint throughput"
  done
}

psql_admin() {
  docker compose -f deploy/compose.yaml exec -T postgres psql -U colophon -d postgres -XAtqc "$1"
}
create_test_database() {
  if [[ -z "$(psql_admin "SELECT 1 FROM pg_database WHERE datname = '$COLOPHON_TEST_DATABASE'")" ]]; then
    psql_admin "CREATE DATABASE $COLOPHON_TEST_DATABASE" >/dev/null
  fi
}

if (( plan_only == 1 )); then
  integration_plan
  exit
elif [[ "${QUICK:-0}" == "1" ]]; then
  echo "note: QUICK=1, skipping the integration checks that CI runs" >&2
elif docker compose -f deploy/compose.yaml ps --quiet 2>/dev/null | grep -q .; then
  plan=$(integration_plan)
  # The plan comes in on fd 3: steps such as `docker compose exec -T` read standard input.
  while IFS= read -r step <&3; do
    if [[ "$step" == \#* ]]; then echo "$step" >&2; continue; fi
    if [[ "$step" == *scripts/load.sh* ]] && ! { command -v k6 >/dev/null || [[ -x "$HOME/.local/bin/k6" ]]; }; then
      echo "note: k6 is not installed here (scripts/install-k6.sh); CI runs the load smoke test" >&2
      continue
    fi
    if [[ "$step" == scripts/testbed.sh || "$step" == scripts/studio.sh ]] && ! command -v npm >/dev/null; then
      echo "note: npm is not installed here; CI runs the testbed suite" >&2
      continue
    fi
    eval "$step"
  done 3<<<"$plan"
else
  echo "note: the local stack is not running (scripts/stack.sh up); CI runs the integration checks" >&2
fi

echo "all checks passed"
