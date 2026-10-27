<script>
	import Status from '#lib/components/Status.svelte';
</script>

A **destination** (a *website connection* in the editor guide) is something Colophon tells about each release. There are three kinds; owners and developers add them in the Studio under **Settings**, or through the API:

```http
POST /api/v1/projects/{project}/environments/production/destinations
Content-Type: application/json

{"kind": "webhook", "name": "Site rebuild", "url": "https://build.example.com/hooks/colophon"}
```

| `kind` | What it does for each release | Done when |
|---|---|---|
| `webhook` (the default) | Posts a `release.committed` announcement. | The receiver answers `2xx`. |
| `cdn` | Posts a purge by tag covering every release since its last successful purge. | The CDN answers `2xx`: the purge was *accepted*. |
| `probe` | Fetches the URL and reads which release the answer was built from. | It sees this release or a later one. |

`PUT …/destinations/{id}` with `{"enabled": false}` turns one off. Jobs and their attempts are at `GET …/jobs?generation=N`, and each release's [receipt](/concepts/receipts-and-probes/) at `GET …/releases/{generation}`.

## Addresses

A destination's URL must be `https` and resolve only to public addresses. It is checked when saved and again before every attempt; the worker's resolver drops private addresses, so DNS can't point it inside your network later, and it never follows redirects (a redirect fails the job). A development server bound to loopback may use `http` and private addresses, so local receivers work.

## The webhook request

```http
POST /hooks/colophon HTTP/1.1
Content-Type: application/json
Idempotency-Key: 6c0e…
Colophon-Generation: 42

{
  "event": "release.committed",
  "project": "0192a7…",
  "environment": "production",
  "epoch": 1,
  "generation": 42,
  "changeset": "0192e0…",
  "published_at": 1793280000,
  "entries": [
    { "id": "0192c1…", "revision_id": "0192d4…", "collection": "articles", "title": "Release notes" }
  ]
}
```

- `published_at` is a Unix timestamp in seconds. `entries` lists what the release changed.
- `Idempotency-Key` is the same for every attempt of one job. **Deduplicate on it**: a worker that crashed after sending can't know the send landed, so it sends again with the same key.
- `Colophon-Generation` is the release's generation. **Treat it as the order**: if you have already handled a higher generation, drop a lower one. Colophon never sends an older release after a newer one to the same destination, and supersedes announcements that haven't started when a newer release commits, but a receiver that keeps its own high-water mark is safe even against a worker paused for longer than its lease.

## The purge request

A `cdn` destination receives the same two headers and a body of tags:

```json
{ "tags": ["…entry tag…", "…collection tag…"] }
```

The tags match the `Cache-Tag` header on `/content/v1` answers: each changed entry's tag and its collection's, or the environment's tag alone when there would be more than 30. When nothing changed since the last successful purge, the job succeeds without a request.

## What your endpoint should answer

| Your answer | Colophon |
|---|---|
| `2xx` | Done. |
| `408`, `425`, `429`, `500`, `502`, `503`, `504`, a timeout, no connection | Retries with full-jitter doubling backoff, capped at 60 s, honouring a bounded `Retry-After`; at most five attempts within an hour. |
| Any other status, or a redirect | Fails for good. The release's label becomes *Needs attention*. |

Each attempt has a deadline (10 s by default, `COLOPHON_WORKER_ATTEMPT_MS`), and a destination has at most one attempt in flight.

## Probes

A `probe` destination's URL is fetched with `Accept: text/html, application/json`. It passes when the answer is not an error and names this release or a later one, either in a `Colophon-Generation` header or in the first megabyte of HTML:

```html
<meta name="colophon-generation" content="42">
```

Otherwise it looks again no sooner than every 5 seconds, up to 10 attempts. A probe can be pinned to a worker location (`location`, matching a worker's `COLOPHON_WORKER_LOCATION`); only workers there run it.

## Not built yet

- Signed announcements, an HMAC header with a per-destination secret. Until then, don't treat an announcement as authenticated: re-read what you need from `/content/v1`. <Status planned="not scheduled" />
- An authenticated CDN adapter (Cloudflare), with its credentials in the environment, never the database. Today's `cdn` kind posts to a URL with no credentials, which suits the test CDN. <Status planned="P3" />
- Webhooks for other events than a release. <Status planned="not scheduled" />
