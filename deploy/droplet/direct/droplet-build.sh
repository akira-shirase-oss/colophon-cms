#!/bin/bash
# Incremental release build of one commit on the droplet; CPU at low priority so the live server is unaffected.
set -euo pipefail
commit=${1:?commit}
export PATH=/root/.cargo/bin:$PATH CARGO_TARGET_DIR=/opt/colophon-build/target SQLX_OFFLINE=true CARGO_BUILD_JOBS=3
cd /opt/colophon-build/src
git fetch -q origin "$commit" 2>/dev/null || git fetch -q origin main
git checkout -q --detach "$commit"
start=$(date +%s)
nice -n 15 cargo build --release --locked -q -p colophon-server -p colophon-cli 2>&1 | tail -5
echo "built in $(( $(date +%s) - start )) s"
