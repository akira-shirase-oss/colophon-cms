#!/usr/bin/env bash
# Tests upgrade.sh's release channel against a local release and a stand-in `gh` that verifies a
# file against a bundle the way the real one is asked to (this repository, release.yml, the
# version's tag) and fails like gh otherwise:
#   - a release whose bundle attests every archive unpacks;
#   - no bundle, a bundle for another tag or another workflow, an archive the bundle doesn't attest
#     (replaced along with SHA256SUMS: checksums alone would take it), and no gh each refuse, and
#     nothing is unpacked;
#   - COLOPHON_RELEASE_UNVERIFIED=rehearsal takes a local release without a bundle, saying so
#     loudly, and is refused for a release from GitHub;
#   - a switch refuses when the new binary can't read the migration status, or reads "unknown".
#
#   deploy/droplet/test-upgrade-release.sh
set -euo pipefail
cd "$(dirname "$0")"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }
arch=aarch64
[[ "$(uname -m)" == x86_64 ]] && arch=x86_64
v=0.9.1-alpha.3
repo=akira-shirase-oss/colophon-cms

mkdir -p "$work/bin"
cat >"$work/bin/gh" <<'STUB'
#!/usr/bin/env bash
file=$3; shift 3
while (($#)); do case "$1" in --bundle) bundle=$2; shift 2 ;; --repo) repo=$2; shift 2 ;; --signer-workflow) wf=$2; shift 2 ;; --source-ref) ref=$2; shift 2 ;; *) shift ;; esac; done
[[ -s "${bundle:-}" ]] || exit 1
digest=$(sha256sum "$file" | cut -d' ' -f1)
jq -e --arg d "$digest" --arg r "$repo" --arg w "$wf" --arg ref "$ref" \
  '(.subjects | index($d)) != null and .repo == $r and .workflow == $w and .ref == $ref' "$bundle" >/dev/null || exit 1
STUB
chmod +x "$work/bin/gh"
command -v sha256sum >/dev/null || { printf '#!/bin/sh\nexec shasum -a 256 "$@"\n' >"$work/bin/sha256sum"; chmod +x "$work/bin/sha256sum"; }

# build <base> [workflow] [ref]: a release v$v under <base>/v$v, its bundle attesting each archive.
build() {
  local d=$1/v$v wf=${2:-$repo/.github/workflows/release.yml} ref=${3:-refs/tags/v$v}
  mkdir -p "$d/x/colophon-studio-$v"
  printf '#!/bin/sh\necho "colophon-server %s"\n' "$v" >"$d/x/colophon-server"
  # The CLI answers migrate-status with $STATUS_JSON (empty: no answer, like a database it can't reach).
  cat >"$d/x/colophon" <<'CLI'
#!/bin/sh
if [ "$1 $2" = "admin migrate-status" ]; then
  [ -n "$STATUS_JSON" ] || exit 1
  echo "$STATUS_JSON"
fi
exit 0
CLI
  chmod +x "$d/x/colophon-server" "$d/x/colophon"; echo '<html></html>' >"$d/x/colophon-studio-$v/index.html"
  tar -czf "$d/colophon-server-$v-$arch-unknown-linux-musl.tar.gz" -C "$d/x" colophon-server
  tar -czf "$d/colophon-cli-$v-$arch-unknown-linux-musl.tar.gz" -C "$d/x" colophon
  tar -czf "$d/colophon-studio-$v.tar.gz" -C "$d/x" "colophon-studio-$v"
  rm -rf "$d/x"
  (cd "$d" && for s in ./*.tar.gz; do sha256sum "${s#./}"; done >SHA256SUMS)
  jq -n --arg wf "$wf" --arg ref "$ref" --arg repo "$repo" \
    --argjson s "$(cd "$d" && sha256sum ./*.tar.gz | cut -d' ' -f1 | jq -R . | jq -s .)" \
    '{subjects: $s, repo: $repo, workflow: $wf, ref: $ref}' >"$d/colophon-$v.intoto.jsonl"
}
# run <case> [args...] (env through the caller): upgrade.sh against $work/<case>.
run() {
  local c=$1; shift
  env PATH="${RUN_PATH:-$work/bin:$PATH}" COLOPHON_UPGRADE_TEST=1 COLOPHON_ROOT="$work/$c/root" COLOPHON_RELEASE_BASE="$work/$c" \
    "$here/upgrade.sh" "$@" >"$work/$c.out" 2>&1
}
here=$PWD
unpacked() { [[ -x "$work/$1/root/releases/$v/bin/colophon-server" ]]; }

build "$work/good"
run good --fetch "$v" || fail "a good release was refused: $(cat "$work/good.out")"
unpacked good || fail "not unpacked: $(cat "$work/good.out")"
grep -q "provenance verified: release.yml on refs/tags/v$v" "$work/good.out" || fail "no provenance line: $(cat "$work/good.out")"
echo "  ok    a release whose bundle attests each archive unpacks"

refused() { # <case> <expected text> <what>
  if run "$1" --fetch "$v"; then fail "$3 was accepted"; fi
  grep -q "$2" "$work/$1.out" || fail "$3: $(cat "$work/$1.out")"
  ! unpacked "$1" || fail "$3 left a release unpacked"
}
build "$work/nobundle"; rm "$work/nobundle/v$v/colophon-$v.intoto.jsonl"
refused nobundle 'no provenance bundle' "a release without a bundle"
build "$work/othertag" "" refs/tags/v0.0.1
refused othertag "provenance doesn't verify (not release.yml on refs/tags/v$v" "a bundle for another tag"
build "$work/otherwf" "$repo/.github/workflows/edge.yml"
refused otherwf "provenance doesn't verify" "a bundle from another workflow"
# An archive replaced along with SHA256SUMS: the checksums agree, the provenance doesn't.
build "$work/swapped"
d=$work/swapped/v$v
mkdir -p "$d/y" && printf '#!/bin/sh\necho evil\n' >"$d/y/colophon" && chmod +x "$d/y/colophon"
tar -czf "$d/colophon-cli-$v-$arch-unknown-linux-musl.tar.gz" -C "$d/y" colophon && rm -rf "$d/y"
(cd "$d" && for s in ./*.tar.gz; do sha256sum "${s#./}"; done >SHA256SUMS)
refused swapped "colophon-cli-$v-$arch-unknown-linux-musl.tar.gz's provenance doesn't verify" "an archive replaced with SHA256SUMS"
echo "  ok    no bundle, another tag, another workflow, and an archive replaced with its checksum each refuse; nothing unpacked"

mkdir -p "$work/nogh"
for t in jq tar gzip mktemp sha256sum shasum date uname cut grep sed tr rm mv ln mkdir chmod cat cp basename dirname readlink ls tail head env sh bash; do
  p=$(PATH="$work/bin:$PATH" command -v "$t" 2>/dev/null) && ln -sf "$p" "$work/nogh/$t"
done
build "$work/noghcase"
if RUN_PATH="$work/nogh" run noghcase --fetch "$v"; then fail "a release was taken without gh to check it"; fi
grep -q "gh isn't installed" "$work/noghcase.out" || fail "no gh: $(cat "$work/noghcase.out")"
! unpacked noghcase || fail "no gh: unpacked"
echo "  ok    without gh, nothing is taken"

build "$work/rehearsal"; rm "$work/rehearsal/v$v/colophon-$v.intoto.jsonl"
COLOPHON_RELEASE_UNVERIFIED=rehearsal run rehearsal --fetch "$v" || fail "the rehearsal opt-out was refused for a local release: $(cat "$work/rehearsal.out")"
unpacked rehearsal || fail "the rehearsal wasn't unpacked"
grep -q 'WARNING: PROVENANCE NOT CHECKED' "$work/rehearsal.out" || fail "the opt-out wasn't said loudly: $(cat "$work/rehearsal.out")"
if env PATH="$work/bin:$PATH" COLOPHON_UPGRADE_TEST=1 COLOPHON_ROOT="$work/gh/root" COLOPHON_RELEASE_BASE=https://github.com/$repo/releases/download \
  COLOPHON_RELEASE_UNVERIFIED=rehearsal ./upgrade.sh --fetch "$v" >"$work/gh.out" 2>&1; then
  fail "the opt-out was taken for a release from GitHub"
fi
grep -q 'for a local rehearsal directory only' "$work/gh.out" || fail "opt-out from GitHub: $(cat "$work/gh.out")"
echo "  ok    the rehearsal opt-out: a local release only, said loudly; from GitHub, refused before any download"

# A release already unpacked (a droplet re-running install.sh on the version it runs, whose release
# predates the bundle) is neither fetched again nor refused: nothing new is installed.
mkdir -p "$work/present/root/releases"
cp -R "$work/good/root/releases/$v" "$work/present/root/releases/$v"
mkdir -p "$work/present/v$v"   # the release source is empty: any fetch would fail
run present --fetch "$v" || fail "an unpacked release was refused or refetched: $(cat "$work/present.out")"
grep -q "$v is already unpacked" "$work/present.out" || fail "present: $(cat "$work/present.out")"
if grep -q 'fetching' "$work/present.out"; then fail "an unpacked release was fetched again"; fi
echo "  ok    a release already unpacked is neither fetched again nor refused (install.sh re-run on its own version)"

# The switch's schema gate (it refuses before migrating, so nothing here touches systemd).
mkdir -p "$work/creds" && printf 'postgres://x:y@db/colophon?sslmode=verify-full' >"$work/creds/migration_database_url"
build "$work/nostatus"
if COLOPHON_CREDENTIALS_DIR="$work/creds" STATUS_JSON='' run nostatus "$v"; then fail "a switch went on without a migration status"; fi
grep -q "can't read the database's migration status" "$work/nostatus.out" || fail "no status: $(cat "$work/nostatus.out")"
build "$work/unknown"
if COLOPHON_CREDENTIALS_DIR="$work/creds" STATUS_JSON='{"data":{"status":"unknown","unknown":["0099_x"]}}' run unknown "$v"; then fail "a switch went on over unknown migrations"; fi
grep -q "migrations v$v doesn't know (0099_x)" "$work/unknown.out" || fail "unknown: $(cat "$work/unknown.out")"
for c in nostatus unknown; do [[ ! -e "$work/$c/root/current" ]] || fail "$c switched"; done
echo "  ok    a switch refuses when the migration status can't be read, or names migrations it doesn't know"
echo "upgrade (release channel): only archives release.yml built for the tag are installed; the rehearsal opt-out is local and loud; the schema gate fails closed"
