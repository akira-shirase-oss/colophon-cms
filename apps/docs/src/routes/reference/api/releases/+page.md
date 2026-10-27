A **release** is what one publish committed: the environment's next generation, the changeset it published, who published it and when. Its **receipt** says how far it got: each website connection's part in delivering it, every look a probe took, and one label. Handlers: `crates/colophon-api/src/changesets.rs` (the list) and `receipts.rs` (one receipt); the label comes from `crates/colophon-domain/src/receipt.rs`. The ideas are in [Releases and generations](/concepts/releases-and-generations/) and [Receipts and probes](/concepts/receipts-and-probes/); the editor's side is [Receipts: is it on the website?](/editors/receipts/).

## The release object

```json
{
  "generation": 42,
  "epoch": 1,
  "changeset_id": "0193f6d8-4b5c-7d6e-8f70-819203a4b5c6",
  "title": "Launch posts",
  "changeset_hash": "sha256:3c9a5e7b1d2f4a6c8e0b2d4f6a8c0e2b4d6f8a1c3e5b7d9f1a3c5e7b9d1f3a5c",
  "published_by": "0193f6c7-1a2b-7c3d-8e4f-5a6b7c8d9e0f",
  "published_by_name": "Pat Publisher",
  "published_at": "2026-10-27T09:36:18.660431Z",
  "entries": 2,
  "unpublished": 1,
  "purge": "accepted",
  "label": "verified",
  "locations": 2
}
```

| Field | Meaning |
|---|---|
| `generation` | The release's number in this environment; it only moves forward. |
| `epoch` | The environment's recovery epoch; a restore will start a new one, and generations from different epochs are never compared. |
| `changeset_id`, `title`, `changeset_hash` | The changeset it published, and the hash that was approved. |
| `published_by`, `published_by_name`, `published_at` | Who published it, and when. |
| `entries` | Entries whose published revision changed in it. |
| `unpublished` | Entries it took down. |
| `purge` | Whether the environment's CDNs accepted a purge covering it: `none` (no CDN job covers it), `pending`, `accepted` or `failed`. Until `accepted`, copies a CDN holds may still be served. |
| `label` | The release's one label, below. |
| `locations` | How many distinct locations a probe saw this release, or a later one, from. |

| `label` | The Studio says | When |
|---|---|---|
| `published` | Published in CMS | Committed; nothing delivers or checks it, or nothing checks the site. |
| `pending` | Website update pending | A delivery or check is still on its way. |
| `verified` | Verified at N locations | Every connection is done and probes saw it at `locations` locations. Never plain "verified". |
| `needs_attention` | Needs attention | A delivery or check gave up: someone should look. A failure outranks everything else. |

A connection counts as done for a release once one of its jobs at that release or a later one succeeded: an announcement of release 43 tells a site about 42's changes too, a purge covers every release since the last one, and a probe that saw 43 saw 42.

## Routes

### `GET {P}/releases`

The environment's releases, and its latest generation.

- **Who:** any member.

**Response:** `200`, newest first, the newest 100, `next_cursor` always `null`, with `Colophon-Generation` naming the environment's latest generation (`0` before the first publish). That number is the `base_generation` to [publish](/reference/api/publish-requests/#post-pchangesetschangesetpublish) on.

```http
HTTP/1.1 200 OK
Colophon-Generation: 42
Content-Type: application/json

{"data":[{"generation":42,"epoch":1,"changeset_id":"0193f6d8-4b5c-7d6e-8f70-819203a4b5c6","title":"Launch posts","changeset_hash":"sha256:3c9a…","published_by":"0193f6c7-1a2b-7c3d-8e4f-5a6b7c8d9e0f","published_by_name":"Pat Publisher","published_at":"2026-10-27T09:36:18.660431Z","entries":2,"unpublished":1,"purge":"accepted","label":"verified","locations":2}],"next_cursor":null}
```

| Status | `code` | When |
|---|---|---|
| `404` | `not_found` | No such project or environment, or you aren't a member. |

### `GET {P}/releases/{generation}`

One release's receipt: [the release](#the-release-object), each connection's part in delivering it, and every probe look made for it.

- **Who:** any member.
- **Path:** `generation`, a whole number.

**Response:** `200`.

```json
{
  "data": {
    "generation": 42,
    "epoch": 1,
    "changeset_id": "0193f6d8-4b5c-7d6e-8f70-819203a4b5c6",
    "title": "Launch posts",
    "changeset_hash": "sha256:3c9a…",
    "published_by": "0193f6c7-1a2b-7c3d-8e4f-5a6b7c8d9e0f",
    "published_by_name": "Pat Publisher",
    "published_at": "2026-10-27T09:36:18.660431Z",
    "entries": 2,
    "unpublished": 1,
    "purge": "accepted",
    "label": "verified",
    "locations": 1,
    "deliveries": [
      {
        "destination_id": "0193f690-6d7e-7f80-9a1b-2c3d4e5f6a7b",
        "name": "Site rebuild",
        "kind": "webhook",
        "location": null,
        "state": "done",
        "covered_by": 42,
        "status": "succeeded",
        "attempts": 1,
        "last_error": null,
        "finished_at": "2026-10-27T09:36:19.012388Z"
      },
      {
        "destination_id": "0193f692-8f90-7a1b-8c2d-3e4f5a6b7c8d",
        "name": "Check the home page",
        "kind": "probe",
        "location": "eu-west",
        "state": "done",
        "covered_by": 42,
        "status": "succeeded",
        "attempts": 2,
        "last_error": null,
        "finished_at": "2026-10-27T09:36:31.554090Z"
      }
    ],
    "observations": [
      {
        "destination_id": "0193f692-8f90-7a1b-8c2d-3e4f5a6b7c8d",
        "location": "eu-west",
        "observed_generation": 41,
        "http_status": 200,
        "body_hash": "sha256:0d4b…",
        "error": "saw release 41, not yet 42",
        "observed_at": "2026-10-27T09:36:24.318807Z"
      },
      {
        "destination_id": "0193f692-8f90-7a1b-8c2d-3e4f5a6b7c8d",
        "location": "eu-west",
        "observed_generation": 42,
        "http_status": 200,
        "body_hash": "sha256:6a1f…",
        "error": null,
        "observed_at": "2026-10-27T09:36:31.553871Z"
      }
    ]
  }
}
```

`deliveries` lists each connection that has a job at this release or a later one, in the order they were created:

| Field | Meaning |
|---|---|
| `state` | `done` (its job for this release succeeded), `covered` (a later release's job succeeded, which covers this one: see `covered_by`), `pending` (a job is still waiting or running) or `failed`. |
| `covered_by` | The earliest generation, from this one on, whose job for this connection succeeded. |
| `status`, `attempts`, `last_error`, `finished_at` | Its own job for this very release, if it had one; otherwise `null`. Job statuses are on [Website connections and jobs](/reference/api/website-connections/#get-pjobs). |

`observations` lists the probe looks made for this release, oldest first, at most 100: the `location` that looked, the generation the answer named (`null` if none), its HTTP status, the SHA-256 of what it read (`null` if nothing was read), and what went wrong, if anything. An observation proves what one location saw at one moment, nothing more.

| Status | `code` | When |
|---|---|---|
| `400` | `bad_request` | `generation` isn't a whole number. |
| `404` | `not_found` | No release with this generation here. |

## Proof

`crates/colophon-server/tests/receipts.rs` and `releases.rs` against the real binary; the label rules as unit tests in `crates/colophon-domain/src/receipt.rs`; the load scenario `perf/k6/publishing.js`.
