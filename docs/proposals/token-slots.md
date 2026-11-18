# Proposed, not decided: API token slots, so creators never wait across another's commit

**Status:** proposed (2026-11-17), for the maintainer. Not built.

## The problem

Making an API token takes the project's owner-role row `FOR NO KEY UPDATE` and holds it until
commit, so the count of active tokens can never race past `MAX_TOKENS` (100). Since
`fix/token-create-hold`, only two statements run under the lock (the lock, then the count and
every insert in one statement), but the commit's WAL flush still happens while the lock is held.
Creations per project are therefore capped near 1 / (flush time): on a host whose flush takes
100 ms, 5–10 a second — below the tokens smoke's ~21 a second.

Evidence (CI load smokes, 20 s at 200 requests a second, one in ten a creation; p99 in ms, tokens
/ members / tokens again, every run as it came out; fresh stack per runner):

| Round | Version | CPU | p99 (ms) | Wait events during the first tokens run |
|---|---|---|---|---|
| 1 (44215128880) | before | AMD EPYC 7763 | 23.7 / 55.4 / 34.2 | |
| 1 | before | AMD EPYC 9V45 | **523.6 fail** / 5.9 / 65.2 | LWLock/WALWrite 33, IO/WalSync 7 of 47 |
| 1 | before | Intel 6975P-C | 4.8 / 3.9 / 4.5 | |
| 1 | after | Intel 8573C | 5.2 / 10.6 / 9.3 | |
| 1 | after | Intel 6975P-C | 3.6 / 4.7 / 4.1 | |
| 1 | after | Intel 6975P-C | 9.0 / 3.7 / 3.8 | |
| 2 (44216571685) | before | AMD EPYC 9V74 | 6.7 / 73.9 / 7.1 | |
| 2 | before | AMD EPYC 9V45 | 6.7 / 5.8 / 8.1 | |
| 2 | before | Intel 8573C | 59.4 / 45.1 / 14.7 | |
| 2 | before | Intel 6975P-C | 4.2 / 3.9 / 3.9 | |
| 2 | before | AMD EPYC 9V74 | **250.4 fail** / 44.5 / **219.6 fail** | WALWrite 91, Lock/tuple 13, WalSync 9 |
| 2 | before | AMD EPYC 9V74 | 82.9 / 74.5 / 7.5 | |
| 2 | after | AMD EPYC 7763 | 9.3 / 93.6 / 8.7 | |
| 2 | after | Intel 8370C | 6.6 / 50.5 / 7.5 | |
| 2 | after | AMD EPYC 9V74 | 178.8 / **443.4 fail (members)** / 176.0 | WALWrite 47, WalSync 12, Lock/tuple 8 |
| 2 | after | AMD EPYC 7763 | 143.3 / 78.2 / 45.3 | |
| 2 | after | AMD EPYC 9V45 | 8.0 / 15.6 / 7.7 | |
| 2 | after | AMD EPYC 9V74 | 10.0 / 7.3 / 7.6 | |

Before that, main's own run 44208638779 on an AMD EPYC 9V74 failed tokens at 241.8 ms
(p50 1.8, p90 94; create p90 202, read p90 206), with creators queued 100–200 ms on the owner row
and each holding one of sixteen pooled connections.

## The proposal

`api_token_slots (project_id, slot smallint 1..MAX_TOKENS, token_id uuid NULL)`, one row per slot,
made with the project (and backfilled for existing projects). A creation claims a free slot —
empty, or naming a revoked or expired token — with
`SELECT … FROM api_token_slots WHERE project_id = $1 AND (free) ORDER BY slot FOR UPDATE SKIP LOCKED LIMIT 1`,
writes the token, points the slot at it and commits. Concurrent creators lock different slots and
never wait on each other, so their commits group; the cap stays exact because only `MAX_TOKENS`
slots exist. No free slot is `too_many`, as today.

## Trade-offs

- A second table to keep consistent with `api_tokens`: revocation and expiry free a slot lazily,
  at the next claim that finds it naming a revoked or expired token.
- One more row write per creation; the slot rows are hot only under concurrent creation.
- Roles keep the owner-row lock (creating roles is rare and has its own count).
- SKIP LOCKED makes "the cap is full" answer only from slots not being claimed right now: with one
  slot left and two racers, one gets it and the other is told the project is full — the same
  outcome as today, without the wait.

## Tests it would need

Twenty creators racing for the last slot make exactly one token (the existing test, kept); a
revoked or expired token's slot is reused; a backfilled project with 100 active tokens refuses
the 101st; creation under a simulated 100 ms flush keeps p99 near one flush.
