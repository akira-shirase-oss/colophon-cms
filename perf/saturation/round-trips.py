"""D-223: per-endpoint statement counts from a Postgres log written by perf/saturation/round-trips.sh.

Statements between two `rt-marker:<endpoint>:<n>` markers belong to that request. They're grouped by transaction
(pid and virtual transaction id, `%v`), background work is left out, and per endpoint it prints the median over
the repeats of: statements (each a sequential round trip on a remote database), transactions, and D-217's release
checks (one statement after each transaction, which D-226 samples once armed).
"""
import collections
import re
import statistics
import sys

LINE = re.compile(r"^\S+ \S+ UTC \[(\d+)\] (\S+) LOG:  (?:statement|execute [^:]*): (.*)$")
MARKER = re.compile(r"rt-marker:([a-z-]+):(\d+)")
# The worker, the change-hint drain and the sweeps poll on connections of their own: not any request's cost.
BACKGROUND = ("change_hints", "WITH due AS", "bulk_jobs", "scheduled_by", "colophon_payload_tick", "budget_reservations",
              "FROM jobs", "asset_jobs", "WITH closing AS", "WITH stale AS", "deltas AS", "pg_listening_channels", "LISTEN ")
CHECK = "now() = statement_timestamp()"

current = None
per_request = collections.defaultdict(lambda: collections.defaultdict(list))  # (endpoint, n) -> (pid, vx) -> statements
for raw in open(sys.argv[1], errors="replace"):
    m = LINE.match(raw.rstrip("\n"))
    if not m:
        continue
    pid, vx, sql = m.groups()
    marker = MARKER.search(sql)
    if marker:
        current = None if marker.group(1) == "end" else (marker.group(1), marker.group(2))
        continue
    if current:
        per_request[current][(pid, vx)].append(sql)

rows = collections.defaultdict(lambda: {"statements": [], "transactions": [], "checks": [], "shape": None})
for (endpoint, _), groups in per_request.items():
    statements = transactions = checks = 0
    shape = []
    for sqls in groups.values():
        if any(b in s for s in sqls for b in BACKGROUND):
            continue
        if all(CHECK in s for s in sqls):
            checks += len(sqls)
            continue
        statements += len(sqls)
        transactions += 1
        shape.append(" ; ".join(re.sub(r"\s+", " ", s)[:40] for s in sqls))
    rows[endpoint]["statements"].append(statements + checks)
    rows[endpoint]["transactions"].append(transactions)
    rows[endpoint]["checks"].append(checks)
    rows[endpoint]["shape"] = rows[endpoint]["shape"] or shape

print("endpoint\tround trips\ttransactions\trelease checks\tof which the request's own work (first repeat)")
for endpoint, r in rows.items():
    med = lambda xs: statistics.median(xs) if xs else 0
    print(f"{endpoint}\t{med(r['statements']):g}\t{med(r['transactions']):g}\t{med(r['checks']):g}\t{' || '.join(r['shape'])[:900]}")
