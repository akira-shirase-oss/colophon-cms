#!/usr/bin/env bash
# Tests upgrade.sh's edge channel (D-211) against a local edge release and a stand-in `gh` that
# verifies a bundle the way the real one is asked to (this repository, edge.yml, refs/heads/main)
# and reports the attested source commit:
#   - a good build unpacks to releases/edge-<12-char commit>;
#   - a tampered archive, a mixed-build EDGE (fetched again once, then refused), a branch-built
#     attestation and a missing bundle each refuse, and nothing is unpacked.
#
#   deploy/droplet/test-upgrade-edge.sh
set -euo pipefail
cd "$(dirname "$0")"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail() {
  echo "FAIL: $*" >&2
  exit 1
}
arch=aarch64
[[ "$(uname -m)" == x86_64 ]] && arch=x86_64
commit=0123456789abcdef0123456789abcdef01234567
other=fedcba9876543210fedcba9876543210fedcba98

# The stand-in gh: `attestation verify FILE --bundle B --repo R --signer-workflow W --source-ref REF`.
# The bundle is a small JSON naming what a real one would attest: its subject's digest, the
# repository, workflow, ref and commit. It answers like gh --format json, or fails like gh.
mkdir -p "$work/bin"
cat >"$work/bin/gh" <<'STUB'
#!/usr/bin/env bash
file=$3; shift 3
while (($#)); do case "$1" in --bundle) bundle=$2; shift 2 ;; --repo) repo=$2; shift 2 ;; --signer-workflow) wf=$2; shift 2 ;; --source-ref) ref=$2; shift 2 ;; *) shift ;; esac; done
[[ -s "${bundle:-}" ]] || exit 1
digest=$(sha256sum "$file" | cut -d' ' -f1)
jq -e --arg d "$digest" --arg r "$repo" --arg w "$wf" --arg ref "$ref" \
  '.digest == $d and .repo == $r and .workflow == $w and .ref == $ref' "$bundle" >/dev/null || exit 1
jq -c '[{verificationResult: {signature: {certificate: {sourceRepositoryDigest: .commit}}}}]' "$bundle"
STUB
chmod +x "$work/bin/gh"
command -v sha256sum >/dev/null || { printf '#!/bin/sh\nexec shasum -a 256 "$@"\n' >"$work/bin/sha256sum"; chmod +x "$work/bin/sha256sum"; }

# build <dir> <edge-commit> <attested-commit> <ref>: an edge release directory.
build() {
  local d=$1 edge=$2 attested=$3 ref=$4 s
  mkdir -p "$d/x/colophon-studio-edge"
  printf '#!/bin/sh\necho "colophon-server edge"\n' >"$d/x/colophon-server"; printf '#!/bin/sh\necho colophon\n' >"$d/x/colophon"
  chmod +x "$d/x/colophon-server" "$d/x/colophon"; echo '<html></html>' >"$d/x/colophon-studio-edge/index.html"
  tar -czf "$d/colophon-server-edge-$arch-unknown-linux-musl.tar.gz" -C "$d/x" colophon-server
  tar -czf "$d/colophon-cli-edge-$arch-unknown-linux-musl.tar.gz" -C "$d/x" colophon
  tar -czf "$d/colophon-studio-edge.tar.gz" -C "$d/x" colophon-studio-edge
  rm -rf "$d/x"; echo "$edge" >"$d/EDGE"
  (cd "$d" && for s in EDGE ./*.tar.gz; do sha256sum "${s#./}"; done >SHA256SUMS)
  jq -n --arg d "$(cd "$d" && sha256sum SHA256SUMS | cut -d' ' -f1)" --arg c "$attested" --arg ref "$ref" \
    '{digest: $d, repo: "akira-shirase-oss/colophon-cms", workflow: "akira-shirase-oss/colophon-cms/.github/workflows/edge.yml", ref: $ref, commit: $c}' \
    >"$d/SHA256SUMS.intoto.jsonl"
}
run() { # <case>: upgrade.sh --channel edge --fetch against $work/<case>/edge
  env PATH="$work/bin:$PATH" COLOPHON_UPGRADE_TEST=1 COLOPHON_ROOT="$work/$1/root" COLOPHON_RELEASE_BASE="$work/$1" \
    ./upgrade.sh --channel edge --fetch >"$work/$1.out" 2>&1
}

build "$work/good/edge" "$commit" "$commit" refs/heads/main
run good || fail "a good edge build was refused: $(cat "$work/good.out")"
[[ -x "$work/good/root/releases/edge-${commit:0:12}/bin/colophon-server" ]] || fail "not unpacked as edge-${commit:0:12}: $(cat "$work/good.out")"

build "$work/tampered/edge" "$commit" "$commit" refs/heads/main
echo 'extra' >>"$work/tampered/edge/colophon-cli-edge-$arch-unknown-linux-musl.tar.gz"
if run tampered; then fail "a tampered archive was accepted"; fi
grep -q "doesn't match SHA256SUMS" "$work/tampered.out" || fail "tampered: $(cat "$work/tampered.out")"

build "$work/mixed/edge" "$other" "$commit" refs/heads/main
if run mixed; then fail "a mixed-build EDGE was accepted"; fi
if ! grep -q 'fetching once more' "$work/mixed.out" || ! grep -q 'two builds mixed' "$work/mixed.out"; then fail "mixed: $(cat "$work/mixed.out")"; fi

build "$work/branch/edge" "$commit" "$commit" refs/heads/feature
if run branch; then fail "a branch-built attestation was accepted"; fi
grep -q "provenance doesn't verify" "$work/branch.out" || fail "branch: $(cat "$work/branch.out")"

build "$work/nobundle/edge" "$commit" "$commit" refs/heads/main
rm "$work/nobundle/edge/SHA256SUMS.intoto.jsonl"
if run nobundle; then fail "a build without a provenance bundle was accepted"; fi
grep -q 'no provenance bundle' "$work/nobundle.out" || fail "no bundle: $(cat "$work/nobundle.out")"

for c in tampered mixed branch nobundle; do
  [[ -z "$(ls "$work/$c/root/releases" 2>/dev/null)" ]] || fail "$c left a release unpacked"
done
echo "upgrade --channel edge: the good build unpacks; tampered, mixed, branch-built and unbundled builds refuse"
