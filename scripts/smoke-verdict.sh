#!/usr/bin/env bash
# The `smokes` job's verdict (D-209 addendum): every smoke ran, and the outcomes match perf/expected-smoke-failures.tsv.
# Thresholds are untouched; this only decides which outcomes are known debts with an owner.
#
#   scripts/smoke-verdict.sh LIST "smoke smoke ..." "failed failed ..." [METRICS]
#     LIST      the expected list (tab-separated; # comments)
#     second    every smoke that ran; third, the ones that failed
#     METRICS   tab-separated `smoke p99_ms l08_worst_ratio` per smoke that ran (from each run's record)
# Fails, naming each, on:
#   - a failure not on the list;
#   - a strict entry that passed (remove it from the list);
#   - a flapping entry past its CEILING, whatever its outcome, or with no measurement to judge it by;
#   - a listed smoke that didn't run; an entry without its decision, owner, measurements or (flapping) ceiling.
# Prints each listed smoke's outcome: expected failure, pass (flapping), flap, with its margin.
set -euo pipefail
list=$1
read -r -a ran <<<"$2"
failed=()
[[ -n "${3:-}" ]] && read -r -a failed <<<"$3"
metrics=${4:-/dev/null}

# Plain space-delimited sets and awk lookups, so this runs on macOS's bash 3.2 as well as CI's.
in_set() { [[ "$2" == *" $1 "* ]]; }
runs=" ${ran[*]:-} " fails=" ${failed[*]:-} " listed=" "
column() { awk -F'\t' -v s="$1" -v c="$2" '$1 == s && $1 !~ /^#/ {print $c; exit}' "$list"; }
metric() { awk -F'\t' -v s="$1" -v c="$2" '$1 == s {print $c; exit}' "$metrics"; }

problems=0
problem() { echo "::error::$*"; problems=$((problems + 1)); }

while IFS=$'\t' read -r smoke kind decision owner ceiling measured owed; do
  [[ -z "$smoke" || "$smoke" == \#* ]] && continue
  if [[ -z "$decision" || -z "$owner" || -z "$measured" || -z "$owed" ]]; then
    echo "smoke-verdict: $list: '$smoke' needs a decision, an owner, what it measured and what is owed" >&2
    exit 1
  fi
  case "$kind" in
    strict) ;;
    flapping)
      if [[ ! "$ceiling" =~ ^(p99|l08)\<=[0-9]+(\.[0-9]+)?$ ]]; then
        echo "smoke-verdict: $list: flapping '$smoke' needs a ceiling, p99<=MS or l08<=RATIO (has '$ceiling')" >&2
        exit 1
      fi
      if [[ "$measured" != *→* ]]; then
        echo "smoke-verdict: $list: flapping '$smoke' needs both runs' measurements (a → b), has '$measured'" >&2
        exit 1
      fi
      ;;
    *) echo "smoke-verdict: $list: '$smoke' is '$kind': strict or flapping" >&2; exit 1 ;;
  esac
  listed+="$smoke "
done <"$list"

for smoke in ${failed[@]+"${failed[@]}"}; do
  in_set "$smoke" "$listed" || problem "smoke $smoke failed and is not on $list"
done
for smoke in $listed; do
  kind=$(column "$smoke" 2)
  why="$(column "$smoke" 3), $(column "$smoke" 4)"
  if ! in_set "$smoke" "$runs"; then
    problem "$list lists $smoke, which did not run: remove it or run it"
    continue
  fi
  outcome=pass
  in_set "$smoke" "$fails" && outcome=fail
  if [[ "$kind" == strict ]]; then
    if [[ "$outcome" == fail ]]; then
      echo "expected failure: $smoke ($why: $(column "$smoke" 6))"
    else
      problem "smoke $smoke PASSED: remove it from $list ($why)"
    fi
    continue
  fi
  # Flapping: either outcome against the normal limit, never past the ceiling.
  ceiling=$(column "$smoke" 5)
  what=${ceiling%%<=*} bound=${ceiling#*<=}
  case "$what" in
    p99) value=$(metric "$smoke" 2) unit=" ms" ;;
    l08) value=$(metric "$smoke" 3) unit="x" ;;
  esac
  if [[ -z "$value" ]]; then
    problem "flapping smoke $smoke has no $what measured this run, so its ceiling $ceiling can't be judged ($why)"
    continue
  fi
  margin=$(awk -v v="$value" -v b="$bound" 'BEGIN {printf "%.0f%% of its ceiling", 100 * v / b}')
  if awk -v v="$value" -v b="$bound" 'BEGIN {exit !(v > b)}'; then
    problem "flapping smoke $smoke is past its CEILING: $what $value$unit > $bound$unit ($margin; $why)"
  elif [[ "$outcome" == fail ]]; then
    echo "flap: $smoke failed its limit, $what $value$unit, under its ceiling $bound$unit ($margin; $why)"
  else
    echo "pass: $smoke, $what $value$unit ($margin; flapping, $why)"
  fi
done

if ((problems > 0)); then
  exit 1
fi
echo "smoke-verdict: ${#ran[@]} ran, ${#failed[@]} failed; every outcome is as the list expects"
