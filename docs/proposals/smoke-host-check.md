# Proposed, not decided: CI load smokes check the host's flush latency first

**Status:** proposed (2026-11-17), for the maintainer. Not built.

## The problem

The integration job's load smokes gate on p99 (tokens 200 ms, members 200 ms, …). On some GitHub
runners PostgreSQL's WAL flush is slow — the waits during failing runs are `LWLock/WALWrite` and
`IO/WalSync` — and every write-heavy smoke on that host goes slow together, whatever the code:
on one AMD EPYC 9V74 host the members smoke failed at 443.4 ms in the same job where tokens
passed at 176–179 ms; on another, before any fix, tokens failed at 250.4 and 219.6 ms while the
same commit passed at 6.7 / 7.1 ms on a third 9V74. A red gate then says "slow disk", not "slow
software". All runs from 2026-11-17 are listed in `docs/proposals/token-slots.md`: the same commit
ranges from 4 ms to 524 ms by host.

## The proposal

Before the smokes, measure the host: `pg_test_fsync` (or a short `pgbench` of one-row commits)
inside the PostgreSQL container, recording the median and p99 flush time. Record that figure in
every smoke result (beside the CPU model, which results already carry). When the host's flush
p99 is past a threshold (to be set from the runs above — e.g. 20 ms), a smoke whose p99 misses
its gate reports **inconclusive: host flush X ms** instead of pass or fail. Inconclusive is never
green: the job fails with that word, and the remedy is a rerun on another host, not a code change
— the same rule as the local quiet-machine requirement for full runs.

## Trade-offs

- About a minute more per job; one more number in every result.
- A threshold to maintain, from recorded host figures.
- Inconclusive runs still block a merge, so nobody merges past a real regression on a slow host;
  but nobody chases a disk as a code bug either.

## Tests it would need

The host check against a known-slow fake (a container with `fsync` throttled) reports
inconclusive; a fast host's results are unchanged; every result file carries the host figure.
