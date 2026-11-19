#!/usr/bin/env bash
# The `smokes` job's verdict (D-209 addendum): every smoke ran, and the outcomes match perf/expected-smoke-failures.tsv.
# Thresholds are untouched; this only decides which outcomes are known debts with an owner.
#
#   scripts/smoke-verdict.sh LIST "smoke smoke ..." "failed failed ..." [METRICS]
#     LIST      the expected list (tab-separated; # comments)
#     second    every smoke that ran; third, the ones that failed
#     METRICS   tab-separated `smoke p99_ms l08_worst_ratio hard_failures detail` per smoke that ran (from each run's
#               record); hard_failures counts failed checks, real request errors and (assets) refused uploads
# Fails, naming each, on:
#   - any smoke with hard failures, or none measured: correctness is never tolerated, listed or not, flapping or not;
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
# Which job's entries this run judges (D-221): the smokes job's are plain names (`assets`); another job's carry its
# scope (`integration/members-steady`, with SMOKE_SCOPE=integration). Entries outside the scope are not this run's.
scope=${SMOKE_SCOPE:-}
in_scope() {
  if [[ -z "$scope" ]]; then [[ "$1" != */* ]]; else [[ "$1" == "$scope"/* ]]; fi
}
# Where a temporary ceiling's decision is looked up: its row there means its fix merged and the tolerance expired.
decisions=${SMOKE_DECISIONS:-docs/DECISIONS.md}

# Plain space-delimited sets and awk lookups, so this runs on macOS's bash 3.2 as well as CI's.
in_set() { [[ "$2" == *" $1 "* ]]; }
runs=" ${ran[*]:-} " fails=" ${failed[*]:-} " listed=" "
column() { awk -F'\t' -v s="$1" -v c="$2" '$1 == s && $1 !~ /^#/ {print $c; exit}' "$list"; }
metric() { awk -F'\t' -v s="$1" -v c="$2" '$1 == s {print $c; exit}' "$metrics"; }

problems=0
problem() { echo "::error::$*"; problems=$((problems + 1)); }

while IFS=$'\t' read -r smoke kind decision owner ceiling measured owed; do
  [[ -z "$smoke" || "$smoke" == \#* ]] && continue
  in_scope "$smoke" || continue
  if [[ -z "$decision" || -z "$owner" || -z "$measured" || -z "$owed" ]]; then
    echo "smoke-verdict: $list: '$smoke' needs a decision, an owner, what it measured and what is owed" >&2
    exit 1
  fi
  case "$kind" in
    strict) ;;
    flapping)
      if [[ ! "$ceiling" =~ ^(p99|l08)\<=[0-9]+(\.[0-9]+)?( EXPIRES-WITH D-[0-9]+[a-z]?)?$ ]]; then
        echo "smoke-verdict: $list: flapping '$smoke' needs a ceiling, p99<=MS or l08<=RATIO [EXPIRES-WITH D-NNN] (has '$ceiling')" >&2
        exit 1
      fi
      if [[ "$ceiling" == *" EXPIRES-WITH "* ]]; then
        reason=${ceiling##* }
        # A decision is done once its row exists and no longer says `Open:` (D-221's row exists while it is open).
        if grep "^| $reason |" "$decisions" | grep -qv "Open:"; then
          echo "smoke-verdict: $list: '$smoke''s temporary ceiling expired with $reason, which is done ($decisions has its row, no longer Open): remove the entry, or restore it as an ordinary flapper at 2x its limit" >&2
          exit 1
        fi
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

# Correctness first, for every smoke that ran: neither a list entry nor a ceiling can absorb a hard failure.
for smoke in ${ran[@]+"${ran[@]}"}; do
  hard=$(metric "$smoke" 4)
  if [[ -z "$hard" ]]; then
    problem "smoke $smoke has no hard-failure count measured: correctness can't be judged"
  elif [[ "$hard" != 0 ]]; then
    problem "smoke $smoke has hard failures ($hard: $(metric "$smoke" 5)): correctness is never tolerated, listed or not, flapping or not"
  fi
done
# Below half the reference speed (D-221, SMOKE_LATENCY_JUDGED=no) latency is not judged on this runner: a too-slow
# runner can't fail us on latency, and can't hide errors either, so only the hard failures above count.
if [[ "${SMOKE_LATENCY_JUDGED:-yes}" == no ]]; then
  echo "latency not judged on this runner (under half the reference speed, D-221): only hard failures count"
  if ((problems > 0)); then
    exit 1
  fi
  echo "smoke-verdict: ${#ran[@]} ran; no hard failures; latency not judged on this runner"
  exit 0
fi
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
  bound=${bound%% *}
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
