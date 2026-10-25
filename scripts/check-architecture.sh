#!/usr/bin/env bash
# The domain crate holds Colophon's rules and must stay free of I/O, so the simulator can
# drive it with a fake clock and a fake network (docs/ARCHITECTURE.md, "Crate dependency
# direction"). This fails if a banned crate appears anywhere in its dependency tree,
# including transitively.
#
# Usage: scripts/check-architecture.sh [crate]       (default: colophon-domain)
# ARCH_BANNED="a b" overrides the banned list; used to prove the check can fail.
set -euo pipefail
cd "$(dirname "$0")/.."

crate="${1:-colophon-domain}"
banned="${ARCH_BANNED:-tokio axum hyper tower tower-http sqlx reqwest redis fred aws-sdk-s3 object_store}"

tree=$(cargo tree --locked -p "$crate" -e normal,build --prefix none --format '{p}' | awk '{print $1}' | sort -u)

found=()
for name in $banned; do
  if grep -qx -- "$name" <<<"$tree"; then
    found+=("$name")
  fi
done

if (( ${#found[@]} )); then
  echo "error: $crate must not depend on: ${found[*]}" >&2
  exit 1
fi

echo "architecture ok: $crate has none of [$banned] in its dependency tree ($(wc -l <<<"$tree" | tr -d ' ') crates)"
