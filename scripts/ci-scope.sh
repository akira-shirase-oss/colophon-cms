#!/usr/bin/env bash
# What a CI run needs (D-241), as GitHub step outputs: full, code, rust and shards (the integration
# shards, as JSON). Run by ci.yml's `changes` job; also runnable by hand to see what a range would
# run: scripts/ci-scope.sh prints the outputs when GITHUB_OUTPUT isn't set.
#
#   full    every job: the schedule, workflow_dispatch, or release.yml's call (FULL_INPUT=true)
#   code    anything beyond docs/, *.md and perf/results/ changed: the test jobs run
#   rust    code, and not only the Studio, the testbed or their UI packages: the Rust jobs run
#   shards  ["server-1","server-2","workspace","studio-1","studio-2"], the Studio's alone for a frontend-only
#           change, [] for docs alone, and "smokes" added when full
#
# The change is measured from the pull request's base, the push's previous head, or (a new branch,
# a force push) where the branch left origin/main; when none of those can be found, everything runs.
# Environment: EVENT, FULL_INPUT, PR_BASE, BEFORE (ci.yml passes GitHub's).
set -euo pipefail
cd "$(dirname "$0")/.."

out() {
  if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    echo "$1=$2" >>"$GITHUB_OUTPUT"
  fi
  echo "$1=$2"
}

everything() {
  out full "$1"
  out code true
  out rust true
  if [[ "$1" == true ]]; then
    out shards '["server-1","server-2","workspace","studio-1","studio-2","smokes"]'
  else
    out shards '["server-1","server-2","workspace","studio-1","studio-2"]'
  fi
  exit 0
}

event=${EVENT:-push}
if [[ "$event" == schedule || "${FULL_INPUT:-}" == true ]]; then
  echo "a full run ($event)"
  everything true
fi

zero=0000000000000000000000000000000000000000
base=""
if [[ "$event" == pull_request && -n "${PR_BASE:-}" ]]; then
  base=$PR_BASE
elif [[ -n "${BEFORE:-}" && "${BEFORE}" != "$zero" ]] && git cat-file -e "${BEFORE}^{commit}" 2>/dev/null; then
  base=$BEFORE
else
  base=$(git merge-base origin/main HEAD 2>/dev/null || true)
fi
if [[ -z "$base" ]]; then
  echo "no base to compare with: everything runs"
  everything false
fi

files=$(git diff --name-only "$base" HEAD)
echo "changed since ${base:0:12}: $(wc -l <<<"$files" | tr -d ' ') file(s)"
code=false rust=false
while IFS= read -r path; do
  [[ -z "$path" ]] && continue
  if [[ "$path" =~ ^(docs/|perf/results/) || "$path" == *.md ]]; then
    continue
  fi
  code=true
  # Only the Studio, the testbed and the UI packages they build on: the Rust jobs can't see it.
  if [[ ! "$path" =~ ^(apps/studio/|apps/testbed/|packages/ui/|packages/editor/) ]]; then
    rust=true
  fi
done <<<"$files"

out full false
out code "$code"
out rust "$rust"
if [[ "$rust" == true ]]; then
  out shards '["server-1","server-2","workspace","studio-1","studio-2"]'
elif [[ "$code" == true ]]; then
  out shards '["studio-1","studio-2"]'
else
  out shards '[]'
fi
