<script>
	import Status from '#lib/components/Status.svelte';
</script>

A release has to be announced: a webhook that rebuilds the site, a CDN to purge, a probe to check. Doing that inside the publish transaction would hold the database open while waiting on someone else's server, and a crash between "sent" and "recorded" would leave nobody knowing what happened. So Colophon never makes an external call inside a transaction (invariant 2). It writes the intent down, then a worker does it.

These targets are **destinations** in the API, and *website connections* in the Studio's plain words: rebuild the site, clear the CDN, check the site. Configuring them needs `destinations.manage` (owners and developers by default).

## The outbox

Committing a release inserts one **job** per enabled destination, in the same transaction as the release. If the release commits, its jobs exist; if it doesn't, they don't. There is no window where a release exists and nobody knows it must be announced. A new release also supersedes the environment's older jobs that are still waiting (pending or retrying): a webhook only needs the newest release, and a superseded CDN purge loses nothing, because the next purge covers every release since that CDN's last success ([Unpublish and revocation](/concepts/unpublish-and-revocation/)).

## The worker

`colophon-server --role worker` (or `--role all`, the default) runs `crates/colophon-worker`. It:

1. **Claims** due jobs, and running jobs whose lease ran out, with `FOR UPDATE SKIP LOCKED`, as the database role `colophon_worker`. That role sees jobs in every project, their destinations, the releases being announced and which entries a release changed, and nothing of their content. Each claim raises the job's lease generation.
2. **Checks** that no newer release is already headed to the same destination, and that enough of its lease remains for a whole attempt.
3. **Sends** with no transaction open: `POST` with an `Idempotency-Key` that stays the same for every attempt of the job, and `Colophon-Generation`.
4. **Records** the outcome with an update that only succeeds if it still holds the lease. A worker whose lease expired can't overwrite a newer holder's result.

The worker has its own database pool (`COLOPHON_WORKER_DB_CONNECTIONS`, 4 by default), so deliveries can't starve editors' requests. It polls every `COLOPHON_WORKER_POLL_MS` (500 ms by default), holds a claim for `COLOPHON_WORKER_LEASE_MS` (30 s) and runs at most `COLOPHON_WORKER_SLOTS` attempts at once (8). [Running the worker](/operations/worker/) covers the settings.

## Retries, and when to stop

| The answer | What happens |
|---|---|
| `2xx` | Done. |
| `408`, `425`, `429`, `500`, `502`, `503`, `504`, a timeout, no connection | Retried with full-jitter doubling backoff from 1 s, capped at 60 s, respecting a bounded `Retry-After`. At most five attempts within an hour. |
| Anything else, redirects included, or an address that isn't public | Failed for good; the release says *Needs attention*. |

## One announcement in flight per destination

A destination has at most one job running under a live lease, and an attempt's deadline (`COLOPHON_WORKER_ATTEMPT_MS`, 10 s by default) is shorter than the lease, which the server checks at startup. The simulator found why this matters: without it, one worker could still be sending release N while another sent N+1, and a slow network could deliver N after N+1. With it, nothing one holder sent can still be on its way when the next starts.

## Only public addresses

Destination addresses come from project members, so without rules a webhook could reach the cloud's metadata endpoint or the database. A destination must be `https`, carry no credentials in its URL, and point at a public address, checked when it is saved and before every attempt; the worker's own resolver hands its HTTP client only public addresses, so DNS can't point it inside later, and it follows no redirects. A development server bound to loopback may use `http` and private addresses. The [security model](/resources/security/) lists exactly which ranges count as private.

## What it can't promise

A worker killed after sending but before recording can't know whether the send landed, so the job is sent again with the same key. Colophon never claims a duplicate is impossible; it gives the receiver what it needs to drop one. Receivers should deduplicate by `Idempotency-Key`, and treat `Colophon-Generation` as the order. A worker paused (not killed) for longer than its lease could still send late, which is why the generation header matters. [Webhooks](/reference/webhooks/) describes the payload.

## How it is tested

- **F03**: the worker is killed after announcing a release but before recording it. Another claims the job and announces it again with the same key; the job succeeds after two attempts.
- **F05**: a release waiting when a newer one commits is superseded and never sent.
- The simulator's publishing scenario crashes workers at any step on a lossy network, with eight planted bugs that must each be caught. The real-process crash loop kills the whole server twelve times per CI run ([Crash tests](/operations/crash-tests/)).
- `crates/colophon-server/tests/worker.rs` runs the worker against a real receiver; `perf/k6/publishing.js` publishes with the worker running.

## Not built yet

- Signed announcements: an HMAC header comes with destination secrets. <Status planned="not scheduled" />
- Waking workers with database notifications instead of polling is an open decision (O-010).
- The per-destination and per-project concurrency limits in module 05; supersession keeps at most one announcement per destination worth sending meanwhile.
- On the `e/schedules` branch, a second loop beside the deliveries runs scheduled publishes as they fall due, on the worker's own pool ([Scheduled publishing](/concepts/scheduled-publishing/)). <Status building="e/schedules" />

## Design

[Module 05, worker](/design/modules/05-worker/); decisions D-046 and D-047 in the [decisions](/design/decisions/). On GitHub: [docs/modules/05-worker.md](https://github.com/akira-shirase-oss/colophon-cms/blob/main/docs/modules/05-worker.md).
