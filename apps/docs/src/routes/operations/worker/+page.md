<script>
	import Status from '#lib/components/Status.svelte';
</script>

The worker does everything that has to happen outside the database after a publish: it announces each release to webhooks, asks CDNs to purge what changed, and looks for the release on your website to fill in its receipt. It finds all its work in PostgreSQL, so a worker that crashes, restarts or runs twice loses nothing and doubles nothing it could prevent. This page is for running it; [The outbox and the worker](/concepts/outbox-and-worker/) explains the idea. The code is `crates/colophon-worker/src/lib.rs`, `crates/colophon-db/src/jobs.rs` and `crates/colophon-domain/src/job.rs`.

## Running it

The worker runs inside `colophon-server`. The role decides whether it starts:

| Role | Serves HTTP | Runs the worker |
|---|---|---|
| `--role all` (the default) | yes | yes |
| `--role worker` | yes | yes |
| `--role api` | yes | no |

```sh runnable
cargo run --release -p colophon-server -- --role worker
```

`--role all` in one process is the v0.1 reference profile and what the local stack and the demo run. To split them, run one process with `--role api` and one with `--role worker`. Today `--role worker` still binds `COLOPHON_LISTEN_ADDR` and answers every route, the API included (D-046), so give it its own address and keep it off the public network. Nothing breaks if no process runs the worker: releases still commit and public delivery still serves them; their webhooks, purges and probes wait, and receipts say *Website update pending*, until a worker runs.

More than one worker may run at once, against the same database. They never claim the same job (`FOR UPDATE SKIP LOCKED`; tested in `crates/colophon-db/tests/jobs.rs`).

## What it does with each job

Committing a release inserts one job per enabled website connection (destination) in the same transaction: the transactional outbox. A committed release can't lose its deliveries, and one that didn't commit can't send any.

| Destination kind | What an attempt does | Succeeds when |
|---|---|---|
| `webhook` | `POST`s the `release.committed` announcement | the receiver answers `2xx` |
| `cdn` | `POST`s a purge by tag, covering every release since that destination's last successful purge | the CDN answers `2xx` |
| `probe` | `GET`s the URL and reads which release it was built from, from the `Colophon-Generation` header or a `colophon-generation` marker in the first megabyte | it shows this release or a later one |

Every `POST` carries the job's `Idempotency-Key`, the same on every attempt, and `Colophon-Generation`. The worker follows no redirects and sends `User-Agent: colophon/` plus its version. No database transaction is open while it waits for a destination. [Webhooks](/reference/webhooks/) has the payload and what a receiver must do.

## Leases and attempts

1. **Claim.** The worker claims due jobs (waiting ones whose time has come, and running ones whose lease ran out) for `COLOPHON_WORKER_LEASE_MS` (30 s). Each claim raises the job's lease generation and its attempt count.
2. **Check.** If a newer release is already on its way to the same destination, this job is marked `superseded` and never sent (committing a release also supersedes that destination's older jobs that haven't started): an older announcement must never arrive after a newer one (F05). A purge is never lost this way: the newer job covers both.
3. **Send.** It sends only if a whole attempt (`COLOPHON_WORKER_ATTEMPT_MS`, 10 s) still fits inside the lease, measured from the moment before it asked for the claim; otherwise it gives up and lets the lease run out. So nothing it sent can still be on its way when the next holder starts.
4. **Record.** The outcome is written only through the lease it holds. A late result from a lease that already ran out and was claimed again is refused, and logged as `late result refused`.

A destination has at most one job in flight under a live lease (D-047), so releases reach it in order.

## Retries, deadlines and failure

| Outcome | What happens |
|---|---|
| `2xx` | `succeeded` |
| `408`, `425`, `429`, `500`, `502`, `503`, `504`, a timeout, a dropped connection | `retrying`, after a backoff |
| Any other status, a redirect included, or an address that isn't allowed | `failed` at once: someone has to fix the destination |
| A probe that sees an older release or none | `retrying`, no sooner than 5 s later |

- **Backoff:** full jitter, a random wait up to 1 s before the first retry, doubling each time, capped at 60 s. A `Retry-After` in seconds is respected, up to 60 s.
- **Attempts:** at most 5 per job (10 for a probe). A claim counts as an attempt, so a crash during an attempt uses one.
- **Deadline:** a job may run for one hour from its release. Every 10 seconds the worker fails jobs past it (`ran out of time`); a retry that would land after it fails instead.

A failed job turns the release's receipt to *Needs attention*. Any member can list a release's jobs with their status, attempts, last HTTP status and last error: `GET {P}/jobs?generation=N` ([Website connections and jobs](/reference/api/website-connections/)).

## What happens on a crash

- **Killed with attempts in flight:** their leases run out after `COLOPHON_WORKER_LEASE_MS`; the next claim repeats each attempt with the **same** idempotency key. Whether the first one reached the destination is unknown, so Colophon never claims the repeat is impossible; it gives the receiver what it needs to drop it. F03 in `crates/colophon-server/tests/faults.rs` kills the worker right after it sent, before it recorded, and checks exactly this.
- **Stopped with SIGTERM:** it keeps working while the process drains its HTTP requests ([Health checks](/operations/health-checks/#shutting-down)), then stops claiming and gives attempts in flight 5 seconds to finish; any still running are claimed again once their leases run out.
- **The database goes away:** claims fail and are retried at the next poll; nothing is kept only in memory, so nothing is lost. Readiness reports the database `down`.
- **The crash loop** (`crates/colophon-server/tests/crash_loop.rs`) kills the whole server, worker included, with `SIGKILL` at random moments while it publishes, and checks that every announcement ends delivered or superseded and that the receiver never got an older release after a newer one. See [Crash tests](/operations/crash-tests/).

## Settings

| Variable | Default | What it does |
|---|---|---|
| `COLOPHON_WORKER_POLL_MS` | 500 | How long it sleeps when there was nothing to claim. With work waiting it claims as soon as a slot frees, so a backlog drains at the destinations' speed. |
| `COLOPHON_WORKER_LEASE_MS` | 30,000 | How long a claim lasts: how long a crashed worker's jobs wait before another worker takes them. |
| `COLOPHON_WORKER_ATTEMPT_MS` | 10,000 | One attempt's deadline, connection included; must be shorter than the lease. |
| `COLOPHON_WORKER_SLOTS` | 8 | Attempts in flight at once, across all destinations. |
| `COLOPHON_WORKER_DB_CONNECTIONS` | 4 | Its own database pool, separate from the API's. The first publishing load test found the worker sharing the API's pool: at a hundred deliveries a second, editors' requests queued behind it. |
| `COLOPHON_WORKER_LOCATION` | `local` | Its location label. A probe may be pinned to a location; only workers with that label run it, and each observation records where it was made. |

Ranges and the rules between them are in [Configuration](/operations/configuration/#the-worker).

## Database role

The worker logs in with the server's `COLOPHON_DATABASE_URL` (`colophon_app` locally) and switches each transaction to `colophon_worker` (`SET LOCAL ROLE`). That role sees jobs in every project, their destinations and the releases being announced (which entries changed, never their content), and writes only jobs and probe observations. It can't read drafts: `crates/colophon-db/tests/jobs.rs` checks it.

## Locations and receipts

A receipt says *Verified at N locations*, where N counts the distinct locations whose probes saw the release. One worker means one location, so a default install says *verified at 1 location*, never a bare "verified". To check from more places, run a worker in each, each with its own `COLOPHON_WORKER_LOCATION`, and pin a probe to each. [Receipts and probes](/concepts/receipts-and-probes/) has the rest.

## Addresses it will call

Destinations must be `https://` and resolve only to public addresses: the worker resolves names itself and drops private, loopback and link-local answers, so a DNS change can't point it inside your network later. A server listening on a loopback address is treated as a development server and may also call `http://` and private addresses, which is how the local stack's test receivers work.

## Not built yet

- Signed announcements (an HMAC header with destination secrets). <Status planned="not scheduled" />
- Per-destination and per-project concurrency limits, and admission budgets that refuse new work before it is accepted (F09). <Status planned="P6" />
- Wake-ups instead of polling (O-010), and reconciliation beyond repeating with the same key. <Status planned="not scheduled" />
- Scheduled publishing, which the worker will run when it is due. <Status building="e/schedules" />
- `colophon jobs inspect` on the command line. <Status planned="not scheduled" />
- A worker that doesn't serve HTTP. <Status planned="not scheduled" />

## Design

[Module 05, the worker](/design/modules/05-worker/); decisions D-046 (the outbox and leases), D-047 (one announcement in flight), D-050 (purges) and D-051 (probes) in the [decisions](/design/decisions/).
