#!/usr/bin/env bash
# scripts/load.sh's warnings gate (scripts/lib/load-warnings.jq): a slow statement under a second is
# D-221's evidence, judged by the latency limits, so it doesn't fail a run (CI 44632793507 failed four
# smokes whose p99s were 33-124 ms against 500 on their 100-200 ms COMMITs alone). One over a second
# still fails, and so does every other WARN or ERROR. Fails naming the case that broke.
#
#   scripts/test-load-warnings.sh
set -euo pipefail
cd "$(dirname "$0")/.."
lib="$(cat scripts/lib/load-warnings.jq)"
log=$(cat <<'LINES'
{"level":"WARN","target":"sqlx::query","message":"slow statement: execution time exceeded alert threshold","summary":"COMMIT","elapsed_secs":0.217}
{"level":"WARN","target":"sqlx::query","message":"slow statement: execution time exceeded alert threshold","summary":"SELECT","elapsed_secs":0.999}
{"level":"WARN","target":"sqlx::query","message":"slow statement: execution time exceeded alert threshold","summary":"COMMIT","elapsed_secs":1.4}
{"level":"WARN","target":"sqlx::query","message":"slow statement: execution time exceeded alert threshold","summary":"COMMIT"}
{"level":"WARN","target":"colophon_api::error","message":"request refused: no database connection free","code":"unavailable"}
{"level":"ERROR","target":"colophon_api::error","message":"request failed","code":"internal"}
{"level":"WARN","target":"sqlx::pool","message":"slow statement acquire"}
{"level":"INFO","target":"colophon_server","message":"listening"}
not json
LINES
)
fail=0
expect() { if [[ "$2" != "$3" ]]; then echo "FAIL: $1: got $2, want $3" >&2; fail=1; fi; }
# As load.sh counts a whole log (count_logged), and as it splices the predicate mid-pipeline.
counted=$(jq -Rn "[inputs | fromjson? | $lib select(counted)] | length" <<<"$log")
spliced=$(jq -Rsc "[split(\"\\n\")[] | fromjson? | $lib select(counted)] | length" <<<"$log")
slow=$(jq -Rn "[inputs | fromjson? | $lib select(slow_statement)] | length" <<<"$log")
# Counted: the 1.4 s statement, the one with no duration (never excused unmeasured), the refusal,
# the internal error and the pool warning (not a statement). Excused: 0.217 s and 0.999 s.
expect "warnings that fail the run" "$counted" 5
expect "the same, spliced mid-pipeline" "$spliced" 5
expect "slow statements recorded as evidence" "$slow" 4
(( fail == 0 )) && echo "load warnings: slow statements under 1 s are evidence; everything else fails the run"
exit "$fail"
