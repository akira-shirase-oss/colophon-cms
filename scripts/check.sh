#!/usr/bin/env bash
# Every check CI runs, in the same order. Run it before pushing.
set -euo pipefail
cd "$(dirname "$0")/.."

# Fail on any compiler warning without changing build fingerprints (Cargo 1.97+).
export CARGO_BUILD_WARNINGS=deny

scripts/check-toolchain.sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
scripts/check-architecture.sh

if command -v cargo-deny >/dev/null || [[ -x "$HOME/.cargo/bin/cargo-deny" ]]; then
  PATH="$PATH:$HOME/.cargo/bin" cargo deny check
else
  echo "note: cargo-deny is not installed here; CI runs it" >&2
fi

echo "all checks passed"
