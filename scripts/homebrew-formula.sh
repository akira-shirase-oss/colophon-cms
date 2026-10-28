#!/usr/bin/env bash
# Renders the Homebrew formula (packaging/homebrew/colophon.rb.in) for one release, taking each
# archive's SHA-256 from the release's SHA256SUMS, and prints it. The release workflow attaches the
# result to the release; it goes into the tap as Formula/colophon.rb.
#
#   scripts/homebrew-formula.sh <version> <SHA256SUMS> > colophon.rb
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <version> <SHA256SUMS>" >&2
  exit 2
fi
version=${1#v}
sums=$2
template="$(dirname "$0")/../packaging/homebrew/colophon.rb.in"

formula=$(sed "s/@VERSION@/$version/g" "$template")
for target in aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-musl x86_64-unknown-linux-musl; do
  archive="colophon-cli-$version-$target.tar.gz"
  sha=$(awk -v name="$archive" '$2 == name || $2 == "*" name { print $1; exit }' "$sums")
  if [[ ! "$sha" =~ ^[0-9a-f]{64}$ ]]; then
    echo "error: $sums has no SHA-256 for $archive" >&2
    exit 1
  fi
  formula=${formula//"@SHA256_$target@"/$sha}
done
if grep -q '@[A-Z0-9_a-z-]*@' <<<"$formula"; then
  echo "error: placeholders left in the formula: $(grep -o '@[A-Z0-9_a-z-]*@' <<<"$formula" | sort -u | tr '\n' ' ')" >&2
  exit 1
fi
printf '%s\n' "$formula"
