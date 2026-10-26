#!/usr/bin/env bash
# Every check CI runs, in the same order. Run it before pushing.
set -euo pipefail
cd "$(dirname "$0")/.."

# Fail on any compiler warning without changing build fingerprints (Cargo 1.97+).
export CARGO_BUILD_WARNINGS=deny

# The same image CI uses, when ShellCheck isn't installed locally.
SHELLCHECK_IMAGE="koalaman/shellcheck:v0.11.0@sha256:61862eba1fcf09a484ebcc6feea46f1782532571a34ed51fedf90dd25f925a8d"

scripts/check-toolchain.sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
# The crash tests (tests/faults.rs) build only with the fault points compiled in.
cargo clippy -p colophon-server --features fault-injection --all-targets --locked -- -D warnings
cargo test --workspace --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
scripts/check-architecture.sh
scripts/check-architecture.sh colophon-sim
# The simulator: 2,000 seeded runs of editors, lost answers and crashes (docs/modules/10-sim.md).
cargo run --quiet --release --locked -p colophon-sim -- --seeds 2000

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

# The integration half of CI (D-018), whenever the local stack is up. QUICK=1 skips it.
if [[ "${QUICK:-0}" == "1" ]]; then
  echo "note: QUICK=1, skipping the integration checks that CI runs" >&2
elif docker compose -f deploy/compose.yaml ps --quiet 2>/dev/null | grep -q .; then
  scripts/stack.sh check
  # These stop and restart stack services, so one at a time.
  cargo test --workspace --locked -- --ignored --test-threads=1
  # F01 and F02: the server killed before and after a commit (needs the fault points).
  cargo test -p colophon-server --features fault-injection --test faults --locked -- --ignored --test-threads=1
  if command -v npm >/dev/null; then
    scripts/testbed.sh
    scripts/studio.sh
  else
    echo "note: npm is not installed here; CI runs the testbed suite" >&2
  fi
  if command -v k6 >/dev/null || [[ -x "$HOME/.local/bin/k6" ]]; then
    PATH="$PATH:$HOME/.local/bin" TOTAL=200000 P99_MS=50 KEEP=0 scripts/load.sh health throughput
    PATH="$PATH:$HOME/.local/bin" TOTAL=100000 P99_MS=50 KEEP=0 scripts/load.sh rich-text throughput
    PATH="$PATH:$HOME/.local/bin" TOTAL=20000 P99_MS=200 KEEP=0 scripts/load.sh session throughput
    PATH="$PATH:$HOME/.local/bin" TOTAL=20000 P99_MS=200 KEEP=0 scripts/load.sh cli throughput
    PATH="$PATH:$HOME/.local/bin" TOTAL=20000 P99_MS=200 KEEP=0 scripts/load.sh members throughput
    PATH="$PATH:$HOME/.local/bin" TOTAL=20000 P99_MS=200 KEEP=0 scripts/load.sh projects throughput
    PATH="$PATH:$HOME/.local/bin" TOTAL=20000 P99_MS=200 KEEP=0 scripts/load.sh changesets throughput
    PATH="$PATH:$HOME/.local/bin" TOTAL=50000 P99_MS=200 KEEP=0 scripts/load.sh delivery throughput
    PATH="$PATH:$HOME/.local/bin" TOTAL=10000 P99_MS=500 KEEP=0 scripts/load.sh entries throughput
    PATH="$PATH:$HOME/.local/bin" TOTAL=10000 P99_MS=500 KEEP=0 scripts/load.sh content-types throughput
    PATH="$PATH:$HOME/.local/bin" TOTAL=50000 P99_MS=50 KEEP=0 scripts/load.sh studio throughput
  else
    echo "note: k6 is not installed here (scripts/install-k6.sh); CI runs the load smoke test" >&2
  fi
else
  echo "note: the local stack is not running (scripts/stack.sh up); CI runs the integration checks" >&2
fi

echo "all checks passed"
