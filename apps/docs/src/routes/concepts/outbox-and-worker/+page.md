<script>
	import Status from '#lib/components/Status.svelte';
</script>

A release has to be announced: a webhook that rebuilds the site, a CDN to purge, a probe to check. Doing that inside the publish transaction would hold the database open while waiting on someone else's server, and a crash between "sent" and "recorded" would leave nobody knowing what happened. So Colophon never makes an external call inside a transaction (invariant 2). It writes the intent down, then a worker does it.

## The outbox

Committing a release inserts one **job** per enabled destination, in the same transaction as the release. If the release commits, its jobs exist; if it doesn't, they don't. There is no window where a release exists and nobody knows it must be announced. A new release also supersedes older jobs for the same destination that haven't started: a webhook only needs the newest release.

## The worker

`colophon-server --role worker` (or `--role all`, the default) runs `crates/colophon-worker`. It:

1. **Claims** due jobs, and running jobs whose lease ran out, with `FOR UPDATE SKIP LOCKED`, as the database role `colophon_worker`, which sees jobs in every project and nothing beyond destinations and the releases being announced. Each claim raises the job's lease generation.
2. **Checks** that no newer release is already headed to the same destination, and that enough of its lease remains for a whole attempt.
3. **Sends** with no transaction open: `POST` with an `Idempotency-Key` that stays the same for every attempt of the job, and `Colophon-Generation`.
4. **Records** the outcome with an update that only succeeds if it still holds the lease. A worker whose lease expired can't overwrite a newer holder's result.

The worker has its own database pool, so deliveries can't starve editors' requests.

## Retries, and when to stop

| The answer | What happens |
|---|---|
| `2xx` | Done. |
| `408`, `425`, `429`, `500`, `502`, `503`, `504`, a timeout, no connection | Retried with full-jitter doubling backoff, capped at 60 s, respecting a bounded `Retry-After`. At most five attempts within an hour. |
| Anything else, redirects included | Failed for good; the release says *Needs attention*. |

## One announcement in flight per destination

A destination has at most one job running under a live lease, and an attempt's deadline (`COLOPHON_WORKER_ATTEMPT_MS`) is shorter than the lease. The simulator found why this matters: without it, one worker could still be sending release N while another sent N+1, and a slow network could deliver N after N+1. With it, nothing one holder sent can still be on its way when the next starts.

## Only public addresses

Destination addresses come from project members, so without rules a webhook could reach the cloud's metadata endpoint or the database. A destination must be `https` and public, checked when it is saved and before every attempt; the worker's resolver keeps only public addresses, so DNS can't point it inside later, and it follows no redirects. A development server bound to loopback may use `http` and private addresses.

## What it can't promise

A worker killed after sending but before recording can't know whether the send landed, so the job is sent again with the same key. Colophon never claims a duplicate is impossible; it gives the receiver what it needs to drop one. Receivers should deduplicate by `Idempotency-Key`, and treat `Colophon-Generation` as the order. A worker paused (not killed) for longer than its lease could still send late, which is why the generation header matters.

## How it is tested

- **F03**: the worker is killed after announcing a release but before recording it. Another claims the job and announces it again with the same key; the job succeeds after two attempts.
- **F05**: a release waiting when a newer one commits is superseded and never sent.
- The simulator's publishing scenario crashes workers at any step on a lossy network, with eight planted bugs that must each be caught. The real-process crash loop kills the whole server twelve times per CI run.

Announcements aren't signed yet: an HMAC header comes with destination secrets. <Status planned="not scheduled" /> Workers poll for work; waking them with database notifications is an open decision (O-010).

## Design

[Module 05, worker](/design/modules/05-worker/); decisions D-046 and D-047 in the [decisions](/design/decisions/). On GitHub: [docs/modules/05-worker.md](https://github.com/akira-shirase-oss/colophon-cms/blob/main/docs/modules/05-worker.md).
