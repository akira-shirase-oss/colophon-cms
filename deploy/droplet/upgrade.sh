#!/usr/bin/env bash
# Install, upgrade or roll back Colophon on the S0 droplet (deploy/droplet/DEPLOY.md). Targets:
# an upgrade in under 2 minutes with under ~5 s unavailable, a rollback in under 1 minute.
#
#   sudo upgrade.sh <version>       install that release and switch to it (install.sh calls this)
#   sudo upgrade.sh --channel edge  the newest build of main (D-211): provenance verified (edge.yml
#                                   on refs/heads/main of this repository), every file against
#                                   SHA256SUMS, EDGE's commit equal to the attested one
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
ROOT="${COLOPHON_ROOT:-/opt/colophon}"
BASE="${COLOPHON_RELEASE_BASE:-https://github.com/akira-shirase-oss/colophon-cms/releases/download}"
# The edge channel's provenance pins (D-211): this repository, edge.yml, and main only.
REPO=akira-shirase-oss/colophon-cms
EDGE_WORKFLOW=$REPO/.github/workflows/edge.yml
EDGE_REF=refs/heads/main
TESTING="${COLOPHON_UPGRADE_TEST:-0}"   # scripts/tests only: no root, no systemd, no /usr/local links
CREDS=/etc/colophon/credentials
LOCAL=http://127.0.0.1:58000
KEEP=3
here=$(cd "$(dirname "$0")" && pwd)

log() { printf '%s  %s\n' "$(date -u +%H:%M:%S)" "$*"; }
die() { log "error: $*" >&2; exit 1; }
[[ $EUID -eq 0 || "$TESTING" == 1 ]] || die "run as root"

arch() {
  case "$(uname -m)" in x86_64) echo x86_64 ;; aarch64 | arm64) echo aarch64 ;; *) die "unsupported machine $(uname -m)" ;; esac
}

# fetch <file> <dest-dir> <tag>: one asset of a release (tag v<version>, or edge).
fetch() {
  local file=$1 dest=$2 tag=$3
  case "$BASE" in
    http://* | https://*) curl -fsSL --retry 3 --max-time 120 -o "$dest/$file" "$BASE/$tag/$file" ;;
    file://*) cp "${BASE#file://}/$tag/$file" "$dest/$file" ;;
    *) cp "$BASE/$tag/$file" "$dest/$file" ;;
  esac
}

# sums_cover <dir> <file>...: each file matches its line in SHA256SUMS (and has one).
sums_cover() {
  local dir=$1 f; shift
  for f in "$@"; do
    grep -qE " [*]?$f\$" "$dir/SHA256SUMS" || { why="SHA256SUMS has no line for $f"; return 1; }
  done
  (cd "$dir" && for f in "$@"; do grep -E " [*]?$f\$" SHA256SUMS; done | sha256sum -c --quiet - >/dev/null 2>&1) \
    || { why="a file doesn't match SHA256SUMS (tampered, or two builds' files mixed)"; return 1; }
}

# verify_edge <dir> <server> <cli> <studio>: the provenance of SHA256SUMS (edge.yml on main, this
# repository), every file against SHA256SUMS (EDGE included), and EDGE's commit equal to the
# attested one. Sets EDGE_COMMIT; on failure, `why`.
verify_edge() {
  local dir=$1 out commit; shift
  [[ -s "$dir/SHA256SUMS.intoto.jsonl" ]] || { why="no provenance bundle (SHA256SUMS.intoto.jsonl)"; return 1; }
  out=$(gh attestation verify "$dir/SHA256SUMS" --bundle "$dir/SHA256SUMS.intoto.jsonl" --repo "$REPO" \
    --signer-workflow "$EDGE_WORKFLOW" --source-ref "$EDGE_REF" --format json 2>/dev/null) \
    || { why="the provenance doesn't verify (not edge.yml on $EDGE_REF of $REPO)"; return 1; }
  commit=$(jq -r '.[0].verificationResult.signature.certificate.sourceRepositoryDigest // empty' <<<"$out" 2>/dev/null)
  [[ "$commit" =~ ^[0-9a-f]{40}$ ]] || { why="the attestation names no source commit"; return 1; }
  sums_cover "$dir" EDGE "$@" || return 1
  [[ "$(tr -d ' \n' <"$dir/EDGE")" == "$commit" ]] || { why="EDGE names $(tr -d ' \n' <"$dir/EDGE"), the attestation $commit (two builds mixed)"; return 1; }
  EDGE_COMMIT=$commit
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

# download <channel> <version> <dir>: the release's assets into <dir>, verified. For the release
# channel, archives against SHA256SUMS; for edge, provenance too, and a mismatch is fetched again
# once (the rolling release is replaced on each merge, so one download can straddle two builds).
download() {
  local channel=$1 version=$2 dir=$3 arch=$4 attempt
  why=""
  if [[ "$channel" == release ]]; then
    local server=colophon-server-$version-$arch-unknown-linux-musl.tar.gz cli=colophon-cli-$version-$arch-unknown-linux-musl.tar.gz studio=colophon-studio-$version.tar.gz
    for f in SHA256SUMS "$server" "$cli" "$studio"; do fetch "$f" "$dir" "v$version"; done
    sums_cover "$dir" "$server" "$cli" "$studio" || die "$why: nothing installed"
    ASSETS=("$server" "$cli" "$studio" "colophon-studio-$version")
    return 0
  fi
  local server=colophon-server-edge-$arch-unknown-linux-musl.tar.gz cli=colophon-cli-edge-$arch-unknown-linux-musl.tar.gz studio=colophon-studio-edge.tar.gz
  for attempt in 1 2; do
    rm -f "$dir"/*
    for f in SHA256SUMS SHA256SUMS.intoto.jsonl EDGE "$server" "$cli" "$studio"; do fetch "$f" "$dir" edge 2>/dev/null || true; done
    if verify_edge "$dir" "$server" "$cli" "$studio"; then
      ASSETS=("$server" "$cli" "$studio" "colophon-studio-edge")
      return 0
    fi
    log "edge: $why$([[ $attempt == 1 ]] && echo '; fetching once more')"
  done
  die "edge: $why: refusing the switch"
}

install_release() {
  local channel=$1 version=$2 mode=${3:-switch} arch tmp release start=$SECONDS
  arch=$(arch)
  tmp=$(mktemp -d); trap 'rm -rf "$tmp"' RETURN
  if [[ "$channel" == release ]]; then
    [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || die "'$version' isn't a version (like 0.1.0-alpha.1)"
    release=$ROOT/releases/$version
    [[ -x "$release/bin/colophon-server" ]] || { log "fetching v$version ($arch)"; download release "$version" "$tmp" "$arch"; }
  else
    log "fetching the edge build ($arch)"
    download edge "" "$tmp" "$arch"
    version=edge-${EDGE_COMMIT:0:12}
    release=$ROOT/releases/$version
    log "edge build $EDGE_COMMIT: provenance (edge.yml, $EDGE_REF) and checksums verified"
  fi
  if [[ ! -x "$release/bin/colophon-server" ]]; then
    log "checksums verified"
    mkdir -p "$release.partial/bin"
    tar -xzf "$tmp/${ASSETS[0]}" -C "$release.partial/bin" colophon-server
    tar -xzf "$tmp/${ASSETS[1]}" -C "$release.partial/bin" colophon
    tar -xzf "$tmp/${ASSETS[2]}" -C "$release.partial"
    mv "$release.partial/${ASSETS[3]}" "$release.partial/studio"
    chmod 0755 "$release.partial/bin/"*
    "$release.partial/bin/colophon-server" --version >/dev/null || die "the new server doesn't run here"
    rm -rf "$release" && mv "$release.partial" "$release"
  else
    log "$version is already unpacked"
  fi

  if [[ "$mode" == fetch ]]; then
    [[ "$TESTING" == 1 ]] || ln -sfn "$release/bin/colophon" /usr/local/bin/colophon
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
channel=release
if [[ "${1:-}" == --channel ]]; then
  channel=${2:-}; shift 2 || true
  [[ "$channel" == release || "$channel" == edge ]] || die "--channel is release or edge"
fi
case "${1:-}" in
  --rollback) rollback ;;
  --status) status ;;
  --fetch)
    if [[ "$channel" == edge ]]; then install_release edge "" fetch
    else [[ -n "${2:-}" ]] || die "usage: upgrade.sh --fetch <version>"; install_release release "$2" fetch; fi ;;
  "")
    if [[ "$channel" == edge ]]; then install_release edge ""
    else echo "usage: upgrade.sh [--channel release] <version> | --channel edge | --rollback | --status" >&2; exit 2; fi ;;
  -*) echo "usage: upgrade.sh [--channel release] <version> | --channel edge | --rollback | --status" >&2; exit 2 ;;
  *) [[ "$channel" == release ]] || die "the edge channel takes no version"; install_release release "$1" ;;
esac
