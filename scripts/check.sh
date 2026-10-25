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
cargo test --workspace --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
scripts/check-architecture.sh

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

if docker compose -f deploy/compose.yaml ps --quiet 2>/dev/null | grep -q .; then
  scripts/stack.sh check
else
  echo "note: the local stack is not running (scripts/stack.sh up); CI starts and checks it" >&2
fi

echo "all checks passed"
