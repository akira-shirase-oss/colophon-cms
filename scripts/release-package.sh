#!/usr/bin/env bash
# Packs one target's release binaries into the archives a GitHub release carries, each with a
# `.sha256` file beside it in `sha256sum` format (docs/modules/19-oss.md, "Release workflow").
#
#   scripts/release-package.sh <target> <version> <out-dir> [<binary-dir>]
#
# Two archives per target, each holding one binary at its root:
#   colophon-cli-<version>-<target>.tar.gz      (colophon)
#   colophon-server-<version>-<target>.tar.gz   (colophon-server)
# Windows targets get .zip and .exe. <binary-dir> defaults to target/<target>/release.
set -euo pipefail

if [[ $# -lt 3 ]]; then
  echo "usage: $0 <target> <version> <out-dir> [<binary-dir>]" >&2
  exit 2
fi
target=$1
version=$2
out=$3
bindir=${4:-target/$target/release}

case "$target" in
  *-windows-*) exe=.exe; kind=zip ;;
  *) exe=; kind=tar.gz ;;
esac

sha256() {
  if command -v sha256sum >/dev/null; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

zip_one() { # zip_one <archive> <dir> <file>
  local archive dir file
  archive=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
  dir=$2
  file=$3
  if command -v 7z >/dev/null; then
    (cd "$dir" && 7z a -tzip -bso0 -bsp0 "$archive" "$file")
  elif command -v zip >/dev/null; then
    (cd "$dir" && zip -q -X "$archive" "$file")
  else
    echo "error: no zip tool (7z or zip) to make $archive" >&2
    exit 1
  fi
}

# Archive entries owned by root, whoever built them: GNU tar on Linux, bsdtar on macOS and Windows.
if tar --version 2>/dev/null | grep -q 'GNU tar'; then
  owner=(--owner=0 --group=0 --numeric-owner)
else
  owner=(--uid 0 --gid 0 --uname root --gname root)
fi

mkdir -p "$out"
for pair in colophon:colophon-cli colophon-server:colophon-server; do
  bin=${pair%%:*}
  component=${pair##*:}
  source="$bindir/$bin$exe"
  if [[ ! -f "$source" ]]; then
    echo "error: $source is missing; build it with: cargo build --release --locked --target $target -p colophon-cli -p colophon-server" >&2
    exit 1
  fi
  stage=$(mktemp -d)
  cp "$source" "$stage/$bin$exe"
  chmod 0755 "$stage/$bin$exe"
  archive="$out/$component-$version-$target.$kind"
  rm -f "$archive"
  if [[ "$kind" == zip ]]; then
    zip_one "$archive" "$stage" "$bin$exe"
  else
    # COPYFILE_DISABLE keeps macOS's tar from adding AppleDouble (._) files.
    COPYFILE_DISABLE=1 tar "${owner[@]}" -czf "$archive" -C "$stage" "$bin$exe"
  fi
  rm -rf "$stage"
  echo "$(sha256 "$archive")  $(basename "$archive")" >"$archive.sha256"
  echo "packed $archive"
done
