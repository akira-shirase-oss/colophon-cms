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
{"level":"INFO","target":"sqlx::query","summary":"COMMIT","rows_affected":0,"elapsed_secs":0.15}
{"level":"WARN","target":"colophon_api::error","message":"request refused: no database connection free","code":"unavailable"}
{"level":"ERROR","target":"colophon_api::error","message":"request failed","code":"internal"}
{"level":"WARN","target":"sqlx::pool","message":"slow statement acquire"}
{"level":"INFO","target":"colophon_server","message":"listening"}
not json
LINES
)
# As sqlx really logs them, elapsed_secs is a string (CI 44790031484's assets log: "1.410548621"); the fixtures above
# use numbers. A string under the tier is evidence; over it, a failure; one that won't parse counts as slow, never as
# evidence, so a malformed field can't hide a slow statement.
strings=$(cat <<'LINES'
{"level":"WARN","target":"sqlx::query","message":"slow statement: execution time exceeded alert threshold","summary":"COMMIT","elapsed_secs":"0.4"}
{"level":"WARN","target":"sqlx::query","message":"slow statement: execution time exceeded alert threshold","summary":"COMMIT","elapsed_secs":"10.1"}
{"level":"WARN","target":"sqlx::query","message":"slow statement: execution time exceeded alert threshold","summary":"COMMIT","elapsed_secs":"2.7"}
{"level":"WARN","target":"sqlx::query","message":"slow statement: execution time exceeded alert threshold","summary":"COMMIT","elapsed_secs":"fast"}
{"level":"WARN","target":"sqlx::query","message":"slow statement: execution time exceeded alert threshold","summary":"COMMIT","elapsed_secs":0.5}
LINES
)
fail=0
expect() { if [[ "$2" != "$3" ]]; then echo "FAIL: $1: got $2, want $3" >&2; fail=1; fi; }
# As load.sh counts a whole log (count_logged), and as it splices the predicate mid-pipeline.
counted=$(jq -Rn "[inputs | fromjson? | $lib select(counted)] | length" <<<"$log")
spliced=$(jq -Rsc "[split(\"\\n\")[] | fromjson? | $lib select(counted)] | length" <<<"$log")
slow=$(jq -Rn "[inputs | fromjson? | $lib select(slow_statement)] | length" <<<"$log")
# Counted: the 1.4 s statement, the one with no duration (never excused unmeasured), the refusal,
# the internal error and the pool warning (not a statement). Excused: 0.217 s and 0.999 s. D-224's
# lower tier is INFO, as sqlx offers a statement under the WARN tier: no "slow statement" message,
# only its elapsed_secs. It is evidence, and never counted.
expect "warnings that fail the run" "$counted" 5
expect "the same, spliced mid-pipeline" "$spliced" 5
expect "slow statements recorded as evidence (D-224's INFO tier too)" "$slow" 5
# Counted: "10.1", "2.7" and "fast". Evidence: "0.4" and the number 0.5.
expect "string durations: counted as load.sh counts them" "$(jq -Rn "[inputs | fromjson? | $lib select(counted)] | length" <<<"$strings")" 3
# The record's slowest is by value, not by text ("2.7" sorts after "10.1"), over the durations that parse.
expect "the record's slowest statement" "$(jq -Rn "$lib [inputs | fromjson? | select(slow_statement) | elapsed] | slow_summary | .max_ms" <<<"$strings")" 10100
(( fail == 0 )) && echo "load warnings: slow statements under 1 s are evidence; everything else fails the run"
exit "$fail"
