<script>
	import Status from '#lib/components/Status.svelte';
</script>

Each publish writes one job per enabled **destination** (a *website connection* in the Studio), in the same transaction as the release, and the worker carries the jobs out: it announces the release to a webhook, asks a CDN to purge what changed, or looks at the site to see which release it serves. This page is what arrives at your endpoint and what Colophon does with your answer. Configuring destinations is on [Website connections and jobs](/reference/api/website-connections/). Code: `crates/colophon-worker/src/lib.rs` and `guard.rs`, `crates/colophon-domain/src/job.rs`, `destination.rs` and `receipt.rs`. The ideas are in [The outbox and the worker](/concepts/outbox-and-worker/).

```http
POST /api/v1/projects/0193f6a2-5b1c-7d3e-9f40-1a2b3c4d5e6f/environments/production/destinations
Content-Type: application/json

{"kind": "webhook", "name": "Site rebuild", "url": "https://build.example.com/hooks/colophon"}
```

| `kind` | What it does for each release | Done when |
|---|---|---|
| `webhook` (the default) | Posts a `release.committed` announcement. | The receiver answers `2xx`. |
| `cdn` <Status done="3.2" /> | Posts a purge by tag covering every release since its last successful purge. | The CDN answers `2xx`: the purge was *accepted*. |
| `probe` <Status done="3.3" /> | Fetches the URL and reads which release the answer was built from. | It sees this release or a later one. |

## The webhook request

```http
POST /hooks/colophon HTTP/1.1
Host: build.example.com
Content-Type: application/json
User-Agent: colophon/0.0.0
Idempotency-Key: colophon:0193f6a2-5b1c-7d3e-9f40-1a2b3c4d5e6f:0193f6a2-5b1d-7a00-8b11-223344556677:42:0193f690-6d7e-7f80-9a1b-2c3d4e5f6a7b
Colophon-Generation: 42

{
  "event": "release.committed",
  "project": "0193f6a2-5b1c-7d3e-9f40-1a2b3c4d5e6f",
  "environment": "production",
  "epoch": 1,
  "generation": 42,
  "changeset": "0193f6d8-4b5c-7d6e-8f70-819203a4b5c6",
  "published_at": 1793093778,
  "entries": [
    { "id": "0193f6b0-2c4d-7e5f-8a91-b2c3d4e5f607", "revision_id": "0193f6b4-1e2f-7a3b-8c4d-5e6f7a8b9c0d", "collection": "articles", "title": "Release notes" }
  ]
}
```

| Field | Meaning |
|---|---|
| `event` | Always `release.committed` today. |
| `project` | The project's id. |
| `environment` | The environment's slug. |
| `epoch` | The environment's recovery epoch; compare generations only within one epoch. |
| `generation` | The release's generation. |
| `changeset` | The id of the changeset it published. |
| `published_at` | When it committed, as a Unix timestamp in seconds. |
| `entries` | The entries this release put live, by id: each one's `id`, `revision_id`, `collection` and `title`. Entries the release took down aren't listed: re-read the collection to see what is gone. |

| Header | Meaning |
|---|---|
| `Idempotency-Key` | `colophon:` then the project id, the environment id, the generation and the destination id, joined by `:`. The same for every attempt of one job. **Deduplicate on it**: a worker that crashed after sending can't know the send landed, so it sends again with the same key (crash test F03). |
| `Colophon-Generation` | The release's generation. **Treat it as the order**: if you have already handled a higher generation, drop a lower one. Colophon never sends an older release after a newer one to the same destination (F05), but a receiver that keeps its own high-water mark is safe even against a worker paused past its lease. |
| `User-Agent` | `colophon/` and the server's version. |

There is no signature yet. Don't treat an announcement as authenticated: use it as a signal, and read what you need from [`/content/v1`](/reference/delivery-api/), pinned to its `generation`. The reference site's runner (`apps/web/src/runner.ts`) does exactly that: it answers `202` at once and builds the newest release announced, one build at a time.

## The purge request

A `cdn` destination <Status done="3.2" /> receives the same three headers and a body of tags:

```json
{ "tags": ["colophon:p:0193f6a25b1c7d3e9f401a2b3c4d5e6f:e:0193f6a25b1d7a008b11223344556677:c:articles", "colophon:p:0193f6a25b1c7d3e9f401a2b3c4d5e6f:e:0193f6a25b1d7a008b11223344556677:n:0193f6b02c4d7e5f8a91b2c3d4e5f607"] }
```

- The tags match the `Cache-Tag` header on `/content/v1` answers ([cache tags](/reference/delivery-api/#cache-tags)): for every entry whose publication started or ended, or that was taken down, since this destination's last successful purge, its entry tag and its collection's tag, sorted.
- More than 30 tags, and it is the environment's tag alone: coarser, and still bounded.
- When nothing changed since the last successful purge, the job succeeds without a request.
- A purge is never superseded in a way that loses anything: it covers every release since the last one that worked.
- A `2xx` means the CDN **accepted** the purge, not that its copies are gone; the release's `purge` state says so ([Releases and receipts](/reference/api/releases/)).

## What your endpoint should answer

| Your answer | Colophon |
|---|---|
| `2xx` | Done. |
| `408`, `425`, `429`, `500`, `502`, `503`, `504`, a timeout, no connection | Tries again later. |
| Any other status, or a redirect | Fails for good. The release's label becomes *Needs attention*. |
| (the address resolves to no public address) | Fails for good. |

- **Answer quickly.** Each attempt has a deadline: 10 seconds by default (`COLOPHON_WORKER_ATTEMPT_MS`), and 5 seconds to connect. Do slow work, such as a build, after answering.
- **Retries** wait a random time between zero and 1 second before the first retry, doubling each time, up to 60 seconds (full jitter). A `Retry-After` in seconds makes the wait at least that long, capped at 60 seconds; an HTTP date in it is ignored.
- **At most 5 attempts** (10 for a probe), **within one hour** of the release. A retry that would fall after the hour fails the job instead; a job still unfinished after the hour fails with "ran out of time".
- **One attempt at a time per destination**, and nothing older after something newer. When a release commits, jobs of earlier releases that haven't started are superseded, and a job about to start is superseded if a newer release's job exists for the same destination.
- No database transaction is open while your endpoint is called, and a late answer from a worker that lost its lease is refused, not recorded.

Each job's state, attempts, last HTTP status and last error are at [`GET {P}/jobs`](/reference/api/website-connections/#get-pjobs), and each release's [receipt](/reference/api/releases/#get-preleasesgeneration) sums them up.

## Addresses

A destination's URL must be `https` and reach only public addresses. It is checked when saved and again before every attempt; the worker's own resolver drops private addresses, so DNS can't point it inside your network later, and it never follows redirects. A development server bound to loopback may use `http` and private addresses, so local receivers work. The full rule is on [Website connections](/reference/api/website-connections/#addresses).

## Probes

A `probe` destination <Status done="3.3" /> has its URL fetched with `GET` and `Accept: text/html, application/json`. It passes when the answer is a `2xx` that names this release or a later one, either in a `Colophon-Generation` header or in the first megabyte of the body, as exactly this marker (attributes in this order, double quotes):

```text
<meta name="colophon-generation" content="42">
```

- An answer that names an older release, or none, is looked at again no sooner than 5 seconds later, up to 10 looks. Any other status is treated as for webhooks.
- Every look is recorded as an observation: the worker's location, the generation seen, the HTTP status, the SHA-256 of what was read, and what went wrong. The release is *verified* at the number of distinct locations that saw it.
- A probe can be pinned to a location (`location`); only workers whose `COLOPHON_WORKER_LOCATION` matches (`local` by default) run it. A probe without one, and every webhook and CDN job, can run on any worker.

The reference site carries the marker on every page ([D-052](/design/decisions/)).

## Running the worker

The worker runs in `colophon-server` (role `worker` or `all`), with its own small connection pool so deliveries never starve editors' requests. Its settings, from `crates/colophon-server/src/config.rs`:

| Setting | Default | What |
|---|---|---|
| `COLOPHON_WORKER_POLL_MS` | 500 | How often it looks for due work when idle. |
| `COLOPHON_WORKER_LEASE_MS` | 30,000 | How long a claim lasts; longer than an attempt. |
| `COLOPHON_WORKER_ATTEMPT_MS` | 10,000 | Each attempt's deadline (100 to 60,000). |
| `COLOPHON_WORKER_SLOTS` | 8 | Attempts in flight at once (1 to 64). |
| `COLOPHON_WORKER_DB_CONNECTIONS` | 4 | Its database pool. |
| `COLOPHON_WORKER_LOCATION` | `local` | Where it runs, for probes. |

More on [The worker](/operations/worker/).

## Proof

`crates/colophon-server/tests/worker.rs`, `purges.rs` and `receipts.rs` against the real binary; F03 (killed after sending; the next claim sends again with the same key) and F05 (an older release never sent after a newer one) on [Crash tests](/operations/crash-tests/); the retry and label rules as unit tests in `job.rs` and `receipt.rs`.

## Not built yet

- Signed announcements: an HMAC header with a per-destination secret. Until then, re-read what you need from `/content/v1`. <Status planned="not scheduled" />
- An authenticated CDN adapter (Cloudflare), with its credentials in the environment, never the database. Today's `cdn` kind posts to a URL with no credentials, which suits the test CDN. <Status planned="P3" />
- Webhooks for other events than a release: entries, media, schema. <Status planned="D" />
