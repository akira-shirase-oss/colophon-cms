#!/usr/bin/env bash
# Colophon's micro-benchmarks (crates/colophon-bench, D-074): Criterion timings of the CPU work a
# request does, on one machine. They are evidence about the code, never a capacity claim.
#
#   scripts/bench.sh                  run every benchmark on this checkout and save the medians in
#                                     perf/results/<date>-bench-<machine>.json
#   scripts/bench.sh compare <base>   build <base> and this checkout, run both on this machine in
#                                     alternating rounds, and fail if a benchmark is slower here
#
# Machines differ too much to compare across them (CI's runners differ 2.3x by CPU model alone,
# docs/modules/17-verification.md), so a comparison is always two builds timed on one machine in
# one go. Each round runs both builds back to back, alternating which goes first, so a slower
# stretch of time (a noisy neighbour, a warmer chip) lands on both. A benchmark regresses only
# when this checkout is slower in EVERY round by more than THRESHOLD_PCT percent and by more than
# MIN_DELTA_NS nanoseconds: noise rarely hits every round the same way, and a few nanoseconds of
# code-layout luck on a 50 ns benchmark isn't worth failing a pull request over.
#
# Environment overrides:
#   WARM_UP_S, MEASURE_S  Criterion's warm-up and measurement seconds per benchmark
#                         (defaults: 3 and 5 for a run, 1 and 3 for each round of a comparison)
#   ROUNDS                rounds in a comparison (default 3)
#   THRESHOLD_PCT         slower than this in every round is a regression (default 15)
#   MIN_DELTA_NS          ...and by more than this many nanoseconds (default 25)
#   FILTER                a Criterion filter (a regex on ids such as entry/validate_page)
#   BENCH_OUT             where a comparison writes report.md, report.json and Criterion's data
#                         (default target/bench-compare)
#   KEEP=0                a run doesn't write its result file
set -euo pipefail
cd "$(dirname "$0")/.."

mode="${1:-run}"
if [[ "$mode" != run && "$mode" != compare ]] || [[ "$mode" == compare && -z "${2:-}" ]]; then
  echo "usage: scripts/bench.sh [run] | scripts/bench.sh compare <base commit or ref>" >&2
  exit 2
fi
for tool in cargo git jq; do
  command -v "$tool" >/dev/null || { echo "error: $tool is required" >&2; exit 1; }
done

if [[ "$mode" == run ]]; then
  warm_up="${WARM_UP_S:-3}" measure="${MEASURE_S:-5}"
else
  warm_up="${WARM_UP_S:-1}" measure="${MEASURE_S:-3}"
fi
rounds="${ROUNDS:-3}" threshold="${THRESHOLD_PCT:-15}" floor="${MIN_DELTA_NS:-25}"
for number in "$warm_up" "$measure" "$rounds" "$threshold" "$floor"; do
  [[ "$number" =~ ^[0-9]+$ ]] || { echo "error: WARM_UP_S, MEASURE_S, ROUNDS, THRESHOLD_PCT and MIN_DELTA_NS are whole numbers" >&2; exit 2; }
done
(( rounds >= 1 && measure >= 1 )) || { echo "error: ROUNDS and MEASURE_S are at least 1" >&2; exit 2; }

# Builds the benchmarks of the checkout in $1 (sharing this checkout's target directory, so
# dependencies build once) and prints the path of each benchmark executable.
build_benches() {
  (cd "$1" && CARGO_TARGET_DIR="$target_dir" cargo bench -p colophon-bench --locked --no-run \
    --message-format=json-render-diagnostics) |
    jq -r 'select(.reason == "compiler-artifact" and (.target.kind | index("bench")) and .executable != null) | .executable'
}

# Runs one benchmark executable, saving its estimates under the Criterion baseline $2.
run_bench() {
  local executable="$1" baseline="$2" log="$3"
  local args=(--bench --noplot --warm-up-time "$warm_up" --measurement-time "$measure" --save-baseline "$baseline")
  if [[ -n "${FILTER:-}" ]]; then args+=("$FILTER"); fi
  if ! CRITERION_HOME="$criterion_home" "$executable" "${args[@]}" >"$log" 2>&1; then
    tail -40 "$log" >&2
    echo "error: $(basename "$executable") failed (log: $log)" >&2
    exit 1
  fi
}

# One JSON line per benchmark of the Criterion baseline $1: its id, median and the median's 95%
# confidence interval, in nanoseconds per iteration.
collect() {
  find "$criterion_home" -type f -path "*/$1/benchmark.json" | sort | while IFS= read -r file; do
    jq -c --slurpfile estimates "$(dirname "$file")/estimates.json" \
      '$estimates[0].median as $m | {id: .full_id, median_ns: $m.point_estimate, low_ns: $m.confidence_interval.lower_bound, high_ns: $m.confidence_interval.upper_bound}' \
      "$file"
  done
}

load_average() {
  if [[ "$(uname -s)" == Darwin ]]; then sysctl -n vm.loadavg | awk '{print $2}'; else cut -d' ' -f1 /proc/loadavg; fi
}

# jq: nanoseconds as people read them.
JQ_TIME='def t: if . >= 1e6 then "\(. / 1e4 | round / 100) ms" elif . >= 1e3 then "\(. / 10 | round / 100) µs" else "\(. * 100 | round / 100) ns" end;'

target_dir="${CARGO_TARGET_DIR:-$PWD/target}"
commit=$(git rev-parse --short HEAD)
dirty=$([[ -n "$(git status --porcelain -- crates Cargo.toml Cargo.lock)" ]] && echo true || echo false)

if [[ "$mode" == run ]]; then
  work=$(mktemp -d)
  trap 'rm -rf "$work"' EXIT
  criterion_home="$work/criterion"
  echo "building the benchmarks (bench profile)"
  executables=$(build_benches .)
  [[ -n "$executables" ]] || { echo "error: no benchmark executables were built" >&2; exit 1; }
  load_start=$(load_average)
  while IFS= read -r executable; do
    echo "running $(basename "$executable"): ${warm_up} s warm-up, ${measure} s per benchmark"
    run_bench "$executable" run "$work/$(basename "$executable").log"
  done <<<"$executables"
  load_end=$(load_average)
  benchmarks=$(collect run | jq -s .)

  if [[ "$(uname -s)" == Darwin ]]; then
    cpu=$(sysctl -n machdep.cpu.brand_string)
    cores=$(sysctl -n hw.ncpu)
    memory_gb=$(( $(sysctl -n hw.memsize) / 1024 / 1024 / 1024 ))
  else
    cpu=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//')
    cores=$(nproc)
    memory_gb=$(( $(grep MemTotal /proc/meminfo | awk '{print $2}') / 1024 / 1024 ))
  fi
  machine=$(printf '%s-%sc' "$cpu" "$cores" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '-' | sed 's/-$//')
  stamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  criterion_version=$(awk '/^name = "criterion"$/ { getline; gsub(/"/, "", $3); print $3; exit }' Cargo.lock)
  record=$(jq -n \
    --arg stamp "$stamp" --arg commit "$commit" --argjson dirty "$dirty" \
    --arg cpu "$cpu" --argjson cores "$cores" --argjson memory_gb "$memory_gb" --arg machine "$machine" \
    --arg os "$(uname -s) $(uname -r)" --arg rustc "$(rustc --version)" --arg criterion "$criterion_version" \
    --argjson warm_up "$warm_up" --argjson measure "$measure" --arg filter "${FILTER:-}" \
    --argjson load_start "$load_start" --argjson load_end "$load_end" --argjson benchmarks "$benchmarks" \
    '{
      kind: "micro-benchmarks",
      note: "Criterion timings of single functions on one machine (crates/colophon-bench): evidence about the code, not a capacity claim",
      date: $stamp, commit: $commit, dirty: $dirty,
      machine: { id: $machine, cpu: $cpu, cores: $cores, memory_gb: $memory_gb, os: $os },
      rustc: $rustc, criterion: $criterion, profile: "bench (the release defaults)",
      params: { warm_up_s: $warm_up, measurement_s: $measure, filter: $filter },
      load_average_1m: { start: $load_start, end: $load_end },
      benchmarks: $benchmarks
    }')
  echo
  echo "$record" | jq -r "$JQ_TIME"'.benchmarks[] | "\(.id)\t\(.median_ns | t)\t(\(.low_ns | t) to \(.high_ns | t))"' | column -t -s $'\t'
  echo "load average (1 min): $load_start at the start, $load_end at the end"
  if [[ "${KEEP:-1}" == "1" ]]; then
    mkdir -p perf/results
    file="perf/results/${stamp//:/}-bench-$machine.json"
    echo "$record" | jq . >"$file"
    echo "saved $file"
  fi
  exit 0
fi

# compare <base>
base_sha=$(git rev-parse --verify --quiet "$2^{commit}") || { echo "error: $2 is not a commit here" >&2; exit 2; }
out="${BENCH_OUT:-$target_dir/bench-compare}"
mkdir -p "$out"
# Only what this script writes is cleared, never the directory itself.
rm -rf "${out:?}/bin" "${out:?}/logs" "${out:?}/criterion" "${out:?}/runs.jsonl" "${out:?}/report.json" "${out:?}/report.md"
mkdir -p "$out/bin" "$out/logs"
criterion_home="$out/criterion"
worktree=$(mktemp -d)
cleanup() {
  git worktree remove --force "$worktree/base" 2>/dev/null || true
  rm -rf "$worktree"
}
trap cleanup EXIT

# Copies the executables listed in $2 to $out/bin/<side>-<name>, without Cargo's hash.
keep() {
  local side="$1" executable
  while IFS= read -r executable; do
    cp "$executable" "$out/bin/$side-$(basename "$executable" | sed 's/-[0-9a-f]*$//')"
  done <<<"$2"
}

echo "base: $(git log -1 --format='%h %s' "$base_sha")"
echo "head: $commit$([[ "$dirty" == true ]] && echo ' with uncommitted changes')"
echo "building this checkout's benchmarks"
head_executables=$(build_benches .)
[[ -n "$head_executables" ]] || { echo "error: no benchmark executables were built" >&2; exit 1; }
keep head "$head_executables"

git worktree add --quiet --detach "$worktree/base" "$base_sha"
if [[ ! -f "$worktree/base/crates/colophon-bench/Cargo.toml" ]]; then
  message="The base ($(git rev-parse --short "$base_sha")) has no benchmarks to compare with; nothing was compared."
  echo "$message" | tee "$out/report.md"
  if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then echo "$message" >>"$GITHUB_STEP_SUMMARY"; fi
  exit 0
fi
echo "building the base's benchmarks"
base_executables=$(build_benches "$worktree/base")
[[ -n "$base_executables" ]] || { echo "error: the base built no benchmark executables" >&2; exit 1; }
keep base "$base_executables"

load_start=$(load_average)
for round in $(seq 1 "$rounds"); do
  # Odd rounds run the base first, even rounds this checkout first.
  if (( round % 2 == 1 )); then order=(base head); else order=(head base); fi
  for side in "${order[@]}"; do
    for executable in "$out/bin/$side"-*; do
      name=$(basename "$executable")
      echo "round $round/$rounds: $name"
      run_bench "$executable" "$side-$round" "$out/logs/$name-$round.log"
    done
  done
done
load_end=$(load_average)

for side in base head; do
  for round in $(seq 1 "$rounds"); do
    collect "$side-$round" | jq -c --arg side "$side" --argjson round "$round" '. + {side: $side, round: $round}'
  done
done >"$out/runs.jsonl"

jq -s --argjson t "$threshold" --argjson floor "$floor" '
  def median: sort | .[(length - 1) / 2 | floor];
  [ group_by(.id)[] |
    { id: .[0].id,
      base: (map(select(.side == "base")) | sort_by(.round) | map(.median_ns)),
      head: (map(select(.side == "head")) | sort_by(.round) | map(.median_ns)) } |
    if (.base | length) == 0 then . + {verdict: "new"}
    elif (.head | length) == 0 then . + {verdict: "removed"}
    else
      ([.base, .head] | transpose | map(.[1] / .[0])) as $ratios |
      ([.base, .head] | transpose | map(.[1] - .[0])) as $deltas |
      . + { base_ns: (.base | median), head_ns: (.head | median),
            change_pct: ((($ratios | median) - 1) * 100),
            least_slowdown_pct: ((($ratios | min) - 1) * 100),
            verdict: (if ($ratios | min) > (1 + $t / 100) and ($deltas | min) > $floor then "regression"
                      elif ($ratios | max) < (1 - $t / 100) then "improvement"
                      else "unchanged" end) }
    end ]' "$out/runs.jsonl" >"$out/report.json"

{
  echo "### Micro-benchmarks: $commit against $(git rev-parse --short "$base_sha")"
  echo
  echo "Both builds ran on this machine ($(uname -sm)), in $rounds alternating rounds of ${measure} s per benchmark. A regression is slower in every round by more than ${threshold}% and ${floor} ns. Load average (1 min): $load_start at the start, $load_end at the end."
  echo
  echo "| Benchmark | Base (median) | Head (median) | Change (median of rounds) | Verdict |"
  echo "|---|---:|---:|---:|---|"
  jq -r "$JQ_TIME"'.[] |
    if .verdict == "new" then "| `\(.id)` | | \(.head | sort | .[(length - 1) / 2 | floor] | t) | | new |"
    elif .verdict == "removed" then "| `\(.id)` | \(.base | sort | .[(length - 1) / 2 | floor] | t) | | | removed |"
    else "| `\(.id)` | \(.base_ns | t) | \(.head_ns | t) | \(if .change_pct >= 0 then "+" else "" end)\(.change_pct * 10 | round / 10)% | \(if .verdict == "regression" then "**regression**" else .verdict end) |"
    end' "$out/report.json"
} >"$out/report.md"
cat "$out/report.md"
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then cat "$out/report.md" >>"$GITHUB_STEP_SUMMARY"; fi

regressions=$(jq '[.[] | select(.verdict == "regression")] | length' "$out/report.json")
if (( regressions > 0 )); then
  echo "FAIL: $regressions benchmark(s) slower than the base in every round (report: $out/report.md)" >&2
  exit 1
fi
echo "no regression against the base (report: $out/report.md)"
