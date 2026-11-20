# Which server log lines fail a load run (scripts/load.sh's warnings gate). D-221's evidence is not a
# failure while it stays under a second: sqlx's "slow statement" WARN, at the 100 ms threshold CI sets
# to name stalls, is judged by the latency limits and counted apart in the record (slow_statements).
# A statement over a second under steady load still fails, and so does every other WARN or ERROR.
# D-224 split them: INFO from 100 ms, as sqlx offers a statement (no "slow statement" message, only its
# elapsed_secs), and WARN from 1 s with the message. The excuse below stays for a server whose tiers are
# lowered (COLOPHON_DB_SLOW_STATEMENT_MS under 1000).
def slow_statement: .target == "sqlx::query" and (.elapsed_secs != null or ((.message // "") | startswith("slow statement")));
def evidence: slow_statement and ((.elapsed_secs // 1e9) < 1);
def counted: (.level == "WARN" or .level == "ERROR") and (evidence | not);
