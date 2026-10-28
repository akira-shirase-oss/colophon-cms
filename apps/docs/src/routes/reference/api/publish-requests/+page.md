<script>
	import Status from '#lib/components/Status.svelte';
</script>

A **changeset** (a *publish request* in the Studio) pins exact revisions of up to 100 entries, each to publish or to take down. Its hash is computed from those pins, a review is stored with the hash it was given on and counts only while the changeset keeps it, and publishing commits an approved changeset as the environment's next release. This page covers the changeset routes and [schedules](#schedules), which publish an approved changeset at a chosen time; the release they produce is on [Releases and receipts](/reference/api/releases/). Handlers: `crates/colophon-api/src/changesets.rs` and `schedules.rs`; rules: `crates/colophon-domain/src/changeset.rs` and `schedule.rs`; the publish itself, shared by both: `colophon_db::publish`. The ideas are in [Publish requests](/concepts/publish-requests/), [Releases and generations](/concepts/releases-and-generations/) and [Unpublish and revocation](/concepts/unpublish-and-revocation/); the editor's side is [Publish requests and review](/editors/review/) and [Publishing](/editors/publishing/).

Each action needs its permission in **every** collection the changeset touches: `content.update` to create, submit and refresh, `content.review` to review, `content.publish` to publish, and the same as publishing to schedule. Since R.3, creating, submitting and refreshing check `content.update` per pinned entry: a role whose `content.update` is limited to its own entries (`"condition": "own"`) may pin only entries its holder created, because pinning an entry asks for it to be published ([Roles](/reference/api/roles/#narrowing-a-permission)). With the built-in roles: every role may create, submit and refresh; owners and publishers review, publish and schedule.

## The changeset object

```json
{
  "data": {
    "id": "0193f6d8-4b5c-7d6e-8f70-819203a4b5c6",
    "title": "Launch posts",
    "hash": "sha256:3c9a5e7b1d2f4a6c8e0b2d4f6a8c0e2b4d6f8a1c3e5b7d9f1a3c5e7b9d1f3a5c",
    "status": { "state": "approved", "generation": null, "by": "Pat Publisher", "self_review": false, "edited_items": 0 },
    "created_by": "0193f6c4-9e0f-7a1b-8c2d-3e4f5a6b7c8d",
    "created_at": "2026-10-27T09:20:11.408273Z",
    "submitted_by": "0193f6c4-9e0f-7a1b-8c2d-3e4f5a6b7c8d",
    "submitted_at": "2026-10-27T09:21:40.902114Z",
    "updated_at": "2026-10-27T09:35:02.117650Z",
    "items": [
      {
        "kind": "publish",
        "entry_id": "0193f6b0-2c4d-7e5f-8a91-b2c3d4e5f607",
        "collection": "articles",
        "revision_id": "0193f6b4-1e2f-7a3b-8c4d-5e6f7a8b9c0d",
        "title": "Release notes",
        "content_hash": "sha256:9f2c4e1a7b3d5f6e8a0c2b4d6f8e1a3c5b7d9f0e2a4c6b8d0f1e3a5c7b9d2f4e",
        "schema_version": 1,
        "current_revision_id": "0193f6b4-1e2f-7a3b-8c4d-5e6f7a8b9c0d",
        "edited": false,
        "published_revision_id": "0193f6b1-7d8e-7f90-a1b2-c3d4e5f60718"
      }
    ],
    "reviews": [
      {
        "id": "0193f6dd-5c6d-7e7f-8091-a2b3c4d5e6f7",
        "reviewer_id": "0193f6c7-1a2b-7c3d-8e4f-5a6b7c8d9e0f",
        "reviewer_name": "Pat Publisher",
        "decision": "approve",
        "changeset_hash": "sha256:3c9a5e7b1d2f4a6c8e0b2d4f6a8c0e2b4d6f8a1c3e5b7d9f1a3c5e7b9d1f3a5c",
        "current": true,
        "comment": "Looks right.",
        "created_at": "2026-10-27T09:35:02.117650Z"
      }
    ]
  }
}
```

| Field | Meaning |
|---|---|
| `hash` | SHA-256 of the canonical list of pins (`colophon/changeset v1`), whatever order they were added in. The `ETag` is this value in quotes. |
| `items[].kind` | `publish` (puts `revision_id` live) or `unpublish` (takes `revision_id`, the published one, down). |
| `items[].revision_id`, `content_hash`, `schema_version` | The exact revision pinned, its content hash and the content-type version it was validated against. |
| `items[].current_revision_id` | For `publish`, the entry's draft now; for `unpublish`, its published revision now, or `null` once nothing is. |
| `items[].edited` | The entry moved on since it was pinned: edited, or, for `unpublish`, published anew or already taken down. |
| `items[].published_revision_id` | What readers get now, if anything: what the item changes from. |
| `reviews` | Newest first, at most 100. `current` says the review was given on the current hash and still counts. |

### Status

`status.state` is decided from the facts, in this order (`changeset::status`):

| `state` | When | Can publish |
|---|---|---|
| `published` | It was published as the release in `status.generation`. A changeset is published once. | no |
| `stale` | At least one item is `edited`; `edited_items` says how many. Refresh it. | no |
| `open` | Not submitted for review. | no |
| `awaiting_review` | Submitted, no decision on the current hash. | no |
| `changes_requested` | The latest decision on the current hash asked for changes; `by` says who (a display name, or the identity id when there is none). | no |
| `approved` | The latest decision on the current hash approved it, less than 7 days ago; `by` says who, `self_review` whether they also submitted it (allowed, and always shown). | yes |
| `approval_expired` | That approval is more than 7 days old. Review it again. | no |

## Routes

### `GET {P}/changesets`

The environment's changesets.

- **Who:** any member.

**Response:** `200`, newest first, the newest 100, `next_cursor` always `null`. Each is a summary:

```json
{
  "data": [
    {
      "id": "0193f6d8-4b5c-7d6e-8f70-819203a4b5c6",
      "title": "Launch posts",
      "hash": "sha256:3c9a…",
      "status": { "state": "awaiting_review", "generation": null, "by": null, "self_review": false, "edited_items": 0 },
      "items": 3,
      "unpublish": 1,
      "submitted_at": "2026-10-27T09:21:40.902114Z",
      "updated_at": "2026-10-27T09:21:40.902114Z"
    }
  ],
  "next_cursor": null
}
```

`items` counts all its items; `unpublish` how many of them take an entry down.

### `POST {P}/changesets`

Creates a changeset that pins each entry's current draft revision to publish, and each published revision of the entries in `unpublish` to take down.

- **Who:** `content.update` in every collection of the items (every built-in role).
- **Headers:** `Idempotency-Key` (required).
- **Body:**

```json
{
  "title": "Launch posts",
  "entries": ["0193f6b0-2c4d-7e5f-8a91-b2c3d4e5f607", "0193f6b2-3d4e-7f50-9a1b-2c3d4e5f6071"],
  "unpublish": ["0193f5ff-0e1f-7a2b-8c3d-4e5f6a7b8c9d"]
}
```

| Field | Type | Rules |
|---|---|---|
| `title` | string | Required. Trimmed, 1 to 200 characters, no control characters. |
| `entries` | array of entry ids | Optional, default empty. Entries of any collection in this environment, to publish at their current draft. |
| `unpublish` | array of entry ids | Optional, default empty. Entries published in this environment, to take down. |

Between them, 1 to 100 different entries; an entry can't be in both. "Same request" for the idempotency key means the same title and the same sets of ids, in any order.

**Response:** `201 Created`, `ETag: "sha256:…"`, [the changeset](#the-changeset-object), in state `open`.

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `403` | `forbidden` | | Your role can't change content in one of the items' collections. |
| `409` | `conflict` | `idempotency_key_reused` | The key was used for a different changeset. |
| `422` | `validation_failed` | `invalid_title` | `/title` breaks the rule. |
| `422` | `validation_failed` | `invalid_entries` | No entries, more than 100, or one listed twice (`/entries`). |
| `422` | `validation_failed` | `unknown_entry` | An id isn't a UUID, or (`/entries`) isn't an entry in this environment. |
| `422` | `validation_failed` | `not_published` | An entry in `/unpublish` isn't published here. |
| `428` | `precondition_required` | | No `Idempotency-Key`. |

### `GET {P}/changesets/{changeset}`

One changeset, with its items, status and reviews.

- **Who:** any member.

**Response:** `200`, `ETag: "sha256:…"`, [the changeset](#the-changeset-object).

| Status | `code` | When |
|---|---|---|
| `404` | `not_found` | No such changeset here. |

### `POST {P}/changesets/{changeset}/submit`

Asks for review. Submitting one that is already submitted changes nothing.

- **Who:** `content.update` in every collection the changeset touches.
- **Headers:** `If-Match: "sha256:…"`, the hash you looked at (required).
- **Body:** none.

**Response:** `200`, `ETag`, the changeset as it is now.

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `400` | `bad_request` | | `If-Match` isn't one changeset `ETag`. |
| `403` | `forbidden` | | Your role can't change content in one of its collections. |
| `404` | `not_found` | | No such changeset. |
| `409` | `conflict` | `stale_changeset` | Its hash changed since you looked; the `ETag` header names the current one. |
| `428` | `precondition_required` | | No `If-Match`. |

### `POST {P}/changesets/{changeset}/refresh`

Re-pins every item whose entry moved on: a `publish` item to the entry's current draft, an `unpublish` item to its current published revision. If anything changed, the changeset gets a new hash, so reviews given on the old one stop counting. An `unpublish` item whose entry is no longer published keeps its pin and leaves the changeset stale: there is nothing left for it to take down.

- **Who:** `content.update` in every collection the changeset touches.
- **Headers:** `If-Match: "sha256:…"` (required).
- **Body:** none.

**Response:** `200`, `ETag` (the new hash), the changeset as it is now.

Errors: as for `submit`.

### `POST {P}/changesets/{changeset}/reviews`

Approves the exact hash in `If-Match`, or asks for changes to it. The review is stored with that hash, under a lock on the changeset.

- **Who:** `content.review` in every collection the changeset touches (owners, publishers).
- **Headers:** `If-Match: "sha256:…"` (required).
- **Body:**

```json
{ "decision": "approve", "comment": "Looks right." }
```

| Field | Type | Rules |
|---|---|---|
| `decision` | string | Required. `approve` or `request_changes`. |
| `comment` | string | Optional, default empty. At most 2,000 characters; stored trimmed. |

**Response:** `200`, `ETag`, the changeset with the new review first.

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `400` | `bad_request` | | `If-Match` is malformed. |
| `403` | `forbidden` | | Your role can't review content in one of its collections. |
| `404` | `not_found` | | No such changeset. |
| `409` | `conflict` | `stale_changeset` | Its hash changed since you looked. |
| `409` | `conflict` | | It hasn't been submitted for review yet, or an entry moved on after it was pinned (refresh it first). |
| `422` | `validation_failed` | `invalid_decision` | `/decision` isn't one of the two. |
| `422` | `validation_failed` | `invalid_comment` | `/comment` is over 2,000 characters. |
| `428` | `precondition_required` | | No `If-Match`. |

Projects can't yet require a reviewer other than the submitter; the rule is in the domain for when they can.

### `POST {P}/changesets/{changeset}/publish`

Commits the approved changeset as the release after `base_generation`, in one transaction: the environment takes the next generation, each `publish` item becomes what readers get, each `unpublish` item is taken down at every generation, and one delivery job per enabled [website connection](/reference/api/website-connections/) is written with it ([The outbox and the worker](/concepts/outbox-and-worker/)).

- **Who:** `content.publish` in every collection the changeset touches (owners, publishers).
- **Headers:** `If-Match: "sha256:…"` and `Idempotency-Key` (both required).
- **Body:**

```json
{ "base_generation": 41 }
```

| Field | Type | Rules |
|---|---|---|
| `base_generation` | integer | Required, 0 or more. The environment's generation you looked at: `Colophon-Generation` from `GET {P}/releases`. |

**Response:** `201 Created`, no `ETag`:

```json
{
  "data": {
    "generation": 42,
    "epoch": 1,
    "changeset_id": "0193f6d8-4b5c-7d6e-8f70-819203a4b5c6",
    "changeset_hash": "sha256:3c9a5e7b1d2f4a6c8e0b2d4f6a8c0e2b4d6f8a1c3e5b7d9f1a3c5e7b9d1f3a5c",
    "entries_changed": 2,
    "entries_unpublished": 1
  }
}
```

`entries_changed` counts entries whose published revision changed; `entries_unpublished` those taken down. A replay with the same key answers the same, with `Idempotent-Replayed: true`, even after the changeset is published.

If another release committed first, nothing is written:

```http
HTTP/1.1 409 Conflict
Colophon-Generation: 42
Content-Type: application/json

{"error":{"code":"conflict","message":"another release committed first: the environment is now at generation 42; look again, then publish","details":{"reason":"generation_moved","pointer":""},"request_id":"0193f6e2-8b9c-7d0e-9f1a-2b3c4d5e6f70"}}
```

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `400` | `bad_request` | | `If-Match` or the key is malformed. |
| `403` | `forbidden` | | Your role can't publish content in one of its collections. |
| `404` | `not_found` | | No such changeset. |
| `409` | `conflict` | `generation_moved` | Another release committed first; `Colophon-Generation` names the environment's generation now. |
| `409` | `conflict` | `stale_changeset` | Its hash changed since you looked. |
| `409` | `conflict` | `idempotency_key_reused` | The key was used for a different publish. |
| `409` | `conflict` | `slug_taken` | An item would put live a URL name another entry still publishes. Publish that entry's new URL name first, or in the same changeset. |
| `409` | `conflict` | | Not approved: already published, stale, approval expired, or never approved. The message says which and what to do. |
| `422` | `validation_failed` | `invalid_generation` | `/base_generation` is negative. |
| `428` | `precondition_required` | | No `If-Match`, or no `Idempotency-Key`. |

Since 3.6, publishing takes the content-type lock of every collection in the changeset, shared and in order, before reading its items, so no content type there changes between that read and the commit.

The Studio's one-click *Publish* (step 3.7, D-059) is no route of its own: it makes a changeset of the saved version, submits it, approves it as a recorded self-review and publishes it, each an ordinary call above, when the project lets submitters approve their own requests (`self_review` on [`GET /api/v1/projects/{project}`](/reference/api/projects/#get-apiv1projectsproject)) and the role may approve and publish in that collection. Otherwise it stops after submitting.

## Schedules

A schedule publishes one approved changeset at a time someone chose, in a time zone they chose (D-060). It keeps what was chosen (the local time, the IANA zone and, at a clock change, which of two moments) and the instant that resolves to, the changeset hash and the approval it publishes under. When it falls due, the worker publishes it **as the person who scheduled it**, so their role as it is then decides, through the same `colophon_db::publish` routine as `POST …/publish`, on the environment's latest generation, retrying up to three times if another release commits first. The release and the schedule's outcome commit in one transaction. Anything that would refuse a publish (an entry edited after approval, an expired approval, a role that may no longer publish, an author no longer in the project, a URL name another entry still publishes) **blocks** the schedule with the reason in plain words, and nothing is published. A run that starts more than 60 seconds after its time is labelled `delayed`. The idea is in [Scheduled publishing](/concepts/scheduled-publishing/); the worker's side in [The worker](/operations/worker/#schedules).

### The schedule object

```json
{
  "data": {
    "id": "0193f71a-2b3c-7d4e-8f50-61728394a5b6",
    "changeset_id": "0193f6d8-4b5c-7d6e-8f70-819203a4b5c6",
    "title": "Launch posts",
    "changeset_hash": "sha256:3c9a5e7b1d2f4a6c8e0b2d4f6a8c0e2b4d6f8a1c3e5b7d9f1a3c5e7b9d1f3a5c",
    "local_time": "2026-11-01T01:30:00",
    "time_zone": "America/New_York",
    "choice": "earlier",
    "run_at": "2026-11-01T05:30:00Z",
    "state": "scheduled",
    "delayed": false,
    "scheduled_by": "0193f6c4-9e0f-7a1b-8c2d-3e4f5a6b7c8d",
    "scheduled_by_name": "Pat Publisher",
    "created_at": "2026-10-27T09:40:12.551203Z"
  }
}
```

| Field | Meaning |
|---|---|
| `title`, `changeset_hash` | The changeset's title, and the hash the schedule publishes: an edit after scheduling blocks it. |
| `local_time`, `time_zone`, `choice` | What was chosen, as given (seconds added). `choice` is `earlier` or `later`, only for a time the clocks show twice or skip. |
| `run_at` | The instant that resolves to, in UTC. |
| `state` | `scheduled` (pending), `published`, `blocked` or `cancelled`. |
| `reason` | Why it was blocked, in plain words; only when `blocked`. |
| `generation` | The release it published; only when `published`. |
| `started_at`, `finished_at` | When the worker started it and when it finished, or when it was cancelled. |
| `delayed` | `true` when it started more than 60 seconds after `run_at`: never passed off as on time. |
| `scheduled_by`, `scheduled_by_name` | Who scheduled it, whose role the worker publishes with. |

### `POST {P}/changesets/{changeset}/schedule`

Schedules an approved changeset.

- **Who:** what publishing needs, now: `content.publish` in every collection it touches (owners, publishers).
- **Headers:** `If-Match: "sha256:…"`, the changeset's hash as approved (required).
- **Body:**

```json
{ "local_time": "2026-11-01T01:30", "time_zone": "America/New_York", "choice": "earlier" }
```

| Field | Type | Rules |
|---|---|---|
| `local_time` | string | Required. `YYYY-MM-DDTHH:MM`, seconds optional, a wall-clock time without an offset. |
| `time_zone` | string | Required. An IANA time zone (`Europe/London`), resolved with the time-zone database bundled into the domain crate, so every machine agrees. |
| `choice` | string | Optional: `earlier` or `later`. Needed only for a local time the clocks show twice (they go back) or skip (they go forward); ignored anywhere else. |

The instant must be in the future, at most a year and a day ahead (366 days), and before the approval it publishes under expires. A changeset has at most one pending schedule.

**Response:** `201 Created`, the schedule.

A time that needs a choice is refused with both candidate instants, earlier first; in New York on 1 November 2026, 01:30 happens twice:

```http
HTTP/1.1 422 Unprocessable Entity
Content-Type: application/json

{"error":{"code":"validation_failed","message":"…","details":{"reason":"ambiguous_local_time","pointer":"/local_time","candidates":["2026-11-01T05:30:00Z","2026-11-01T06:30:00Z"]},"request_id":"…"}}
```

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `400` | `bad_request` | | `If-Match` is malformed. |
| `403` | `forbidden` | | Your role can't publish content in one of its collections, so it can't schedule it either. |
| `404` | `not_found` | | No such changeset. |
| `409` | `conflict` | `stale_changeset` | Its hash changed since you looked. |
| `409` | `conflict` | | Not approved (already published, stale, approval expired, or never approved), or it already has a pending schedule: cancel that one to choose another time. |
| `422` | `validation_failed` | `invalid_local_time` | `/local_time` isn't a date and time. |
| `422` | `validation_failed` | `unknown_time_zone` | `/time_zone` isn't an IANA time zone. |
| `422` | `validation_failed` | `invalid_choice` | `/choice` is neither `earlier` nor `later`. |
| `422` | `validation_failed` | `ambiguous_local_time` | The clocks show that time twice there; `candidates` has both instants. Send a `choice`. |
| `422` | `validation_failed` | `skipped_local_time` | The clocks skip that time there; `candidates` has the two instants it could mean (read with the offset before the change, and after it), earlier first. Send a `choice`. |
| `422` | `validation_failed` | `in_the_past` | Not after now. |
| `422` | `validation_failed` | `too_far_ahead` | More than a year and a day ahead. |
| `422` | `validation_failed` | `after_approval_expires` | At or after the moment the approval expires; schedule it earlier, or have it approved closer to the time. |
| `428` | `precondition_required` | | No `If-Match`. |

### `GET {P}/schedules`

What is scheduled in the environment, then what happened to recent schedules.

- **Who:** any member.
- **Query:** `changeset`, a changeset's id, for that changeset's schedules only.

**Response:** `200`, `{"data": [...], "next_cursor": null}`: pending schedules (`scheduled`) in time order, at most 100, then the 20 most recently finished (published, blocked or cancelled), newest first. Each half reads its own partial index, so the list stays bounded however many schedules the environment ever had. There is no paging.

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `422` | `validation_failed` | `invalid_changeset` | `changeset` isn't a changeset id. |

### `DELETE {P}/schedules/{schedule}`

Cancels a pending schedule. A run already in progress finishes first; cancelling never undoes a release.

- **Who:** the person who scheduled it, or anyone who may publish everything it publishes.
- **Body:** none.

**Response:** `200`, the schedule, now `cancelled`.

| Status | `code` | When |
|---|---|---|
| `403` | `forbidden` | It isn't yours, and your role can't publish its content. |
| `404` | `not_found` | No such schedule. |
| `409` | `conflict` | It already published, was blocked, or was cancelled already. |

## Proof

`crates/colophon-server/tests/changesets.rs` and `releases.rs` against the real binary; `crates/colophon-db/tests/changesets.rs` and `releases.rs` against PostgreSQL, including two releases on one base generation, of which only one commits; the domain's status rules as unit tests in `changeset.rs`; the load scenarios `perf/k6/changesets.js` and `perf/k6/publishing.js`.

Schedules: the domain's tests resolve New York's and London's clock changes (`crates/colophon-domain/src/schedule.rs`); `crates/colophon-db/tests/schedules.rs` passes the clock in instead of waiting, and checks that a schedule publishes once and not before its time, is labelled delayed when late, is blocked by an expired approval, an edit, a demotion or a departure, and that two workers at once publish it once; `crates/colophon-server/tests/schedules.rs` against the real binary and worker; the testbed's `apps/testbed/tests/schedules.spec.ts`; and the load scenario `perf/k6/schedules.js`, whose full runs are on [Performance](/resources/performance/).

## Not built yet

- Recurring schedules. <Status planned="not scheduled" />
- A project setting that requires a reviewer other than the submitter. <Status planned="not scheduled" />
- Rolling back as a new release. <Status planned="P4" />
