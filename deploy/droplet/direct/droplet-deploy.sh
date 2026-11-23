#!/bin/bash
# Direct deploy on the droplet, no GitHub CI: install a build made on this machine (server + CLI from
# /opt/colophon-build/target/release) plus a Studio tarball uploaded by scripts/deploy.sh, with
# upgrade.sh's safety steps: refuse a database migrated by a newer build, migrate while the old release
# serves, switch atomically, /readyz + smoke.sh, roll back automatically on any failure. Then clean up
# so the disk never fills: keep the current and the previous release, delete the uploaded tarball,
# wipe the build cache past 10 GB, trim the build log.
#   droplet-deploy.sh <commit> <studio-tarball>
set -euo pipefail
ROOT=/opt/colophon
CREDS=/etc/colophon/credentials
LOCAL=http://127.0.0.1:58000
BUILD=/opt/colophon-build
SMOKE=/opt/colophon-src/deploy/droplet/smoke.sh
commit=${1:?commit}; studio_tgz=${2:?studio tarball}
start=$SECONDS
log() { printf '%s  %s\n' "$(date -u +%H:%M:%S)" "$*"; }
die() { log "error: $*" >&2; exit 1; }
current_version() { [[ -L $ROOT/current ]] && basename "$(readlink "$ROOT/current")" || echo ""; }
ready() { local deadline=$((SECONDS + $1)); while ((SECONDS < deadline)); do
  curl -fsS --max-time 2 "$LOCAL/readyz" 2>/dev/null | grep -q '"status":"ready"' && return 0; sleep 0.5; done; return 1; }
link_to() { local r=$1
  ln -sfn "$r" "$ROOT/current.next" && mv -Tf "$ROOT/current.next" "$ROOT/current"
  ln -sfn "$ROOT/current/bin/colophon-server" /usr/local/bin/colophon-server
  ln -sfn "$ROOT/current/bin/colophon" /usr/local/bin/colophon
  mkdir -p /usr/local/share/colophon
  ln -sfn "$ROOT/current/studio" /usr/local/share/colophon/studio.next && mv -Tf /usr/local/share/colophon/studio.next /usr/local/share/colophon/studio; }
restart_and_check() { systemctl restart colophon; ready 60 || { log "not ready within 60 s"; return 1; }
  [[ -x $SMOKE ]] || return 0
  local out; out=$(mktemp); SMOKE_SCOPE=local SMOKE_EXPECT_VERSION="" "$SMOKE" >"$out" 2>&1 \
    || { log "smoke failed: $(grep FAIL "$out" | head -3 | tr -s ' ')"; return 1; }; rm -f "$out"; }

[[ -s $studio_tgz ]] || die "no Studio tarball at $studio_tgz"
bin=$BUILD/target/release
[[ -x $bin/colophon-server && -x $bin/colophon ]] || die "no build in $bin"
version=src-${commit:0:12}
release=$ROOT/releases/$version
mkdir -p "$ROOT/releases"
rm -rf "$release.partial"; mkdir -p "$release.partial/bin" "$release.partial/studio"
install -m 0755 "$bin/colophon-server" "$bin/colophon" "$release.partial/bin/"
tar -xzf "$studio_tgz" -C "$release.partial/studio"
[[ -f $release.partial/studio/index.html ]] || die "the Studio tarball has no index.html"
"$release.partial/bin/colophon-server" --version >/dev/null || die "the new server doesn't run here"
rm -rf "$release" && mv "$release.partial" "$release"
log "unpacked $version ($("$release/bin/colophon-server" --version 2>/dev/null | head -1))"

old=$(current_version)
if [[ -s $CREDS/migration_database_url ]]; then
  state=$("$release/bin/colophon" admin migrate-status --database-url-file "$CREDS/migration_database_url" --json 2>/dev/null | jq -r '.data.status // empty' || true)
  case "$state" in current | behind) ;; unknown) die "the database has migrations this build doesn't know: refusing";; *) die "can't read migration status: nothing switched";; esac
  log "migrating (old release keeps serving)"
  "$release/bin/colophon" admin migrate --database-url-file "$CREDS/migration_database_url" || die "migration failed: nothing switched; ${old:-none} still serves"
fi
[[ -n $old && $old != "$version" ]] && echo "$old" >"$ROOT/previous"
log "switching ${old:-nothing} -> $version"
link_to "$release"
if ! restart_and_check; then
  if [[ -n $old && $old != "$version" && -d $ROOT/releases/$old ]]; then
    log "ROLLING BACK to $old"; link_to "$ROOT/releases/$old"
    restart_and_check || die "the rollback to $old isn't ready either: journalctl -u colophon"
    die "$version failed its checks; rolled back to $old"
  fi
  die "$version failed its checks and there is nothing to roll back to"
fi
log "$version is live ($((SECONDS - start)) s)"

# Cleanup: keep current + previous; everything else goes.
prev=$(cat "$ROOT/previous" 2>/dev/null || true)
for r in "$ROOT"/releases/*; do n=$(basename "$r"); [[ $n == "$version" || $n == "$prev" ]] && continue; rm -rf "$r" && log "removed old release $n"; done
rm -f "$studio_tgz"
size=$(du -sk "$BUILD/target" 2>/dev/null | cut -f1 || echo 0)
if (( size > 10 * 1024 * 1024 )); then rm -rf "$BUILD/target" && log "build cache was $((size / 1048576)) GB: wiped (next build is cold)"; fi
[[ -f /var/log/colophon-build.log ]] && tail -n 2000 /var/log/colophon-build.log >/var/log/colophon-build.log.tmp && mv /var/log/colophon-build.log.tmp /var/log/colophon-build.log
log "disk: $(df -h / | awk 'NR==2{print $4" free of "$2}'), build cache $(du -sh "$BUILD/target" 2>/dev/null | cut -f1 || echo 0)"
