#!/usr/bin/env bash
# Prints the release version: `version` in the root Cargo.toml's [workspace.package], which every
# crate and npm package carries (docs/modules/19-oss.md, "Versions"). Given a git tag, it also
# checks that the tag is `v` plus that version, so a tag can never release something else.
#
#   scripts/release-version.sh            # 0.1.0-alpha.1
#   scripts/release-version.sh v0.1.0     # fails: the workspace is 0.1.0-alpha.1
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(awk '
  /^\[workspace\.package\]/ { in_section = 1; next }
  /^\[/ { in_section = 0 }
  in_section && /^version[[:space:]]*=/ { sub(/^[^"]*"/, ""); sub(/".*$/, ""); print; exit }
' Cargo.toml)

# SemVer 2.0: MAJOR.MINOR.PATCH, an optional pre-release, no build metadata (tags carry none).
semver='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$'
if [[ ! "$version" =~ $semver ]]; then
  echo "error: [workspace.package] version in Cargo.toml is \"$version\", not a semantic version" >&2
  exit 1
fi

if [[ $# -gt 0 && "$1" != "v$version" ]]; then
  echo "error: the tag $1 doesn't match the workspace version $version; tag the commit v$version" >&2
  exit 1
fi
echo "$version"
