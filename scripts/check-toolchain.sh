#!/usr/bin/env bash
# Fails when the Rust compiler that would build this repository is not the one pinned in
# rust-toolchain.toml. Only rustup's proxies honour that file; on a machine where another
# `cargo` (for example Homebrew's) comes first on PATH, the pin is silently ignored.
set -euo pipefail
cd "$(dirname "$0")/.."

want=$(sed -n 's/^channel = "\(.*\)"$/\1/p' rust-toolchain.toml)
have_rustc=$(rustc --version | awk '{print $2}')
have_cargo=$(cargo --version | awk '{print $2}')

if [[ "$have_rustc" != "$want" || "$have_cargo" != "$want" ]]; then
  {
    echo "error: rust-toolchain.toml pins $want, but this shell runs rustc $have_rustc and cargo $have_cargo."
    echo "       $(command -v cargo) is not honouring the pin. Put rustup's proxies first on PATH:"
    echo "         export PATH=\"\$(brew --prefix rustup)/bin:\$PATH\"   # rustup installed with Homebrew"
    echo "         export PATH=\"\$HOME/.cargo/bin:\$PATH\"              # rustup installed from rustup.rs"
  } >&2
  exit 1
fi

echo "toolchain ok: rustc $have_rustc, cargo $have_cargo (pinned $want)"
