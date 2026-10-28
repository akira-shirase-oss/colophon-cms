#!/usr/bin/env bash
# Tests scripts/homebrew-formula.sh: it renders the formula from a release's SHA256SUMS, the
# result is valid Ruby, and, evaluated with a stand-in for Homebrew's Formula DSL, each platform
# block pairs the right archive with that archive's hash. A SHA256SUMS missing a target is refused.
# (This doesn't install anything with brew; the tap does that once a release exists.)
#
#   scripts/test-homebrew-formula.sh
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(scripts/release-version.sh)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail() {
  echo "FAIL: $*" >&2
  exit 1
}

# A SHA256SUMS with a distinct made-up hash for every file a release carries.
for target in aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-musl x86_64-unknown-linux-musl x86_64-pc-windows-msvc; do
  ext=tar.gz
  if [[ "$target" == *windows* ]]; then ext=zip; fi
  for component in colophon-cli colophon-server; do
    name="$component-$version-$target.$ext"
    echo "$(printf '%s' "$name" | shasum -a 256 | awk '{print $1}')  $name"
  done
done >"$work/SHA256SUMS"

scripts/homebrew-formula.sh "$version" "$work/SHA256SUMS" >"$work/colophon.rb"
ruby -c "$work/colophon.rb" >/dev/null || fail "the rendered formula is not valid Ruby"

cat >"$work/dsl.rb" <<'RUBY'
# Just enough of Homebrew's Formula DSL to see which url and sha256 each platform gets.
class Formula
  @rows = []
  class << self
    attr_reader :rows
    def desc(*) = nil
    def homepage(*) = nil
    def version(value = nil) = (value ? @version = value : @version)
    def on_macos = (@os = "macos"; yield)
    def on_linux = (@os = "linux"; yield)
    def on_arm = (@cpu = "arm"; yield)
    def on_intel = (@cpu = "intel"; yield)
    def url(value) = @url = value
    def sha256(value) = Formula.rows << [@os, @cpu, @url, value, version]
    def test(*) = nil
  end
end
load ARGV.fetch(0)
Formula.rows.each { |row| puts row.join(" ") }
RUBY
ruby "$work/dsl.rb" "$work/colophon.rb" >"$work/rows"

[[ $(wc -l <"$work/rows") -eq 4 ]] || fail "expected four platform blocks, got: $(cat "$work/rows")"
seen=
while read -r os cpu url sha formula_version; do
  case "$os $cpu" in
    "macos arm") target=aarch64-apple-darwin ;;
    "macos intel") target=x86_64-apple-darwin ;;
    "linux arm") target=aarch64-unknown-linux-musl ;;
    "linux intel") target=x86_64-unknown-linux-musl ;;
    *) fail "unexpected block $os $cpu" ;;
  esac
  seen="$seen $target"
  archive="colophon-cli-$version-$target.tar.gz"
  [[ "$url" == "https://github.com/akira-shirase-oss/colophon-cms/releases/download/v$version/$archive" ]] ||
    fail "$os $cpu downloads $url"
  [[ "$sha" == "$(awk -v n="$archive" '$2 == n { print $1 }' "$work/SHA256SUMS")" ]] || fail "$os $cpu has the wrong sha256"
  [[ "$formula_version" == "$version" ]] || fail "the formula says version $formula_version"
done <"$work/rows"
for target in aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-musl x86_64-unknown-linux-musl; do
  [[ " $seen " == *" $target "* ]] || fail "no block installs $target"
done

grep -v aarch64-unknown-linux-musl "$work/SHA256SUMS" >"$work/partial"
if scripts/homebrew-formula.sh "$version" "$work/partial" >/dev/null 2>"$work/stderr"; then
  fail "rendered a formula without the Linux arm64 hash"
fi
grep -q "has no SHA-256 for colophon-cli-$version-aarch64-unknown-linux-musl.tar.gz" "$work/stderr" || fail "wrong message: $(cat "$work/stderr")"

echo "homebrew formula: four platforms, each with its archive and hash; a missing hash refused"
