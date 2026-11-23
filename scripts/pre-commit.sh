#!/usr/bin/env bash
# The pre-commit hook (D-241; installed by scripts/install-hooks.sh): fast, and only what the staged
# files touch. Every commit: the leak check and, where this checkout asks for one, the identity.
# Rust: rustfmt; clippy and the unit tests of the changed crates (no stack, nothing ignored).
# Frontend: each changed package's type check and unit tests. Scripts: ShellCheck on the changed
# ones. A docs-only commit (docs/, *.md, perf/results/) runs the leak check alone. Everything
# heavier is the pre-push hook's (scripts/pre-push.sh) and CI's.
# Bash 3.2 (macOS /bin/bash): no mapfile, and no -u (an empty array would be "unbound").
set -o pipefail
cd "$(git rev-parse --show-toplevel)" || exit 1
hook_name=pre-commit
# shellcheck source=scripts/hook-lib.sh
. scripts/hook-lib.sh
hook_skip_if_asked
hook_toolchain

staged=()
while IFS= read -r path; do staged+=("$path"); done < <(git diff --cached --name-only --diff-filter=ACMR)
if (( ${#staged[@]} == 0 )); then
  echo "pre-commit: nothing staged" >&2
  exit 0
fi

diff=$(mktemp)
trap 'rm -f "$diff"' EXIT
git diff --cached -U0 --no-color >"$diff"
step "identity (author and committer)" identity_check
step "leak check (staged lines)" leak_check "$diff"

docs_only=1 crates=() workspace=0 packages=() scripts=() rust=0
for path in "${staged[@]}"; do
  case "$path" in
    docs/* | *.md | perf/results/*) continue ;;
  esac
  docs_only=0
  case "$path" in
    crates/*/*)
      dir=${path#crates/}
      dir=${dir%%/*}
      [[ " ${crates[*]} " == *" $dir "* ]] || crates+=("$dir")
      [[ "$path" == *.rs ]] && rust=1
      ;;
    Cargo.toml | Cargo.lock | rust-toolchain.toml | .cargo/* | clippy.toml | rustfmt.toml)
      workspace=1 rust=1
      ;;
    .sqlx/*)
      [[ " ${crates[*]} " == *" colophon-db "* ]] || crates+=(colophon-db)
      ;;
    apps/*/* | packages/*/*)
      pkg=$(cut -d/ -f1-2 <<<"$path")
      [[ -f "$pkg/package.json" && " ${packages[*]} " != *" $pkg "* ]] && packages+=("$pkg")
      ;;
  esac
  case "$path" in
    *.sh | .githooks/*) [[ -f "$path" ]] && scripts+=("$path") ;;
  esac
done

if (( docs_only == 1 )); then
  echo "pre-commit: docs only; the leak check is the whole hook" >&2
  hook_verdict
  exit 0
fi

# Package names, from each changed crate's own Cargo.toml.
names=()
for dir in "${crates[@]}"; do
  manifest="crates/$dir/Cargo.toml"
  [[ -f "$manifest" ]] || continue
  names+=("$(sed -n 's/^name = "\(.*\)"$/\1/p' "$manifest" | head -1)")
done
selection=()
if (( workspace == 1 )); then
  selection=(--workspace)
else
  for name in "${names[@]}"; do selection+=(-p "$name"); done
fi

if (( rust == 1 )) || (( ${#selection[@]} > 0 )); then
  step "rustfmt" cargo fmt --all --check
fi
if (( ${#selection[@]} > 0 )); then
  step "clippy (${selection[*]})" cargo clippy "${selection[@]}" --all-targets --locked -- -D warnings
  step "unit tests (${selection[*]})" cargo test "${selection[@]}" --lib --locked --quiet
fi

for pkg in "${packages[@]}"; do
  name=$(jq -r .name "$pkg/package.json")
  if jq -e '.scripts.check' "$pkg/package.json" >/dev/null; then
    step "types ($name)" npm run --silent -w "$name" check
  fi
  if jq -e '.scripts.test' "$pkg/package.json" >/dev/null; then
    step "unit tests ($name)" npm run --silent -w "$name" test
  fi
done

if (( ${#scripts[@]} > 0 )); then
  if command -v shellcheck >/dev/null 2>&1; then
    step "shellcheck (${#scripts[@]} file(s))" shellcheck -x "${scripts[@]}"
  else
    echo "pre-commit: shellcheck is not installed; the pre-push check and CI run it" >&2
  fi
fi

hook_verdict
