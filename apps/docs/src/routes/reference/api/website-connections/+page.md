<script>
	import Status from '#lib/components/Status.svelte';
</script>

A **destination** (a *website connection* in the Studio) is something Colophon tells about each release: a webhook that rebuilds a site, a CDN that clears its copies, or a probe that checks the site. The Studio names the three kinds in plain words, *Site rebuild*, *CDN clear* and *Site check* (`apps/studio/src/lib/words.ts`, D-059), under Settings. Each publish writes one **job** per enabled destination in the same transaction, and the worker carries them out. This page covers configuring destinations and reading jobs; what the worker sends, and what a receiver should answer, is on [Webhooks](/reference/webhooks/). Handler: `crates/colophon-api/src/destinations.rs`; address rules: `crates/colophon-domain/src/destination.rs`. The ideas are in [The outbox and the worker](/concepts/outbox-and-worker/) and [Receipts and probes](/concepts/receipts-and-probes/).

Destinations are seen and changed only with `destinations.manage` (owners and developers), because an address can carry a secret. Jobs, which name a destination but not its address, are visible to any member.

## The destination object

```json
{
  "id": "0193f690-6d7e-7f80-9a1b-2c3d4e5f6a7b",
  "kind": "webhook",
  "name": "Site rebuild",
  "url": "https://build.example.com/hooks/colophon",
  "enabled": true,
  "location": null,
  "created_at": "2026-10-25T16:02:11.540913Z"
}
```

| `kind` | For each release | Done when |
|---|---|---|
| `webhook` (the default) | Posts a `release.committed` announcement. | The receiver answers `2xx`. |
| `cdn` <Status done="3.2" /> | Posts a purge by tag covering every release since its last successful purge. | The CDN answers `2xx`: the purge was accepted. |
| `probe` <Status done="3.3" /> | Fetches the URL and reads which release the answer was built from. | It sees this release or a later one. |

`location` applies to probes only: a probe with a location is run only by workers whose `COLOPHON_WORKER_LOCATION` matches (`local` by default); without one, any worker runs it.

## Addresses

A destination's URL must be `https`, with a host, without a user name or password, and, when the host is an IP address, a public one. It is checked when saved and again before every attempt. A name is resolved only when the worker connects, by its own resolver, which drops every address that isn't public, so a name can't resolve inside your network, then or later; one with no public address fails its job for good. The worker never follows redirects. Loopback, private, link-local, shared, reserved, documentation, benchmarking, multicast and unspecified addresses are not public, IPv4-mapped IPv6 included.

A development server whose `COLOPHON_LISTEN_ADDR` is a loopback address may use `http` and private addresses, so local receivers work.

## Routes

### `GET {P}/destinations`

The environment's destinations.

- **Who:** `destinations.manage` (owners, developers).

**Response:** `200`, in the order they were created, at most 100, `next_cursor` always `null`.

```json
{ "data": [ { "id": "0193f690-6d7e-7f80-9a1b-2c3d4e5f6a7b", "kind": "webhook", "name": "Site rebuild", "url": "https://build.example.com/hooks/colophon", "enabled": true, "location": null, "created_at": "2026-10-25T16:02:11.540913Z" } ], "next_cursor": null }
```

| Status | `code` | When |
|---|---|---|
| `403` | `forbidden` | Your role lacks `destinations.manage`. |
| `404` | `not_found` | No such project or environment, or you aren't a member. |

### `POST {P}/destinations`

Adds a destination. It receives jobs from the next release on.

- **Who:** `destinations.manage` (owners, developers).
- **Headers:** no `Idempotency-Key`: a retry adds a second destination, which can be turned off.
- **Body:**

```json
{ "kind": "probe", "name": "Check the home page", "url": "https://www.example.com/", "location": "eu-west" }
```

| Field | Type | Rules |
|---|---|---|
| `kind` | string | Optional, default `webhook`. `webhook`, `cdn` or `probe`. |
| `name` | string | Required. Trimmed, 1 to 100 characters, no control characters. |
| `url` | string | Required. Trimmed, at most 2,000 characters, and an [allowed address](#addresses). |
| `location` | string | Optional, probes only. Trimmed, 1 to 63 lowercase letters, digits and hyphens, starting with a letter or digit. |

**Response:** `201 Created`, `{"data": …}` with [the destination](#the-destination-object), enabled.

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `403` | `forbidden` | | Your role lacks `destinations.manage`. |
| `422` | `validation_failed` | `invalid_kind` | `/kind` isn't one of the three. |
| `422` | `validation_failed` | `invalid_location` | `/location` on a kind other than `probe`, or breaking the rule. |
| `422` | `validation_failed` | `invalid_name` | `/name` breaks the rule. |
| `422` | `validation_failed` | `invalid_url` | `/url` is too long, not a URL, not `https`, carries credentials, or names an address that isn't public. The message says which. |

### `PUT {P}/destinations/{destination}`

Turns a destination on or off. A disabled destination gets no jobs for later releases; jobs already written for it are still carried out. A destination's kind, name and URL can't be changed: add a new one and turn the old one off.

- **Who:** `destinations.manage` (owners, developers).
- **Body:**

```json
{ "enabled": false }
```

**Response:** `204 No Content`.

| Status | `code` | When |
|---|---|---|
| `403` | `forbidden` | Your role lacks `destinations.manage`. |
| `404` | `not_found` | No such destination here. |

### `GET {P}/jobs`

The worker's jobs: one per destination per release.

- **Who:** any member.
- **Query:** `generation`, optional: only that release's jobs.

**Response:** `200`, newest release first, at most 100, `next_cursor` always `null`.

```json
{
  "data": [
    {
      "id": "0193f6e0-7a8b-7c9d-8e0f-1a2b3c4d5e6f",
      "destination_id": "0193f690-6d7e-7f80-9a1b-2c3d4e5f6a7b",
      "destination_name": "Site rebuild",
      "generation": 42,
      "status": "retrying",
      "attempts": 2,
      "last_http_status": 503,
      "last_error": "HTTP 503",
      "created_at": "2026-10-27T09:36:18.660431Z",
      "finished_at": null
    }
  ],
  "next_cursor": null
}
```

| `status` | Meaning |
|---|---|
| `pending` | Waiting for its first attempt. |
| `running` | A worker holds its lease; an attempt is in flight. |
| `retrying` | Waiting to be tried again. |
| `succeeded` | Delivered, or for a probe, the release was seen. |
| `failed` | Given up: refused for good, out of attempts, or out of time (`last_error` is "ran out of time" after an hour). |
| `superseded` | A newer release's job for the same destination made it obsolete; it is never sent after that one. |

`attempts` counts attempts started; `last_http_status` and `last_error` describe the latest. `finished_at` is set once the job is final.

| Status | `code` | When |
|---|---|---|
| `400` | `bad_request` | `generation` isn't a whole number. |
| `404` | `not_found` | No such project or environment, or you aren't a member. |

## Not built yet

- An authenticated CDN adapter (Cloudflare), with its credentials in the environment, never the database. Today's `cdn` kind posts to a URL with no credentials, which suits the test CDN. <Status planned="P3" />
- Website connections for other events than a release. <Status planned="D" />
