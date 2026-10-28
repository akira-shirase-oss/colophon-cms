#!/bin/sh
# Installs the colophon CLI from a Colophon release: downloads the archive for this machine,
# checks its SHA-256 against the release's SHA256SUMS, and puts `colophon` in a directory you
# own. It never asks for sudo. (docs/modules/08-cli.md, "Install")
#
#   curl -fsSL https://raw.githubusercontent.com/akira-shirase-oss/colophon-cms/main/scripts/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/akira-shirase-oss/colophon-cms/main/scripts/install.sh | sh -s -- --version 0.1.0-alpha.1
#
# Options, or the environment variable in brackets:
#   --version V   the release to install [COLOPHON_VERSION]; default the newest, pre-releases included
#   --dir D       where to put colophon [COLOPHON_INSTALL_DIR]; default ~/.local/bin
#   --target T    the build to install (a Rust target, below); default this machine's
# COLOPHON_DOWNLOAD_URL replaces https://github.com/akira-shirase-oss/colophon-cms/releases/download
# (a mirror, or a local copy for testing); files are fetched from <url>/v<version>/<file>.
#
# macOS (Apple silicon, Intel) and Linux (x86-64, 64-bit Arm; static binaries, any distribution).
# On Windows, use npm (npx @colophon-cms/cli) or the release's .zip.
set -eu

REPOSITORY=akira-shirase-oss/colophon-cms
TARGETS="aarch64-apple-darwin x86_64-apple-darwin x86_64-unknown-linux-musl aarch64-unknown-linux-musl"

say() { printf '%s\n' "$*" >&2; }
fail() {
  say "error: $*"
  exit 1
}

version=${COLOPHON_VERSION:-}
dir=${COLOPHON_INSTALL_DIR:-}
target=
while [ $# -gt 0 ]; do
  case "$1" in
    --version) [ $# -ge 2 ] || fail "--version needs a value"; version=$2; shift 2 ;;
    --dir) [ $# -ge 2 ] || fail "--dir needs a value"; dir=$2; shift 2 ;;
    --target) [ $# -ge 2 ] || fail "--target needs a value"; target=$2; shift 2 ;;
    -h | --help) sed -n '2,20p' "$0" 2>/dev/null || say "see https://github.com/$REPOSITORY"; exit 0 ;;
    *) fail "unknown option $1 (try --help)" ;;
  esac
done
version=${version#v}
if [ -z "$dir" ]; then
  [ -n "${HOME:-}" ] || fail "HOME is not set; choose a directory with --dir"
  dir=$HOME/.local/bin
fi

# Which build this machine needs.
if [ -z "$target" ]; then
  os=$(uname -s)
  arch=$(uname -m)
  case "$os" in
    Darwin)
      # A shell under Rosetta reports x86_64 on Apple silicon; the native build is better.
      if [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
        arch=arm64
      fi
      suffix=apple-darwin
      ;;
    Linux) suffix=unknown-linux-musl ;;
    MINGW* | MSYS* | CYGWIN* | Windows_NT) fail "on Windows, use npm (npx @colophon-cms/cli) or the .zip from https://github.com/$REPOSITORY/releases" ;;
    *) fail "no prebuilt colophon for $os; build it with Rust: cargo install --locked --git https://github.com/$REPOSITORY colophon-cli" ;;
  esac
  case "$arch" in
    arm64 | aarch64) arch=aarch64 ;;
    x86_64 | amd64) arch=x86_64 ;;
    *) fail "no prebuilt colophon for $os on $arch; build it with Rust: cargo install --locked --git https://github.com/$REPOSITORY colophon-cli" ;;
  esac
  target=$arch-$suffix
fi
case " $TARGETS " in
  *" $target "*) ;;
  *) fail "no prebuilt colophon for $target; this script installs one of: $TARGETS" ;;
esac

# How to download: curl or wget, HTTPS only unless COLOPHON_DOWNLOAD_URL says otherwise.
base=${COLOPHON_DOWNLOAD_URL:-https://github.com/$REPOSITORY/releases/download}
base=${base%/}
download() { # download <url> <file>
  case "$1" in https://*) https_only=1 ;; *) https_only= ;; esac
  if command -v curl >/dev/null 2>&1; then
    if [ -n "$https_only" ]; then
      curl --proto '=https' --tlsv1.2 -fsSL --retry 3 -o "$2" "$1"
    else
      curl -fsSL --retry 3 -o "$2" "$1"
    fi
  elif command -v wget >/dev/null 2>&1; then
    if [ -n "$https_only" ]; then
      wget --https-only -q -O "$2" "$1"
    else
      wget -q -O "$2" "$1"
    fi
  else
    fail "neither curl nor wget is installed"
  fi
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  elif command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 "$1" | awk '{print $NF}'
  else
    fail "no SHA-256 tool (sha256sum, shasum or openssl) to check the download with"
  fi
}

work=$(mktemp -d 2>/dev/null || mktemp -d -t colophon-install)
partial=
trap 'rm -rf "$work" ${partial:+"$partial"}' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# The newest release, pre-releases included (GitHub's "latest" skips them).
if [ -z "$version" ]; then
  [ -z "${COLOPHON_DOWNLOAD_URL:-}" ] || fail "with COLOPHON_DOWNLOAD_URL, say which release with --version"
  download "https://api.github.com/repos/$REPOSITORY/releases?per_page=1" "$work/releases.json" ||
    fail "could not ask GitHub for the newest release; say which with --version"
  version=$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\{0,1\}\([^"]*\)".*/\1/p' "$work/releases.json" | head -n 1)
  [ -n "$version" ] || fail "GitHub lists no release yet; say which with --version"
fi

archive=colophon-cli-$version-$target.tar.gz
say "Installing colophon $version ($target) into $dir"
download "$base/v$version/SHA256SUMS" "$work/SHA256SUMS" ||
  fail "could not download $base/v$version/SHA256SUMS: is $version a release?"
download "$base/v$version/$archive" "$work/$archive" ||
  fail "could not download $base/v$version/$archive"

expected=$(awk -v name="$archive" '$2 == name || $2 == "*" name { print $1; exit }' "$work/SHA256SUMS")
[ -n "$expected" ] || fail "SHA256SUMS of $version doesn't list $archive"
actual=$(sha256 "$work/$archive")
if [ "$actual" != "$expected" ]; then
  fail "$archive doesn't match SHA256SUMS (got $actual, expected $expected); nothing was installed"
fi

mkdir -p "$work/unpacked"
tar -xzf "$work/$archive" -C "$work/unpacked" colophon || fail "$archive has no colophon in it"

# Copy beside the destination, then rename: an existing colophon is replaced in one step, never
# left half-written.
mkdir -p "$dir" || fail "could not create $dir"
partial=$dir/.colophon.new.$$
cp "$work/unpacked/colophon" "$partial"
chmod 0755 "$partial"
mv -f "$partial" "$dir/colophon"
partial=

say "Installed: $("$dir/colophon" --version)"
case ":${PATH:-}:" in
  *":$dir:"*) ;;
  *) say "$dir is not on your PATH; add it, for example: export PATH=\"$dir:\$PATH\"" ;;
esac
