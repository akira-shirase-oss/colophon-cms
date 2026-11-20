# Which server log lines fail a load run (scripts/load.sh's warnings gate). D-221's evidence is not a
# failure while it stays under a second: sqlx's "slow statement" WARN, at the 100 ms threshold CI sets
# to name stalls, is judged by the latency limits and counted apart in the record (slow_statements).
# A statement over a second under steady load still fails, and so does every other WARN or ERROR.
# D-224 will split these into INFO >= 100 ms and WARN >= 1 s, after which nothing here is excused.
def slow_statement: .target == "sqlx::query" and ((.message // "") | startswith("slow statement"));
def evidence: slow_statement and ((.elapsed_secs // 1e9) < 1);
def counted: (.level == "WARN" or .level == "ERROR") and (evidence | not);
