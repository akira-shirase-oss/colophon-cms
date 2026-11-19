#!/usr/bin/env bash
# The `smokes` job's verdict (D-209 addendum): every smoke ran, and the ones that failed are EXACTLY the ones
# perf/expected-smoke-failures.tsv expects to fail. Thresholds are untouched; this only decides which failures
# are known debts with an owner.
#
#   scripts/smoke-verdict.sh LIST "smoke smoke ..." "failed failed ..."
#     LIST      the expected-failures file (tab-separated; # comments)
#     second    every smoke that ran
#     third     the smokes that failed
# Fails, naming each, on: a failure not on the list; a listed smoke that passed (remove it from the list); a listed
# smoke that didn't run (the list names something that no longer exists).
set -euo pipefail
list=$1
read -r -a ran <<<"$2"
failed=()
[[ -n "${3:-}" ]] && read -r -a failed <<<"$3"

# Plain space-delimited sets, so this runs on macOS's bash 3.2 as well as CI's.
expected=" " runs=" ${ran[*]:-} " fails=" ${failed[*]:-} " notes=""
in_set() { [[ "$2" == *" $1 "* ]]; }
while IFS=$'\t' read -r smoke decision owner measured owed; do
  [[ -z "$smoke" || "$smoke" == \#* ]] && continue
  if [[ -z "$decision" || -z "$owner" || -z "$owed" ]]; then
    echo "smoke-verdict: $list: '$smoke' needs a decision, an owner and what is owed" >&2
    exit 1
  fi
  expected+="$smoke "
  notes+="$smoke"$'\t'"$decision, $owner: $measured"$'\n'
done <"$list"
why() { printf '%s' "$notes" | awk -F'\t' -v s="$1" '$1 == s {print $2; exit}'; }

problems=0
for smoke in ${failed[@]+"${failed[@]}"}; do
  if in_set "$smoke" "$expected"; then
    echo "expected failure: $smoke ($(why "$smoke"))"
  else
    echo "::error::smoke $smoke failed and is not an expected failure in $list"
    problems=$((problems + 1))
  fi
done
for smoke in $expected; do
  if ! in_set "$smoke" "$runs"; then
    echo "::error::$list lists $smoke, which did not run: remove it or run it"
    problems=$((problems + 1))
  elif ! in_set "$smoke" "$fails"; then
    note=$(why "$smoke")
    echo "::error::smoke $smoke PASSED: remove it from $list (${note%%:*})"
    problems=$((problems + 1))
  fi
done
if ((problems > 0)); then
  exit 1
fi
echo "smoke-verdict: ${#ran[@]} ran; ${#failed[@]} failed, every one an expected failure with an owner"
