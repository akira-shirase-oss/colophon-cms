#!/usr/bin/env bash
# Install, upgrade or roll back Colophon on the S0 droplet (deploy/droplet/DEPLOY.md). Targets:
# an upgrade in under 2 minutes with under ~5 s unavailable, a rollback in under 1 minute.
#
#   sudo upgrade.sh <version>       install that release and switch to it (install.sh calls this)
#   sudo upgrade.sh --rollback      switch back to the release before the current one
#   sudo upgrade.sh --status        which releases are here and which one is current
#   sudo upgrade.sh --fetch <version>   verify and unpack only (install.sh, before the service exists)
#
# Layout: each release unpacks to /opt/colophon/releases/<version>/{bin,studio}; the `current`
# symlink names the live one, and /usr/local/bin/colophon-server, /usr/local/bin/colophon and
# /usr/local/share/colophon/studio point through it, so the systemd unit never changes. A switch
# is: verify the archives against SHA256SUMS, unpack, run the migrations with the NEW binary while
# the old one keeps serving, flip `current` atomically, restart (the server drains gracefully;
# Caddy holds and retries requests meanwhile, deploy/droplet/Caddyfile), poll /readyz, run
# smoke.sh. Any failure after the flip flips back and restarts the previous release, automatically.
#
# Migrations are never rolled back: Colophon's rule is that each release's migrations are
# forward-compatible with the previous release's binary, so a rollback swaps binaries only. A
# migration that fails leaves everything as it was (nothing has switched yet). D-202 (migration
# lock_timeout and retry) isn't in yet: a migration waiting on a lock waits.
#
# COLOPHON_RELEASE_BASE overrides where releases come from (default: GitHub releases; a directory
# path or file:// URL for a rehearsal). UPGRADE_NO_VERSION_CHECK=1, for rehearsals only, skips the
# check that /version reports the version just installed. The last 3 releases are kept.
set -euo pipefail
ROOT=/opt/colophon
BASE="${COLOPHON_RELEASE_BASE:-https://github.com/akira-shirase-oss/colophon-cms/releases/download}"
CREDS=/etc/colophon/credentials
LOCAL=http://127.0.0.1:58000
KEEP=3
here=$(cd "$(dirname "$0")" && pwd)

log() { printf '%s  %s\n' "$(date -u +%H:%M:%S)" "$*"; }
die() { log "error: $*" >&2; exit 1; }
[[ $EUID -eq 0 ]] || die "run as root"

arch() {
  case "$(uname -m)" in x86_64) echo x86_64 ;; aarch64 | arm64) echo aarch64 ;; *) die "unsupported machine $(uname -m)" ;; esac
}

# fetch <file> <dest-dir>: from the release base, by version.
fetch() {
  local file=$1 dest=$2 version=$3
  case "$BASE" in
    http://* | https://*) curl -fsSL --retry 3 --max-time 120 -o "$dest/$file" "$BASE/v$version/$file" ;;
    file://*) cp "${BASE#file://}/v$version/$file" "$dest/$file" ;;
    *) cp "$BASE/v$version/$file" "$dest/$file" ;;
  esac
}

current_version() { [[ -L $ROOT/current ]] && basename "$(readlink "$ROOT/current")" || echo ""; }

ready() { # waits up to $1 seconds for /readyz to say ready
  local deadline=$((SECONDS + $1))
  while ((SECONDS < deadline)); do
    if curl -fsS --max-time 2 "$LOCAL/readyz" 2>/dev/null | grep -q '"status":"ready"'; then return 0; fi
    sleep 0.5
  done
  return 1
}

link_to() { # the release the fixed paths point through
  local release=$1
  ln -sfn "$release" "$ROOT/current.next" && mv -Tf "$ROOT/current.next" "$ROOT/current"
  ln -sfn "$ROOT/current/bin/colophon-server" /usr/local/bin/colophon-server
  ln -sfn "$ROOT/current/bin/colophon" /usr/local/bin/colophon
  mkdir -p /usr/local/share/colophon
  ln -sfn "$ROOT/current/studio" /usr/local/share/colophon/studio.next && mv -Tf /usr/local/share/colophon/studio.next /usr/local/share/colophon/studio
}

restart_and_check() { # restart onto `current` (expected version $1); ready and smoke-clean, or non-zero
  systemctl restart colophon
  ready 60 || { log "not ready within 60 s"; return 1; }
  if [[ -x "$here/smoke.sh" && "${UPGRADE_SKIP_SMOKE:-0}" != 1 ]]; then
    local expect="${1:-}"
    # A rehearsal installs one build under two names, so it can't expect /version to differ.
    [[ "${UPGRADE_NO_VERSION_CHECK:-0}" == 1 ]] && expect=""
    SMOKE_SCOPE=local SMOKE_EXPECT_VERSION="$expect" "$here/smoke.sh" >/tmp/colophon-upgrade-smoke.log 2>&1 \
      || { log "smoke failed: $(grep FAIL /tmp/colophon-upgrade-smoke.log | head -3 | tr -s ' ')"; return 1; }
  fi
}

prune() { # keep the current release, the previous one and up to $KEEP in all, newest first
  local cur prev
  cur=$(current_version); prev=$(cat "$ROOT/previous" 2>/dev/null || true)
  # shellcheck disable=SC2012  # release directories are version names we made
  ls -1t "$ROOT/releases" | tail -n +$((KEEP + 1)) | while read -r old; do
    [[ "$old" == "$cur" || "$old" == "$prev" ]] && continue
    rm -rf "${ROOT:?}/releases/$old" && log "pruned release $old"
  done
}

install_release() {
  local version=$1 mode=${2:-switch} arch tmp release start=$SECONDS
  [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || die "'$version' isn't a version (like 0.1.0-alpha.1)"
  arch=$(arch)
  release=$ROOT/releases/$version
  local server=colophon-server-$version-$arch-unknown-linux-musl.tar.gz cli=colophon-cli-$version-$arch-unknown-linux-musl.tar.gz studio=colophon-studio-$version.tar.gz
  if [[ ! -x "$release/bin/colophon-server" ]]; then
    tmp=$(mktemp -d); trap 'rm -rf "$tmp"' RETURN
    log "fetching v$version ($arch)"
    for f in SHA256SUMS "$server" "$cli" "$studio"; do fetch "$f" "$tmp" "$version"; done
    (cd "$tmp" && grep -E " [*]?($server|$cli|$studio)\$" SHA256SUMS | sha256sum -c --quiet -) || die "checksums don't match SHA256SUMS: nothing installed"
    log "checksums verified"
    mkdir -p "$release.partial/bin"
    tar -xzf "$tmp/$server" -C "$release.partial/bin" colophon-server
    tar -xzf "$tmp/$cli" -C "$release.partial/bin" colophon
    tar -xzf "$tmp/$studio" -C "$release.partial"
    mv "$release.partial/colophon-studio-$version" "$release.partial/studio"
    chmod 0755 "$release.partial/bin/"*
    "$release.partial/bin/colophon-server" --version >/dev/null || die "the new server doesn't run here"
    rm -rf "$release" && mv "$release.partial" "$release"
  else
    log "v$version is already unpacked"
  fi

  if [[ "$mode" == fetch ]]; then
    ln -sfn "$release/bin/colophon" /usr/local/bin/colophon
    log "v$version unpacked at $release (not switched)"; return 0
  fi
  local old; old=$(current_version)
  if [[ "$old" == "$version" ]] && systemctl is-active --quiet colophon && ready 5; then
    log "v$version is already current and ready"; return 0
  fi

  # A database migrated by a newer binary than this one is refused: serving it would be guessing.
  # (--rollback is the deliberate exception: forward-compatible for one release.)
  if [[ -s $CREDS/migration_database_url ]]; then
    local schema
    schema=$("$release/bin/colophon" admin migrate-status --database-url-file "$CREDS/migration_database_url" --json 2>/dev/null || true)
    if [[ "$(jq -r '.data.status // empty' <<<"$schema" 2>/dev/null)" == unknown ]]; then
      die "the database has migrations v$version doesn't know ($(jq -r '.data.unknown | join(", ")' <<<"$schema")): a newer build migrated it; refusing to switch to an older one (use --rollback for the one release you came from)"
    fi
  fi

  # The new binary migrates while the old one serves (forward-compatible for one release).
  if [[ -s $CREDS/migration_database_url ]]; then
    log "migrating with v$version (D-202's lock_timeout and retry not in yet)"
    "$release/bin/colophon" admin migrate --database-url-file "$CREDS/migration_database_url" \
      || die "migration failed: nothing switched; v${old:-none} still serves"
  fi

  [[ -n "$old" ]] && echo "$old" >"$ROOT/previous"
  log "switching ${old:-nothing} -> $version"
  link_to "$release"
  if ! restart_and_check "$version"; then
    if [[ -n "$old" && -d $ROOT/releases/$old ]]; then
      log "ROLLING BACK to $old"
      link_to "$ROOT/releases/$old"
      restart_and_check "$old" || die "the rollback to $old isn't ready either: see journalctl -u colophon"
      die "v$version failed its checks; rolled back to $old"
    fi
    die "v$version failed its checks and there is no previous release to go back to"
  fi
  prune
  log "v$version is live ($((SECONDS - start)) s)"
}

rollback() {
  local start=$SECONDS prev cur
  prev=$(cat "$ROOT/previous" 2>/dev/null || true); cur=$(current_version)
  [[ -n "$prev" && -d $ROOT/releases/$prev ]] || die "no previous release to roll back to"
  log "rolling back $cur -> $prev (binaries only; the schema stays, forward-compatible)"
  echo "$cur" >"$ROOT/previous"
  link_to "$ROOT/releases/$prev"
  restart_and_check "$prev" || die "$prev isn't ready after the rollback: see journalctl -u colophon"
  log "rolled back to $prev ($((SECONDS - start)) s)"
}

status() {
  echo "current:  $(current_version)"
  echo "previous: $(cat "$ROOT/previous" 2>/dev/null || echo none)"
  # shellcheck disable=SC2012
  echo "releases: $(ls -1t "$ROOT/releases" 2>/dev/null | paste -sd' ' -)"
}

mkdir -p "$ROOT/releases"
case "${1:-}" in
  --rollback) rollback ;;
  --status) status ;;
  --fetch) [[ -n "${2:-}" ]] || die "usage: upgrade.sh --fetch <version>"; install_release "$2" fetch ;;
  "" | -*) echo "usage: upgrade.sh <version> | --rollback | --status" >&2; exit 2 ;;
  *) install_release "$1" ;;
esac
