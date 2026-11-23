# Which server log lines fail a load run (scripts/load.sh's warnings gate). D-221's evidence is not a
# failure while it stays under a second: sqlx's "slow statement" WARN, at the 100 ms threshold CI sets
# to name stalls, is judged by the latency limits and counted apart in the record (slow_statements).
# A statement over a second under steady load still fails, and so does every other WARN or ERROR.
# D-224 split them: INFO from 100 ms, as sqlx offers a statement (no "slow statement" message, only its
# elapsed_secs), and WARN from 1 s with the message. The excuse below stays for a server whose tiers are
# lowered (COLOPHON_DB_SLOW_STATEMENT_MS under 1000).
def slow_statement: .target == "sqlx::query" and (.elapsed_secs != null or ((.message // "") | startswith("slow statement")));
# A statement's duration in seconds. sqlx logs elapsed_secs as a string ("1.410548621"); one that won't parse is null.
def elapsed: .elapsed_secs | tonumber? // null;
# Evidence only when its duration parses and is under a second: a missing or malformed one counts as slow.
def evidence: slow_statement and ((.elapsed_secs | tonumber? // 1e9) < 1);
# The record's slow_statements, from a list of `elapsed`: how many, and the slowest by value (not by text).
def slow_summary: {count: length, max_ms: ((map(select(. != null)) | max // 0) * 1000 | round)};
def counted: (.level == "WARN" or .level == "ERROR") and (evidence | not);
