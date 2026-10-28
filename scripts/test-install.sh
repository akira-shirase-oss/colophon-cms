#!/usr/bin/env bash
# Tests scripts/install.sh against fake releases served over HTTP from a temporary directory on
# 127.0.0.1 (docs/modules/08-cli.md, "Install"). It installs (finding this machine's build itself),
# upgrades in place, refuses a tampered archive without touching what was installed, and fails
# plainly on a release that doesn't exist, an archive SHA256SUMS doesn't list, an unsupported
# target and a missing version. Every case runs under each POSIX shell found (sh, dash, bash),
# and once piped into sh as `curl … | sh` does.
#
#   scripts/test-install.sh [<colophon binary built for this machine>]
#
# Without a binary, the fake releases carry a stand-in script that prints a version line.
# Needs python3 (the HTTP server), curl or wget, and tar.
set -euo pipefail
cd "$(dirname "$0")/.."

real_binary=${1:-}
version=$(scripts/release-version.sh)
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) target=aarch64-apple-darwin ;;
  Darwin-x86_64) target=x86_64-apple-darwin ;;
  Linux-x86_64) target=x86_64-unknown-linux-musl ;;
  Linux-aarch64 | Linux-arm64) target=aarch64-unknown-linux-musl ;;
  *)
    echo "skip: there is no release build for $(uname -s)-$(uname -m)"
    exit 0
    ;;
esac

work=$(mktemp -d)
server=
cleanup() {
  if [[ -n "$server" ]]; then
    kill "$server" 2>/dev/null || true
    wait "$server" 2>/dev/null || true
  fi
  rm -rf "$work"
}
trap cleanup EXIT

checks=0
pass() { checks=$((checks + 1)); }
die() {
  echo "FAIL: $*" >&2
  exit 1
}

# A release <v> for this target in $work/www/v<v>/, made by the release workflow's packaging script.
release() { # release <version> [<binary>]
  local v=$1 binary=${2:-} bin="$work/bin-$1"
  mkdir -p "$bin"
  if [[ -n "$binary" ]]; then
    cp "$binary" "$bin/colophon"
  else
    printf '#!/bin/sh\necho "colophon %s (commit stand-in, built stand-in)"\n' "$v" >"$bin/colophon"
  fi
  printf '#!/bin/sh\necho server\n' >"$bin/colophon-server"
  chmod 0755 "$bin/colophon" "$bin/colophon-server"
  scripts/release-package.sh "$target" "$v" "$work/www/v$v" "$bin" >/dev/null
  (cd "$work/www/v$v" && cat ./*.sha256 >SHA256SUMS && rm ./*.sha256)
}

release "$version" "$real_binary"
release 99.0.0
# A tampered archive: SHA256SUMS lists the real hash, the file has gained bytes since.
release 77.0.0
printf 'tampered' >>"$work/www/v77.0.0/colophon-cli-77.0.0-$target.tar.gz"
# An archive SHA256SUMS doesn't list.
release 66.0.0
grep -v colophon-cli "$work/www/v66.0.0/SHA256SUMS" >"$work/sums" && mv "$work/sums" "$work/www/v66.0.0/SHA256SUMS"

# Serve $work/www on a free port.
python3 -c '
import functools, http.server, sys
class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass
handler = functools.partial(Quiet, directory=sys.argv[1])
server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
print(server.server_address[1], flush=True)
server.serve_forever()
' "$work/www" >"$work/port" &
server=$!
for _ in $(seq 50); do
  [[ -s "$work/port" ]] && break
  sleep 0.1
done
port=$(cat "$work/port")
[[ -n "$port" ]] || die "the HTTP server didn't start"
export COLOPHON_DOWNLOAD_URL="http://127.0.0.1:$port"
unset COLOPHON_VERSION COLOPHON_INSTALL_DIR

if [[ -n "$real_binary" ]]; then
  expected=$("$real_binary" --version)
else
  expected="colophon $version (commit stand-in, built stand-in)"
fi

shells=()
for shell in sh dash bash; do
  if command -v "$shell" >/dev/null; then shells+=("$shell"); fi
done

for shell in "${shells[@]}"; do
  dir="$work/home-$shell/bin"
  run() { "$shell" scripts/install.sh "$@" 2>"$work/stderr"; }

  # Installs this machine's build, detected, into a directory it creates.
  run --version "$version" --dir "$dir" || die "$shell: install failed: $(cat "$work/stderr")"
  [[ "$("$dir/colophon" --version)" == "$expected" ]] || die "$shell: installed the wrong binary"
  [[ -x "$dir/colophon" ]] || die "$shell: not executable"
  grep -q "Installed: $expected" "$work/stderr" || die "$shell: no confirmation: $(cat "$work/stderr")"
  pass

  # Upgrades in place, accepting a leading v; nothing half-written is left behind.
  run --version v99.0.0 --dir "$dir" || die "$shell: upgrade failed: $(cat "$work/stderr")"
  [[ "$("$dir/colophon" --version)" == "colophon 99.0.0 (commit stand-in, built stand-in)" ]] || die "$shell: upgrade didn't replace"
  [[ -z "$(find "$dir" -name '.colophon.new.*')" ]] || die "$shell: left a partial file"
  pass

  # A tampered archive is refused and the installed colophon is untouched.
  if run --version 77.0.0 --dir "$dir"; then die "$shell: installed a tampered archive"; fi
  grep -q "doesn't match SHA256SUMS" "$work/stderr" || die "$shell: wrong message: $(cat "$work/stderr")"
  grep -q "nothing was installed" "$work/stderr" || die "$shell: doesn't say nothing was installed"
  [[ "$("$dir/colophon" --version)" == "colophon 99.0.0 (commit stand-in, built stand-in)" ]] || die "$shell: tampering changed the install"
  [[ -z "$(find "$dir" -name '.colophon.new.*')" ]] || die "$shell: left a partial file"
  pass

  # An archive SHA256SUMS doesn't list is refused.
  if run --version 66.0.0 --dir "$dir"; then die "$shell: installed an unlisted archive"; fi
  grep -q "doesn't list colophon-cli-66.0.0-$target.tar.gz" "$work/stderr" || die "$shell: wrong message: $(cat "$work/stderr")"
  pass

  # A release that doesn't exist.
  if run --version 55.0.0 --dir "$dir"; then die "$shell: installed a release that doesn't exist"; fi
  grep -q "is 55.0.0 a release?" "$work/stderr" || die "$shell: wrong message: $(cat "$work/stderr")"
  pass

  # A target there is no build for.
  if run --version "$version" --dir "$dir" --target riscv64gc-unknown-linux-gnu; then die "$shell: accepted an unknown target"; fi
  grep -q "no prebuilt colophon for riscv64gc-unknown-linux-gnu" "$work/stderr" || die "$shell: wrong message: $(cat "$work/stderr")"
  pass

  # A mirror needs an explicit version (there's no GitHub to ask for the newest).
  if run --dir "$dir"; then die "$shell: guessed a version"; fi
  grep -q "say which release with --version" "$work/stderr" || die "$shell: wrong message: $(cat "$work/stderr")"
  pass

  # Options through the environment.
  COLOPHON_VERSION=$version COLOPHON_INSTALL_DIR="$dir" run || die "$shell: environment options failed: $(cat "$work/stderr")"
  [[ "$("$dir/colophon" --version)" == "$expected" ]] || die "$shell: environment options installed the wrong binary"
  pass
done

# As `curl -fsSL …/install.sh | sh -s -- …` runs it: the script on standard input.
dir="$work/piped/bin"
sh -s -- --version "$version" --dir "$dir" <scripts/install.sh 2>"$work/stderr" || die "piped: $(cat "$work/stderr")"
[[ "$("$dir/colophon" --version)" == "$expected" ]] || die "piped: installed the wrong binary"
grep -q "is not on your PATH" "$work/stderr" || die "piped: no PATH hint: $(cat "$work/stderr")"
pass

echo "install.sh: $checks checks passed for $target under ${shells[*]} and piped into sh"
