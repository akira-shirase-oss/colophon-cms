#!/bin/bash
# One command, no GitHub CI: build origin/main on the droplet (incremental), build the Studio here at the same
# commit, upload it, install with droplet-deploy.sh (migrate, switch, smoke, auto-rollback, cleanup), then check
# through Cloudflare. Prints the total time.   deploy.sh [git-ref, default origin/main]
set -euo pipefail
REPO=$(cd "$(dirname "$0")/../../.." && pwd)
HOST=${COLOPHON_DEPLOY_HOST:?set COLOPHON_DEPLOY_HOST to the droplet ssh alias}
URL=${COLOPHON_PUBLIC_URL:?set COLOPHON_PUBLIC_URL, e.g. https://colophon.example.com}
KIT=$(cd "$(dirname "$0")" && pwd)
ref=${1:-origin/main}
t0=$SECONDS
step() { printf '%3ss  %s\n' "$((SECONDS - t0))" "$*"; }

git -C "$REPO" fetch -q origin
commit=$(git -C "$REPO" rev-parse "$ref")
step "deploying ${commit:0:12}"
scp -q "$KIT/droplet-deploy.sh" "$KIT/droplet-build.sh" "$HOST:/root/" && ssh "$HOST" 'chmod 700 /root/droplet-deploy.sh /root/droplet-build.sh'

# Server build on the droplet (background) while the Studio builds here.
# Detached on the droplet, so a dropped SSH connection can't stall the deploy; polled below.
ssh "$HOST" "rm -f /root/build.status; (nohup sh -c '/root/droplet-build.sh $commit > /root/build.out 2>&1; echo \$? > /root/build.status' >/dev/null 2>&1 &)"
wt=$(mktemp -d "${TMPDIR:-/tmp}/colophon-studio.XXXX"); rmdir "$wt"
git -C "$REPO" worktree add -q --detach "$wt" "$commit"
(cd "$wt" && npm ci --no-audit --no-fund --loglevel=error >/dev/null && npm run build -w @colophon/studio >/dev/null) || { step "Studio build failed"; exit 1; }
COPYFILE_DISABLE=1 tar --no-xattrs -czf "$KIT/studio.tgz" -C "$wt/apps/studio/build" .
git -C "$REPO" worktree remove --force "$wt"
step "Studio built"
scp -q "$KIT/studio.tgz" "$HOST:/root/studio-$commit.tgz"; rm -f "$KIT/studio.tgz"
until st=$(ssh -o ConnectTimeout=15 "$HOST" 'cat /root/build.status 2>/dev/null'); [ -n "$st" ]; do sleep 10; done
[ "$st" = 0 ] || { step "server build failed:"; ssh "$HOST" 'tail -20 /root/build.out'; exit 1; }
step "server built on the droplet ($(ssh "$HOST" "grep -o 'built in [0-9]* s' /root/build.out | tail -1"))"

ssh "$HOST" "rm -f /root/deploy.status; (nohup sh -c '/root/droplet-deploy.sh $commit /root/studio-$commit.tgz > /var/log/colophon-deploy.log 2>&1; echo \$? > /root/deploy.status' >/dev/null 2>&1 &)"
until st=$(ssh -o ConnectTimeout=15 "$HOST" 'cat /root/deploy.status 2>/dev/null'); [ -n "$st" ]; do sleep 5; done
ssh "$HOST" 'tail -6 /var/log/colophon-deploy.log'
[ "$st" = 0 ] || { step "deploy failed (it rolled back if it had switched)"; exit 1; }

for p in /version /readyz /studio/ /studio/signup; do
  code=$(curl -s -o /dev/null -w '%{http_code}' -m 10 "$URL$p"); step "$p $code"; [[ $code == 200 ]] || exit 1; done
curl -s -m 10 "$URL/version"; echo
step "done"
