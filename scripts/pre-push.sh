#!/usr/bin/env bash
# The pre-push hook (D-241; installed by scripts/install-hooks.sh): what CI would otherwise be the
# first to catch. Warns when the branch doesn't contain origin/main as last fetched (no fetch here),
# runs the leak check over everything being pushed, then QUICK=1 scripts/check.sh: every check CI
# runs except the stack's. A push whose new commits touch only docs/, *.md or perf/results/ runs
# the leak check alone. Git hands the refs being pushed on stdin: "<local ref> <local sha> <remote
# ref> <remote sha>" a line.
# Bash 3.2 (macOS /bin/bash): no -u (an empty array would be "unbound").
set -o pipefail
cd "$(git rev-parse --show-toplevel)" || exit 1
hook_name=pre-push
# shellcheck source=scripts/hook-lib.sh
. scripts/hook-lib.sh
hook_skip_if_asked
hook_toolchain

zero=0000000000000000000000000000000000000000
ranges=() heads=()
while read -r _local_ref local_sha _remote_ref remote_sha; do
  [[ "$local_sha" == "$zero" ]] && continue # a deletion pushes nothing
  heads+=("$local_sha")
  if [[ "$remote_sha" == "$zero" ]]; then
    # A new branch: what it adds over origin/main, as last fetched.
    base=$(git merge-base origin/main "$local_sha" 2>/dev/null || true)
    ranges+=("${base:+$base..}$local_sha")
  else
    ranges+=("$remote_sha..$local_sha")
  fi
done
if (( ${#ranges[@]} == 0 )); then
  echo "pre-push: nothing to check (deletions only)" >&2
  exit 0
fi

if git rev-parse -q --verify origin/main >/dev/null && ! git merge-base --is-ancestor origin/main HEAD; then
  echo "pre-push: WARNING: this branch doesn't contain origin/main (as last fetched); merge it before CI does the comparing" >&2
fi

diff=$(mktemp)
trap 'rm -f "$diff"' EXIT
changed=()
for range in "${ranges[@]}"; do
  if [[ "$range" == *..* ]]; then
    git diff -U0 --no-color "$range" >>"$diff"
    while IFS= read -r path; do changed+=("$path"); done < <(git diff --name-only "$range")
  else
    git show -U0 --no-color --format= "$range" >>"$diff"
    while IFS= read -r path; do changed+=("$path"); done < <(git show --name-only --format= "$range")
  fi
done
hook_clean_git_env
step "leak check (everything pushed)" leak_check "$diff"

docs_only=1
for path in "${changed[@]}"; do
  case "$path" in
    docs/* | *.md | perf/results/*) ;;
    *) docs_only=0 ;;
  esac
done
if (( docs_only == 1 )); then
  echo "pre-push: docs only; the leak check is the whole hook" >&2
  hook_verdict
  exit 0
fi

# Git opened its connection to the remote before this hook ran, and a remote drops one left idle
# for the minutes QUICK takes: the push then dies after a green check. So a commit whose QUICK
# passed is remembered, and pushing it again (or a branch at it) skips straight through.
passed="$(git rev-parse --git-common-dir)/colophon-quick-passed"
all_passed=1
for head in "${heads[@]}"; do
  grep -qx "$head" "$passed" 2>/dev/null || all_passed=0
done
if (( all_passed == 1 )); then
  echo "pre-push: QUICK=1 scripts/check.sh already passed for ${heads[*]:0:1}; not run again" >&2
  hook_verdict
  exit 0
fi

# check.sh prints its own progress; it runs in the open, timed as one step.
echo "pre-push: QUICK=1 scripts/check.sh (every CI check but the stack's; several minutes) ..." >&2
quick_started=$(date +%s)
if QUICK=1 scripts/check.sh; then
  echo "pre-push: QUICK=1 scripts/check.sh            ok   ($(( $(date +%s) - quick_started ))s)" >&2
  printf '%s\n' "${heads[@]}" >>"$passed"
  echo "pre-push: if the push now fails with a dropped connection, push again: this commit won't be checked twice" >&2
else
  echo "pre-push: QUICK=1 scripts/check.sh            FAIL ($(( $(date +%s) - quick_started ))s)" >&2
  [[ -n "$hook_failed" ]] || hook_failed="QUICK=1 scripts/check.sh"
fi
hook_verdict
